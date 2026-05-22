# Phase 2: 嵌入与向量存储引擎 — 执行计划

**Created:** 2026-05-23
**Status:** Ready for execution
**Dependencies:** Phase 1（Tauri 脚手架 + ~/.kingdee-kb/ 目录结构）

---

## Overview

本阶段实现 KingdeeKB 的核心向量化能力：bge-small-zh-v1.5 模型自动下载与推理、usearch HNSW 索引读写与持久化、rusqlite 元数据存储。完成后，中文文本能被向量化并存储，为 Phase 3（入库）和 Phase 4-5（检索）提供共同上游依赖。

**成功标准（来自 ROADMAP.md）：**
1. bge-small-zh-v1.5 模型在首次使用时自动下载到 `~/.kingdee-kb/models/`，下载过程可观测进度
2. 输入中文文本后返回 512 维向量，语义相近的中文句子向量余弦相似度 ≥ 0.7
3. 向量能写入 usearch HNSW 索引并持久化到磁盘（`~/.kingdee-kb/index/`），重启后索引可读
4. 通过余弦相似度从索引中检索 Top-K 向量，结果按相似度降序返回

---

## Task 1: SPIKE — 验证 usearch + bge-small-zh-v1.5 ONNX 在 Windows 上的可行性

**Goal:** 在实际 Windows 环境中端到端验证 `usearch` + `fastembed-rs` (bge-small-zh-v1.5 ONNX) 的编译、模型下载、向量生成和 HNSW 索引读写。

**Steps:**
1. 在 `src-tauri/` 下创建 `tests/spike_embedding.rs` 测试文件
2. 添加临时依赖到 `src-tauri/Cargo.toml`：
   ```toml
   fastembed = { version = "5", features = ["ort-download-binaries-native-tls"] }
   usearch = "2"
   ```
3. 编写 Spike 测试代码：
   ```rust
   #[test]
   fn spike_bge_embedding() {
       // 1. 初始化 fastembed-rs，触发 bge-small-zh-v1.5 模型自动下载
       let model = TextEmbedding::try_new(InitOptions {
           model_name: EmbeddingModel::BGESmallZHV15,
           show_download_progress: true,
           ..Default::default()
       }).expect("模型初始化失败");
       
       // 2. 生成 embedding
       let texts = vec![
           "金蝶云星空如何配置期货点价",
           "金蝶苍穹期货点价模块设置",
           "今天天气不错",
       ];
       let embeddings = model.embed(texts).expect("Embedding 失败");
       
       // 验证维度 = 512
       assert_eq!(embeddings[0].len(), 512);
       
       // 3. 验证语义相似度
       let sim_01 = cosine_similarity(&embeddings[0], &embeddings[1]);
       let sim_02 = cosine_similarity(&embeddings[0], &embeddings[2]);
       assert!(sim_01 > 0.7, "语义相近句子相似度应 ≥ 0.7, got {}", sim_01);
       assert!(sim_02 < sim_01, "不相关句子相似度应更低");
       
       // 4. 写入 usearch HNSW 索引
       let index = new_index(&IndexOptions {
           dimensions: 512,
           metric: MetricKind::Cos,
           connectivity: 16,
           ..Default::default()
       }).expect("创建索引失败");
       
       index.add(0, &embeddings[0]).expect("添加向量失败");
       index.add(1, &embeddings[1]).expect("添加向量失败");
       
       // 5. 检索
       let results = index.search(&embeddings[0], 2).expect("检索失败");
       assert_eq!(results[0].key, 0); // 自身应该排第一
       
       // 6. 持久化到临时文件
       let tmp = tempfile::tempdir().unwrap();
       let path = tmp.path().join("test.usearch");
       index.save(path.to_str().unwrap()).expect("保存索引失败");
       
       // 7. 重新加载
       let loaded = new_index(&IndexOptions { dimensions: 512, metric: MetricKind::Cos, connectivity: 16, ..Default::default() }).unwrap();
       loaded.load(path.to_str().unwrap()).expect("加载索引失败");
       let results2 = loaded.search(&embeddings[0], 2).expect("检索失败");
       assert_eq!(results2[0].key, 0);
   }
   ```
4. 运行 `cargo test spike_embedding -- --nocapture` 验证
5. 记录实际耗时（模型下载、首次推理、HNSW 构建、保存/加载）

**Files to create/modify:**
- `src-tauri/tests/spike_embedding.rs`（Spike 测试）
- `src-tauri/Cargo.toml`（临时添加 fastembed、usearch 依赖）

**Verification:**
- [ ] `cargo test spike_embedding` 全部通过
- [ ] bge-small-zh-v1.5 模型自动下载到本地缓存
- [ ] 生成 512 维向量
- [ ] 语义相近中文句子余弦相似度 ≥ 0.7
- [ ] usearch HNSW 索引可写入、可检索、可持久化、可重新加载
- [ ] 全程无 Python 依赖、无 sidecar 进程

**Dependencies:** None（Phase 1 已完成，Cargo.toml 可直接修改）

---

## Task 2: 添加 Phase 2 依赖到 Cargo.toml

**Goal:** 将 fastembed-rs、usearch、rusqlite 正式添加为项目依赖。

**Steps:**
1. 在 `src-tauri/Cargo.toml` 中添加：
   ```toml
   # Embedding 模型推理
   fastembed = { version = "5", features = ["ort-download-binaries-native-tls"] }
   
   # HNSW 向量索引
   usearch = "2"
   
   # 元数据存储
   rusqlite = { version = "0.32", features = ["bundled"] }
   ```
2. 运行 `cargo check` 验证依赖解析无冲突
3. 验证与现有 Tauri 2.2、tokio、serde 依赖兼容

**Files to modify:**
- `src-tauri/Cargo.toml`

**Verification:**
- [ ] `cargo check` 编译通过，无依赖冲突
- [ ] fastembed-rs 的 ONNX Runtime 二进制自动下载成功
- [ ] 现有 Tauri 功能不受影响

**Dependencies:** Task 1（Spike 验证通过后才可正式添加）

---

## Task 3: 模型自动下载管理器（INFR-06）

**Goal:** 实现 `ModelManager` 结构体，负责 bge-small-zh-v1.5 模型的首次自动下载、缓存路径管理、进度回调。

**Steps:**
1. 在 `src-tauri/src/services/` 目录创建 `mod.rs` 和 `embedding.rs`
2. 实现 `ModelManager`：
   ```rust
   pub struct ModelManager {
       model_dir: PathBuf,  // ~/.kingdee-kb/models/
       model: Option<TextEmbedding>,
   }
   
   impl ModelManager {
       pub fn new(model_dir: PathBuf) -> Self { ... }
       pub fn init(&mut self) -> Result<(), String> {
           // 首次使用时自动下载 bge-small-zh-v1.5 到 model_dir
           // 使用 InitOptions::with_model_name(EmbeddingModel::BGESmallZHV15)
           // show_download_progress: true
       }
       pub fn is_ready(&self) -> bool { ... }
   }
   ```
3. 实现进度回调机制（通过 Tauri event 发送下载进度到前端）
4. 在 `src-tauri/src/lib.rs` 的 `run()` 中初始化 `ModelManager`
5. 创建 Tauri 命令 `get_model_status` 供前端查询模型就绪状态

**Files to create/modify:**
- `src-tauri/src/services/mod.rs`（服务模块声明）
- `src-tauri/src/services/embedding.rs`（ModelManager 实现）
- `src-tauri/src/lib.rs`（初始化 ModelManager、注册 Tauri 命令）

**Verification:**
- [ ] 首次启动时模型自动下载到 `~/.kingdee-kb/models/`
- [ ] 下载进度通过 Tauri event 可观测
- [ ] `get_model_status` 命令返回正确状态
- [ ] 模型已缓存后不再重复下载

**Dependencies:** Task 2

---

## Task 4: Embedding 服务（SRCH-01）

**Goal:** 实现 `EmbeddingService`，提供文本向量化能力——接收中文文本，返回 512 维 f32 向量。

**Steps:**
1. 在 `src-tauri/src/services/embedding.rs` 中实现 `EmbeddingService`：
   ```rust
   pub struct EmbeddingService {
       model: TextEmbedding,  // fastembed-rs 模型实例
   }
   
   impl EmbeddingService {
       pub fn new(model: TextEmbedding) -> Self { ... }
       
       /// 单条文本 embedding
       pub fn embed_text(&self, text: &str) -> Result<Vec<f32>, String> {
           let embeddings = self.model.embed(vec![text])?;
           Ok(embeddings.into_iter().next().unwrap())
       }
       
       /// 批量 embedding（batch_size=64）
       pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
           // 按 64 条分批调用 model.embed()
           // 合并结果返回
       }
   }
   ```
2. 创建 Tauri 命令：
   ```rust
   #[tauri::command]
   async fn embed_text(text: String) -> Result<Vec<f32>, String> { ... }
   
   #[tauri::command]
   async fn embed_batch(texts: Vec<String>) -> Result<Vec<Vec<f32>>, String> { ... }
   ```
3. 在 `lib.rs` 中注册命令
4. 添加单元测试验证 512 维输出

**Files to create/modify:**
- `src-tauri/src/services/embedding.rs`（EmbeddingService 扩展）
- `src-tauri/src/lib.rs`（注册 Tauri 命令）
- `src-tauri/src/services/mod.rs`（导出模块）

**Verification:**
- [ ] `embed_text("金蝶云星空")` 返回 512 维向量
- [ ] `embed_batch` 批量处理 100+ 条文本无错误
- [ ] 语义相近中文句子余弦相似度 ≥ 0.7
- [ ] Tauri IPC `invoke("embed_text", { text })` 正常工作

**Dependencies:** Task 3

---

## Task 5: usearch HNSW 索引管理

**Goal:** 实现 `VectorIndex` 结构体，封装 usearch HNSW 索引的创建、添加向量、保存、加载、检索操作。

**Steps:**
1. 在 `src-tauri/src/services/` 创建 `vector_index.rs`
2. 实现 `VectorIndex`：
   ```rust
   pub struct VectorIndex {
       index: Index,
       index_path: PathBuf,  // ~/.kingdee-kb/index/vectors.usearch
   }
   
   impl VectorIndex {
       pub fn new(index_dir: PathBuf) -> Result<Self, String> {
           // 创建 usearch Index
           // dimensions: 512, metric: Cos, M: 16, ef_construction: 200
       }
       
       pub fn add(&self, key: u64, vector: &[f32]) -> Result<(), String> { ... }
       pub fn add_batch(&self, keys: &[u64], vectors: &[Vec<f32>]) -> Result<(), String> { ... }
       pub fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<SearchResult>, String> { ... }
       pub fn remove(&self, key: u64) -> Result<(), String> { ... }
       pub fn save(&self) -> Result<(), String> { ... }
       pub fn load(path: PathBuf) -> Result<Self, String> { ... }
       pub fn len(&self) -> usize { ... }
   }
   
   #[derive(Serialize, Deserialize)]
   pub struct SearchResult {
       pub key: u64,
       pub distance: f32,
   }
   ```
3. 创建 Tauri 命令：
   ```rust
   #[tauri::command]
   async fn search_similar(query: Vec<f32>, top_k: u32) -> Result<Vec<SearchResult>, String> { ... }
   
   #[tauri::command]
   async fn get_index_stats() -> Result<IndexStats, String> { ... }
   ```
4. 应用启动时自动加载已有索引（如果存在）
5. 应用关闭时自动保存索引

**Files to create/modify:**
- `src-tauri/src/services/vector_index.rs`（VectorIndex 实现）
- `src-tauri/src/services/mod.rs`（导出模块）
- `src-tauri/src/lib.rs`（注册命令、启动加载/关闭保存）

**Verification:**
- [ ] 索引创建成功，HNSW 参数 M=16, ef_construction=200
- [ ] 写入 100+ 向量后检索正常，结果按距离升序返回
- [ ] 索引持久化到 `~/.kingdee-kb/index/vectors.usearch`
- [ ] 重启应用后索引可加载，历史数据可检索
- [ ] `remove` 操作正常（usearch 支持标记删除）

**Dependencies:** Task 2

---

## Task 6: rusqlite 元数据存储

**Goal:** 实现 `MetadataStore` 结构体，使用 rusqlite 管理 chunk 元数据——存储 chunk↔vector 的映射关系、文档信息、标签。

**Steps:**
1. 在 `src-tauri/src/services/` 创建 `metadata.rs`
2. 实现数据库 Schema：
   ```sql
   CREATE TABLE IF NOT EXISTS documents (
       id INTEGER PRIMARY KEY AUTOINCREMENT,
       title TEXT NOT NULL,
       source_path TEXT,
       sha256 TEXT UNIQUE,  -- 去重
       created_at TEXT DEFAULT (datetime('now')),
       project TEXT DEFAULT 'default'
   );
   
   CREATE TABLE IF NOT EXISTS chunks (
       id INTEGER PRIMARY KEY AUTOINCREMENT,
       vector_key INTEGER UNIQUE,  -- 对应 usearch 中的 key
       document_id INTEGER REFERENCES documents(id),
       content TEXT NOT NULL,
       section_path TEXT,  -- 章节路径
       tags TEXT,  -- JSON 数组
       line_no INTEGER,
       created_at TEXT DEFAULT (datetime('now'))
   );
   
   CREATE INDEX IF NOT EXISTS idx_chunks_vector_key ON chunks(vector_key);
   CREATE INDEX IF NOT EXISTS idx_chunks_document_id ON chunks(document_id);
   CREATE INDEX IF NOT EXISTS idx_documents_sha256 ON documents(sha256);
   CREATE INDEX IF NOT EXISTS idx_documents_project ON documents(project);
   ```
3. 实现 `MetadataStore`：
   ```rust
   pub struct MetadataStore {
       db: Connection,
   }
   
   impl MetadataStore {
       pub fn new(db_path: PathBuf) -> Result<Self, String> {
           let db = Connection::open(db_path)?;
           // PRAGMA journal_mode=WAL;
           // PRAGMA integrity_check;
           // CREATE TABLE IF NOT EXISTS ...
       }
       
       pub fn insert_document(&self, ...) -> Result<i64, String> { ... }
       pub fn insert_chunk(&self, ...) -> Result<i64, String> { ... }
       pub fn get_chunk_by_vector_key(&self, key: u64) -> Result<Option<ChunkMeta>, String> { ... }
       pub fn get_chunks_by_vector_keys(&self, keys: &[u64]) -> Result<Vec<ChunkMeta>, String> { ... }
       pub fn get_document_by_sha256(&self, sha256: &str) -> Result<Option<DocumentMeta>, String> { ... }
       pub fn delete_document(&self, id: i64) -> Result<(), String> { ... }
   }
   ```
4. 创建 Tauri 命令：
   ```rust
   #[tauri::command]
   async fn get_knowledge_stats() -> Result<KnowledgeStats, String> { ... }
   ```
5. 应用启动时初始化数据库（幂等，重复启动不报错）

**Files to create/modify:**
- `src-tauri/src/services/metadata.rs`（MetadataStore 实现）
- `src-tauri/src/services/mod.rs`（导出模块）
- `src-tauri/src/lib.rs`（注册命令、启动初始化）

**Verification:**
- [ ] `~/.kingdee-kb/metadata.db` 数据库创建成功
- [ ] `PRAGMA journal_mode=WAL` 生效
- [ ] 文档和 chunk 的 CRUD 操作正常
- [ ] SHA256 去重查询正常
- [ ] 重复启动幂等性验证通过
- [ ] `get_knowledge_stats` 返回正确的文档数和 chunk 数

**Dependencies:** Task 2

---

## Task 7: Tauri Commands 集成 — AppState 统一管理

**Goal:** 将 EmbeddingService、VectorIndex、MetadataStore 统一注入 Tauri AppState，提供完整的端到端命令。

**Steps:**
1. 创建 `src-tauri/src/app_state.rs`：
   ```rust
   pub struct AppState {
       pub embedding: Arc<Mutex<EmbeddingService>>,
       pub vector_index: Arc<Mutex<VectorIndex>>,
       pub metadata: Arc<Mutex<MetadataStore>>,
   }
   ```
2. 在 `lib.rs` 的 `run()` 中初始化所有服务并注入 `AppState`
3. 创建端到端 Tauri 命令：
   ```rust
   /// 一站式：文本 → embedding → 存入索引 → 存入元数据
   #[tauri::command]
   async fn ingest_text(
       state: State<'_, AppState>,
       text: String,
       title: String,
       tags: Vec<String>,
   ) -> Result<IngestResult, String> { ... }
   
   /// 一站式：查询 → embedding → 检索索引 → 查元数据 → 返回结果
   #[tauri::command]
   async fn search_knowledge(
       state: State<'_, AppState>,
       query: String,
       top_k: u32,
       project: Option<String>,
   ) -> Result<Vec<SearchResultWithMeta>, String> { ... }
   ```
4. 注册所有命令到 `tauri::Builder`
5. 添加错误处理和日志

**Files to create/modify:**
- `src-tauri/src/app_state.rs`（AppState 定义）
- `src-tauri/src/lib.rs`（初始化、注册命令）

**Verification:**
- [ ] `ingest_text` 端到端流程正常（文本 → 向量 → 索引 + 元数据）
- [ ] `search_knowledge` 返回带元数据的检索结果
- [ ] 所有 Tauri 命令通过 `invoke()` 可从前端调用
- [ ] 无死锁（Mutex 使用正确）

**Dependencies:** Task 3, Task 4, Task 5, Task 6

---

## Task 8: 余弦相似度验证（≥ 0.7）

**Goal:** 用真实中文 ERP 样本验证 bge-small-zh-v1.5 的语义相似度达到 ≥ 0.7 标准。

**Steps:**
1. 创建 `src-tauri/tests/semantic_similarity.rs` 测试文件
2. 准备测试用例：
   ```rust
   #[test]
   fn test_chinese_semantic_similarity() {
       let model = init_model();
       
       // 语义相近对（应 ≥ 0.7）
       let similar_pairs = vec![
           ("金蝶云星空如何配置期货点价", "金蝶苍穹期货点价模块设置"),
           ("客户要做二开怎么处理", "客户需要二次开发该如何操作"),
           ("PCR审批流程配置", "PCR审批流程设置方法"),
           ("物料主数据维护", "物料基础信息管理"),
       ];
       
       // 语义不相关对（应 < 0.5）
       let different_pairs = vec![
           ("金蝶云星空如何配置期货点价", "今天天气真好"),
           ("PCR审批流程配置", "我喜欢吃火锅"),
       ];
       
       for (a, b) in similar_pairs {
           let sim = compute_similarity(model, a, b);
           assert!(sim >= 0.7, "相似对 ({}, {}) 余弦相似度 {} < 0.7", a, b, sim);
       }
       
       for (a, b) in different_pairs {
           let sim = compute_similarity(model, a, b);
           assert!(sim < 0.5, "不相关对 ({}, {}) 余弦相似度 {} >= 0.5", a, b, sim);
       }
   }
   ```
3. 运行测试并记录所有相似度数值
4. 如果某些对未达标，调整测试用例或记录已知限制

**Files to create:**
- `src-tauri/tests/semantic_similarity.rs`

**Verification:**
- [ ] 所有语义相近对余弦相似度 ≥ 0.7
- [ ] 所有语义不相关对余弦相似度 < 0.5
- [ ] 测试用例覆盖金蝶 ERP 专业术语（期货点价、二开、PCR 等）

**Dependencies:** Task 4

---

## Task 9: 索引持久化与重启验证

**Goal:** 验证 usearch HNSW 索引在应用重启后数据完整、检索结果一致。

**Steps:**
1. 创建 `src-tauri/tests/persistence.rs` 测试文件
2. 编写持久化测试：
   ```rust
   #[test]
   fn test_index_persistence_roundtrip() {
       let tmp = tempfile::tempdir().unwrap();
       let index_path = tmp.path().join("test.usearch");
       let db_path = tmp.path().join("test.db");
       
       // Phase A: 写入数据
       {
           let index = VectorIndex::new(index_path.clone()).unwrap();
           let db = MetadataStore::new(db_path.clone()).unwrap();
           
           let texts = vec!["金蝶云星空", "期货点价", "二开配置"];
           let embeddings = embed_batch(&texts);
           
           for (i, emb) in embeddings.iter().enumerate() {
               index.add(i as u64, emb).unwrap();
               db.insert_chunk(i as u64, &texts[i]).unwrap();
           }
           
           index.save().unwrap();
       }
       
       // Phase B: 重新加载并验证
       {
           let index = VectorIndex::load(index_path).unwrap();
           let db = MetadataStore::new(db_path).unwrap();
           
           assert_eq!(index.len(), 3);
           
           let query = embed_text("金蝶苍穹");
           let results = index.search(&query, 3).unwrap();
           assert!(!results.is_empty());
           
           // 验证元数据可关联
           for r in &results {
               let chunk = db.get_chunk_by_vector_key(r.key).unwrap();
               assert!(chunk.is_some());
           }
       }
   }
   ```
3. 运行测试验证数据完整性
4. 手动测试：启动应用 → 导入数据 → 关闭 → 重新启动 → 检索验证

**Files to create:**
- `src-tauri/tests/persistence.rs`

**Verification:**
- [ ] 索引保存到磁盘后文件存在
- [ ] 索引从磁盘加载后向量数量一致
- [ ] 重启后检索结果与重启前一致
- [ ] 元数据数据库 chunk↔vector 映射完整

**Dependencies:** Task 5, Task 6

---

## Execution Order

```
Task 1 (SPIKE) — 必须首先执行，验证可行性
  └── Task 2 (Cargo.toml 依赖) — Spike 通过后正式添加
        ├── Task 3 (ModelManager) → Task 4 (EmbeddingService) → Task 8 (相似度验证)
        ├── Task 5 (VectorIndex)
        └── Task 6 (MetadataStore) — 可与 Task 5 并行
              └── Task 7 (AppState 集成) — 依赖 Task 3-6
                    └── Task 9 (持久化验证) — 依赖 Task 5, Task 6
```

**并行执行机会：**
- Task 5（VectorIndex）和 Task 6（MetadataStore）可并行开发
- Task 8（相似度验证）可在 Task 4 完成后立即执行

---

## Risk Mitigation

| 风险 | 缓解措施 |
|------|----------|
| bge-small-zh-v1.5 模型下载失败（网络问题） | fastembed-rs 内置重试机制；可手动下载模型文件到 `~/.kingdee-kb/models/` |
| ONNX Runtime 在 Windows 上编译失败 | fastembed-rs `ort-download-binaries-native-tls` feature 自动下载预编译二进制 |
| usearch 索引保存/加载后数据损坏 | Task 9 专门验证持久化；添加 `PRAGMA integrity_check` 到 rusqlite |
| Mutex 死锁（AppState 多服务并发访问） | 保持锁粒度最小化；Tauri command 为 async，锁持有时间短 |
| 模型推理性能不足（CPU） | bge-small-zh 仅 33M 参数，CPU 推理约 200-300 句/秒，满足桌面场景 |
| rusqlite WAL 模式在 Windows 上兼容性 | rusqlite `bundled` feature 自带最新 SQLite，WAL 在本地磁盘完全可靠 |

---

## File Summary

| 文件 | 操作 | 说明 |
|------|------|------|
| `src-tauri/Cargo.toml` | 修改 | 添加 fastembed、usearch、rusqlite 依赖 |
| `src-tauri/src/services/mod.rs` | 新建 | 服务模块声明 |
| `src-tauri/src/services/embedding.rs` | 新建 | ModelManager + EmbeddingService |
| `src-tauri/src/services/vector_index.rs` | 新建 | VectorIndex（usearch HNSW 封装） |
| `src-tauri/src/services/metadata.rs` | 新建 | MetadataStore（rusqlite 封装） |
| `src-tauri/src/app_state.rs` | 新建 | AppState 统一管理 |
| `src-tauri/src/lib.rs` | 修改 | 注册服务、Tauri 命令 |
| `src-tauri/tests/spike_embedding.rs` | 新建 | Spike 测试（Task 1，可删除） |
| `src-tauri/tests/semantic_similarity.rs` | 新建 | 语义相似度验证测试 |
| `src-tauri/tests/persistence.rs` | 新建 | 持久化验证测试 |

---

*Plan created: 2026-05-23*
*Next: `/gsd-execute-phase 2`*
