//! 验证日志存储（verification_logs 表）
//!
//! 记录每次验证（自洽性/相关性/准确性）的结果，支持事后审计与分析。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// 验证日志记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationLog {
    pub id: i64,
    pub scenario: String,
    pub query: Option<String>,
    pub verification_level: String,
    pub overall_confidence: Option<f64>,
    pub checks_json: String,
    pub corrected_output: Option<String>,
    pub original_output: Option<String>,
    pub created_at: String,
}

/// 新增验证日志时的数据传输对象
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVerificationLog {
    pub scenario: String,
    pub query: Option<String>,
    pub verification_level: String,
    pub overall_confidence: Option<f64>,
    pub checks_json: String,
    pub corrected_output: Option<String>,
    pub original_output: Option<String>,
}

/// verification_logs 表的数据操作层
pub struct VerificationLogStore {
    db: Connection,
}

impl VerificationLogStore {
    /// 使用已有的数据库连接创建存储
    pub fn new(db: Connection) -> Self {
        Self { db }
    }

    /// 创建 verification_logs 表（幂等）
    pub fn ensure_table(&self) -> Result<(), String> {
        self.db
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS verification_logs (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                scenario            TEXT NOT NULL,
                query               TEXT,
                verification_level  TEXT NOT NULL,
                overall_confidence  REAL,
                checks_json         TEXT NOT NULL,
                corrected_output    TEXT,
                original_output     TEXT,
                created_at          TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_verification_logs_scenario
                ON verification_logs(scenario);
            CREATE INDEX IF NOT EXISTS idx_verification_logs_level
                ON verification_logs(verification_level);
            CREATE INDEX IF NOT EXISTS idx_verification_logs_created
                ON verification_logs(created_at);
            ",
            )
            .map_err(|e| format!("创建 verification_logs 表失败: {}", e))
    }

    /// 插入一条验证日志
    pub fn insert(&self, input: &CreateVerificationLog) -> Result<VerificationLog, String> {
        self.db
            .execute(
                "INSERT INTO verification_logs
                    (scenario, query, verification_level, overall_confidence, checks_json, corrected_output, original_output)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    input.scenario,
                    input.query,
                    input.verification_level,
                    input.overall_confidence,
                    input.checks_json,
                    input.corrected_output,
                    input.original_output,
                ],
            )
            .map_err(|e| format!("插入 verification_logs 失败: {}", e))?;

        let id = self.db.last_insert_rowid();
        self.get_by_id(id)?
            .ok_or_else(|| "插入后未找到 verification_logs 记录".to_string())
    }

    /// 按 ID 查询
    pub fn get_by_id(&self, id: i64) -> Result<Option<VerificationLog>, String> {
        self.query_one(
            "SELECT id, scenario, query, verification_level, overall_confidence,
                    checks_json, corrected_output, original_output, created_at
             FROM verification_logs
             WHERE id = ?1",
            params![id],
        )
    }

    /// 按场景列出日志（最近优先）
    pub fn list_by_scenario(
        &self,
        scenario: &str,
        limit: i64,
    ) -> Result<Vec<VerificationLog>, String> {
        self.query_list(
            "SELECT id, scenario, query, verification_level, overall_confidence,
                    checks_json, corrected_output, original_output, created_at
             FROM verification_logs
             WHERE scenario = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
            params![scenario, limit],
        )
    }

    /// 列出所有日志（最近优先）
    pub fn list(&self, limit: i64) -> Result<Vec<VerificationLog>, String> {
        self.query_list(
            "SELECT id, scenario, query, verification_level, overall_confidence,
                    checks_json, corrected_output, original_output, created_at
             FROM verification_logs
             ORDER BY created_at DESC
             LIMIT ?1",
            params![limit],
        )
    }

    /// 按验证级别统计
    pub fn count_by_level(&self) -> Result<Vec<(String, i64)>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT verification_level, COUNT(*) as cnt
                 FROM verification_logs
                 GROUP BY verification_level
                 ORDER BY cnt DESC",
            )
            .map_err(|e| format!("准备查询失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| format!("执行查询失败: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("读取行失败: {}", e))?);
        }
        Ok(results)
    }

    // ─── 私有辅助方法 ───

    fn row_to_log(row: &rusqlite::Row) -> rusqlite::Result<VerificationLog> {
        Ok(VerificationLog {
            id: row.get(0)?,
            scenario: row.get(1)?,
            query: row.get(2)?,
            verification_level: row.get(3)?,
            overall_confidence: row.get(4)?,
            checks_json: row.get(5)?,
            corrected_output: row.get(6)?,
            original_output: row.get(7)?,
            created_at: row.get(8)?,
        })
    }

    fn query_one(
        &self,
        sql: &str,
        p: impl rusqlite::Params,
    ) -> Result<Option<VerificationLog>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;

        let mut rows = stmt
            .query_map(p, Self::row_to_log)
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
    ) -> Result<Vec<VerificationLog>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;

        let rows = stmt
            .query_map(p, Self::row_to_log)
            .map_err(|e| format!("执行查询失败: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("读取行失败: {}", e))?);
        }
        Ok(results)
    }
}
