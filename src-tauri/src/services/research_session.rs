//! Research session management — interview sessions and Q&A records
//!
//! Provides CRUD for research interview sessions and their Q&A records.
//! Sessions track which module/edition was discussed, with whom, and when.
//! Q&A records store answers to research questions with optional notes.
//! Supports export to CSV and Markdown formats.

use serde::{Deserialize, Serialize};

use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

// ─── Types ───

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    InProgress,
    Completed,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::InProgress => "in_progress",
            SessionStatus::Completed => "completed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "in_progress" => Some(SessionStatus::InProgress),
            "completed" => Some(SessionStatus::Completed),
            _ => None,
        }
    }
}

/// A research interview session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSession {
    pub id: i64,
    pub title: String,
    pub edition: String,
    pub module_code: String,
    pub interviewee: String,
    pub session_date: String,
    pub status: String,
    pub project: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A single Q&A record within a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QARecord {
    pub id: i64,
    pub session_id: i64,
    pub question_id: Option<i64>,
    pub question_text: String,
    pub answer_text: String,
    pub notes: String,
    pub sort_order: i32,
    /// 记录来源：auto（自动生成）或 manual（手动添加）
    pub source: String,
    /// 是否已收藏
    pub is_bookmarked: bool,
    pub created_at: String,
}

/// Full session detail with Q&A records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub session: ResearchSession,
    pub records: Vec<QARecord>,
}

// ─── Store ───

/// Persistent storage for research sessions and Q&A records.
pub struct ResearchSessionStore {
    conn: Mutex<Connection>,
}

impl ResearchSessionStore {
    /// Create a new store backed by the given SQLite database path.
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("Failed to open research session DB: {}", e))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_tables()?;
        Ok(store)
    }

    /// Create an in-memory store (for fallback when DB is corrupted).
    pub fn new_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("Failed to create in-memory research session DB: {}", e))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_tables()?;
        Ok(store)
    }

    fn init_tables(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        // Step 1: Create tables (without index on project yet)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS research_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                edition TEXT NOT NULL,
                module_code TEXT NOT NULL,
                interviewee TEXT DEFAULT '',
                session_date TEXT DEFAULT '',
                status TEXT NOT NULL DEFAULT 'in_progress',
                project TEXT NOT NULL DEFAULT 'default',
                created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS session_qa_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id INTEGER NOT NULL,
                question_id INTEGER,
                question_text TEXT NOT NULL,
                answer_text TEXT DEFAULT '',
                notes TEXT DEFAULT '',
                sort_order INTEGER DEFAULT 0,
                created_at TEXT DEFAULT (datetime('now')),
                FOREIGN KEY (session_id) REFERENCES research_sessions(id) ON DELETE CASCADE
            );",
        )
        .map_err(|e| format!("Failed to init session tables: {}", e))?;

        // Step 2: Migration — add project column if missing (for old DBs)
        let has_project = conn
            .prepare("SELECT project FROM research_sessions LIMIT 0")
            .is_ok();
        if !has_project {
            conn.execute(
                "ALTER TABLE research_sessions ADD COLUMN project TEXT NOT NULL DEFAULT 'default'",
                [],
            )
            .map_err(|e| format!("Failed to add project column: {}", e))?;
        }

        // Step 3: 迁移 — 为 session_qa_records 添加 source 和 is_bookmarked 列
        let has_source = {
            let mut stmt = conn
                .prepare("PRAGMA table_info(session_qa_records)")
                .map_err(|e| format!("Failed to query table info: {}", e))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| format!("Failed to read table info: {}", e))?;
            let found = rows.flatten().any(|col| col == "source");
            found
        };
        if !has_source {
            conn.execute(
                "ALTER TABLE session_qa_records ADD COLUMN source TEXT CHECK(source IN ('auto','manual')) DEFAULT 'auto'",
                [],
            )
            .map_err(|e| format!("Failed to add source column: {}", e))?;
        }

        let has_bookmarked = {
            let mut stmt = conn
                .prepare("PRAGMA table_info(session_qa_records)")
                .map_err(|e| format!("Failed to query table info: {}", e))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| format!("Failed to read table info: {}", e))?;
            let found = rows.flatten().any(|col| col == "is_bookmarked");
            found
        };
        if !has_bookmarked {
            conn.execute(
                "ALTER TABLE session_qa_records ADD COLUMN is_bookmarked INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(|e| format!("Failed to add is_bookmarked column: {}", e))?;
        }

        // Step 4: Create indexes (now safe because project column exists)
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_qa_session ON session_qa_records(session_id);
             CREATE INDEX IF NOT EXISTS idx_sessions_module ON research_sessions(module_code);
             CREATE INDEX IF NOT EXISTS idx_sessions_project ON research_sessions(project);",
        )
        .map_err(|e| format!("Failed to create session indexes: {}", e))?;

        Ok(())
    }

    // ─── Session CRUD ───

    /// Create a new session.
    pub fn create_session(
        &self,
        title: &str,
        edition: &str,
        module_code: &str,
        interviewee: &str,
        session_date: &str,
        project: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO research_sessions (title, edition, module_code, interviewee, session_date, project)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![title, edition, module_code, interviewee, session_date, project],
        )
        .map_err(|e| format!("Failed to create session: {}", e))?;
        Ok(conn.last_insert_rowid())
    }

    /// List all sessions, newest first. Optionally filter by project.
    pub fn list_sessions(&self, project: Option<&str>) -> Result<Vec<ResearchSession>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let (sql, params_vec): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match project {
            Some(p) => (
                "SELECT id, title, edition, module_code, interviewee, session_date, status, project, created_at, updated_at
                 FROM research_sessions WHERE project = ?1 ORDER BY updated_at DESC",
                vec![Box::new(p.to_string())],
            ),
            None => (
                "SELECT id, title, edition, module_code, interviewee, session_date, status, project, created_at, updated_at
                 FROM research_sessions ORDER BY updated_at DESC",
                vec![],
            ),
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| format!("Failed to prepare list: {}", e))?;

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(ResearchSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    edition: row.get(2)?,
                    module_code: row.get(3)?,
                    interviewee: row.get(4)?,
                    session_date: row.get(5)?,
                    status: row.get(6)?,
                    project: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })
            .map_err(|e| format!("Failed to query sessions: {}", e))?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row.map_err(|e| format!("Failed to read session row: {}", e))?);
        }
        Ok(sessions)
    }

    /// Get a session by ID.
    pub fn get_session(&self, id: i64) -> Result<Option<ResearchSession>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, edition, module_code, interviewee, session_date, status, project, created_at, updated_at
                 FROM research_sessions WHERE id = ?1",
            )
            .map_err(|e| format!("Failed to prepare get_session: {}", e))?;

        let mut rows = stmt
            .query_map(params![id], |row| {
                Ok(ResearchSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    edition: row.get(2)?,
                    module_code: row.get(3)?,
                    interviewee: row.get(4)?,
                    session_date: row.get(5)?,
                    status: row.get(6)?,
                    project: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })
            .map_err(|e| format!("Failed to query session: {}", e))?;

        match rows.next() {
            Some(row) => Ok(Some(
                row.map_err(|e| format!("Failed to read session: {}", e))?,
            )),
            None => Ok(None),
        }
    }

    /// Get session with all its Q&A records.
    pub fn get_session_detail(&self, id: i64) -> Result<Option<SessionDetail>, String> {
        let session = match self.get_session(id)? {
            Some(s) => s,
            None => return Ok(None),
        };
        let records = self.get_records(id)?;
        Ok(Some(SessionDetail { session, records }))
    }

    /// Update session metadata.
    pub fn update_session(
        &self,
        id: i64,
        title: &str,
        interviewee: &str,
        session_date: &str,
        status: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let affected = conn
            .execute(
                "UPDATE research_sessions SET title=?1, interviewee=?2, session_date=?3, status=?4, updated_at=datetime('now')
                 WHERE id=?5",
                params![title, interviewee, session_date, status, id],
            )
            .map_err(|e| format!("Failed to update session: {}", e))?;
        if affected == 0 {
            return Err(format!("Session {} not found", id));
        }
        Ok(())
    }

    /// Delete a session and all its Q&A records (CASCADE).
    pub fn delete_session(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        // Manually delete records first (SQLite CASCADE may not be enabled)
        conn.execute(
            "DELETE FROM session_qa_records WHERE session_id = ?1",
            params![id],
        )
        .map_err(|e| format!("Failed to delete session records: {}", e))?;
        conn.execute("DELETE FROM research_sessions WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete session: {}", e))?;
        Ok(())
    }

    // ─── Q&A Record CRUD ───

    /// Add a Q&A record to a session.
    pub fn add_record(
        &self,
        session_id: i64,
        question_id: Option<i64>,
        question_text: &str,
        answer_text: &str,
        notes: &str,
        sort_order: i32,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO session_qa_records (session_id, question_id, question_text, answer_text, notes, sort_order, source, is_bookmarked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'manual', 0)",
            params![session_id, question_id, question_text, answer_text, notes, sort_order],
        )
        .map_err(|e| format!("Failed to add record: {}", e))?;
        Ok(conn.last_insert_rowid())
    }

    /// Get all records for a session, ordered by sort_order.
    pub fn get_records(&self, session_id: i64) -> Result<Vec<QARecord>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, question_id, question_text, answer_text, notes, sort_order,
                        COALESCE(source, 'manual'), COALESCE(is_bookmarked, 0), created_at
                 FROM session_qa_records WHERE session_id = ?1 ORDER BY sort_order, id",
            )
            .map_err(|e| format!("Failed to prepare get_records: {}", e))?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let source: String = row.get(7)?;
                let is_bookmarked_int: i32 = row.get(8)?;
                Ok(QARecord {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    question_id: row.get(2)?,
                    question_text: row.get(3)?,
                    answer_text: row.get(4)?,
                    notes: row.get(5)?,
                    sort_order: row.get(6)?,
                    source,
                    is_bookmarked: is_bookmarked_int != 0,
                    created_at: row.get(9)?,
                })
            })
            .map_err(|e| format!("Failed to query records: {}", e))?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|e| format!("Failed to read record row: {}", e))?);
        }
        Ok(records)
    }

    /// Update a Q&A record.
    pub fn update_record(&self, id: i64, answer_text: &str, notes: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let affected = conn
            .execute(
                "UPDATE session_qa_records SET answer_text=?1, notes=?2 WHERE id=?3",
                params![answer_text, notes, id],
            )
            .map_err(|e| format!("Failed to update record: {}", e))?;
        if affected == 0 {
            return Err(format!("Record {} not found", id));
        }
        Ok(())
    }

    /// Delete a Q&A record.
    pub fn delete_record(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute("DELETE FROM session_qa_records WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete record: {}", e))?;
        Ok(())
    }

    /// Reorder records for a session by setting their sort_order.
    pub fn reorder_records(&self, session_id: i64, record_ids: &[i64]) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        for (order, record_id) in record_ids.iter().enumerate() {
            conn.execute(
                "UPDATE session_qa_records SET sort_order=?1 WHERE id=?2 AND session_id=?3",
                params![order as i32, record_id, session_id],
            )
            .map_err(|e| format!("Failed to reorder record {}: {}", record_id, e))?;
        }
        Ok(())
    }

    // ─── Export ───

    /// Export session Q&A to CSV format.
    pub fn export_csv(&self, session_id: i64) -> Result<String, String> {
        let records = self.get_records(session_id)?;
        let mut csv = String::from("序号,问题,回答,备注\n");
        for (i, r) in records.iter().enumerate() {
            let q = r.question_text.replace('"', "\"\"");
            let a = r.answer_text.replace('"', "\"\"");
            let n = r.notes.replace('"', "\"\"");
            csv.push_str(&format!("{},\"{}\",\"{}\",\"{}\"\n", i + 1, q, a, n));
        }
        Ok(csv)
    }

    /// Export session Q&A to Markdown format.
    pub fn export_markdown(&self, session_id: i64) -> Result<String, String> {
        let detail = self.get_session_detail(session_id)?;
        let detail = detail.ok_or("Session not found")?;
        let s = &detail.session;

        let mut md = format!(
            "# 调研记录：{}\n\n**版本：** {} | **模块：** {} | **受访人：** {} | **日期：** {}\n\n---\n\n",
            s.title, s.edition, s.module_code, s.interviewee, s.session_date
        );

        for (i, r) in detail.records.iter().enumerate() {
            md.push_str(&format!("### {}. {}\n\n", i + 1, r.question_text));
            if !r.answer_text.is_empty() {
                md.push_str(&format!("**回答：** {}\n\n", r.answer_text));
            }
            if !r.notes.is_empty() {
                md.push_str(&format!("**备注：** {}\n\n", r.notes));
            }
        }
        Ok(md)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_store() -> ResearchSessionStore {
        let store = ResearchSessionStore::new(Path::new(":memory:")).unwrap();
        store
    }

    #[test]
    fn test_create_and_list_session() {
        let store = new_store();
        let id = store
            .create_session(
                "测试会话",
                "enterprise",
                "BOS",
                "张三",
                "2026-05-24",
                "default",
            )
            .unwrap();
        assert!(id > 0);

        let sessions = store.list_sessions(None).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "测试会话");
        assert_eq!(sessions[0].edition, "enterprise");
        assert_eq!(sessions[0].project, "default");
    }

    #[test]
    fn test_session_detail_empty() {
        let store = new_store();
        let id = store
            .create_session("空会话", "enterprise", "BOS", "", "", "default")
            .unwrap();
        let detail = store.get_session_detail(id).unwrap().unwrap();
        assert_eq!(detail.records.len(), 0);
    }

    #[test]
    fn test_crud_records() {
        let store = new_store();
        let sid = store
            .create_session("CRUD测试", "enterprise", "BOS", "李四", "", "default")
            .unwrap();

        let rid = store
            .add_record(sid, None, "问题1？", "回答1", "备注1", 0)
            .unwrap();
        assert!(rid > 0);

        let records = store.get_records(sid).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].question_text, "问题1？");

        store.update_record(rid, "回答已更新", "").unwrap();
        let records = store.get_records(sid).unwrap();
        assert_eq!(records[0].answer_text, "回答已更新");

        store.delete_record(rid).unwrap();
        let records = store.get_records(sid).unwrap();
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_export_csv_and_markdown() {
        let store = new_store();
        let sid = store
            .create_session(
                "导出测试",
                "enterprise",
                "CM",
                "王五",
                "2026-05-24",
                "default",
            )
            .unwrap();
        store
            .add_record(sid, None, "问题1", "答案1", "", 0)
            .unwrap();
        store
            .add_record(sid, None, "问题2", "答案2", "备注2", 1)
            .unwrap();

        let csv = store.export_csv(sid).unwrap();
        assert!(csv.contains("问题1"));
        assert!(csv.contains("答案1"));

        let md = store.export_markdown(sid).unwrap();
        assert!(md.contains("导出测试"));
        assert!(md.contains("问题1"));
        assert!(md.contains("答案2"));
    }

    #[test]
    fn test_delete_session_cascades() {
        let store = new_store();
        let sid = store
            .create_session("删除测试", "enterprise", "BOS", "", "", "default")
            .unwrap();
        store.add_record(sid, None, "问题", "答案", "", 0).unwrap();

        store.delete_session(sid).unwrap();
        let s = store.get_session(sid).unwrap();
        assert!(s.is_none());

        let records = store.get_records(sid).unwrap();
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_list_sessions_filter_by_project() {
        let store = new_store();
        store
            .create_session("会话A", "enterprise", "BOS", "", "", "project_a")
            .unwrap();
        store
            .create_session("会话B", "enterprise", "BOS", "", "", "project_b")
            .unwrap();
        store
            .create_session("会话C", "enterprise", "BOS", "", "", "project_a")
            .unwrap();

        let all = store.list_sessions(None).unwrap();
        assert_eq!(all.len(), 3);

        let filtered = store.list_sessions(Some("project_a")).unwrap();
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|s| s.project == "project_a"));

        let filtered_b = store.list_sessions(Some("project_b")).unwrap();
        assert_eq!(filtered_b.len(), 1);
        assert_eq!(filtered_b[0].title, "会话B");
    }
}
