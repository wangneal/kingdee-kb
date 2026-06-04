//! 验证缓存（verification_cache 表）
//!
//! 避免对相同 query+context 重复验证。
//! 缓存 key: SHA256(query + context_hash)
//! TTL: 24 小时

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// 验证缓存记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCache {
    pub id: i64,
    pub query_hash: String,
    pub result_json: String,
    pub created_at: String,
    pub expires_at: String,
}

/// 验证缓存数据操作层
pub struct VerificationCacheStore {
    db: Connection,
}

impl VerificationCacheStore {
    pub fn new(db: Connection) -> Self {
        Self { db }
    }

    pub fn ensure_table(&self) -> Result<(), String> {
        self.db
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS verification_cache (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                query_hash       TEXT NOT NULL,
                result_json      TEXT NOT NULL,
                created_at       TEXT NOT NULL DEFAULT (datetime('now')),
                expires_at       TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_cache_hash ON verification_cache(query_hash);
            ",
            )
            .map_err(|e| format!("创建 verification_cache 表失败: {}", e))
    }

    /// 插入缓存记录
    pub fn insert(
        &self,
        query_hash: &str,
        result_json: &str,
        ttl_hours: i64,
    ) -> Result<VerificationCache, String> {
        self.db
            .prepare_cached(
                "INSERT INTO verification_cache (query_hash, result_json, expires_at)
                 VALUES (?1, ?2, datetime('now', ?3))",
            )
            .map_err(|e| format!("prepare 失败: {}", e))?
            .insert(params![
                query_hash,
                result_json,
                format!("+{} hours", ttl_hours)
            ])
            .map_err(|e| format!("插入 verification_cache 失败: {}", e))?;

        let id = self.db.last_insert_rowid();
        self.get_by_id(id)?
            .ok_or_else(|| "插入后读取失败".to_string())
    }

    pub fn get_by_id(&self, id: i64) -> Result<Option<VerificationCache>, String> {
        self.db
            .prepare_cached("SELECT id, query_hash, result_json, created_at, expires_at FROM verification_cache WHERE id = ?1")
            .map_err(|e| format!("prepare 失败: {}", e))?
            .query_row(params![id], |row| {
                Ok(VerificationCache {
                    id: row.get(0)?,
                    query_hash: row.get(1)?,
                    result_json: row.get(2)?,
                    created_at: row.get(3)?,
                    expires_at: row.get(4)?,
                })
            })
            .ok()
            .map_or(Ok(None), |r| Ok(Some(r)))
    }

    /// 按 query_hash 查找未过期的缓存
    pub fn find_valid(&self, query_hash: &str) -> Result<Option<String>, String> {
        self.db
            .prepare_cached(
                "SELECT result_json FROM verification_cache
                 WHERE query_hash = ?1 AND expires_at > datetime('now')
                 ORDER BY id DESC LIMIT 1",
            )
            .map_err(|e| format!("prepare 失败: {}", e))?
            .query_row(params![query_hash], |row| row.get(0))
            .ok()
            .map_or(Ok(None), |r| Ok(Some(r)))
    }

    /// 清理过期缓存
    pub fn clean_expired(&self) -> Result<usize, String> {
        let n = self
            .db
            .prepare_cached("DELETE FROM verification_cache WHERE expires_at <= datetime('now')")
            .map_err(|e| format!("prepare 失败: {}", e))?
            .execute([])
            .map_err(|e| format!("清理过期缓存失败: {}", e))?;
        Ok(n)
    }
}
