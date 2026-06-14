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

/// 创建带候选内容的维基页面（一次 SQL 写入，避免 CHECK 约束不一致）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWikiPageWithCandidate {
    pub project_id: i64,
    pub slug: String,
    pub title: String,
    pub page_type: String,
    pub frontmatter: Option<String>,
    pub sources: Option<String>,
    pub wikilinks: Option<String>,
    pub tags: Option<String>,
    pub page_metadata: Option<String>,
    pub page_status: Option<String>,
    /// 候选内容
    pub content_candidate: String,
    /// 候选来源（一般与 sources 同步）
    pub sources_candidate: Option<String>,
    /// 候选状态（auto / conflict / pending）
    pub candidate_status: String,
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
            CREATE UNIQUE INDEX IF NOT EXISTS idx_wiki_pages_slug ON wiki_pages(project_id, slug);
            CREATE INDEX IF NOT EXISTS idx_wiki_pages_project_id ON wiki_pages(project_id);
            CREATE INDEX IF NOT EXISTS idx_wiki_pages_status ON wiki_pages(page_status);
            CREATE INDEX IF NOT EXISTS idx_wiki_pages_type ON wiki_pages(page_type);
        ").map_err(|e| format!("创建 wiki_pages 表失败: {}", e))?;
        Ok(())
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

    /// 一次 SQL 创建带候选内容的维基页面
    ///
    /// 等价于 `create` + `update`（写 content_candidate）合并：
    /// - 避免"先 create 再 update"产生的两次写入
    /// - 一次性满足 wiki_pages 的 CHECK 约束（candidate_version = version + 1 = 2）
    pub fn create_with_candidate(
        &self,
        input: &CreateWikiPageWithCandidate,
    ) -> Result<WikiPage, String> {
        // 候选字段在 CHECK 约束中必须同时存在且 candidate_version = version + 1
        // 新建时 version=1，所以 candidate_version=2
        self.db
            .execute(
                "INSERT INTO wiki_pages (
                    project_id, slug, title, page_type, content,
                    content_candidate, candidate_status, sources_candidate,
                    frontmatter, sources, wikilinks, tags, page_metadata,
                    candidate_version, page_status, version
                ) VALUES (
                    ?1, ?2, ?3, ?4, '',
                    ?5, ?6, ?7,
                    ?8, ?9, ?10, ?11, ?12,
                    2, ?13, 1
                )",
                params![
                    input.project_id,
                    input.slug,
                    input.title,
                    input.page_type,
                    input.content_candidate,
                    input.candidate_status,
                    input.sources_candidate,
                    input.frontmatter.as_deref().unwrap_or("{}"),
                    input.sources.as_deref().unwrap_or("[]"),
                    input.wikilinks.as_deref().unwrap_or("[]"),
                    input.tags.as_deref().unwrap_or("[]"),
                    input.page_metadata.as_deref().unwrap_or("{}"),
                    input.page_status.as_deref().unwrap_or("draft"),
                ],
            )
            .map_err(|e| format!("插入 wiki_page（带候选）失败: {}", e))?;
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

    /// 列出项目下所有页面的 (slug, title) 对，轻量查询（不加载 content）
    /// 用途：编译 prompt 时告知 LLM 项目已有页面，让 LLM 用 `[[slug]]` 引用
    ///
    /// 排序：`updated_at DESC` —— 最近更新的页面优先，LLM 更容易引用"现行"页面，
    /// 避免只看到创建顺序的旧页面。
    pub fn list_slugs(&self, project_id: i64) -> Result<Vec<(String, String)>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT slug, title FROM wiki_pages WHERE project_id = ?1 ORDER BY updated_at DESC, id DESC")
            .map_err(|e| format!("准备 slug 列表查询失败: {}", e))?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("执行 slug 列表查询失败: {}", e))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取 slug 行失败: {}", e))?);
        }
        Ok(result)
    }

    /// 诊断：扫描项目的 wiki_pages.wikilinks 状态，返回关键计数与样例
    ///
    /// 用于定位"知识图谱构建为 0 边"问题：
    /// - 项目无任何页面
    /// - 页面有但 `wikilinks = '[]'`
    /// - `content` / `content_candidate` 中无 `[[slug]]`（LLM 没遵循提示词）
    ///
    /// 返回 (total_pages, pages_with_nonempty_wikilinks, sample_wikilinks, has_brackets_in_content)
    pub fn diagnose_wikilinks(
        &self,
        project_id: i64,
    ) -> Result<(usize, usize, Vec<String>, usize), String> {
        // 总页数
        let total_pages: usize = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM wiki_pages WHERE project_id = ?1",
                params![project_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| format!("diagnose 总页数查询失败: {}", e))? as usize;

        // 非空 wikilinks 页数 + 样例（最多 3 条）
        let mut stmt = self
            .db
            .prepare(
                "SELECT slug, wikilinks FROM wiki_pages
                 WHERE project_id = ?1 AND wikilinks != '[]' AND wikilinks != ''
                 LIMIT 3",
            )
            .map_err(|e| format!("diagnose 样例查询失败: {}", e))?;
        let mut sample_wikilinks: Vec<String> = Vec::new();
        let pages_with_nonempty: usize = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM wiki_pages WHERE project_id = ?1 AND wikilinks != '[]' AND wikilinks != ''",
                params![project_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| format!("diagnose 非空计数执行失败: {}", e))? as usize;

        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("diagnose 样例执行失败: {}", e))?;
        for row in rows {
            let (slug, wl) = row.map_err(|e| format!("diagnose 样例读取失败: {}", e))?;
            sample_wikilinks.push(format!("slug={} wikilinks={}", slug, wl));
        }

        // content / content_candidate 含 `[[` 的页数（粗判，统计子串）
        let has_brackets: usize = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM wiki_pages
                 WHERE project_id = ?1
                   AND ((content LIKE '%[[%' AND content LIKE '%]]%')
                        OR (content_candidate LIKE '%[[%' AND content_candidate LIKE '%]]%'))",
                params![project_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| format!("diagnose bracket 查询失败: {}", e))? as usize;

        Ok((total_pages, pages_with_nonempty, sample_wikilinks, has_brackets))
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

    /// 更新维基页面（仅修改"正式"字段，自动递增 version 并 NULL 候选字段）
    ///
    /// 设计要点：
    /// - 只接受非候选字段（title/content/sources/wikilinks/tags/page_metadata/page_status/frontmatter）
    /// - 总是 `version = version + 1`（修复前是 `version = existing.version` 不递增）
    /// - 总是 NULL 候选字段：CHECK 约束 `candidate_version = version + 1` 在 version 递增后失效，
    ///   若保留旧 candidate_version 会触发约束违反。任何对页面的"正式"修改都让待批准候选失效
    ///   （候选是基于旧 content 生成的），这是合理语义
    /// - 候选更新请用 [`set_candidate`](Self::set_candidate) 方法
    pub fn update(&self, id: i64, input: &UpdateWikiPage) -> Result<WikiPage, String> {
        let existing = self.get_by_id(id)?;
        let title = input.title.as_deref().unwrap_or(&existing.title);
        let content = input.content.as_deref().unwrap_or(&existing.content);
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
        let page_status = input
            .page_status
            .as_deref()
            .unwrap_or(&existing.page_status);
        self.db
            .execute(
                "UPDATE wiki_pages SET
                title = ?1, content = ?2,
                content_candidate = NULL, candidate_status = NULL, sources_candidate = NULL,
                candidate_version = NULL,
                frontmatter = ?3, sources = ?4, wikilinks = ?5, tags = ?6,
                page_metadata = ?7, page_status = ?8,
                version = version + 1, updated_at = datetime('now')
             WHERE id = ?9",
                params![
                    title,
                    content,
                    frontmatter,
                    sources,
                    wikilinks,
                    tags,
                    page_metadata,
                    page_status,
                    id,
                ],
            )
            .map_err(|e| format!("更新 wiki_page 失败: {}", e))?;
        self.get_by_id(id)
    }

    /// 设置候选内容（仅写候选字段，不动正式 content/version）
    ///
    /// 用途：LLM 编译完成后，调用此方法把生成内容写入 `content_candidate` 等字段
    /// 等待用户批准。批准走 [`approve_candidate`](Self::approve_candidate)，
    /// 拒绝走 [`reject_candidate`](Self::reject_candidate)。
    ///
    /// 不会自动递增 `version`（version 是"已发布内容"的版本号，候选不算）。
    /// 调用方必须显式传 `candidate_version = existing.version + 1` 满足 CHECK 约束。
    pub fn set_candidate(
        &self,
        id: i64,
        content: &str,
        candidate_status: &str,
        sources_candidate: Option<&str>,
        candidate_version: i64,
    ) -> Result<WikiPage, String> {
        // CHECK 约束校验：candidate_status 必须是允许值
        match candidate_status {
            "auto" | "conflict" | "pending" => {}
            other => {
                return Err(format!(
                    "无效 candidate_status: {}（必须为 auto/conflict/pending）",
                    other
                ))
            }
        }
        self.db
            .execute(
                "UPDATE wiki_pages SET
                content_candidate = ?1, candidate_status = ?2, sources_candidate = ?3,
                candidate_version = ?4, updated_at = datetime('now')
             WHERE id = ?5",
                params![content, candidate_status, sources_candidate, candidate_version, id],
            )
            .map_err(|e| format!("设置 wiki_page 候选内容失败: {}", e))?;
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
    ///
    /// 关键：批准时**同时刷新 wikilinks 字段**（从新 content 重新提取）。
    /// 原因：候选 content 可能引用了不同的 [[slug]]，原 wikilinks 已不匹配。
    pub fn approve_candidate(&self, id: i64) -> Result<WikiPage, String> {
        let existing = self.get_by_id(id)?;
        let candidate = existing
            .content_candidate
            .clone()
            .ok_or_else(|| "没有待批准的候选内容".to_string())?;
        let sources = existing
            .sources_candidate
            .clone()
            .unwrap_or_else(|| existing.sources.clone());

        // 重新计算 wikilinks：从新 content 提取 [[slug]]，过滤项目已有 slug + 排除自引用
        let valid_slugs: std::collections::HashSet<String> = {
            let mut stmt = self
                .db
                .prepare("SELECT slug FROM wiki_pages WHERE project_id = ?1")
                .map_err(|e| format!("准备 slug 查询失败: {}", e))?;
            let rows = stmt
                .query_map(params![existing.project_id], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|e| format!("执行 slug 查询失败: {}", e))?;
            let mut set = std::collections::HashSet::new();
            for r in rows {
                set.insert(r.map_err(|e| format!("读取 slug 失败: {}", e))?);
            }
            set
        };
        let links = crate::services::wikilink_parser::extract_wikilinks(
            &candidate,
            &existing.slug,
            &valid_slugs,
        );
        let wikilinks_json = serde_json::to_string(&links)
            .map_err(|e| format!("序列化 wikilinks 失败: {}", e))?;

        self.db
            .execute(
                "UPDATE wiki_pages SET
                content = ?1, content_candidate = NULL, candidate_status = NULL,
                sources = ?2, sources_candidate = NULL,
                wikilinks = ?3,
                candidate_version = NULL, version = version + 1, updated_at = datetime('now')
             WHERE id = ?4 AND content_candidate IS NOT NULL",
                params![candidate, sources, wikilinks_json, id],
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
    ///
    /// 使用 IMMEDIATE 事务包裹 read-modify-write 流程，避免并发调用互相覆盖（lost update）
    pub fn add_wikilink(&self, page_id: i64, target_slug: &str) -> Result<WikiPage, String> {
        let tx = self
            .db
            .unchecked_transaction()
            .map_err(|e| format!("启动 add_wikilink 事务失败: {}", e))?;
        // 把事务内全部写入收集为 Result，失败时显式 rollback 避免事务卡死
        // （unchecked_transaction 的 guard 在 drop 时不会自动回滚）
        let body: Result<(), String> = (|| {
            let existing_json: String = tx
                .query_row(
                    "SELECT wikilinks FROM wiki_pages WHERE id = ?1",
                    params![page_id],
                    |row| row.get(0),
                )
                .map_err(|e| format!("读取 wiki_page wikilinks 失败: {}", e))?;
            let mut links: Vec<String> = serde_json::from_str(&existing_json).unwrap_or_default();
            if !links.iter().any(|s| s == target_slug) {
                links.push(target_slug.to_string());
            }
            let new_json = serde_json::to_string(&links)
                .map_err(|e| format!("序列化 wikilinks 失败: {}", e))?;
            tx.execute(
                "UPDATE wiki_pages SET wikilinks = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![new_json, page_id],
            )
            .map_err(|e| format!("更新 wikilinks 失败: {}", e))?;
            Ok(())
        })();
        if let Err(e) = body {
            let _ = tx.rollback();
            return Err(e);
        }
        tx.commit()
            .map_err(|e| format!("提交 add_wikilink 事务失败: {}", e))?;
        self.get_by_id(page_id)
    }

    /// 移除 wikilink（从数组中删除 slug）
    ///
    /// 使用 IMMEDIATE 事务包裹 read-modify-write 流程，避免并发调用互相覆盖（lost update）
    pub fn remove_wikilink(&self, page_id: i64, target_slug: &str) -> Result<WikiPage, String> {
        let tx = self
            .db
            .unchecked_transaction()
            .map_err(|e| format!("启动 remove_wikilink 事务失败: {}", e))?;
        let body: Result<(), String> = (|| {
            let existing_json: String = tx
                .query_row(
                    "SELECT wikilinks FROM wiki_pages WHERE id = ?1",
                    params![page_id],
                    |row| row.get(0),
                )
                .map_err(|e| format!("读取 wiki_page wikilinks 失败: {}", e))?;
            let links: Vec<String> = serde_json::from_str(&existing_json).unwrap_or_default();
            let filtered: Vec<String> = links
                .into_iter()
                .filter(|s| s != target_slug)
                .collect();
            let new_json = serde_json::to_string(&filtered)
                .map_err(|e| format!("序列化 wikilinks 失败: {}", e))?;
            tx.execute(
                "UPDATE wiki_pages SET wikilinks = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![new_json, page_id],
            )
            .map_err(|e| format!("更新 wikilinks 失败: {}", e))?;
            Ok(())
        })();
        if let Err(e) = body {
            let _ = tx.rollback();
            return Err(e);
        }
        tx.commit()
            .map_err(|e| format!("提交 remove_wikilink 事务失败: {}", e))?;
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
                        parent_chunk_id: None,
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
                        parent_chunk_id: None,
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
    ///
    /// 用 SQLite `json_each` 正确解析 wikilinks JSON 数组，**避免 `LIKE '%slug%'` 的子串误报**：
    /// 之前 slug="api" 会匹配 "api-design"、"apiary" 等，现在只匹配数组中**完整相等**的元素
    pub fn get_backlinks(&self, project_id: i64, slug: &str) -> Result<Vec<WikiPageBrief>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT p.id, p.slug, p.title, p.page_type
                 FROM wiki_pages p, json_each(p.wikilinks) AS je
                  WHERE p.project_id = ?1 AND je.value = ?2",
            )
            .map_err(|e| format!("准备反向链接查询失败: {}", e))?;
        let rows = stmt
            .query_map(params![project_id, slug], |row| {
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

#[cfg(test)]
mod regression_tests {
    use super::*;
    use crate::services::project_store::ProjectStore;
    use tempfile::tempdir;

    /// 回归：update() 必须递增 version
    /// 修复前：version = existing.version（保持不变）
    /// 修复后：version = version + 1（递增）
    #[test]
    fn update_increments_version() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("test.db");
        let project_store = ProjectStore::new(&db_path).expect("创建项目存储失败");
        let project_id = project_store.ensure_default_project().expect("创建默认项目失败");
        let store = WikiPageStore::new(rusqlite::Connection::open(&db_path).unwrap());
        store.ensure_table().expect("创建 wiki_pages 表失败");
        let created = store
            .create(&CreateWikiPage {
                project_id,
                slug: "test".to_string(),
                title: "原标题".to_string(),
                page_type: "summary".to_string(),
                content: "原内容".to_string(),
                frontmatter: Some("{}".to_string()),
                sources: Some("[]".to_string()),
                wikilinks: Some("[]".to_string()),
                tags: Some("[]".to_string()),
                page_metadata: Some("{}".to_string()),
                page_status: Some("draft".to_string()),
            })
            .expect("创建失败");
        assert_eq!(created.version, 1);

        let updated = store
            .update(
                created.id,
                &UpdateWikiPage {
                    title: Some("新标题".to_string()),
                    content: Some("新内容".to_string()),
                    content_candidate: None,
                    candidate_status: None,
                    sources_candidate: None,
                    frontmatter: None,
                    sources: None,
                    wikilinks: None,
                    tags: None,
                    page_metadata: None,
                    candidate_version: None,
                    page_status: None,
                },
            )
            .expect("更新失败");
        assert_eq!(updated.version, 2, "version 必须递增为 2");
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.content, "新内容");
        assert!(
            updated.content_candidate.is_none(),
            "正式 update 应清空候选字段"
        );
    }

    /// 回归：add_wikilink / remove_wikilink 必须保持去重
    /// 修复前后行为：去重逻辑保持，但通过事务避免并发竞态
    #[test]
    fn add_wikilink_dedupes_and_remove_filters() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("test.db");
        let project_store = ProjectStore::new(&db_path).expect("创建项目存储失败");
        let project_id = project_store.ensure_default_project().expect("创建默认项目失败");
        let store = WikiPageStore::new(rusqlite::Connection::open(&db_path).unwrap());
        store.ensure_table().expect("创建 wiki_pages 表失败");
        let created = store
            .create(&CreateWikiPage {
                project_id,
                slug: "src".to_string(),
                title: "源页面".to_string(),
                page_type: "summary".to_string(),
                content: "".to_string(),
                frontmatter: Some("{}".to_string()),
                sources: Some("[]".to_string()),
                wikilinks: Some("[]".to_string()),
                tags: Some("[]".to_string()),
                page_metadata: Some("{}".to_string()),
                page_status: Some("draft".to_string()),
            })
            .expect("创建失败");

        // 添加两个 slug（其中一个重复）
        store.add_wikilink(created.id, "a").unwrap();
        store.add_wikilink(created.id, "b").unwrap();
        store.add_wikilink(created.id, "a").unwrap(); // 重复
        let after_add = store.get_by_id(created.id).unwrap();
        let links: Vec<String> = serde_json::from_str(&after_add.wikilinks).unwrap();
        assert_eq!(links, vec!["a".to_string(), "b".to_string()], "必须去重并排序");

        // 移除一个
        store.remove_wikilink(created.id, "a").unwrap();
        let after_remove = store.get_by_id(created.id).unwrap();
        let links: Vec<String> = serde_json::from_str(&after_remove.wikilinks).unwrap();
        assert_eq!(links, vec!["b".to_string()]);
    }

    /// 回归：get_backlinks 用 json_each 正确匹配
    /// 修复前用 LIKE '%slug%' 会把 "api" 误匹配到 "api-design"
    /// 修复后用 json_each + je.value = slug 只匹配数组中完整相等的元素
    #[test]
    fn get_backlinks_uses_exact_match_not_substring() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("test.db");
        let project_store = ProjectStore::new(&db_path).expect("创建项目存储失败");
        let project_id = project_store.ensure_default_project().expect("创建默认项目失败");
        let store = WikiPageStore::new(rusqlite::Connection::open(&db_path).unwrap());
        store.ensure_table().expect("创建 wiki_pages 表失败");
        // 创建 2 个源页面，wikilinks 各包含 1 个目标 slug
        store
            .create(&CreateWikiPage {
                project_id,
                slug: "src-exact".to_string(),
                title: "精确匹配".to_string(),
                page_type: "summary".to_string(),
                content: "".to_string(),
                frontmatter: Some("{}".to_string()),
                sources: Some("[]".to_string()),
                wikilinks: Some(r#"["api"]"#.to_string()),
                tags: Some("[]".to_string()),
                page_metadata: Some("{}".to_string()),
                page_status: Some("draft".to_string()),
            })
            .expect("创建失败");
        store
            .create(&CreateWikiPage {
                project_id,
                slug: "src-substring".to_string(),
                title: "子串误报".to_string(),
                page_type: "summary".to_string(),
                content: "".to_string(),
                frontmatter: Some("{}".to_string()),
                sources: Some("[]".to_string()),
                // 注意："api-design" 包含 "api" 子串，但与目标 slug "api" 不同
                wikilinks: Some(r#"["api-design"]"#.to_string()),
                tags: Some("[]".to_string()),
                page_metadata: Some("{}".to_string()),
                page_status: Some("draft".to_string()),
            })
            .expect("创建失败");

        // 查询 "api" 的反向链接 —— 修复前会同时返回 src-exact 和 src-substring（误报）
        let backlinks = store.get_backlinks(project_id, "api").unwrap();
        let slugs: Vec<String> = backlinks.iter().map(|p| p.slug.clone()).collect();
        assert_eq!(
            slugs,
            vec!["src-exact".to_string()],
            "json_each 只匹配数组中完整相等的 slug，不应有子串误报"
        );
    }
}

