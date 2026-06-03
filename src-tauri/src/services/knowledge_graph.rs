//! 知识图谱存储（knowledge_graph 表）
//!
//! ⚠️ 实验性能力 — 当前为独立功能，不依赖也不影响主流程。
//! 后续若需作为主搜索链路依赖，需先评估稳定性和性能。
//!
//! 管理页面间的关联关系（wikilink、tag、source、co_citation 等信号）。
//! 提供图构建、递归遍历、邻居查询和统计功能。

use std::collections::HashMap;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

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

    /// 创建 knowledge_graph 表及其索引（幂等）
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
        if self
            .db
            .prepare("SELECT project_id FROM knowledge_graph LIMIT 0")
            .is_err()
        {
            self.db
                .execute("ALTER TABLE knowledge_graph ADD COLUMN project_id INTEGER", [])
                .map_err(|e| format!("添加 knowledge_graph.project_id 失败: {}", e))?;
        }
        Ok(())
    }

    // ─── 图构建 ───

    /// 构建/重建知识图谱。先清空项目旧数据，再从 wiki_pages 提取 4 信号写入边。
    /// 返回插入的边数。
    pub fn build_knowledge_graph(&self, project_id: i64) -> Result<usize, String> {
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
            .execute_batch("BEGIN TRANSACTION")
            .map_err(|e| format!("开始事务失败: {}", e))?;

        let mut insert_err: Option<String> = None;
        for row in rows {
            let (slug, wikilinks_json) =
                row.map_err(|e| format!("读取 wikilink 行失败: {}", e))?;
            let targets: Vec<String> = serde_json::from_str(&wikilinks_json).unwrap_or_default();
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
            let _ = self.db.execute_batch("ROLLBACK");
            return Err(e);
        }
        self.db
            .execute_batch("COMMIT")
            .map_err(|e| format!("提交 wikilink 事务失败: {}", e))?;
        Ok(count)
    }

    /// S2: tag 共现信号。共享同一 tag 的页面两两关联（weight=0.6）
    fn build_signal_tag(&self, project_id: i64) -> Result<usize, String> {
        let tag_map = self.collect_field_map(project_id, "tags")?;
        let pairs = Self::generate_co_occurrence_pairs(&tag_map);

        self.db
            .execute_batch("BEGIN TRANSACTION")
            .map_err(|e| format!("开始事务失败: {}", e))?;
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
                    let _ = self.db.execute_batch("ROLLBACK");
                    return Err(format!("插入 tag 共现边失败: {}", e));
                }
            }
        }
        self.db
            .execute_batch("COMMIT")
            .map_err(|e| format!("提交 tag 事务失败: {}", e))?;
        Ok(count)
    }

    /// S3: source 共源信号。引用同一 raw_source 的页面两两关联（weight=0.4）
    fn build_signal_source(&self, project_id: i64) -> Result<usize, String> {
        let source_map = self.collect_field_map(project_id, "sources")?;
        let pairs = Self::generate_co_occurrence_pairs(&source_map);

        self.db
            .execute_batch("BEGIN TRANSACTION")
            .map_err(|e| format!("开始事务失败: {}", e))?;
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
                    let _ = self.db.execute_batch("ROLLBACK");
                    return Err(format!("插入 source 共源边失败: {}", e));
                }
            }
        }
        self.db
            .execute_batch("COMMIT")
            .map_err(|e| format!("提交 source 事务失败: {}", e))?;
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
            let (source, target) =
                row.map_err(|e| format!("读取 co_citation 行失败: {}", e))?;
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
            .execute_batch("BEGIN TRANSACTION")
            .map_err(|e| format!("开始事务失败: {}", e))?;
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
                    let _ = self.db.execute_batch("ROLLBACK");
                    return Err(format!("插入 co_citation 边失败: {}", e));
                }
            }
        }
        self.db
            .execute_batch("COMMIT")
            .map_err(|e| format!("提交 co_citation 事务失败: {}", e))?;
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
            .query_map(params![project_id, seed_slug, min_weight, max_depth], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })
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
    pub fn get_neighbors(
        &self,
        project_id: i64,
        slug: &str,
    ) -> Result<Vec<GraphNeighbor>, String> {
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
            let (s, signal, weight) =
                row.map_err(|e| format!("读取邻居行失败: {}", e))?;
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
            let (signal, count) =
                row.map_err(|e| format!("读取信号统计行失败: {}", e))?;
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

    // ─── 图扩展检索 ───

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
            let entry = grouped
                .entry(path.target_slug.clone())
                .or_insert_with(|| {
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
            let (slug, json_str) =
                row.map_err(|e| format!("读取 {} 行失败: {}", field, e))?;
            let values: Vec<String> = serde_json::from_str(&json_str).unwrap_or_default();
            for val in values {
                if !val.is_empty() {
                    map.entry(val).or_default().push(slug.clone());
                }
            }
        }
        Ok(map)
    }

    /// 从共现映射生成去重的页面对（source < target 排序）
    fn generate_co_occurrence_pairs(map: &HashMap<String, Vec<String>>) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        for slugs in map.values() {
            if slugs.len() < 2 {
                continue;
            }
            for i in 0..slugs.len() {
                for j in (i + 1)..slugs.len() {
                    let (a, b) = if slugs[i] < slugs[j] {
                        (&slugs[i], &slugs[j])
                    } else {
                        (&slugs[j], &slugs[i])
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
        let placeholders: Vec<String> = (2..=(slugs.len() + 1))
            .map(|i| format!("?{}", i))
            .collect();
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
        let placeholders: Vec<String> = (2..=(slugs.len() + 1))
            .map(|i| format!("?{}", i))
            .collect();
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
            let (slug, title, page_type) =
                row.map_err(|e| format!("读取页面信息行失败: {}", e))?;
            map.insert(slug, (title, page_type));
        }
        Ok(map)
    }
}
