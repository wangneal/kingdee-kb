//! 原始导入文件管理（raw_sources 表）
//!
//! 记录导入知识库的原始文件，使用 SHA256 指纹实现去重，
//! 支持软删除（标记删除而非物理删除）。

use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};

/// 原始导入文件记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSource {
    pub id: i64,
    pub project_id: i64,
    pub identity: String,
    pub original_path: String,
    pub storage_path: String,
    pub sha256: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub status: String,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

/// raw_sources 表的数据操作层
pub struct RawSourceStore {
    db: Connection,
}

impl RawSourceStore {
    /// 使用已有的数据库连接创建存储
    pub fn new(db: Connection) -> Self {
        let _ = db.busy_timeout(std::time::Duration::from_secs(5));
        Self { db }
    }

    /// 创建 raw_sources 表及其索引（幂等）
    pub fn ensure_table(&self) -> Result<(), String> {
        if self.has_column("raw_sources", "project")? {
            self.migrate_legacy_table()?;
        }
        self.db
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS raw_sources (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id    INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                identity      TEXT NOT NULL,
                original_path TEXT NOT NULL,
                storage_path  TEXT NOT NULL,
                sha256        TEXT NOT NULL,
                file_size     INTEGER,
                mime_type     TEXT,
                status        TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','deleted')),
                created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                deleted_at    TEXT,
                UNIQUE(project_id, identity)
            );

            ",
            )
            .map_err(|e| format!("创建 raw_sources 表失败: {}", e))?;
        self.ensure_column("raw_sources", "project_id", "INTEGER")?;
        self.backfill_project_id("raw_sources")?;
        self.db
            .execute_batch(
                "
            CREATE INDEX IF NOT EXISTS idx_raw_sources_project_id ON raw_sources(project_id);
            CREATE INDEX IF NOT EXISTS idx_raw_sources_status ON raw_sources(status);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_raw_sources_project_identity
                ON raw_sources(project_id, identity);
            ",
            )
            .map_err(|e| format!("创建 raw_sources 索引失败: {}", e))?;
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
            CREATE TABLE raw_sources_new (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id    INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                identity      TEXT NOT NULL,
                original_path TEXT NOT NULL,
                storage_path  TEXT NOT NULL,
                sha256        TEXT NOT NULL,
                file_size     INTEGER,
                mime_type     TEXT,
                status        TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','deleted')),
                created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                deleted_at    TEXT,
                UNIQUE(project_id, identity)
            );
            INSERT INTO raw_sources_new (
                id, project_id, identity, original_path, storage_path, sha256,
                file_size, mime_type, status, created_at, deleted_at
            )
            SELECT id,
                   COALESCE(
                       (SELECT id FROM projects WHERE name = raw_sources.project LIMIT 1),
                       (SELECT id FROM projects WHERE name = '默认项目' LIMIT 1),
                       (SELECT id FROM projects WHERE status = 'active' ORDER BY id ASC LIMIT 1)
                   ),
                   identity, original_path, storage_path, sha256,
                   file_size, mime_type, status, created_at, deleted_at
            FROM raw_sources;
            DROP TABLE raw_sources;
            ALTER TABLE raw_sources_new RENAME TO raw_sources;
            COMMIT;
        ";
        if let Err(e) = self.db.execute_batch(sql) {
            let _ = self.db.execute_batch("ROLLBACK;");
            return Err(format!("迁移旧版 raw_sources 表失败: {}", e));
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

    /// 插入一条新记录，返回插入后的完整 RawSource
    pub fn insert(&self, source: &InsertRawSource) -> Result<RawSource, String> {
        self.db
            .execute(
                "INSERT INTO raw_sources (project_id, identity, original_path, storage_path, sha256, file_size, mime_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    source.project_id,
                    source.identity,
                    source.original_path,
                    source.storage_path,
                    source.sha256,
                    source.file_size,
                    source.mime_type,
                ],
            )
            .map_err(|e| format!("插入 raw_source 失败: {}", e))?;

        let id = self.db.last_insert_rowid();
        self.get_by_id(id)
    }

    /// 按项目列出所有 active 的源文件
    pub fn list_by_project(&self, project_id: i64) -> Result<Vec<RawSource>, String> {
        self.query_list(
            "SELECT id, project_id, identity, original_path, storage_path, sha256, file_size,
                    mime_type, status, created_at, deleted_at
             FROM raw_sources
              WHERE project_id = ?1 AND status = 'active'
              ORDER BY created_at DESC",
            params![project_id],
        )
    }

    /// 按项目和标识查找（返回任意状态）
    pub fn find_by_identity(
        &self,
        project_id: i64,
        identity: &str,
    ) -> Result<Option<RawSource>, String> {
        self.query_one(
            "SELECT id, project_id, identity, original_path, storage_path, sha256, file_size,
                    mime_type, status, created_at, deleted_at
             FROM raw_sources
              WHERE project_id = ?1 AND identity = ?2",
            params![project_id, identity],
        )
    }

    /// 软删除：将状态标记为 deleted 并记录删除时间
    pub fn soft_delete(&self, id: i64) -> Result<(), String> {
        let rows = self
            .db
            .execute(
                "UPDATE raw_sources SET status = 'deleted', deleted_at = datetime('now') WHERE id = ?1",
                params![id],
            )
            .map_err(|e| format!("软删除 raw_source 失败: {}", e))?;

        if rows == 0 {
            return Err(format!("raw_source 未找到: {}", id));
        }
        Ok(())
    }

    // ─── 私有辅助方法 ───

    fn row_to_source(row: &rusqlite::Row) -> SqlResult<RawSource> {
        Ok(RawSource {
            id: row.get(0)?,
            project_id: row.get(1)?,
            identity: row.get(2)?,
            original_path: row.get(3)?,
            storage_path: row.get(4)?,
            sha256: row.get(5)?,
            file_size: row.get(6)?,
            mime_type: row.get(7)?,
            status: row.get(8)?,
            created_at: row.get(9)?,
            deleted_at: row.get(10)?,
        })
    }

    fn get_by_id(&self, id: i64) -> Result<RawSource, String> {
        self.query_one(
            "SELECT id, project_id, identity, original_path, storage_path, sha256, file_size,
                    mime_type, status, created_at, deleted_at
             FROM raw_sources WHERE id = ?1",
            params![id],
        )?
        .ok_or_else(|| format!("未找到 raw_source: id={}", id))
    }

    fn query_one(&self, sql: &str, p: impl rusqlite::Params) -> Result<Option<RawSource>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;

        let mut rows = stmt
            .query_map(p, Self::row_to_source)
            .map_err(|e| format!("执行查询失败: {}", e))?;

        match rows.next() {
            Some(Ok(s)) => Ok(Some(s)),
            Some(Err(e)) => Err(format!("读取行失败: {}", e)),
            None => Ok(None),
        }
    }

    fn query_list(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<RawSource>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;

        let rows = stmt
            .query_map(p, Self::row_to_source)
            .map_err(|e| format!("执行查询失败: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("读取行失败: {}", e))?);
        }
        Ok(results)
    }
}

/// 插入 raw_source 时使用的数据传输对象（不含自动生成字段）
#[derive(Debug, Clone)]
pub struct InsertRawSource {
    pub project_id: i64,
    pub identity: String,
    pub original_path: String,
    pub storage_path: String,
    pub sha256: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_table_before_creating_project_index() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "
            CREATE TABLE projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL
            );
            INSERT INTO projects (name, status) VALUES ('项目甲', 'active'), ('项目乙', 'active');
            CREATE TABLE raw_sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project TEXT NOT NULL,
                identity TEXT NOT NULL,
                original_path TEXT NOT NULL,
                storage_path TEXT NOT NULL,
                sha256 TEXT NOT NULL,
                file_size INTEGER,
                mime_type TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                deleted_at TEXT
            );
            INSERT INTO raw_sources (project, identity, original_path, storage_path, sha256)
            VALUES
                ('项目甲', 'legacy.md', '/a/legacy.md', '/raw/a/legacy.md', 'legacy-sha-a'),
                ('项目乙', 'legacy.md', '/b/legacy.md', '/raw/b/legacy.md', 'legacy-sha-b');
            ",
        )
        .unwrap();

        let store = RawSourceStore::new(db);
        store.ensure_table().unwrap();

        let sources = store.list_by_project(1).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].identity, "legacy.md");
        assert_eq!(sources[0].project_id, 1);
        let other_sources = store.list_by_project(2).unwrap();
        assert_eq!(other_sources.len(), 1);
        assert_eq!(other_sources[0].identity, "legacy.md");
        assert_eq!(other_sources[0].project_id, 2);

        store
            .insert(&InsertRawSource {
                project_id: 1,
                identity: "new.md".to_string(),
                original_path: "/a/new.md".to_string(),
                storage_path: "/raw/a/new.md".to_string(),
                sha256: "new-sha".to_string(),
                file_size: None,
                mime_type: None,
            })
            .unwrap();
        assert_eq!(store.list_by_project(1).unwrap().len(), 2);
        assert!(!store.has_column("raw_sources", "project").unwrap());
    }
}
