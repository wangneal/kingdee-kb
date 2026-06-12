//! usearch HNSW 向量索引管理
//!
//! 封装 usearch Index，提供创建/插入/搜索/保存/加载操作。
//! 使用 512 维向量、余弦距离、BF16 量化。
//! 索引持久化到 `~/.kingdee-kb/index/vectors.usearch`。
//!
//! ## 安全性：add() 前自动 reserve
//!
//! usearch C++ 的 `add()` 会访问 `contexts_[config.thread]`，该缓冲区仅通过
//! `try_reserve()` 分配。在未 reserve 的索引上调用 `add()` 会导致
//! Access Violation 0xc0000005（空指针 + 偏移）。我们在首次 `add()` 前
//! 自动 reserve `MIN_RESERVE_CAPACITY` 以防止此崩溃。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use usearch::{new_index, Index, IndexOptions, MetricKind, ScalarKind};

/// bge-small-zh-v1.5 (512 维) 的默认 HNSW 参数
const DEFAULT_CONNECTIVITY: usize = 16;
const DEFAULT_EXPANSION_ADD: usize = 200;
const DEFAULT_EXPANSION_SEARCH: usize = 64;

/// 确保 usearch `contexts_` 缓冲区已分配的最小预留容量。
///
/// usearch 的 `add()` 在 `contexts_` 为空时会崩溃（Access Violation 0xc0000005），
/// 这发生在从未调用过 `reserve()` 时。我们在首次 `add()` 前自动预留
/// 此最小容量以防止崩溃。
const MIN_RESERVE_CAPACITY: usize = 1024;

/// 向量索引的搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// 索引中的键（向量 ID）
    pub key: u64,
    /// 余弦距离（0 = 完全相同，2 = 完全相反）
    pub distance: f32,
    /// 余弦相似度 = 1 - 距离（适用于余弦度量）
    pub similarity: f32,
}

/// 索引统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub vector_count: usize,
    pub dimensions: usize,
    pub index_path: String,
}

/// 向量索引碎片整理计划。
pub struct VectorCompactionPlan {
    new_index: Index,
    surviving_count: usize,
    /// prepare_compaction 时的版本号，apply_compaction 用于检测并发修改
    compaction_version: u64,
}

/// HNSW 向量索引管理器
pub struct VectorIndex {
    index: Index,
    index_path: PathBuf,
    dimensions: usize,
    /// 跟踪 contexts_ 是否已分配（已调用 reserve 或索引已加载数据）。
    /// 防止冗余的 reserve 调用和 Access Violation 崩溃。
    reserved: bool,
    /// 逻辑删除计数，用于触发后台碎片整理
    deleted_count: std::sync::atomic::AtomicUsize,
    /// 上次重建时间，用于冷却期判断
    last_compact: std::sync::Mutex<std::time::Instant>,
    /// 最大已使用 key 值，用于 compact 时的遍历范围
    max_key: std::sync::atomic::AtomicU64,
    /// 每次 add/remove 递增，用于 compaction 版本校验防止竞态
    compaction_version: std::sync::atomic::AtomicU64,
}

impl VectorIndex {
    const COMPACT_THRESHOLD: f64 = 0.2; // 20%
    const COMPACT_COOLDOWN_SECS: u64 = 300; // 5 分钟

    /// 创建新的空索引，持久化到指定目录
    /// `dimensions` 应与嵌入模型输出维度匹配（BGE 为 512，MiniLM 为 384）。
    pub fn new(index_dir: PathBuf) -> Result<Self, String> {
        Self::with_dimensions(index_dir, 512)
    }

    /// 创建指定维度的新空索引
    pub fn with_dimensions(index_dir: PathBuf, dimensions: usize) -> Result<Self, String> {
        std::fs::create_dir_all(&index_dir)
            .map_err(|e| format!("Failed to create index directory: {}", e))?;

        let index_path = index_dir.join("vectors.usearch");

        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::BF16,
            connectivity: DEFAULT_CONNECTIVITY,
            expansion_add: DEFAULT_EXPANSION_ADD,
            expansion_search: DEFAULT_EXPANSION_SEARCH,
            multi: true,
        };

        let index = new_index(&options).map_err(|e| format!("Failed to create index: {}", e))?;

        Ok(Self {
            index,
            index_path,
            dimensions,
            reserved: false,
            deleted_count: std::sync::atomic::AtomicUsize::new(0),
            last_compact: std::sync::Mutex::new(std::time::Instant::now()),
            max_key: std::sync::atomic::AtomicU64::new(0),
            compaction_version: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// 从磁盘加载已有索引
    /// `dimensions` 必须与保存时的索引维度一致。
    pub fn load(index_path: PathBuf) -> Result<Self, String> {
        Self::load_with_dimensions(index_path, 512)
    }

    /// 从磁盘加载已有索引，支持显式指定维度。
    ///
    /// 如果磁盘上的索引是用 `multi: false`（旧版）构建的，会自动删除并
    /// 创建一个新的 `multi: true` 索引。调用方需要重新导入文档以填充索引。
    pub fn load_with_dimensions(index_path: PathBuf, dimensions: usize) -> Result<Self, String> {
        // 尝试读取已有索引文件的元数据。
        // 如果是 multi:false 构建的，删除它——无法用 multi:true 加载
        // （usearch 会拒绝不匹配的 multi 标志）。
        if index_path.exists() {
            if let Ok(meta) = Index::metadata(index_path.to_str().unwrap_or("")) {
                if !meta.multi {
                    // 旧版索引，multi:false —— 删除并重建。
                    // 调用方需要重新导入，但这可以防止
                    // 运行时的 "Duplicate keys" 崩溃。
                    let _ = std::fs::remove_file(&index_path);
                }
            }
        }

        if !index_path.exists() {
            // 无索引文件（上面已删除或从未创建）——返回一个全新索引。
            let index_dir = index_path.parent().ok_or("Invalid index path: no parent")?;
            return Self::with_dimensions(index_dir.to_path_buf(), dimensions);
        }

        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::BF16,
            connectivity: DEFAULT_CONNECTIVITY,
            expansion_add: DEFAULT_EXPANSION_ADD,
            expansion_search: DEFAULT_EXPANSION_SEARCH,
            multi: true,
        };

        let index = new_index(&options).map_err(|e| format!("Failed to create index: {}", e))?;

        let path_str = index_path.to_str().ok_or("Invalid index path (non-UTF8)")?;

        index
            .load(path_str)
            .map_err(|e| format!("Failed to load index from {:?}: {}", index_path, e))?;

        // 已加载且包含向量的索引，其 contexts_ 已经分配
        let reserved = index.capacity() > 0 || index.size() > 0;

        let max_k = index.size() as u64;
        Ok(Self {
            index,
            index_path: index_path.to_path_buf(),
            dimensions,
            reserved,
            deleted_count: std::sync::atomic::AtomicUsize::new(0),
            last_compact: std::sync::Mutex::new(std::time::Instant::now()),
            max_key: std::sync::atomic::AtomicU64::new(max_k),
            compaction_version: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// 确保 usearch `contexts_` 缓冲区在 `add()`/`search()` 前已分配。
    ///
    /// 如果尚未执行 reserve，会自动预留 `MIN_RESERVE_CAPACITY`。
    /// 这防止了 `add()` 解引用空 `contexts_` 指针时的
    /// Access Violation 0xc0000005 崩溃。
    ///
    /// 同时处理当前容量不足时的动态扩容。
    fn ensure_reserved(&mut self) -> Result<(), String> {
        let current_size = self.index.size();
        let current_cap = self.index.capacity();

        // 需要扩容的情况：
        // 1. 从未 reserve 过（capacity == 0）
        // 2. 当前 size 已达到 capacity 的 80%（提前扩容避免溢出）
        let needs_reserve = current_cap == 0 || current_size >= (current_cap * 80 / 100);

        if needs_reserve {
            // 新容量：至少 MIN_RESERVE_CAPACITY，或者当前容量的 2 倍
            let new_cap = if current_cap == 0 {
                MIN_RESERVE_CAPACITY
            } else {
                std::cmp::max(current_cap * 2, current_size + MIN_RESERVE_CAPACITY)
            };

            tracing::info!(
                "[VectorIndex] 预分配容量: {}（当前 size: {}，当前 cap: {}）",
                new_cap, current_size, current_cap
            );

            self.index
                .reserve(new_cap)
                .map_err(|e| format!("Failed to reserve {}: {}", new_cap, e))?;
            self.reserved = true;
        }
        Ok(())
    }

    /// 添加单个向量到索引
    pub fn add(&mut self, key: u64, vector: &[f32]) -> Result<(), String> {
        self.ensure_reserved()?;
        self.index
            .add(key, vector)
            .map_err(|e| format!("Failed to add vector {}: {}", key, e))?;
        // 更新最大 key
        self.max_key
            .fetch_max(key, std::sync::atomic::Ordering::Relaxed);
        self.compaction_version
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// 批量添加多个向量
    pub fn add_batch(&mut self, keys: &[u64], vectors: &[Vec<f32>]) -> Result<(), String> {
        if keys.len() != vectors.len() {
            return Err(format!(
                "keys.len() ({}) != vectors.len() ({})",
                keys.len(),
                vectors.len()
            ));
        }

        self.ensure_reserved()?;

        for (key, vector) in keys.iter().zip(vectors.iter()) {
            self.index
                .add(*key, vector)
                .map_err(|e| format!("Failed to add vector {}: {}", key, e))?;
            self.max_key
                .fetch_max(*key, std::sync::atomic::Ordering::Relaxed);
        }
        self.compaction_version
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        Ok(())
    }

    /// 搜索 top_k 个最近邻
    pub fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<SearchResult>, String> {
        let results = self
            .index
            .search(query, top_k)
            .map_err(|e| format!("Search failed: {}", e))?;

        let search_results: Vec<SearchResult> = results
            .keys
            .iter()
            .zip(results.distances.iter())
            .map(|(&key, &distance)| SearchResult {
                key,
                distance,
                similarity: 1.0 - distance,
            })
            .collect();

        Ok(search_results)
    }

    /// 从索引中按 key 移除向量。
    ///
    /// 返回移除的向量数量（未找到返回 0，找到返回 1）。
    pub fn remove(&self, key: u64) -> Result<usize, String> {
        let result = self
            .index
            .remove(key)
            .map_err(|e| format!("Failed to remove key {}: {}", key, e));
        if result.is_ok() {
            self.deleted_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.compaction_version
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        result
    }

    /// 按 keys 批量移除向量。
    pub fn remove_keys(&self, keys: &[i64]) -> Result<usize, String> {
        let mut count = 0;
        for key in keys {
            if self.index.remove(*key as u64).is_ok() {
                count += 1;
            }
        }
        self.deleted_count
            .fetch_add(count, std::sync::atomic::Ordering::SeqCst);
        if count > 0 {
            self.compaction_version
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(count)
    }

    /// 检查是否超过碎片整理阈值（20% 逻辑删除 + 5 分钟冷却）
    pub fn check_compact(&self) -> bool {
        let total = self.len() as f64;
        if total <= 0.0 {
            return false;
        }
        if let Ok(last) = self.last_compact.lock() {
            if last.elapsed() < std::time::Duration::from_secs(Self::COMPACT_COOLDOWN_SECS) {
                return false;
            }
        }
        let ratio = self
            .deleted_count
            .load(std::sync::atomic::Ordering::Relaxed) as f64
            / total;
        ratio >= Self::COMPACT_THRESHOLD
    }

    /// 后台碎片整理：重建 HNSW 图，剔除已删除节点
    /// 在 tokio::task::spawn_blocking 中执行，不阻塞主线程
    pub fn compact(&mut self) -> Result<(), String> {
        let n = self.len() as u64;
        if n == 0
            || self
                .deleted_count
                .load(std::sync::atomic::Ordering::Relaxed)
                == 0
        {
            return Ok(());
        }
        // 收集幸存向量（key = 1..=max_key，逐 key 尝试获取）
        // 注意：不能使用 1..=n（当前存活数），key 由 autoincrement 生成可能不连续
        let max_k = self.max_key.load(std::sync::atomic::Ordering::Relaxed);
        let mut surviving: Vec<(u64, Vec<f32>)> = Vec::with_capacity(n as usize);
        for key in 1..=max_k {
            let mut vec = vec![0.0f32; self.dimensions];
            if self.index.get(key, &mut vec).is_ok() {
                surviving.push((key, vec));
            }
        }
        // 创建新索引
        let options = IndexOptions {
            dimensions: self.dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::BF16,
            connectivity: DEFAULT_CONNECTIVITY,
            expansion_add: DEFAULT_EXPANSION_ADD,
            expansion_search: DEFAULT_EXPANSION_SEARCH,
            multi: true, // 保持与当前配置一致
        };
        let new_index = new_index(&options).map_err(|e| format!("创建新索引失败: {}", e))?;
        new_index
            .reserve(surviving.len())
            .map_err(|e| format!("预留容量失败: {}", e))?;
        for (key, vec) in &surviving {
            new_index
                .add(*key, vec)
                .map_err(|e| format!("添加向量失败: {}", e))?;
        }
        // 原子替换
        let old = std::mem::replace(&mut self.index, new_index);
        drop(old); // 释放旧索引
        self.deleted_count
            .store(0, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut last) = self.last_compact.lock() {
            *last = std::time::Instant::now();
        }
        Ok(())
    }

    /// 在共享读锁下准备碎片整理计划，避免长时间占用外层写锁。
    pub fn prepare_compaction(&self) -> Result<Option<VectorCompactionPlan>, String> {
        let n = self.len() as u64;
        if n == 0
            || self
                .deleted_count
                .load(std::sync::atomic::Ordering::Relaxed)
                == 0
        {
            return Ok(None);
        }

        let max_k = self.max_key.load(std::sync::atomic::Ordering::Relaxed);
        let mut surviving: Vec<(u64, Vec<f32>)> = Vec::with_capacity(n as usize);
        for key in 1..=max_k {
            let mut vec = vec![0.0f32; self.dimensions];
            if self.index.get(key, &mut vec).is_ok() {
                surviving.push((key, vec));
            }
        }

        let options = IndexOptions {
            dimensions: self.dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::BF16,
            connectivity: DEFAULT_CONNECTIVITY,
            expansion_add: DEFAULT_EXPANSION_ADD,
            expansion_search: DEFAULT_EXPANSION_SEARCH,
            multi: true,
        };
        let new_index = new_index(&options).map_err(|e| format!("创建新索引失败: {}", e))?;
        new_index
            .reserve(surviving.len())
            .map_err(|e| format!("预留容量失败: {}", e))?;
        for (key, vec) in &surviving {
            new_index
                .add(*key, vec)
                .map_err(|e| format!("添加向量失败: {}", e))?;
        }

        let version = self
            .compaction_version
            .load(std::sync::atomic::Ordering::SeqCst);

        Ok(Some(VectorCompactionPlan {
            new_index,
            surviving_count: surviving.len(),
            compaction_version: version,
        }))
    }

    /// 应用已准备好的碎片整理计划，只在替换阶段占用外层写锁。
    /// 如果 prepare_compaction 后有新 add/remove，返回错误（调用方应重试）。
    pub fn apply_compaction(&mut self, plan: VectorCompactionPlan) -> Result<(), String> {
        let current_version = self
            .compaction_version
            .load(std::sync::atomic::Ordering::SeqCst);
        if current_version != plan.compaction_version {
            return Err(format!(
                "碎片整理版本不匹配（prepare={}, current={}），有并发修改，请重试",
                plan.compaction_version, current_version
            ));
        }
        let old = std::mem::replace(&mut self.index, plan.new_index);
        drop(old);
        self.deleted_count
            .store(0, std::sync::atomic::Ordering::SeqCst);
        self.reserved = plan.surviving_count > 0;
        if let Ok(mut last) = self.last_compact.lock() {
            *last = std::time::Instant::now();
        }
        Ok(())
    }

    /// 保存索引到磁盘
    pub fn save(&self) -> Result<(), String> {
        let path_str = self
            .index_path
            .to_str()
            .ok_or("Invalid index path (non-UTF8)")?;

        self.index
            .save(path_str)
            .map_err(|e| format!("Failed to save index to {:?}: {}", self.index_path, e))
    }

    /// 获取索引中的向量数量
    pub fn len(&self) -> usize {
        self.index.size()
    }

    /// 检查索引是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 获取索引容量
    pub fn capacity(&self) -> usize {
        self.index.capacity()
    }

    /// 为预期的向量数量预留容量
    pub fn reserve(&mut self, capacity: usize) -> Result<(), String> {
        self.index
            .reserve(capacity)
            .map_err(|e| format!("Failed to reserve {}: {}", capacity, e))?;
        self.reserved = true;
        Ok(())
    }

    /// 获取索引统计信息
    pub fn stats(&self) -> IndexStats {
        IndexStats {
            vector_count: self.len(),
            dimensions: self.dimensions,
            index_path: self.index_path.to_string_lossy().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_vector(dim: usize, seed: u64) -> Vec<f32> {
        let mut v = Vec::with_capacity(dim);
        let mut s = seed;
        for _ in 0..dim {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            v.push(((s as f64 / u64::MAX as f64) * 2.0 - 1.0) as f32);
        }
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        for x in &mut v {
            *x /= norm;
        }
        v
    }

    #[test]
    fn test_vector_index_crud() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = VectorIndex::new(tmp.path().to_path_buf()).unwrap();

        assert_eq!(index.len(), 0);
        assert!(index.is_empty());

        let v0 = random_vector(512, 42);
        index.add(1, &v0).unwrap();
        assert_eq!(index.len(), 1);
        assert!(!index.is_empty());

        // 搜索自身
        let results = index.search(&v0, 3).unwrap();
        assert_eq!(results[0].key, 1);
        assert!(results[0].distance < 0.01);
        assert!(results[0].similarity > 0.99);

        // 移除
        let removed = index.remove(1).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(index.len(), 0); // remove() 会减少 size

        // 保存并重新加载
        index.save().unwrap();
        let index_path = tmp.path().join("vectors.usearch");
        assert!(index_path.exists());

        let loaded = VectorIndex::load(index_path).unwrap();
        assert_eq!(loaded.len(), 0); // 向量在保存前已被移除
    }

    #[test]
    fn test_vector_index_batch() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = VectorIndex::new(tmp.path().to_path_buf()).unwrap();

        let vectors: Vec<Vec<f32>> = (0..10).map(|i| random_vector(512, i)).collect();
        let keys: Vec<u64> = (0..10).collect();
        index.add_batch(&keys, &vectors).unwrap();
        assert_eq!(index.len(), 10);

        let results = index.search(&vectors[0], 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].key, 0); // 自身应该是第一个
    }

    #[test]
    fn test_add_without_explicit_reserve_does_not_crash() {
        // 此测试验证核心修复：在全新索引上调用 add()
        // （不显式 reserve）不应导致 Access Violation 0xc0000005。
        // 之前，usearch 的 add() 会解引用空的 contexts_ 指针。
        let tmp = tempfile::tempdir().unwrap();
        let mut index = VectorIndex::new(tmp.path().to_path_buf()).unwrap();

        // 不显式调用 reserve() —— 自动预留机制应生效
        let v = random_vector(512, 1);
        index.add(1, &v).unwrap(); // 不应崩溃
        assert_eq!(index.len(), 1);
    }
}
