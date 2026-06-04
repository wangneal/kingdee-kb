//! 分析缓存管理（analysis_cache 表）
//!
//! 缓存源代码分析结果，以 project_id + source_identity + sha256 为唯一键实现去重。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// 分析缓存记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisCache {
    pub id: i64,
    pub project_id: i64,
    pub source_identity: String,
    pub sha256: String,
    pub analysis_json: String,
    pub analyzer_version: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 创建分析缓存时的数据传输对象
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAnalysisCache {
    pub project_id: i64,
    pub source_identity: String,
    pub sha256: String,
    pub analysis_json: String,
    pub analyzer_version: Option<String>,
}

/// analysis_cache 表的数据操作层
pub struct AnalysisCacheStore {
    db: Connection,
}

impl AnalysisCacheStore {
    /// 使用已有的数据库连接创建存储
    pub fn new(db: Connection) -> Self {
        let _ = db.busy_timeout(std::time::Duration::from_secs(5));
        Self { db }
    }

    /// 创建 analysis_cache 表（幂等）
    pub fn ensure_table(&self) -> Result<(), String> {
        if self.has_column("analysis_cache", "project")? {
            self.migrate_legacy_table()?;
        }
        self.db
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS analysis_cache (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id       INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                source_identity  TEXT NOT NULL,
                sha256           TEXT NOT NULL,
                analysis_json    TEXT NOT NULL,
                analyzer_version TEXT NOT NULL DEFAULT '1',
                created_at       TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at       TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(project_id, source_identity, sha256)
            );

            ",
            )
            .map_err(|e| format!("创建 analysis_cache 表失败: {}", e))?;
        self.ensure_column("analysis_cache", "project_id", "INTEGER")?;
        self.backfill_project_id("analysis_cache")?;
        self.db
            .execute_batch(
                "
            CREATE INDEX IF NOT EXISTS idx_analysis_cache_project_id ON analysis_cache(project_id);
            CREATE INDEX IF NOT EXISTS idx_analysis_cache_source ON analysis_cache(source_identity);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_analysis_cache_project_source_sha
                ON analysis_cache(project_id, source_identity, sha256);
            ",
            )
            .map_err(|e| format!("创建 analysis_cache 索引失败: {}", e))?;
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
            CREATE TABLE analysis_cache_new (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id       INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                source_identity  TEXT NOT NULL,
                sha256           TEXT NOT NULL,
                analysis_json    TEXT NOT NULL,
                analyzer_version TEXT NOT NULL DEFAULT '1',
                created_at       TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at       TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(project_id, source_identity, sha256)
            );
            INSERT INTO analysis_cache_new (
                id, project_id, source_identity, sha256, analysis_json,
                analyzer_version, created_at, updated_at
            )
            SELECT id,
                   COALESCE(
                       (SELECT id FROM projects WHERE name = analysis_cache.project LIMIT 1),
                       (SELECT id FROM projects WHERE name = '默认项目' LIMIT 1),
                       (SELECT id FROM projects WHERE status = 'active' ORDER BY id ASC LIMIT 1)
                   ),
                   source_identity, sha256, analysis_json,
                   analyzer_version, created_at, updated_at
            FROM analysis_cache;
            DROP TABLE analysis_cache;
            ALTER TABLE analysis_cache_new RENAME TO analysis_cache;
            COMMIT;
        ";
        if let Err(e) = self.db.execute_batch(sql) {
            let _ = self.db.execute_batch("ROLLBACK;");
            return Err(format!("迁移旧版 analysis_cache 表失败: {}", e));
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

    /// 插入或替换分析缓存记录
    pub fn upsert(&self, input: &CreateAnalysisCache) -> Result<AnalysisCache, String> {
        self.db
            .execute(
                "INSERT INTO analysis_cache (project_id, source_identity, sha256, analysis_json, analyzer_version)
                  VALUES (?1, ?2, ?3, ?4, ?5)
                  ON CONFLICT(project_id, source_identity, sha256) DO UPDATE SET
                    analysis_json = excluded.analysis_json,
                    analyzer_version = excluded.analyzer_version,
                    updated_at = datetime('now')",
                params![
                    input.project_id,
                    input.source_identity,
                    input.sha256,
                    input.analysis_json,
                    input.analyzer_version.as_deref().unwrap_or("1"),
                ],
            )
            .map_err(|e| format!("插入/更新 analysis_cache 失败: {}", e))?;

        self.get_by_key(input.project_id, &input.source_identity, &input.sha256)?
            .ok_or_else(|| "插入后未找到 analysis_cache 记录".to_string())
    }

    /// 按唯一键获取缓存
    pub fn get_by_key(
        &self,
        project_id: i64,
        source_identity: &str,
        sha256: &str,
    ) -> Result<Option<AnalysisCache>, String> {
        self.query_one(
            "SELECT id, project_id, source_identity, sha256, analysis_json, analyzer_version, created_at, updated_at
              FROM analysis_cache
              WHERE project_id = ?1 AND source_identity = ?2 AND sha256 = ?3",
            params![project_id, source_identity, sha256],
        )
    }

    /// 列出项目的所有分析缓存
    pub fn list_by_project(&self, project_id: i64) -> Result<Vec<AnalysisCache>, String> {
        self.query_list(
            "SELECT id, project_id, source_identity, sha256, analysis_json, analyzer_version, created_at, updated_at
              FROM analysis_cache
              WHERE project_id = ?1
              ORDER BY updated_at DESC",
            params![project_id],
        )
    }

    /// 删除一条缓存记录
    pub fn delete(&self, id: i64) -> Result<(), String> {
        let rows = self
            .db
            .execute("DELETE FROM analysis_cache WHERE id = ?1", params![id])
            .map_err(|e| format!("删除 analysis_cache 失败: {}", e))?;

        if rows == 0 {
            return Err(format!("analysis_cache 未找到: id={}", id));
        }
        Ok(())
    }

    /// 清空项目的所有分析缓存
    pub fn delete_by_project(&self, project_id: i64) -> Result<usize, String> {
        let rows = self
            .db
            .execute(
                "DELETE FROM analysis_cache WHERE project_id = ?1",
                params![project_id],
            )
            .map_err(|e| format!("清空项目 analysis_cache 失败: {}", e))?;
        Ok(rows)
    }

    // ─── 私有辅助方法 ───

    fn row_to_cache(row: &rusqlite::Row) -> rusqlite::Result<AnalysisCache> {
        Ok(AnalysisCache {
            id: row.get(0)?,
            project_id: row.get(1)?,
            source_identity: row.get(2)?,
            sha256: row.get(3)?,
            analysis_json: row.get(4)?,
            analyzer_version: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    }

    fn query_one(
        &self,
        sql: &str,
        p: impl rusqlite::Params,
    ) -> Result<Option<AnalysisCache>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;

        let mut rows = stmt
            .query_map(p, Self::row_to_cache)
            .map_err(|e| format!("执行查询失败: {}", e))?;

        match rows.next() {
            Some(Ok(s)) => Ok(Some(s)),
            Some(Err(e)) => Err(format!("读取行失败: {}", e)),
            None => Ok(None),
        }
    }

    fn query_list(
        &self,
        sql: &str,
        p: impl rusqlite::Params,
    ) -> Result<Vec<AnalysisCache>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;

        let rows = stmt
            .query_map(p, Self::row_to_cache)
            .map_err(|e| format!("执行查询失败: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("读取行失败: {}", e))?);
        }
        Ok(results)
    }
}
