//! 知识图谱存储（knowledge_graph 表）
//!
//! ⚠️ 实验性能力 — 当前为独立功能，不依赖也不影响主流程。
//! 后续若需作为主搜索链路依赖，需先评估稳定性和性能。
//!

// 管理页面间的关联关系（wikilink、tag、source、co_citation 等信号）。
// 提供图构建、递归遍历、邻居查询和统计功能。

use std::collections::HashMap;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 知识图谱边记录
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub id: i64,
    pub project_id: i64,
    pub source_slug: String,
    pub target_slug: String,
    pub signal: String,
    pub weight: f64,
    pub created_at: String,
}

/// 图遍历路径结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphPath {
    pub target_slug: String,
    pub target_title: String,
    pub depth: i64,
    pub signals: Vec<String>,
    pub combined_weight: f64,
}

/// 图邻居结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNeighbor {
    pub slug: String,
    pub title: String,
    pub signal: String,
    pub weight: f64,
}

/// 图扩展检索推荐结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRecommendation {
    /// 推荐页面的 slug
    pub slug: String,
    /// 推荐页面的标题
    pub title: String,
    /// 页面类型（blueprint / fitgap / research 等）
    pub page_type: String,
    /// 组合权重（多信号平均值）
    pub combined_weight: f64,
    /// 最小跳数（越小越近）
    pub depth: i64,
    /// 关联路径说明，如 ["通过 wikilink 关联", "通过 tag 共现关联"]
    pub paths: Vec<String>,
    /// 命中的信号类型列表
    pub matched_signals: Vec<String>,
}

/// 全图节点（用于前端可视化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullGraphNode {
    pub slug: String,
    pub title: String,
    pub page_type: String,
    pub degree: i64,
}

/// 全图边（用于前端可视化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullGraphEdge {
    pub source: String,
    pub target: String,
    pub signal: String,
    pub weight: f64,
}

/// 全图数据（节点 + 边）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullGraph {
    pub nodes: Vec<FullGraphNode>,
    pub edges: Vec<FullGraphEdge>,
}

/// 图统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub total_edges: i64,
    pub total_nodes: i64,
    pub signal_breakdown: HashMap<String, i64>,
     pub avg_degree: f64,
 }

/// 知识图谱数据操作层
pub struct GraphStore {
    db: Connection,
}

impl GraphStore {
    /// 使用已有的数据库连接创建存储
    pub fn new(db: Connection) -> Self {
        let _ = db.busy_timeout(std::time::Duration::from_secs(5));
        Self { db }
    }

    /// 创建 knowledge_graph 表及其索引（幂等）。
    ///
    /// 单一 schema：`project_id INTEGER REFERENCES projects(id)`。项目尚未发布，
    /// 不存在老 schema 兼容问题，迁移函数一律不写。
    pub fn ensure_table(&self) -> Result<(), String> {
        self.db
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS knowledge_graph (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                    source_slug TEXT NOT NULL,
                    target_slug TEXT NOT NULL,
                    signal      TEXT NOT NULL CHECK(signal IN ('wikilink','tag','source','co_citation')),
                    weight      REAL NOT NULL DEFAULT 1.0,
                    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                    UNIQUE(project_id, source_slug, target_slug, signal)
                );
                CREATE INDEX IF NOT EXISTS idx_kg_source ON knowledge_graph(project_id, source_slug);
                CREATE INDEX IF NOT EXISTS idx_kg_target ON knowledge_graph(project_id, target_slug);
                ",
            )
            .map_err(|e| format!("创建 knowledge_graph 表失败: {}", e))?;

        Ok(())
    }

    // ─── 图构建 ───

    /// 构建/重建知识图谱。先清空项目旧数据，再从 wiki_pages 提取 4 信号写入边。
    /// 返回插入的边数。
    ///
    /// 整个 4 信号构建过程在单个事务内完成，保证原子性——要么全部成功，要么全部回滚。
    /// **Backfill 拆出事务**（自愈逻辑已迁移到 [`backfill_empty_wikilinks`](Self::backfill_empty_wikilinks)），
    /// 由调用方单独调用，避免长事务阻塞 wiki_pages 并发写入。
    pub fn build_knowledge_graph(&self, project_id: i64) -> Result<usize, String> {
        // 外层事务：保证 4 信号的构建原子性
        self.db
            .execute_batch("BEGIN IMMEDIATE TRANSACTION")
            .map_err(|e| format!("开始图谱构建事务失败: {}", e))?;

        let result = self.build_knowledge_graph_inner(project_id);

        match result {
            Ok(count) => {
                self.db
                    .execute_batch("COMMIT")
                    .map_err(|e| format!("提交图谱构建事务失败: {}", e))?;
                Ok(count)
            }
            Err(e) => {
                let _ = self.db.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// 图谱构建内部逻辑（由外层事务保护，**不**包含 backfill）
    fn build_knowledge_graph_inner(&self, project_id: i64) -> Result<usize, String> {
        // 0. Backfill 已拆出事务 —— 调用方应在 build_knowledge_graph 之前先调用
        //    backfill_empty_wikilinks(...)，避免大项目下长事务阻塞 wiki_pages 写入

        // 1. 清空该项目旧图数据
        self.db
            .execute(
                "DELETE FROM knowledge_graph WHERE project_id = ?1",
                params![project_id],
            )
            .map_err(|e| format!("清空旧图数据失败: {}", e))?;

        let mut total_inserted: usize = 0;

        // 2. S1: wikilink 信号（weight=1.0）
        total_inserted += self.build_signal_wikilink(project_id)?;

        // 3. S2: tag 共现信号（weight=0.6）
        total_inserted += self.build_signal_tag(project_id)?;

        // 4. S3: source 共源信号（weight=0.4）
        total_inserted += self.build_signal_source(project_id)?;

        // 5. S4: co_citation 共引信号（weight=0.3）
        total_inserted += self.build_signal_co_citation(project_id)?;

        Ok(total_inserted)
    }

    /// S1: 从 wiki_pages.wikilinks JSON 数组提取 wikilink 边
    fn build_signal_wikilink(&self, project_id: i64) -> Result<usize, String> {
        let mut stmt = self
            .db
            .prepare("SELECT slug, wikilinks FROM wiki_pages WHERE project_id = ?1")
            .map_err(|e| format!("准备 wikilink 查询失败: {}", e))?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("执行 wikilink 查询失败: {}", e))?;

        let mut count: usize = 0;
        self.db
            .execute_batch("SAVEPOINT sp_wikilink")
            .map_err(|e| format!("创建 wikilink 保存点失败: {}", e))?;

        let mut insert_err: Option<String> = None;
        for row in rows {
            let (slug, wikilinks_json) = row.map_err(|e| format!("读取 wikilink 行失败: {}", e))?;
            let targets = parse_string_array("wikilinks", &wikilinks_json)?;
            for target in targets {
                if target.is_empty() || target == slug {
                    continue;
                }
                let res = self.db.execute(
                    "INSERT OR IGNORE INTO knowledge_graph (project_id, source_slug, target_slug, signal, weight)
                     VALUES (?1, ?2, ?3, 'wikilink', 1.0)",
                    params![project_id, slug, target],
                );
                match res {
                    Ok(rows_affected) => {
                        if rows_affected > 0 {
                            count += 1;
                        }
                    }
                    Err(e) => {
                        insert_err = Some(format!("插入 wikilink 边失败: {}", e));
                        break;
                    }
                }
            }
            if insert_err.is_some() {
                break;
            }
        }

        if let Some(e) = insert_err {
            let _ = self.db.execute_batch("ROLLBACK TO sp_wikilink");
            return Err(e);
        }
        self.db
            .execute_batch("RELEASE sp_wikilink")
            .map_err(|e| format!("释放 wikilink 保存点失败: {}", e))?;
        Ok(count)
    }

    /// Backfill: 修复历史空 wikilinks
    ///
    /// 扫描项目下所有 `wikilinks = '[]'` 的页面，从其 `content_candidate`（优先）或 `content`
    /// 重新提取 `[[slug]]` 引用并写入 `wikilinks`。
    ///
    /// 行为：
    /// - 只回填"完全空"的页面（`wikilinks = '[]'`），避免覆盖用户已手动设置的链接
    /// - 提取的 slug 必须存在于项目当前 slugs 中（防御 LLM 幻觉）
    /// - **不在 `build_knowledge_graph` 事务内**：自愈逻辑已拆出，由调用方单独调用，
    ///   避免大项目下长事务阻塞 wiki_pages 并发写入
    /// - 返回回填的页面数
    ///
    /// 调用方应在 `build_knowledge_graph` 之前先调用此方法。
    pub fn backfill_empty_wikilinks(&self, project_id: i64) -> Result<usize, String> {
        // 查询项目所有 (slug) 用于过滤
        let valid_slugs: std::collections::HashSet<String> = {
            let mut stmt = self
                .db
                .prepare("SELECT slug FROM wiki_pages WHERE project_id = ?1")
                .map_err(|e| format!("准备 slug 查询失败: {}", e))?;
            let rows = stmt
                .query_map(params![project_id], |row| row.get::<_, String>(0))
                .map_err(|e| format!("执行 slug 查询失败: {}", e))?;
            let mut set = std::collections::HashSet::new();
            for r in rows {
                set.insert(r.map_err(|e| format!("读取 slug 失败: {}", e))?);
            }
            set
        };

        // 找出 wikilinks 为空但 content_candidate 或 content 非空的页面
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, slug, content, content_candidate
                 FROM wiki_pages
                 WHERE project_id = ?1 AND wikilinks = '[]'
                   AND (content_candidate IS NOT NULL AND content_candidate != '' OR
                        content IS NOT NULL AND content != '')",
            )
            .map_err(|e| format!("准备 backfill 查询失败: {}", e))?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(|e| format!("执行 backfill 查询失败: {}", e))?;

        // 收集需要更新的页面（避免在迭代中修改同一连接）
        let mut to_update: Vec<(i64, String)> = Vec::new();
        for row in rows {
            let (id, slug, content, content_candidate) =
                row.map_err(|e| format!("读取 backfill 行失败: {}", e))?;
            // 优先用 content_candidate（最新 LLM 输出），否则用 content
            let source_text = content_candidate
                .filter(|s| !s.is_empty())
                .or(content)
                .unwrap_or_default();
            if source_text.is_empty() {
                continue;
            }
            // 提取 [[slug]]，并过滤掉自引用和无效 slug
            let links = crate::services::wikilink_parser::extract_wikilinks(
                &source_text,
                &slug,
                &valid_slugs,
            );
            if links.is_empty() {
                continue;
            }
            to_update.push((id, serde_json::to_string(&links)
                .map_err(|e| format!("序列化 wikilinks 失败: {}", e))?));
        }

        // 批量更新
        for (id, wikilinks_json) in &to_update {
            self.db
                .execute(
                    "UPDATE wiki_pages SET wikilinks = ?1, updated_at = datetime('now') WHERE id = ?2",
                    params![wikilinks_json, id],
                )
                .map_err(|e| format!("backfill 更新 wikilinks 失败: {}", e))?;
        }
        Ok(to_update.len())
    }

    /// S2: tag 共现信号。共享同一 tag 的页面两两关联（weight=0.6）
    fn build_signal_tag(&self, project_id: i64) -> Result<usize, String> {
        let tag_map = self.collect_field_map(project_id, "tags")?;
        let pairs = Self::generate_co_occurrence_pairs(&tag_map);

        self.db
            .execute_batch("SAVEPOINT sp_tag")
            .map_err(|e| format!("创建 tag 保存点失败: {}", e))?;
        let mut count: usize = 0;
        for (a, b) in &pairs {
            let res = self.db.execute(
                "INSERT OR IGNORE INTO knowledge_graph (project_id, source_slug, target_slug, signal, weight)
                 VALUES (?1, ?2, ?3, 'tag', 0.6)",
                params![project_id, a, b],
            );
            match res {
                Ok(rows_affected) => {
                    if rows_affected > 0 {
                        count += 1;
                    }
                }
                Err(e) => {
                    let _ = self.db.execute_batch("ROLLBACK TO sp_tag");
                    return Err(format!("插入 tag 共现边失败: {}", e));
                }
            }
        }
        self.db
            .execute_batch("RELEASE sp_tag")
            .map_err(|e| format!("释放 tag 保存点失败: {}", e))?;
        Ok(count)
    }

    /// S3: source 共源信号。引用同一 raw_source 的页面两两关联（weight=0.4）
    fn build_signal_source(&self, project_id: i64) -> Result<usize, String> {
        let source_map = self.collect_sources_map(project_id)?;
        let pairs = Self::generate_co_occurrence_pairs(&source_map);

        self.db
            .execute_batch("SAVEPOINT sp_source")
            .map_err(|e| format!("创建 source 保存点失败: {}", e))?;
        let mut count: usize = 0;
        for (a, b) in &pairs {
            let res = self.db.execute(
                "INSERT OR IGNORE INTO knowledge_graph (project_id, source_slug, target_slug, signal, weight)
                 VALUES (?1, ?2, ?3, 'source', 0.4)",
                params![project_id, a, b],
            );
            match res {
                Ok(rows_affected) => {
                    if rows_affected > 0 {
                        count += 1;
                    }
                }
                Err(e) => {
                    let _ = self.db.execute_batch("ROLLBACK TO sp_source");
                    return Err(format!("插入 source 共源边失败: {}", e));
                }
            }
        }
        self.db
            .execute_batch("RELEASE sp_source")
            .map_err(|e| format!("释放 source 保存点失败: {}", e))?;
        Ok(count)
    }

    /// S4: co_citation 共引信号。被同一组页面引用的来源页面两两关联（weight=0.3）
    fn build_signal_co_citation(&self, project_id: i64) -> Result<usize, String> {
        // 找到被多个页面引用的目标页面，以及引用它们的来源页面列表
        let mut stmt = self
            .db
            .prepare(
                "SELECT source_slug, target_slug FROM knowledge_graph
                 WHERE project_id = ?1 AND signal = 'wikilink'",
            )
            .map_err(|e| format!("准备 co_citation 查询失败: {}", e))?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("执行 co_citation 查询失败: {}", e))?;

        // target_slug → [source_slug, ...]
        let mut citation_map: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let (source, target) = row.map_err(|e| format!("读取 co_citation 行失败: {}", e))?;
            citation_map.entry(target).or_default().push(source);
        }

        // 为被 2+ 页面引用的目标，生成引用者之间的共引边
        let mut pairs: Vec<(String, String)> = Vec::new();
        for sources in citation_map.values() {
            if sources.len() < 2 {
                continue;
            }
            for i in 0..sources.len() {
                for j in (i + 1)..sources.len() {
                    let (a, b) = if sources[i] < sources[j] {
                        (&sources[i], &sources[j])
                    } else {
                        (&sources[j], &sources[i])
                    };
                    pairs.push((a.clone(), b.clone()));
                }
            }
        }

        self.db
            .execute_batch("SAVEPOINT sp_co_citation")
            .map_err(|e| format!("创建 co_citation 保存点失败: {}", e))?;
        let mut count: usize = 0;
        for (a, b) in &pairs {
            let res = self.db.execute(
                "INSERT OR IGNORE INTO knowledge_graph (project_id, source_slug, target_slug, signal, weight)
                 VALUES (?1, ?2, ?3, 'co_citation', 0.3)",
                params![project_id, a, b],
            );
            match res {
                Ok(rows_affected) => {
                    if rows_affected > 0 {
                        count += 1;
                    }
                }
                Err(e) => {
                    let _ = self.db.execute_batch("ROLLBACK TO sp_co_citation");
                    return Err(format!("插入 co_citation 边失败: {}", e));
                }
            }
        }
        self.db
            .execute_batch("RELEASE sp_co_citation")
            .map_err(|e| format!("释放 co_citation 保存点失败: {}", e))?;
        Ok(count)
    }

    // ─── 图查询 ───

    /// 递归图遍历（SQLite CTE）。从 seed 页面出发，沿边展开 N 层。
    pub fn traverse_graph(
        &self,
        project_id: i64,
        seed_slug: &str,
        max_depth: i64,
        min_weight: f64,
    ) -> Result<Vec<GraphPath>, String> {
        let mut stmt = self
            .db
            .prepare(
                "WITH RECURSIVE graph_walk AS (
                    SELECT target_slug, 1 AS depth, signal, weight
                    FROM knowledge_graph
                    WHERE project_id = ?1 AND source_slug = ?2 AND weight >= ?3
                    UNION
                    SELECT kg.target_slug, gw.depth + 1, kg.signal, kg.weight
                    FROM graph_walk gw
                    JOIN knowledge_graph kg ON kg.source_slug = gw.target_slug
                        AND kg.project_id = ?1 AND kg.weight >= ?3
                    WHERE gw.depth < ?4
                )
                SELECT target_slug, MAX(depth) as depth,
                       GROUP_CONCAT(DISTINCT signal) as signals,
                       AVG(weight) as avg_weight
                FROM graph_walk
                GROUP BY target_slug
                ORDER BY avg_weight DESC",
            )
            .map_err(|e| format!("准备遍历查询失败: {}", e))?;

        let rows = stmt
            .query_map(
                params![project_id, seed_slug, min_weight, max_depth],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, f64>(3)?,
                    ))
                },
            )
            .map_err(|e| format!("执行遍历查询失败: {}", e))?;

        let mut results: Vec<GraphPath> = Vec::new();
        let mut target_slugs: Vec<String> = Vec::new();
        let mut raw_data: Vec<(String, i64, String, f64)> = Vec::new();

        for row in rows {
            let (slug, depth, signals_str, avg_weight) =
                row.map_err(|e| format!("读取遍历行失败: {}", e))?;
            target_slugs.push(slug.clone());
            raw_data.push((slug, depth, signals_str, avg_weight));
        }

        // 批量查询目标页面标题
        let title_map = self.get_title_map(project_id, &target_slugs)?;

        for (slug, depth, signals_str, avg_weight) in raw_data {
            let title = title_map.get(&slug).cloned().unwrap_or_default();
            let signals: Vec<String> = signals_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            results.push(GraphPath {
                target_slug: slug,
                target_title: title,
                depth,
                signals,
                combined_weight: avg_weight,
            });
        }

        Ok(results)
    }

    /// 获取某页面的直接邻居（1 跳）
    pub fn get_neighbors(&self, project_id: i64, slug: &str) -> Result<Vec<GraphNeighbor>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT target_slug, signal, weight
                 FROM knowledge_graph
                  WHERE project_id = ?1 AND source_slug = ?2",
            )
            .map_err(|e| format!("准备邻居查询失败: {}", e))?;

        let rows = stmt
            .query_map(params![project_id, slug], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(|e| format!("执行邻居查询失败: {}", e))?;

        let mut raw_data: Vec<(String, String, f64)> = Vec::new();
        let mut slugs: Vec<String> = Vec::new();
        for row in rows {
            let (s, signal, weight) = row.map_err(|e| format!("读取邻居行失败: {}", e))?;
            slugs.push(s.clone());
            raw_data.push((s, signal, weight));
        }

        let title_map = self.get_title_map(project_id, &slugs)?;

        let mut results = Vec::new();
        for (s, signal, weight) in raw_data {
            let title = title_map.get(&s).cloned().unwrap_or_default();
            results.push(GraphNeighbor {
                slug: s,
                title,
                signal,
                weight,
            });
        }

        Ok(results)
    }

    /// 获取图统计信息
    pub fn get_graph_stats(&self, project_id: i64) -> Result<GraphStats, String> {
        // 总边数
        let total_edges: i64 = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM knowledge_graph WHERE project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("查询总边数失败: {}", e))?;

        // 总节点数（source 和 target 的并集）
        let total_nodes: i64 = self
            .db
            .query_row(
                "SELECT COUNT(DISTINCT slug) FROM (
                    SELECT source_slug AS slug FROM knowledge_graph WHERE project_id = ?1
                    UNION
                    SELECT target_slug AS slug FROM knowledge_graph WHERE project_id = ?1
                 )",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("查询总节点数失败: {}", e))?;

        // 按信号类型统计
        let mut stmt = self
            .db
            .prepare(
                "SELECT signal, COUNT(*) FROM knowledge_graph
                 WHERE project_id = ?1 GROUP BY signal",
            )
            .map_err(|e| format!("准备信号统计查询失败: {}", e))?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| format!("执行信号统计查询失败: {}", e))?;

        let mut signal_breakdown = HashMap::new();
        for row in rows {
            let (signal, count) = row.map_err(|e| format!("读取信号统计行失败: {}", e))?;
            signal_breakdown.insert(signal, count);
        }

        // 平均度数 = 总边数 / 总节点数
        let avg_degree = if total_nodes > 0 {
            total_edges as f64 / total_nodes as f64
        } else {
            0.0
        };

        Ok(GraphStats {
            total_edges,
            total_nodes,
            signal_breakdown,
            avg_degree,
        })
    }

    /// 获取项目完整图数据（所有节点和边），用于前端可视化。
    ///
    /// 限制最多 5000 条边，防止超大项目内存爆炸。
    pub fn get_full_graph(&self, project_id: i64) -> Result<FullGraph, String> {
        // 1. 获取所有边（LIMIT 5000 防止超大项目）
        let mut stmt = self
            .db
            .prepare(
                "SELECT source_slug, target_slug, signal, weight
                 FROM knowledge_graph
                 WHERE project_id = ?1
                 LIMIT 5000",
            )
            .map_err(|e| format!("准备全图边查询失败: {}", e))?;

        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(FullGraphEdge {
                    source: row.get(0)?,
                    target: row.get(1)?,
                    signal: row.get(2)?,
                    weight: row.get(3)?,
                })
            })
            .map_err(|e| format!("执行全图边查询失败: {}", e))?;

        let mut edges: Vec<FullGraphEdge> = Vec::new();
        let mut all_slugs: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut degree_map: HashMap<String, i64> = HashMap::new();

        for row in rows {
            let edge = row.map_err(|e| format!("读取全图边行失败: {}", e))?;
            all_slugs.insert(edge.source.clone());
            all_slugs.insert(edge.target.clone());
            *degree_map.entry(edge.source.clone()).or_insert(0) += 1;
            *degree_map.entry(edge.target.clone()).or_insert(0) += 1;
            edges.push(edge);
        }

        // 2. 获取节点标题和类型
        let slug_list: Vec<String> = all_slugs.into_iter().collect();
        let page_info = self.get_page_info_map(project_id, &slug_list)?;

        let nodes: Vec<FullGraphNode> = slug_list
            .iter()
            .map(|slug| {
                let (title, page_type) = page_info
                    .get(slug)
                    .cloned()
                    .unwrap_or_else(|| (slug.clone(), "unknown".to_string()));
                FullGraphNode {
                    slug: slug.clone(),
                    title,
                    page_type,
                    degree: degree_map.get(slug).copied().unwrap_or(0),
                }
            })
            .collect();

        Ok(FullGraph { nodes, edges })
    }


    /// 图扩展检索：给定页面，推荐相关页面。
    ///
    /// 使用 `traverse_graph` 获取多跳邻居，按组合权重排序，去重，返回 top K。
    ///
    /// - `project_id` — 项目 ID
    /// - `slug` — 起始页面 slug
    /// - `max_depth` — 最大跳数（默认 2）
    /// - `max_results` — 最大返回数（默认 10）
    /// - `min_weight` — 最低权重阈值（默认 0.3）
    pub fn graph_expand_search(
        &self,
        project_id: i64,
        slug: &str,
        max_depth: i64,
        max_results: i64,
        min_weight: f64,
    ) -> Result<Vec<GraphRecommendation>, String> {
        // 1. 递归遍历获取多跳邻居
        let paths = self.traverse_graph(project_id, slug, max_depth, min_weight)?;

        if paths.is_empty() {
            return Ok(Vec::new());
        }

        // 2. 收集所有目标 slug，批量查询页面详情（title + page_type）
        let target_slugs: Vec<String> = paths.iter().map(|p| p.target_slug.clone()).collect();
        let page_info_map = self.get_page_info_map(project_id, &target_slugs)?;

        // 3. 按 slug 分组：取最小 depth、合并信号、平均权重
        use std::collections::HashMap;
        let mut grouped: HashMap<String, GraphRecommendation> = HashMap::new();

        for path in &paths {
            let entry = grouped.entry(path.target_slug.clone()).or_insert_with(|| {
                let info = page_info_map.get(&path.target_slug);
                GraphRecommendation {
                    slug: path.target_slug.clone(),
                    title: info.map(|(t, _)| t.clone()).unwrap_or_default(),
                    page_type: info.map(|(_, pt)| pt.clone()).unwrap_or_default(),
                    combined_weight: 0.0,
                    depth: path.depth,
                    paths: Vec::new(),
                    matched_signals: Vec::new(),
                }
            });

            // 取最小跳数（越近越相关）
            if path.depth < entry.depth {
                entry.depth = path.depth;
            }

            // 累加权重（后续取平均）
            entry.combined_weight += path.combined_weight;

            // 合并信号
            for signal in &path.signals {
                if !entry.matched_signals.contains(signal) {
                    entry.matched_signals.push(signal.clone());
                    // 生成关联路径说明
                    let path_desc = match signal.as_str() {
                        "wikilink" => "通过 wikilink 关联",
                        "tag" => "通过 tag 共现关联",
                        "source" => "通过 source 共源关联",
                        "co_citation" => "通过共引关系关联",
                        _ => "通过其他信号关联",
                    };
                    entry.paths.push(path_desc.to_string());
                }
            }
        }

        // 4. 计算平均权重，排序
        let mut results: Vec<GraphRecommendation> = grouped
            .into_values()
            .map(|mut r| {
                // 用信号数量做归一化：多信号加成
                let signal_count = r.matched_signals.len().max(1) as f64;
                r.combined_weight /= signal_count;
                // 多信号加成：每多一个信号 +10%（上限 1.0）
                let bonus = 1.0 + (signal_count - 1.0) * 0.1;
                r.combined_weight = (r.combined_weight * bonus).min(1.0);
                r
            })
            .collect();

        // 按 combined_weight 降序，depth 升序
        results.sort_by(|a, b| {
            b.combined_weight
                .partial_cmp(&a.combined_weight)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.depth.cmp(&b.depth))
        });

        // 5. 截断到 max_results
        results.truncate(max_results as usize);

        Ok(results)
    }

    // ─── 私有辅助方法 ───

    /// 收集项目的某个 JSON 数组字段，返回 value → [slug, ...] 映射
    fn collect_field_map(
        &self,
        project_id: i64,
        field: &str,
    ) -> Result<HashMap<String, Vec<String>>, String> {
        // 安全校验：只允许预期的字段名，防止 SQL 注入
        if !["tags", "sources", "wikilinks"].contains(&field) {
            return Err(format!("非法的字段名: {}", field));
        }
        let sql = format!(
            "SELECT slug, {} FROM wiki_pages WHERE project_id = ?1",
            field
        );
        let mut stmt = self
            .db
            .prepare(&sql)
            .map_err(|e| format!("准备 {} 查询失败: {}", field, e))?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("执行 {} 查询失败: {}", field, e))?;

        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let (slug, json_str) = row.map_err(|e| format!("读取 {} 行失败: {}", field, e))?;
            let values = parse_string_array(field, &json_str)?;
            for val in values {
                if !val.is_empty() {
                    map.entry(val).or_default().push(slug.clone());
                }
            }
        }
        Ok(map)
    }

    /// 收集项目的 sources 对象数组，返回 source_key → [slug, ...] 映射
    fn collect_sources_map(&self, project_id: i64) -> Result<HashMap<String, Vec<String>>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT slug, sources FROM wiki_pages WHERE project_id = ?1")
            .map_err(|e| format!("准备 sources 查询失败: {}", e))?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("执行 sources 查询失败: {}", e))?;

        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let (slug, json_str) = row.map_err(|e| format!("读取 sources 行失败: {}", e))?;
            let source_keys = parse_source_keys(&json_str)?;
            for key in source_keys {
                map.entry(key).or_default().push(slug.clone());
            }
        }
        Ok(map)
    }

    /// 从共现映射生成去重的页面对（source < target 排序）。
    /// 每组最多 `MAX_CO_OCCURRENCE_GROUP_SIZE` 个页面参与配对，防止 O(n²) 爆炸。
    fn generate_co_occurrence_pairs(map: &HashMap<String, Vec<String>>) -> Vec<(String, String)> {
        /// 单组最多参与的页面数量，超出时截断取前 N 个
        const MAX_CO_OCCURRENCE_GROUP_SIZE: usize = 50;

        let mut pairs = Vec::new();
        for slugs in map.values() {
            if slugs.len() < 2 {
                continue;
            }
            // 截断超大组，只取前 MAX 个页面参与配对
            let group: Vec<&String> = slugs.iter().take(MAX_CO_OCCURRENCE_GROUP_SIZE).collect();
            for i in 0..group.len() {
                for j in (i + 1)..group.len() {
                    let (a, b) = if group[i] < group[j] {
                        (group[i], group[j])
                    } else {
                        (group[j], group[i])
                    };
                    pairs.push((a.clone(), b.clone()));
                }
            }
        }
        pairs.sort();
        pairs.dedup();
        pairs
    }

    /// 批量查询 slug → title 映射
    fn get_title_map(
        &self,
        project_id: i64,
        slugs: &[String],
    ) -> Result<HashMap<String, String>, String> {
        let mut map = HashMap::new();
        if slugs.is_empty() {
            return Ok(map);
        }
        let placeholders: Vec<String> =
            (2..=(slugs.len() + 1)).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT slug, title FROM wiki_pages WHERE project_id = ?1 AND slug IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = self
            .db
            .prepare(&sql)
            .map_err(|e| format!("准备标题查询失败: {}", e))?;
        let mut params: Vec<&dyn rusqlite::types::ToSql> = vec![&project_id];
        params.extend(slugs.iter().map(|s| s as &dyn rusqlite::types::ToSql));
        let rows = stmt
            .query_map(params.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("执行标题查询失败: {}", e))?;
        for row in rows {
            let (slug, title) = row.map_err(|e| format!("读取标题行失败: {}", e))?;
            map.insert(slug, title);
        }
        Ok(map)
    }

    /// 批量查询 slug → (title, page_type) 映射
    fn get_page_info_map(
        &self,
        project_id: i64,
        slugs: &[String],
    ) -> Result<HashMap<String, (String, String)>, String> {
        let mut map = HashMap::new();
        if slugs.is_empty() {
            return Ok(map);
        }
        let placeholders: Vec<String> =
            (2..=(slugs.len() + 1)).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT slug, title, page_type FROM wiki_pages WHERE project_id = ?1 AND slug IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = self
            .db
            .prepare(&sql)
            .map_err(|e| format!("准备页面信息查询失败: {}", e))?;
        let mut params: Vec<&dyn rusqlite::types::ToSql> = vec![&project_id];
        params.extend(slugs.iter().map(|s| s as &dyn rusqlite::types::ToSql));
        let rows = stmt
            .query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| format!("执行页面信息查询失败: {}", e))?;
        for row in rows {
            let (slug, title, page_type) = row.map_err(|e| format!("读取页面信息行失败: {}", e))?;
            map.insert(slug, (title, page_type));
        }
        Ok(map)
    }
}

fn parse_string_array(field: &str, json_str: &str) -> Result<Vec<String>, String> {
    let value: Value =
        serde_json::from_str(json_str).map_err(|e| format!("解析 {} 字段失败: {}", field, e))?;
    let array = value
        .as_array()
        .ok_or_else(|| format!("{} 字段必须是 JSON 数组", field))?;

    let mut result = Vec::new();
    for item in array {
        let text = item
            .as_str()
            .ok_or_else(|| format!("{} 字段数组元素必须是字符串", field))?;
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            result.push(trimmed.to_string());
        }
    }
    Ok(result)
}

fn parse_source_keys(json_str: &str) -> Result<Vec<String>, String> {
    let value: Value =
        serde_json::from_str(json_str).map_err(|e| format!("解析 sources 字段失败: {}", e))?;
    let array = value
        .as_array()
        .ok_or_else(|| "sources 字段必须是 JSON 数组".to_string())?;

    let mut result = Vec::new();
    for item in array {
        let object = item
            .as_object()
            .ok_or_else(|| "sources 字段数组元素必须是对象".to_string())?;
        if let Some(source_id) = source_value_to_key("source_id", object.get("source_id")) {
            result.push(source_id);
            continue;
        }
        if let Some(document_id) = source_value_to_key("document_id", object.get("document_id")) {
            result.push(document_id);
        }
    }
    result.sort();
    result.dedup();
    Ok(result)
}

fn source_value_to_key(prefix: &str, value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::Number(n)) => Some(format!("{}:{}", prefix, n)),
        Some(Value::String(s)) if !s.trim().is_empty() => Some(format!("{}:{}", prefix, s.trim())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_string_array_rejects_yaml_like_tags() {
        assert!(parse_string_array("tags", "[ERP, 财务]").is_err());
    }

    #[test]
    fn parse_string_array_accepts_json_tags() {
        let values = parse_string_array("tags", r#"["ERP","财务"]"#).unwrap();
        assert_eq!(values, vec!["ERP".to_string(), "财务".to_string()]);
    }

    #[test]
    fn parse_source_keys_uses_document_id_when_source_id_is_null() {
        let keys =
            parse_source_keys(r#"[{"source_id":null,"document_id":42,"chunks":[]}]"#).unwrap();
        assert_eq!(keys, vec!["document_id:42".to_string()]);
    }

    #[test]
    fn parse_source_keys_prefers_source_id() {
        let keys = parse_source_keys(r#"[{"source_id":7,"document_id":42,"chunks":[]}]"#).unwrap();
        assert_eq!(keys, vec!["source_id:7".to_string()]);
    }

    /// 端到端：模拟用户报告的"0 边"场景，找出哪一步出问题
    ///
    /// 场景 A：页面 wikilinks 字段已经存了 `[[slug]]`，调用 build 应有 N 边
    /// 场景 B：页面 wikilinks = '[]' 但 content 里有 `[[slug]]`，backfill 应回填，再 build 应有边
    /// 场景 C：页面 wikilinks = '[]' 且 content 也没 `[[slug]]`，build 真的就是 0 边（数据问题，不是代码问题）
    #[test]
    fn end_to_end_diagnose_zero_edges() {
        use crate::services::wiki_page::{CreateWikiPageWithCandidate, WikiPageStore};
        use rusqlite::Connection;
        use std::env;

        // 用临时文件模拟"两个独立连接"（实际部署中 WikiPageStore 和 GraphStore 各自持有一个连接）
        let path = env::temp_dir().join(format!(
            "kingdee_kb_test_kg_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path_str = path.to_string_lossy().to_string();

        // 1) 初始化 schema（关外键：测试只验证 build 逻辑，不依赖 projects 表）
        {
            let conn = Connection::open(&path_str).unwrap();
            conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
            conn.execute_batch("CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
                .unwrap();
            conn.execute("INSERT INTO projects (id, name) VALUES (1, 'test')", [])
                .unwrap();
            let wiki = WikiPageStore::new(conn);
            wiki.ensure_table().unwrap();
        }

        // 2) 写入测试数据
        {
            let conn = Connection::open(&path_str).unwrap();

            // 场景 B：先把 page-c 的数据准备好（先做 conn 直写，再交给 store）
            // 直接写 page-c 的初始数据：用 SQL 而不是 create_with_candidate 避开顺序问题
            conn.execute(
                "INSERT INTO wiki_pages
                 (project_id, slug, title, page_type, content, content_candidate,
                  candidate_status, sources_candidate, frontmatter, sources,
                  wikilinks, tags, page_metadata, candidate_version, page_status, version)
                 VALUES
                 (1, 'page-c', 'Page C', 'summary', '请看 [[page-a]] 章节', NULL,
                  NULL, NULL, '{}', '[]', '[]', '[]', '{}', NULL, 'draft', 1)",
                [],
            )
            .unwrap();

            let wiki_store = WikiPageStore::new(conn);
            wiki_store
                .create_with_candidate(&CreateWikiPageWithCandidate {
                    project_id: 1,
                    slug: "page-a".into(),
                    title: "Page A".into(),
                    page_type: "summary".into(),
                    content_candidate: "见 [[page-b]] 与 [[page-c]]".into(),
                    candidate_status: "auto".into(),
                    sources_candidate: Some("[]".into()),
                    frontmatter: None,
                    sources: None,
                    wikilinks: Some(r#"["page-b","page-c"]"#.into()),
                    tags: None,
                    page_metadata: None,
                    page_status: None,
                })
                .unwrap();
            wiki_store
                .create_with_candidate(&CreateWikiPageWithCandidate {
                    project_id: 1,
                    slug: "page-b".into(),
                    title: "Page B".into(),
                    page_type: "summary".into(),
                    content_candidate: "回链到 [[page-a]]".into(),
                    candidate_status: "auto".into(),
                    sources_candidate: Some("[]".into()),
                    frontmatter: None,
                    sources: None,
                    wikilinks: Some(r#"["page-a"]"#.into()),
                    tags: None,
                    page_metadata: None,
                    page_status: None,
                })
                .unwrap();

            // 诊断：扫描页面状态
            let (total, non_empty, samples, has_brackets) =
                wiki_store.diagnose_wikilinks(1).unwrap();
            eprintln!(
                "[诊断 1] total={} non_empty={} has_brackets={}",
                total, non_empty, has_brackets
            );
            for s in &samples {
                eprintln!("    样例: {}", s);
            }
            assert_eq!(total, 3, "应有 3 个页面");
            assert_eq!(non_empty, 2, "page-a + page-b 应该有非空 wikilinks");
            assert!(has_brackets >= 1, "page-c 的 content 里有 [[page-a]]");
        }

        // 3) backfill + build：跑 GraphStore
        {
            let conn = Connection::open(&path_str).unwrap();
            let graph_store = GraphStore::new(conn);
            graph_store.ensure_table().unwrap();

            let backfilled = graph_store.backfill_empty_wikilinks(1).unwrap();
            eprintln!("[backfill] backfilled={} (期望 1: page-c)", backfilled);
            assert_eq!(backfilled, 1, "backfill 应该修复 page-c");

            // 验证 backfill 写回了 page-c 的 wikilinks
            {
                let conn = Connection::open(&path_str).unwrap();
                let wiki_store = WikiPageStore::new(conn);
                let (total, non_empty, _, _) = wiki_store.diagnose_wikilinks(1).unwrap();
                eprintln!(
                    "[诊断 2] backfill 后 total={} non_empty={} (期望 3/3)",
                    total, non_empty
                );
                assert_eq!(total, 3);
                assert_eq!(non_empty, 3, "backfill 后 3 个页面都应该有 wikilinks");
            }

            let edges = graph_store.build_knowledge_graph(1).unwrap();
            eprintln!("[build] edges={} (期望至少 4: a→b, a→c, b→a, c→a)", edges);
            // 期望 4 条 wikilink 边（page-a→page-b, page-a→page-c, page-b→page-a, page-c→page-a）
            // 还可能有 tag/source/co_citation 等额外边，所以用 >= 4
            assert!(edges >= 4, "期望至少 4 条 wikilink 边，实际 {}", edges);
        }

        // 4) 场景 C：清空 page-c 的 [[..]]，验证 0 边确实是数据导致
        {
            let conn = Connection::open(&path_str).unwrap();
            conn.execute(
                "UPDATE wiki_pages SET content = 'no links here', wikilinks = '[]'
                 WHERE slug = 'page-c'",
                [],
            )
            .unwrap();
            let graph_store = GraphStore::new(conn);
            let edges_c = graph_store.build_knowledge_graph(1).unwrap();
            eprintln!(
                "[场景 C] 边数={} (期望 2-3：a→b, b→a 仍在，c 失去引用)",
                edges_c
            );
            assert!(edges_c >= 2 && edges_c <= 3, "page-c 失去引用后应减 2 条边");
        }

        // 5) 清理临时文件
        let _ = std::fs::remove_file(&path_str);
    }

    /// 诊断用户真实数据库：跑 `KINGDEE_KB_DIAGNOSE=1 cargo test --lib diagnose_user_database -- --nocapture`
    ///
    /// 会打开 `~/.kingdee-kb/metadata.db`（覆盖用 `KINGDEE_KB_DB=...`），对每个 project_id
    /// 输出 wiki_pages 状态，并尝试 build_knowledge_graph 看实际边数
    #[test]
    fn diagnose_user_database() {
        if std::env::var("KINGDEE_KB_DIAGNOSE").is_err() {
            eprintln!(
                "跳过：设置 KINGDEE_KB_DIAGNOSE=1 才会真正连库。可用 KINGDEE_KB_DB 覆盖数据库路径。"
            );
            return;
        }
        use crate::services::wiki_page::WikiPageStore;
        use rusqlite::Connection;

        let db_path = std::env::var("KINGDEE_KB_DB").unwrap_or_else(|_| {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_default();
            format!("{}\\.kingdee-kb\\metadata.db", home)
        });
        eprintln!(">>> 打开数据库: {}", db_path);
        if !std::path::Path::new(&db_path).exists() {
            eprintln!("!!! 数据库文件不存在: {}", db_path);
            return;
        }

        // 用"复制再读"模式：避免和 Tauri app 进程的文件锁冲突
        // 直接 Connection::open() 在 Windows 上会被 SQLite 锁文件机制阻塞
        let open_via_copy = || -> Result<Connection, String> {
            let tmp = std::env::temp_dir().join(format!(
                "kingdee_kb_diag_{}.db",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            // 处理 WAL/SHM 也要复制（否则读不到最新数据）
            let main = db_path.clone();
            let wal = format!("{}-wal", main);
            let shm = format!("{}-shm", main);
            let journal = format!("{}-journal", main);
            for (src, dst) in [
                (main.clone(), tmp.to_string_lossy().to_string()),
                (wal, format!("{}-wal", tmp.to_string_lossy())),
                (shm, format!("{}-shm", tmp.to_string_lossy())),
            ] {
                if std::path::Path::new(&src).exists() {
                    std::fs::copy(&src, &dst).map_err(|e| format!("复制 {} -> {} 失败: {}", src, dst, e))?;
                }
            }
            let _ = journal; // 未使用
            Connection::open(&tmp).map_err(|e| format!("打开副本失败: {}", e))
        };
        // 写操作不能用只读副本：直接 open，失败时给出明确错误
        let open_writable = || -> Result<Connection, String> {
            Connection::open(&db_path).map_err(|e| format!("打开失败（app 进程可能正持有锁）: {}", e))
        };

        // 列出所有项目
        let projects: Vec<(i64, String)> = {
            let conn = open_via_copy().expect("打开数据库失败");
            let mut stmt = conn
                .prepare("SELECT id, name FROM projects")
                .expect("准备 projects 查询");
            stmt.query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };

        eprintln!(">>> 共 {} 个项目", projects.len());

        for (pid, name) in &projects {
            eprintln!("\n=== 项目 {} ({}) ===", pid, name);

            // 诊断 + 详情
            {
                let conn = open_via_copy().expect("打开数据库失败");
                let wiki_store = WikiPageStore::new(conn);
                let (total, non_empty, samples, has_brackets) =
                    wiki_store.diagnose_wikilinks(*pid).unwrap_or((0, 0, vec![], 0));
                eprintln!("  总页数: {}", total);
                eprintln!("  非空 wikilinks 页数: {}", non_empty);
                eprintln!("  content 含 `[[..]]` 页数: {}", has_brackets);
                for s in &samples {
                    eprintln!("    样例: {}", s);
                }
            }

            // 页面详情（用独立连接，避免穿透 private 字段）
            eprintln!("  --- 页面详情 ---");
            {
                let conn = open_via_copy().expect("打开数据库失败");
                let mut stmt = conn
                    .prepare(
                        "SELECT slug, length(content), length(content_candidate), wikilinks
                         FROM wiki_pages WHERE project_id = ?1",
                    )
                    .expect("准备页面详情查询");
                let rows = stmt
                    .query_map([pid], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    })
                    .expect("执行页面详情查询");
                let mut has_rows = false;
                for r in rows.flatten() {
                    has_rows = true;
                    eprintln!(
                        "    slug={:<30} content_len={:>5} candidate_len={:>5} wikilinks={}",
                        r.0, r.1, r.2, r.3
                    );
                }
                if !has_rows {
                    eprintln!("    (此项目无 wiki_pages)");
                }
            }

            // 实际跑 build（用写连接：需要 INSERT；app 没运行时可成功；app 运行时会被锁住）
            match open_writable() {
                Ok(c) => {
                    let graph_store = GraphStore::new(c);
                    match graph_store.build_knowledge_graph(*pid) {
                        Ok(n) => eprintln!("  >>> build 边数: {}", n),
                        Err(e) => eprintln!("  !!! build 失败: {}", e),
                    }
                }
                Err(e) => eprintln!("  >>> build 跳过（app 进程可能正持有锁，需先关 app）: {}", e),
            }

            // 跑一次 backfill 看能修多少
            match open_writable() {
                Ok(c) => {
                    let graph_store = GraphStore::new(c);
                    match graph_store.backfill_empty_wikilinks(*pid) {
                        Ok(n) => eprintln!("  >>> backfill 修复页数: {}", n),
                        Err(e) => eprintln!("  !!! backfill 失败: {}", e),
                    }
                }
                Err(e) => eprintln!("  >>> backfill 跳过：{}", e),
            }

            // 再诊断一次（看 backfill 后状态）
            {
                let conn = open_via_copy().expect("打开数据库失败");
                let wiki_store = WikiPageStore::new(conn);
                let (total, non_empty, _, _) =
                    wiki_store.diagnose_wikilinks(*pid).unwrap_or((0, 0, vec![], 0));
                eprintln!("  >>> backfill 后：总页数={} 非空wikilinks={}", total, non_empty);
            }
        }
    }

    /// 批量批准：跑 `KINGDEE_KB_BATCH_APPROVE=1 cargo test --lib batch_approve_user_database -- --nocapture`
    ///
    /// 把项目下所有 `content_candidate IS NOT NULL` 的页面一次性批准，
    /// 然后再跑 build_knowledge_graph 看实际边数。
    /// 用 `KINGDEE_KB_BATCH_APPROVE_PROJECT=2` 指定项目 ID（默认 1）。
    #[test]
    fn batch_approve_user_database() {
        if std::env::var("KINGDEE_KB_BATCH_APPROVE").is_err() {
            eprintln!(
                "跳过：设置 KINGDEE_KB_BATCH_APPROVE=1 才会真正批准。可用 KINGDEE_KB_BATCH_APPROVE_PROJECT=<id> 指定项目。"
            );
            return;
        }
        use crate::services::wiki_page::WikiPageStore;
        use rusqlite::Connection;

        let project_id: i64 = std::env::var("KINGDEE_KB_BATCH_APPROVE_PROJECT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let db_path = std::env::var("KINGDEE_KB_DB").unwrap_or_else(|_| {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_default();
            format!("{}\\.kingdee-kb\\metadata.db", home)
        });
        eprintln!(">>> 打开数据库: {}", db_path);
        eprintln!(">>> 目标项目 ID: {}", project_id);

        if !std::path::Path::new(&db_path).exists() {
            eprintln!("!!! 数据库文件不存在: {}", db_path);
            return;
        }

        // 列出所有待批准的页面 id
        let pending_ids: Vec<i64> = {
            let conn = Connection::open(&db_path).expect("打开数据库失败");
            let mut stmt = conn
                .prepare("SELECT id, slug FROM wiki_pages WHERE project_id = ?1 AND content_candidate IS NOT NULL")
                .expect("准备查询");
            stmt.query_map([project_id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .unwrap()
                .filter_map(|r| r.ok())
                .map(|(id, _)| id)
                .collect()
        };
        eprintln!(">>> 共 {} 个待批准页面", pending_ids.len());
        if pending_ids.is_empty() {
            eprintln!("!!! 没有待批准页面，直接返回");
            return;
        }

        // 逐个批准（必须用同一连接持锁，否则会并发写冲突）
        let mut approved = 0;
        let mut failed: Vec<(i64, String)> = Vec::new();
        let conn = Connection::open(&db_path).expect("打开数据库失败");
        let wiki_store = WikiPageStore::new(conn);
        for id in &pending_ids {
            match wiki_store.approve_candidate(*id) {
                Ok(_) => approved += 1,
                Err(e) => {
                    failed.push((*id, e));
                    eprintln!("  !!! 批准 id={} 失败: {}", id, failed.last().unwrap().1);
                }
            }
        }
        eprintln!(">>> 批准完成: 成功={} 失败={}", approved, failed.len());

        // 批准后状态
        {
            let conn = Connection::open(&db_path).expect("打开数据库失败");
            let wiki_store = WikiPageStore::new(conn);
            let (total, non_empty, samples, has_brackets) = wiki_store
                .diagnose_wikilinks(project_id)
                .unwrap_or((0, 0, vec![], 0));
            eprintln!(
                ">>> 批准后诊断: total={} non_empty={} has_brackets={}",
                total, non_empty, has_brackets
            );
            for s in &samples {
                eprintln!("    样例: {}", s);
            }
        }

        // 跑 build 看边数
        {
            let conn = Connection::open(&db_path).expect("打开数据库失败");
            let graph_store = GraphStore::new(conn);
            match graph_store.build_knowledge_graph(project_id) {
                Ok(n) => eprintln!(">>> build 边数: {}", n),
                Err(e) => eprintln!("!!! build 失败: {}", e),
            }
        }
    }
}
