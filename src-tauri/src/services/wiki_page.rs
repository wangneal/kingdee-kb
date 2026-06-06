//! 编排知识页管理（wiki_pages 表）
//!
//! 管理项目维基页面，支持草稿/发布状态、多版本候选项、页面间 wikilinks。

use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};

/// wiki_pages 简略信息，用于搜索候选和反向链接
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPageBrief {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub page_type: String,
}

/// wikilink 目标详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiLinkTarget {
    pub slug: String,
    pub title: String,
    pub page_type: String,
    pub page_status: String,
}

/// 维基页面完整记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPage {
    pub id: i64,
    pub project_id: i64,
    pub slug: String,
    pub title: String,
    pub page_type: String,
    pub content: String,
    pub content_candidate: Option<String>,
    pub candidate_status: Option<String>,
    pub sources_candidate: Option<String>,
    pub frontmatter: String,
    pub sources: String,
    pub wikilinks: String,
    pub tags: String,
    pub page_metadata: String,
    pub candidate_version: Option<i64>,
    pub page_status: String,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// 创建维基页面时的数据传输对象
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWikiPage {
    pub project_id: i64,
    pub slug: String,
    pub title: String,
    pub page_type: String,
    pub content: String,
    pub frontmatter: Option<String>,
    pub sources: Option<String>,
    pub wikilinks: Option<String>,
    pub tags: Option<String>,
    pub page_metadata: Option<String>,
    pub page_status: Option<String>,
}

/// 更新维基页面时的数据传输对象
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWikiPage {
    pub title: Option<String>,
    pub content: Option<String>,
    pub content_candidate: Option<String>,
    pub candidate_status: Option<String>,
    pub sources_candidate: Option<String>,
    pub frontmatter: Option<String>,
    pub sources: Option<String>,
    pub wikilinks: Option<String>,
    pub tags: Option<String>,
    pub page_metadata: Option<String>,
    pub candidate_version: Option<i64>,
    pub page_status: Option<String>,
}

/// wiki_pages 表的数据操作层
pub struct WikiPageStore {
    db: Connection,
}

impl WikiPageStore {
    /// 使用已有的数据库连接创建存储
    pub fn new(db: Connection) -> Self {
        let _ = db.busy_timeout(std::time::Duration::from_secs(5));
        Self { db }
    }

    /// 创建 wiki_pages 表及其索引（幂等）
    pub fn ensure_table(&self) -> Result<(), String> {
        if self.has_column("wiki_pages", "project")? {
            self.migrate_legacy_table()?;
        }
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS wiki_pages (
                id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id         INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                slug               TEXT NOT NULL,
                title              TEXT NOT NULL,
                page_type          TEXT NOT NULL CHECK(page_type IN ('summary','blueprint','fitgap','decision','config')),
                content            TEXT NOT NULL,
                content_candidate  TEXT,
                candidate_status   TEXT CHECK(candidate_status IN ('auto','conflict','pending')),
                sources_candidate  TEXT,
                frontmatter        TEXT NOT NULL DEFAULT '{}',
                sources            TEXT NOT NULL DEFAULT '[]',
                wikilinks          TEXT NOT NULL DEFAULT '[]',
                tags               TEXT NOT NULL DEFAULT '[]',
                page_metadata      TEXT NOT NULL DEFAULT '{}',
                candidate_version  INTEGER,
                page_status        TEXT NOT NULL DEFAULT 'draft' CHECK(page_status IN ('draft','published')),
                version            INTEGER NOT NULL DEFAULT 1,
                created_at         TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at         TEXT NOT NULL DEFAULT (datetime('now')),
                CHECK((content_candidate IS NULL AND candidate_status IS NULL AND candidate_version IS NULL)
                   OR (content_candidate IS NOT NULL AND candidate_status IS NOT NULL AND candidate_version IS NOT NULL AND candidate_version = version + 1))
            );
        ").map_err(|e| format!("创建 wiki_pages 表失败: {}", e))?;

        self.ensure_column("wiki_pages", "project_id", "INTEGER")?;
        self.ensure_column("wiki_pages", "sources_candidate", "TEXT")?;
        self.backfill_project_id("wiki_pages")?;
        self.db
            .execute_batch(
                "
            CREATE UNIQUE INDEX IF NOT EXISTS idx_wiki_pages_slug ON wiki_pages(project_id, slug);
            CREATE INDEX IF NOT EXISTS idx_wiki_pages_project_id ON wiki_pages(project_id);
            CREATE INDEX IF NOT EXISTS idx_wiki_pages_status ON wiki_pages(page_status);
            CREATE INDEX IF NOT EXISTS idx_wiki_pages_type ON wiki_pages(page_type);
            ",
            )
            .map_err(|e| format!("创建 wiki_pages 索引失败: {}", e))?;
        Ok(())
    }

    fn ensure_column(&self, table: &str, column: &str, definition: &str) -> Result<(), String> {
        let mut stmt = self
            .db
            .prepare(&format!("PRAGMA table_info({})", table))
            .map_err(|e| format!("读取表结构失败: {}", e))?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("查询表结构失败: {}", e))?;
        for col in columns {
            if col.map_err(|e| format!("读取列名失败: {}", e))? == column {
                return Ok(());
            }
        }
        self.db
            .execute_batch(&format!(
                "ALTER TABLE {} ADD COLUMN {} {};",
                table, column, definition
            ))
            .map_err(|e| format!("添加列 {}.{} 失败: {}", table, column, e))?;
        Ok(())
    }

    fn migrate_legacy_table(&self) -> Result<(), String> {
        let sql = "
            BEGIN IMMEDIATE;
            CREATE TABLE wiki_pages_new (
                id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id         INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                slug               TEXT NOT NULL,
                title              TEXT NOT NULL,
                page_type          TEXT NOT NULL CHECK(page_type IN ('summary','blueprint','fitgap','decision','config')),
                content            TEXT NOT NULL,
                content_candidate  TEXT,
                candidate_status   TEXT CHECK(candidate_status IN ('auto','conflict','pending')),
                sources_candidate  TEXT,
                frontmatter        TEXT NOT NULL DEFAULT '{}',
                sources            TEXT NOT NULL DEFAULT '[]',
                wikilinks          TEXT NOT NULL DEFAULT '[]',
                tags               TEXT NOT NULL DEFAULT '[]',
                page_metadata      TEXT NOT NULL DEFAULT '{}',
                candidate_version  INTEGER,
                page_status        TEXT NOT NULL DEFAULT 'draft' CHECK(page_status IN ('draft','published')),
                version            INTEGER NOT NULL DEFAULT 1,
                created_at         TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at         TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(project_id, slug),
                CHECK((content_candidate IS NULL AND candidate_status IS NULL AND candidate_version IS NULL)
                   OR (content_candidate IS NOT NULL AND candidate_status IS NOT NULL AND candidate_version IS NOT NULL AND candidate_version = version + 1))
            );
            INSERT INTO wiki_pages_new (
                id, project_id, slug, title, page_type, content, content_candidate,
                candidate_status, sources_candidate, frontmatter, sources, wikilinks, tags, page_metadata,
                candidate_version, page_status, version, created_at, updated_at
            )
            SELECT id,
                   COALESCE(
                       (SELECT id FROM projects WHERE name = wiki_pages.project LIMIT 1),
                       (SELECT id FROM projects WHERE name = '默认项目' LIMIT 1),
                       (SELECT id FROM projects WHERE status = 'active' ORDER BY id ASC LIMIT 1)
                   ),
                   slug, title, page_type, content, content_candidate,
                   candidate_status, NULL, frontmatter, sources, wikilinks, tags, page_metadata,
                   candidate_version, page_status, version, created_at, updated_at
            FROM wiki_pages;
            DROP TABLE wiki_pages;
            ALTER TABLE wiki_pages_new RENAME TO wiki_pages;
            COMMIT;
        ";
        if let Err(e) = self.db.execute_batch(sql) {
            let _ = self.db.execute_batch("ROLLBACK;");
            return Err(format!("迁移旧版 wiki_pages 表失败: {}", e));
        }
        Ok(())
    }

    fn backfill_project_id(&self, table: &str) -> Result<(), String> {
        let legacy_project = if self.has_column(table, "project")? {
            format!(
                "(SELECT id FROM projects WHERE name = {}.project LIMIT 1),",
                table
            )
        } else {
            "NULL,".to_string()
        };
        self.db
            .execute(
                &format!(
                    "UPDATE {} SET project_id = COALESCE(
                        {}
                        (SELECT id FROM projects WHERE name = '默认项目' LIMIT 1),
                        (SELECT id FROM projects WHERE status = 'active' ORDER BY id ASC LIMIT 1)
                    ) WHERE project_id IS NULL",
                    table, legacy_project
                ),
                [],
            )
            .map_err(|e| format!("回填 {}.project_id 失败: {}", table, e))?;
        Ok(())
    }

    fn has_column(&self, table: &str, column: &str) -> Result<bool, String> {
        let mut stmt = self
            .db
            .prepare(&format!("PRAGMA table_info({})", table))
            .map_err(|e| format!("读取表结构失败: {}", e))?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("查询表结构失败: {}", e))?;
        for col in columns {
            if col.map_err(|e| format!("读取列名失败: {}", e))? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 创建一条维基页面，返回完整记录
    pub fn create(&self, input: &CreateWikiPage) -> Result<WikiPage, String> {
        self.db.execute(
            "INSERT INTO wiki_pages (project_id, slug, title, page_type, content, frontmatter, sources, wikilinks, tags, page_metadata, page_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                input.project_id,
                input.slug,
                input.title,
                input.page_type,
                input.content,
                input.frontmatter.as_deref().unwrap_or("{}"),
                input.sources.as_deref().unwrap_or("[]"),
                input.wikilinks.as_deref().unwrap_or("[]"),
                input.tags.as_deref().unwrap_or("[]"),
                input.page_metadata.as_deref().unwrap_or("{}"),
                input.page_status.as_deref().unwrap_or("draft"),
            ],
        ).map_err(|e| format!("插入 wiki_page 失败: {}", e))?;
        let id = self.db.last_insert_rowid();
        self.get_by_id(id)
    }

    /// 按 ID 获取一条维基页面
    pub fn get_by_id(&self, id: i64) -> Result<WikiPage, String> {
        self.query_one(
            "SELECT id, project_id, slug, title, page_type, content, content_candidate,
                    candidate_status, sources_candidate, frontmatter, sources, wikilinks, tags, page_metadata,
                    candidate_version, page_status, version, created_at, updated_at
             FROM wiki_pages WHERE id = ?1",
            params![id],
        )?
        .ok_or_else(|| format!("wiki_page 未找到: id={}", id))
    }

    /// 按项目 + slug 查找页面
    pub fn get_by_slug(&self, project_id: i64, slug: &str) -> Result<Option<WikiPage>, String> {
        self.query_one(
            "SELECT id, project_id, slug, title, page_type, content, content_candidate,
                    candidate_status, sources_candidate, frontmatter, sources, wikilinks, tags, page_metadata,
                    candidate_version, page_status, version, created_at, updated_at
             FROM wiki_pages WHERE project_id = ?1 AND slug = ?2",
            params![project_id, slug],
        )
    }

    /// 列出项目下的所有页面，可按状态过滤
    pub fn list(
        &self,
        project_id: i64,
        page_status: Option<&str>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<WikiPage>, String> {
        if let Some(status) = page_status {
            self.query_list(
                "SELECT id, project_id, slug, title, page_type, content, content_candidate,
                        candidate_status, sources_candidate, frontmatter, sources, wikilinks, tags, page_metadata,
                        candidate_version, page_status, version, created_at, updated_at
                 FROM wiki_pages
                 WHERE project_id = ?1 AND page_status = ?2
                 ORDER BY updated_at DESC LIMIT ?3 OFFSET ?4",
                params![project_id, status, limit.unwrap_or(-1), offset.unwrap_or(0)],
            )
        } else {
            self.query_list(
                "SELECT id, project_id, slug, title, page_type, content, content_candidate,
                        candidate_status, sources_candidate, frontmatter, sources, wikilinks, tags, page_metadata,
                        candidate_version, page_status, version, created_at, updated_at
                 FROM wiki_pages
                 WHERE project_id = ?1
                 ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3",
                params![project_id, limit.unwrap_or(-1), offset.unwrap_or(0)],
            )
        }
    }

    /// 更新维基页面，返回更新后的完整记录
    pub fn update(&self, id: i64, input: &UpdateWikiPage) -> Result<WikiPage, String> {
        let existing = self.get_by_id(id)?;
        let title = input.title.as_deref().unwrap_or(&existing.title);
        let content = input.content.as_deref().unwrap_or(&existing.content);
        let content_candidate: Option<&str> = input
            .content_candidate
            .as_deref()
            .or(existing.content_candidate.as_deref());
        let candidate_status: Option<&str> = input
            .candidate_status
            .as_deref()
            .or(existing.candidate_status.as_deref());
        let sources_candidate: Option<&str> = input
            .sources_candidate
            .as_deref()
            .or(existing.sources_candidate.as_deref());
        let frontmatter = input
            .frontmatter
            .as_deref()
            .unwrap_or(&existing.frontmatter);
        let sources = input.sources.as_deref().unwrap_or(&existing.sources);
        let wikilinks = input.wikilinks.as_deref().unwrap_or(&existing.wikilinks);
        let tags = input.tags.as_deref().unwrap_or(&existing.tags);
        let page_metadata = input
            .page_metadata
            .as_deref()
            .unwrap_or(&existing.page_metadata);
        let candidate_version: Option<i64> = input.candidate_version.or(existing.candidate_version);
        let page_status = input
            .page_status
            .as_deref()
            .unwrap_or(&existing.page_status);
        self.db
            .execute(
                "UPDATE wiki_pages SET
                title = ?1, content = ?2, content_candidate = ?3, candidate_status = ?4,
                sources_candidate = ?5, frontmatter = ?6, sources = ?7, wikilinks = ?8, tags = ?9,
                page_metadata = ?10, candidate_version = ?11, page_status = ?12,
                version = ?13, updated_at = datetime('now')
             WHERE id = ?14",
                params![
                    title,
                    content,
                    content_candidate,
                    candidate_status,
                    sources_candidate,
                    frontmatter,
                    sources,
                    wikilinks,
                    tags,
                    page_metadata,
                    candidate_version,
                    page_status,
                    existing.version,
                    id,
                ],
            )
            .map_err(|e| format!("更新 wiki_page 失败: {}", e))?;
        self.get_by_id(id)
    }

    /// 删除一条维基页面
    pub fn delete(&self, id: i64) -> Result<(), String> {
        let rows = self
            .db
            .execute("DELETE FROM wiki_pages WHERE id = ?1", params![id])
            .map_err(|e| format!("删除 wiki_page 失败: {}", e))?;
        if rows == 0 {
            return Err(format!("wiki_page 未找到: id={}", id));
        }
        Ok(())
    }

    /// 批准候选内容：将 content_candidate 和 sources_candidate 一起提升，重置候选字段，版本递增
    pub fn approve_candidate(&self, id: i64) -> Result<WikiPage, String> {
        let existing = self.get_by_id(id)?;
        let candidate = existing
            .content_candidate
            .ok_or_else(|| "没有待批准的候选内容".to_string())?;
        let sources = existing
            .sources_candidate
            .as_deref()
            .unwrap_or(&existing.sources);
        self.db
            .execute(
                "UPDATE wiki_pages SET
                content = ?1, content_candidate = NULL, candidate_status = NULL,
                sources = ?2, sources_candidate = NULL,
                candidate_version = NULL, version = version + 1, updated_at = datetime('now')
             WHERE id = ?3 AND content_candidate IS NOT NULL",
                params![candidate, sources, id],
            )
            .map_err(|e| format!("批准 wiki_page 候选内容失败: {}", e))?;
        self.get_by_id(id)
    }

    /// 拒绝候选内容：清空候选字段，版本不递增
    pub fn reject_candidate(&self, id: i64) -> Result<WikiPage, String> {
        self.db
            .execute(
                "UPDATE wiki_pages SET
                content_candidate = NULL, candidate_status = NULL,
                sources_candidate = NULL, candidate_version = NULL, updated_at = datetime('now')
             WHERE id = ?1",
                params![id],
            )
            .map_err(|e| format!("拒绝 wiki_page 候选内容失败: {}", e))?;
        self.get_by_id(id)
    }

    /// 插入 50 条种子演示数据，用于阶段五知识图谱开发测试
    pub fn seed_demo_pages(&self, project_id: i64) -> Result<usize, String> {
        let existing: i64 = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM wiki_pages WHERE project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("检查已有数据失败: {}", e))?;
        if existing > 0 {
            return Err(format!(
                "项目 {} 下已有 {} 条 wiki_pages，跳过种子数据",
                project_id, existing
            ));
        }
        let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("seed-wiki-pages.sql");
        let sql = std::fs::read_to_string(&script_path)
            .map_err(|e| format!("读取种子数据脚本失败 ({}): {}", script_path.display(), e))?;
        let sql = sql.replace("__PROJECT__", &project_id.to_string());
        self.db
            .execute_batch(&sql)
            .map_err(|e| format!("执行种子数据脚本失败: {}", e))?;
        let count: i64 = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM wiki_pages WHERE project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("查询种子数据量失败: {}", e))?;
        Ok(count as usize)
    }

    /// 按项目删除所有页面
    pub fn delete_by_project(&self, project_id: i64) -> Result<usize, String> {
        let rows = self
            .db
            .execute(
                "DELETE FROM wiki_pages WHERE project_id = ?1",
                params![project_id],
            )
            .map_err(|e| format!("删除项目 wiki_pages 失败: {}", e))?;
        Ok(rows)
    }

    // ─── Wikilink 相关方法 ───

    /// 搜索 wikilink 候选页面（按标题模糊搜索，排除自身）
    pub fn search_wikilink_candidates(
        &self,
        project_id: i64,
        query: &str,
        exclude_slug: &str,
        limit: i64,
    ) -> Result<Vec<WikiPageBrief>, String> {
        let pattern = format!("%{}%", query);
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, slug, title, page_type
                 FROM wiki_pages
                 WHERE project_id = ?1 AND slug != ?2
                   AND (title LIKE ?3 OR slug LIKE ?3)
                 LIMIT ?4",
            )
            .map_err(|e| format!("准备搜索候选查询失败: {}", e))?;
        let rows = stmt
            .query_map(params![project_id, exclude_slug, pattern, limit], |row| {
                Ok(WikiPageBrief {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    title: row.get(2)?,
                    page_type: row.get(3)?,
                })
            })
            .map_err(|e| format!("执行搜索候选查询失败: {}", e))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("读取候选行失败: {}", e))?);
        }
        Ok(results)
    }

    /// 添加 wikilink（追加 slug 到 wikilinks JSON 数组，去重）
    pub fn add_wikilink(&self, page_id: i64, target_slug: &str) -> Result<WikiPage, String> {
        let existing = self.get_by_id(page_id)?;
        let mut links: Vec<String> = serde_json::from_str(&existing.wikilinks).unwrap_or_default();
        if !links.contains(&target_slug.to_string()) {
            links.push(target_slug.to_string());
        }
        let json =
            serde_json::to_string(&links).map_err(|e| format!("序列化 wikilinks 失败: {}", e))?;
        self.db
            .execute(
                "UPDATE wiki_pages SET wikilinks = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![json, page_id],
            )
            .map_err(|e| format!("更新 wikilinks 失败: {}", e))?;
        self.get_by_id(page_id)
    }

    /// 移除 wikilink（从数组中删除 slug）
    pub fn remove_wikilink(&self, page_id: i64, target_slug: &str) -> Result<WikiPage, String> {
        let existing = self.get_by_id(page_id)?;
        let links: Vec<String> = serde_json::from_str(&existing.wikilinks).unwrap_or_default();
        let filtered: Vec<String> = links.into_iter().filter(|s| s != target_slug).collect();
        let json = serde_json::to_string(&filtered)
            .map_err(|e| format!("序列化 wikilinks 失败: {}", e))?;
        self.db
            .execute(
                "UPDATE wiki_pages SET wikilinks = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![json, page_id],
            )
            .map_err(|e| format!("更新 wikilinks 失败: {}", e))?;
        self.get_by_id(page_id)
    }

    /// 获取 wikilink 目标页面详情（按项目过滤，批量查询被引页面的标题/slug/type/status）
    pub fn get_wikilink_targets(
        &self,
        project_id: i64,
        slugs: &[String],
    ) -> Result<Vec<WikiLinkTarget>, String> {
        if slugs.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<String> = (1..=slugs.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT slug, title, page_type, page_status
             FROM wiki_pages
              WHERE project_id = ?{} AND slug IN ({})",
            slugs.len() + 1,
            placeholders.join(", ")
        );
        let mut stmt = self
            .db
            .prepare(&sql)
            .map_err(|e| format!("准备 wikilink 目标查询失败: {}", e))?;
        let mut params: Vec<&dyn rusqlite::types::ToSql> = slugs
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        params.push(&project_id as &dyn rusqlite::types::ToSql);
        let rows = stmt
            .query_map(params.as_slice(), |row| {
                Ok(WikiLinkTarget {
                    slug: row.get(0)?,
                    title: row.get(1)?,
                    page_type: row.get(2)?,
                    page_status: row.get(3)?,
                })
            })
            .map_err(|e| format!("执行 wikilink 目标查询失败: {}", e))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("读取目标行失败: {}", e))?);
        }
        Ok(results)
    }

    /// 按标题/内容模糊搜索 wiki 页面，返回 HybridSearchResult
    pub fn search_pages(
        &self,
        project_id: Option<i64>,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<crate::services::hybrid_search::HybridSearchResult>, String> {
        use crate::services::hybrid_search::HybridSearchResult;

        let pattern = format!("%{}%", query);

        let mut results = Vec::new();
        if let Some(pid) = project_id {
            let mut stmt = self
                .db
                .prepare(
                    "SELECT id, project_id, title, content, 0.7 AS score
                 FROM wiki_pages
                 WHERE project_id = ?1 AND title LIKE ?2
                 UNION
                 SELECT id, project_id, title, content, 0.4 AS score
                 FROM wiki_pages
                 WHERE project_id = ?1 AND content LIKE ?2 AND title NOT LIKE ?2
                 ORDER BY score DESC
                 LIMIT ?3",
                )
                .map_err(|e| format!("准备搜索 wiki_pages 查询失败: {}", e))?;
            let mapped = stmt
                .query_map(params![pid, pattern, top_k as i64], |row| {
                    let score: f64 = row.get(4)?;
                    Ok(HybridSearchResult {
                        chunk_id: row.get::<_, i64>(0)?,
                        title: row.get::<_, String>(2)?,
                        content: row.get::<_, String>(3)?,
                        score: score as f32,
                        source: "wiki_pages".to_string(),
                        document_id: row.get::<_, i64>(0)?,
                        section_path: None,
                        project: row.get::<_, i64>(1)?.to_string(),
                    })
                })
                .map_err(|e| format!("执行搜索 wiki_pages 查询失败: {}", e))?;
            for row in mapped {
                results.push(row.map_err(|e| format!("读取 wiki_pages 搜索行失败: {}", e))?);
            }
        } else {
            let mut stmt = self
                .db
                .prepare(
                    "SELECT id, project_id, title, content, 0.7 AS score
                 FROM wiki_pages
                 WHERE title LIKE ?1
                 UNION
                 SELECT id, project_id, title, content, 0.4 AS score
                 FROM wiki_pages
                 WHERE content LIKE ?1 AND title NOT LIKE ?1
                 ORDER BY score DESC
                 LIMIT ?2",
                )
                .map_err(|e| format!("准备搜索 wiki_pages 查询失败: {}", e))?;
            let mapped = stmt
                .query_map(params![pattern, top_k as i64], |row| {
                    let score: f64 = row.get(4)?;
                    Ok(HybridSearchResult {
                        chunk_id: row.get::<_, i64>(0)?,
                        title: row.get::<_, String>(2)?,
                        content: row.get::<_, String>(3)?,
                        score: score as f32,
                        source: "wiki_pages".to_string(),
                        document_id: row.get::<_, i64>(0)?,
                        section_path: None,
                        project: row.get::<_, i64>(1)?.to_string(),
                    })
                })
                .map_err(|e| format!("执行搜索 wiki_pages 查询失败: {}", e))?;
            for row in mapped {
                results.push(row.map_err(|e| format!("读取 wiki_pages 搜索行失败: {}", e))?);
            }
        }
        Ok(results)
    }

    /// 获取反向链接（哪些页面引用了当前页面）
    pub fn get_backlinks(&self, project_id: i64, slug: &str) -> Result<Vec<WikiPageBrief>, String> {
        let pattern = format!("%{}%", slug);
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, slug, title, page_type
                 FROM wiki_pages
                  WHERE project_id = ?1 AND wikilinks LIKE ?2",
            )
            .map_err(|e| format!("准备反向链接查询失败: {}", e))?;
        let rows = stmt
            .query_map(params![project_id, pattern], |row| {
                Ok(WikiPageBrief {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    title: row.get(2)?,
                    page_type: row.get(3)?,
                })
            })
            .map_err(|e| format!("执行反向链接查询失败: {}", e))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("读取反向链接行失败: {}", e))?);
        }
        Ok(results)
    }

    // ─── 私有辅助方法 ───

    fn row_to_page(row: &rusqlite::Row) -> SqlResult<WikiPage> {
        Ok(WikiPage {
            id: row.get(0)?,
            project_id: row.get(1)?,
            slug: row.get(2)?,
            title: row.get(3)?,
            page_type: row.get(4)?,
            content: row.get(5)?,
            content_candidate: row.get(6)?,
            candidate_status: row.get(7)?,
            sources_candidate: row.get(8)?,
            frontmatter: row.get(9)?,
            sources: row.get(10)?,
            wikilinks: row.get(11)?,
            tags: row.get(12)?,
            page_metadata: row.get(13)?,
            candidate_version: row.get(14)?,
            page_status: row.get(15)?,
            version: row.get(16)?,
            created_at: row.get(17)?,
            updated_at: row.get(18)?,
        })
    }

    fn query_one(&self, sql: &str, p: impl rusqlite::Params) -> Result<Option<WikiPage>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;
        let mut rows = stmt
            .query_map(p, Self::row_to_page)
            .map_err(|e| format!("执行查询失败: {}", e))?;
        match rows.next() {
            Some(Ok(s)) => Ok(Some(s)),
            Some(Err(e)) => Err(format!("读取行失败: {}", e)),
            None => Ok(None),
        }
    }

    fn query_list(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<WikiPage>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;
        let rows = stmt
            .query_map(p, Self::row_to_page)
            .map_err(|e| format!("执行查询失败: {}", e))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("读取行失败: {}", e))?);
        }
        Ok(results)
    }
}
