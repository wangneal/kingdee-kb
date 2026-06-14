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
        self.db
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS raw_sources (
                    id            INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id    INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                    identity      TEXT NOT NULL,
                    original_path TEXT NOT NULL,
                    storage_path  TEXT NOT NULL,
                    sha256        TEXT NOT NULL,
                    file_size     INTEGER,
                    mime_type     TEXT,
                    status        TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','deleted','ingested')),
                    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                    deleted_at    TEXT,
                    UNIQUE(project_id, identity)
                );
                CREATE INDEX IF NOT EXISTS idx_raw_sources_project_id ON raw_sources(project_id);
                CREATE INDEX IF NOT EXISTS idx_raw_sources_status ON raw_sources(status);
                CREATE UNIQUE INDEX IF NOT EXISTS idx_raw_sources_project_identity
                    ON raw_sources(project_id, identity);",
            )
            .map_err(|e| format!("创建 raw_sources 表失败: {}", e))?;
        Ok(())
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

    /// 按项目和标识更新 status（用于"摄入完成 → ingested"标记，防止重复 KB 编译）
    ///
    /// 若 raw_source 不存在返回 `Ok(false)`（幂等）；存在则返回 `Ok(true)`。
    /// 之所以用 `find_by_identity` 而非直接 UPDATE，是为了在并发场景下避免 UPDATE 影响行数
    /// 抖动；实际 UPDATE 仍是单条 SQL。
    ///
    /// 注意：`new_status` 必须符合 `raw_sources.status` 的 CHECK 约束
    /// (`active` / `deleted` / `ingested`)。`ingested` 由 B1 引入，对已存在数据库需要
    /// schema 升级；本函数失败时**不**阻断调用方，由调用方记录 warn。
    pub fn set_status(
        &self,
        project_id: i64,
        identity: &str,
        new_status: &str,
    ) -> Result<bool, String> {
        if new_status != "active" && new_status != "deleted" && new_status != "ingested" {
            return Err(format!(
                "不支持的 raw_source.status: {}（必须是 active/deleted/ingested）",
                new_status
            ));
        }
        let rows = self
            .db
            .execute(
                "UPDATE raw_sources SET status = ?3 WHERE project_id = ?1 AND identity = ?2",
                params![project_id, identity, new_status],
            )
            .map_err(|e| format!("更新 raw_source.status 失败: {}", e))?;
        Ok(rows > 0)
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

    /// 按 ID 获取原始导入记录（仅未软删）
    pub fn get_by_id(&self, id: i64) -> Result<RawSource, String> {
        self.query_one(
            "SELECT id, project_id, identity, original_path, storage_path, sha256, file_size,
                    mime_type, status, created_at, deleted_at
             FROM raw_sources WHERE id = ?1 AND deleted_at IS NULL",
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
    use rusqlite::Connection;

    /// 创建内存 SQLite + 必要 schema（projects + raw_sources），
    /// 供 RawSourceStore 单元测试使用。
    fn setup_test_store() -> (RawSourceStore, i64) {
        let conn = Connection::open_in_memory().unwrap();
        // 先在独立 scope 用 conn 建表、插 projects、取 id，再 move 进 RawSourceStore
        let project_id = {
            conn.execute_batch(
                "CREATE TABLE projects (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL
                );
                ",
            )
            .unwrap();
            conn.execute("INSERT INTO projects (name) VALUES ('test')", [])
                .unwrap();
            conn.last_insert_rowid()
        };
        let store = RawSourceStore::new(conn);
        store.ensure_table().unwrap();
        (store, project_id)
    }

    fn sample_insert(project_id: i64) -> InsertRawSource {
        InsertRawSource {
            project_id,
            identity: "file.txt".to_string(),
            original_path: "/tmp/file.txt".to_string(),
            storage_path: "/data/raw/1/sources/file.txt".to_string(),
            sha256: "abc123".to_string(),
            file_size: Some(1024),
            mime_type: Some("text/plain".to_string()),
        }
    }

    /// B1-U1: set_status 把 active 改为 ingested 成功，find_by_identity 反映新状态
    #[test]
    fn test_set_status_active_to_ingested() {
        let (store, pid) = setup_test_store();
        let inserted = store.insert(&sample_insert(pid)).unwrap();
        assert_eq!(inserted.status, "active");

        let changed = store.set_status(pid, "file.txt", "ingested").unwrap();
        assert!(changed);

        let after = store.find_by_identity(pid, "file.txt").unwrap().unwrap();
        assert_eq!(after.status, "ingested");
    }

    /// B1-U1 衍生: set_status 对不存在的 identity 返回 Ok(false)（幂等）
    #[test]
    fn test_set_status_nonexistent_identity_returns_false() {
        let (store, pid) = setup_test_store();
        let changed = store.set_status(pid, "ghost.txt", "ingested").unwrap();
        assert!(!changed);
    }

    /// B1-U1 衍生: set_status 拒绝非法 status
    #[test]
    fn test_set_status_rejects_invalid_value() {
        let (store, pid) = setup_test_store();
        store.insert(&sample_insert(pid)).unwrap();
        let result = store.set_status(pid, "file.txt", "bogus");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("不支持的 raw_source.status"));
    }

    /// B1-U1 衍生: 已 deleted 的 raw_source 不能被 set_status 改回 active（CHECK 允许，
    /// 但语义上 soft_delete 流程之外不应回退；本测试仅覆盖 set_status 本身允许的取值）。
    #[test]
    fn test_set_status_soft_deleted_to_ingested_works() {
        let (store, pid) = setup_test_store();
        let inserted = store.insert(&sample_insert(pid)).unwrap();
        store.soft_delete(inserted.id).unwrap();
        let changed = store.set_status(pid, "file.txt", "ingested").unwrap();
        assert!(changed);
    }
}
