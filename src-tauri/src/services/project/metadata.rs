//! SQLite metadata store for chunk↔vector mapping
//!
//! Manages documents and chunks tables with SHA256 dedup,
//! project_id filtering, and WAL journal mode.

use rusqlite::{params, Connection, OptionalExtension, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Document metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub id: i64,
    pub title: String,
    pub source_path: Option<String>,
    pub sha256: Option<String>,
    pub created_at: String,
    pub project_id: i64,
    pub document_scope: String,
    pub chat_session_id: Option<String>,
    pub raw_source_identity: Option<String>,
}

/// Chunk 元数据（HNSW 索引中每条向量对应一条）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMeta {
    pub id: i64,
    pub vector_key: i64,
    pub document_id: i64,
    pub content: String,
    pub section_path: Option<String>,
    pub tags: Option<String>,
    pub line_no: Option<i64>,
    pub parent_chunk_id: Option<i64>,
    pub created_at: String,
}

/// Knowledge base statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeStats {
    pub document_count: i64,
    pub chunk_count: i64,
    pub db_path: String,
}

/// Agent 会话记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionRecord {
    pub id: String,
    pub project_id: i64,
    pub slot: String,
    pub status: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub started_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
}

/// Agent 消息记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessageRecord {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub status: String,
    pub parent_message_id: Option<String>,
    pub created_at: String,
}

/// Agent 工具调用记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolCallRecord {
    pub id: String,
    pub session_id: String,
    pub assistant_message_id: Option<String>,
    pub tool_name: String,
    pub tool_revision: String,
    pub effect: String,
    pub args_json: String,
    pub status: String,
    pub started_at: String,
    pub ended_at: Option<String>,
}

/// Agent 工具结果记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolResultRecord {
    pub id: String,
    pub tool_call_id: String,
    pub result_json: String,
    pub preview_text: String,
    pub output_ref: Option<String>,
    pub status: String,
    pub created_at: String,
}

/// Agent 事件记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEventRecord {
    pub id: String,
    pub session_id: String,
    pub event_type: String,
    pub payload_json: String,
    pub created_at: String,
}

/// Agent 会话快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionSnapshot {
    pub session: AgentSessionRecord,
    pub messages: Vec<AgentMessageRecord>,
    pub tool_calls: Vec<AgentToolCallRecord>,
    pub tool_results: Vec<AgentToolResultRecord>,
    pub events: Vec<AgentEventRecord>,
}

/// SQLite-based metadata store
pub struct MetadataStore {
    db: Connection,
    db_path: PathBuf,
}

impl MetadataStore {
    /// Open or create the metadata database at the given path
    pub fn new(db_path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create DB directory: {}", e))?;
        }

        let db =
            Connection::open(&db_path).map_err(|e| format!("Failed to open database: {}", e))?;

        // 设置数据库忙超时（5秒），以防并发写入时立即返回 SQLITE_BUSY 错误
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("设置数据库忙超时失败: {}", e))?;

        // Enable WAL mode for better concurrent read performance
        db.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| format!("Failed to set WAL mode: {}", e))?;

        // Enable foreign keys
        db.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("Failed to enable foreign keys: {}", e))?;

        let store = Self { db, db_path };
        store.init_schema()?;
        Ok(store)
    }

    /// Create tables and indexes (idempotent)
    fn init_schema(&self) -> Result<(), String> {
        self.db
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                source_path TEXT,
                sha256 TEXT UNIQUE,
                created_at TEXT DEFAULT (datetime('now')),
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                document_scope TEXT NOT NULL DEFAULT 'knowledge',
                chat_session_id TEXT,
                raw_source_identity TEXT DEFAULT NULL
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                vector_key INTEGER UNIQUE,
                document_id INTEGER REFERENCES documents(id),
                content TEXT NOT NULL,
                section_path TEXT,
                tags TEXT,
                line_no INTEGER,
                parent_chunk_id INTEGER,
                created_at TEXT DEFAULT (datetime('now'))
            );

            -- Migrate: add parent_chunk_id column for existing tables (Small-to-Big retrieval)
            -- Use ALTER TABLE with IF NOT EXISTS pattern via try/catch
            CREATE TABLE IF NOT EXISTS vector_key_seq (
                id INTEGER PRIMARY KEY AUTOINCREMENT
            );

            CREATE TABLE IF NOT EXISTS deletion_outbox (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                document_id INTEGER NOT NULL,
                project_id INTEGER,
                status TEXT NOT NULL DEFAULT 'pending',
                error TEXT,
                vector_keys TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS agent_sessions (
                id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                slot TEXT NOT NULL,
                status TEXT NOT NULL,
                provider_id TEXT,
                model_id TEXT,
                started_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                ended_at TEXT
            );

            CREATE TABLE IF NOT EXISTS agent_messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                status TEXT NOT NULL,
                parent_message_id TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_tool_calls (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
                assistant_message_id TEXT,
                tool_name TEXT NOT NULL,
                tool_revision TEXT NOT NULL,
                effect TEXT NOT NULL,
                args_json TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT
            );

            CREATE TABLE IF NOT EXISTS agent_tool_results (
                id TEXT PRIMARY KEY,
                tool_call_id TEXT NOT NULL REFERENCES agent_tool_calls(id) ON DELETE CASCADE,
                result_json TEXT NOT NULL,
                preview_text TEXT NOT NULL,
                output_ref TEXT,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_events (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            ",
            )
            .map_err(|e| format!("Failed to initialize schema: {}", e))?;

        // 迁移：为旧数据库添加 parent_chunk_id 列（Small-to-Big 检索）
        // SQLite 不支持 IF NOT EXISTS 的 ALTER TABLE，忽略"重复列"错误
        let _ = self.db.execute(
            "ALTER TABLE chunks ADD COLUMN parent_chunk_id INTEGER",
            [],
        );

        self.db
            .execute_batch(
                "
            CREATE INDEX IF NOT EXISTS idx_chunks_vector_key ON chunks(vector_key);
            CREATE INDEX IF NOT EXISTS idx_chunks_document_id ON chunks(document_id);
            CREATE INDEX IF NOT EXISTS idx_documents_sha256 ON documents(sha256);
            CREATE INDEX IF NOT EXISTS idx_documents_project_id ON documents(project_id);
            CREATE INDEX IF NOT EXISTS idx_documents_scope ON documents(document_scope);
            CREATE INDEX IF NOT EXISTS idx_documents_chat_session_id ON documents(chat_session_id);
            CREATE INDEX IF NOT EXISTS idx_agent_sessions_project_slot ON agent_sessions(project_id, slot, updated_at);
            CREATE INDEX IF NOT EXISTS idx_agent_messages_session ON agent_messages(session_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_agent_tool_calls_session ON agent_tool_calls(session_id, started_at);
            CREATE INDEX IF NOT EXISTS idx_agent_tool_results_call ON agent_tool_results(tool_call_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_agent_events_session ON agent_events(session_id, created_at);
            ",
            )
            .map_err(|e| format!("Failed to initialize schema indexes: {}", e))?;

        // Seed vector_key_seq with current max + 1 if empty
        let count: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM vector_key_seq", [], |row| row.get(0))
            .unwrap_or(0);
        if count == 0 {
            let max_key: i64 = self
                .db
                .query_row(
                    "SELECT COALESCE(MAX(vector_key), 0) FROM chunks",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            if max_key > 0 {
                // 初始化序列表为下一个可用值
                for _ in 0..max_key {
                    self.db
                        .execute("INSERT INTO vector_key_seq DEFAULT VALUES", [])
                        .map_err(|e| format!("初始化向量序列失败: {}", e))?;
                }
            }
        }

        Ok(())
    }

    // ─── Document operations ───

    /// 插入新文档，返回文档 ID
    pub fn insert_document(
        &self,
        title: &str,
        source_path: Option<&str>,
        sha256: Option<&str>,
        project_id: i64,
        document_scope: Option<&str>,
        chat_session_id: Option<&str>,
        raw_source_identity: Option<&str>,
    ) -> Result<i64, String> {
        let scope = document_scope.unwrap_or("knowledge");
        self.db
            .execute(
                "INSERT OR IGNORE INTO documents (title, source_path, sha256, project_id, document_scope, chat_session_id, raw_source_identity)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![title, source_path, sha256, project_id, scope, chat_session_id, raw_source_identity],
            )
            .map_err(|e| format!("Failed to insert document: {}", e))?;

        // 按 (sha256, project_id, raw_source_identity) 三字段精确定位已有文档
        // 避免两份内容相同但来源不同的文档（如复制品）的 document_id 混淆
        if let Some(hash) = sha256 {
            if let Some(doc) =
                self.get_document_by_source_key(hash, project_id, raw_source_identity)?
            {
                return Ok(doc.id);
            }
        }

        Ok(self.db.last_insert_rowid())
    }

    /// 更新文档的 raw_source_identity 字段
    pub fn update_document_raw_source_identity(
        &self,
        document_id: i64,
        identity: &str,
    ) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE documents SET raw_source_identity = ?1 WHERE id = ?2",
                params![identity, document_id],
            )
            .map_err(|e| format!("更新文档 raw_source_identity 失败: {}", e))?;
        Ok(())
    }

    /// Get a document by its SHA256 hash
    pub fn get_document_by_sha256(&self, sha256: &str) -> Result<Option<DocumentMeta>, String> {
        self.query_one_document(
            "SELECT id, title, source_path, sha256, created_at, project_id, document_scope, chat_session_id, raw_source_identity
             FROM documents WHERE sha256 = ?1",
            params![sha256],
        )
    }

    /// 按 (sha256, project_id, raw_source_identity) 三字段精确定位文档
    ///
    /// 用于同名重复内容文件的寻址防错：两份内容一致但来源不同的文档（如复制品）
    /// 不会因仅用 sha256 查询而混淆 document_id。
    pub fn get_document_by_source_key(
        &self,
        sha256: &str,
        project_id: i64,
        raw_source_identity: Option<&str>,
    ) -> Result<Option<DocumentMeta>, String> {
        match raw_source_identity {
            Some(identity) => self.query_one_document(
                "SELECT id, title, source_path, sha256, created_at, project_id, document_scope, chat_session_id, raw_source_identity
                 FROM documents WHERE sha256 = ?1 AND project_id = ?2 AND raw_source_identity = ?3",
                params![sha256, project_id, identity],
            ),
            None => self.query_one_document(
                "SELECT id, title, source_path, sha256, created_at, project_id, document_scope, chat_session_id, raw_source_identity
                 FROM documents WHERE sha256 = ?1 AND project_id = ?2 AND raw_source_identity IS NULL",
                params![sha256, project_id],
            ),
        }
    }

    /// 按 (sha256, project_id, raw_source_identity) 查找所有关联文档
    /// 用于 raw_source 级联删除时定位待清除的 documents/chunks/vectors
    pub fn list_documents_by_source_key(
        &self,
        sha256: &str,
        project_id: i64,
        raw_source_identity: &str,
    ) -> Result<Vec<DocumentMeta>, String> {
        self.query_documents(
            "SELECT id, title, source_path, sha256, created_at, project_id, document_scope, chat_session_id, raw_source_identity
             FROM documents WHERE sha256 = ?1 AND project_id = ?2 AND raw_source_identity = ?3",
            params![sha256, project_id, raw_source_identity],
        )
    }

    /// Get a document by its ID
    pub fn get_document(&self, id: i64) -> Result<Option<DocumentMeta>, String> {
        self.query_one_document(
            "SELECT id, title, source_path, sha256, created_at, project_id, document_scope, chat_session_id, raw_source_identity
             FROM documents WHERE id = ?1",
            params![id],
        )
    }

    /// Count chunks for a given document (returns 0 if document has no chunks)
    pub fn get_document_chunk_count(&self, document_id: i64) -> Result<i64, String> {
        self.db
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE document_id = ?1",
                params![document_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                format!(
                    "Failed to count chunks for document {document_id}: {e}",
                    document_id = document_id,
                    e = e
                )
            })
    }

    /// Get multiple documents by their IDs (batch fetch to eliminate N+1 queries)
    pub fn get_documents_by_ids(&self, ids: &[i64]) -> Result<HashMap<i64, DocumentMeta>, String> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders: Vec<String> = ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{0}", i + 1))
            .collect();
        let sql = format!(
            "SELECT id, title, source_path, sha256, created_at, project_id, document_scope, chat_session_id, raw_source_identity
             FROM documents WHERE id IN ({0})",
            placeholders.join(", ")
        );

        let mut stmt = self
            .db
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare batch query: {0}", e))?;

        let params: Vec<&dyn rusqlite::types::ToSql> = ids
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), Self::row_to_document)
            .map_err(|e| format!("Failed to batch query documents: {0}", e))?;

        let mut map = HashMap::new();
        for row in rows {
            let doc = row.map_err(|e| format!("Failed to read document row: {0}", e))?;
            map.insert(doc.id, doc);
        }
        Ok(map)
    }

    /// Get all documents, optionally filtered by project
    pub fn list_documents(&self, project_id: Option<i64>) -> Result<Vec<DocumentMeta>, String> {
        if let Some(pid) = project_id {
            self.query_documents(
                "SELECT id, title, source_path, sha256, created_at, project_id, document_scope, chat_session_id, raw_source_identity
                 FROM documents WHERE project_id = ?1 AND document_scope = 'knowledge' ORDER BY created_at DESC",
                params![pid],
            )
        } else {
            self.query_documents(
                "SELECT id, title, source_path, sha256, created_at, project_id, document_scope, chat_session_id, raw_source_identity
                 FROM documents WHERE document_scope = 'knowledge' ORDER BY created_at DESC",
                [],
            )
        }
    }

    /// 删除文档及其关联的 chunk
    /// 若指定 project_id，删除前校验文档归属
    pub fn delete_document(&self, id: i64, project_id: Option<i64>) -> Result<(), String> {
        if let Some(pid) = project_id {
            let doc_project_id: i64 = self
                .db
                .query_row(
                    "SELECT project_id FROM documents WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|e| format!("Document {} not found: {}", id, e))?;
            if doc_project_id != pid {
                return Err(format!(
                    "Document {} belongs to project {}, not {}",
                    id, doc_project_id, pid
                ));
            }
        }

        self.db
            .execute("DELETE FROM chunks WHERE document_id = ?1", params![id])
            .map_err(|e| format!("Failed to delete chunks: {}", e))?;
        self.db
            .execute("DELETE FROM documents WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete document: {}", e))?;
        Ok(())
    }

    /// Batch-delete multiple documents (and their chunks) in a single transaction
    /// If project_id is specified, verify all documents belong to that project before deleting
    pub fn delete_documents_batch(
        &self,
        document_ids: Vec<i64>,
        project_id: Option<i64>,
    ) -> Result<u64, String> {
        if document_ids.is_empty() {
            return Ok(0);
        }

        if let Some(pid) = project_id {
            for &doc_id in &document_ids {
                let doc_project_id: i64 = self
                    .db
                    .query_row(
                        "SELECT project_id FROM documents WHERE id = ?1",
                        params![doc_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("Document {} not found: {}", doc_id, e))?;
                if doc_project_id != pid {
                    return Err(format!(
                        "Document {} belongs to project {}, not {}",
                        doc_id, doc_project_id, pid
                    ));
                }
            }
        }

        // Build placeholders: "?,?,?" for IN clause
        let placeholders: String = document_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let params: Vec<&dyn rusqlite::types::ToSql> = document_ids
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();

        let tx = self
            .db
            .unchecked_transaction()
            .map_err(|e| format!("启动批量删除事务失败: {}", e))?;

        // 把事务体收集为 Result，失败时显式 rollback
        // （unchecked_transaction 的 guard 在 drop 时不会自动回滚）
        let body: Result<u64, String> = (|| {
            let _chunks_deleted = tx
                .execute(
                    &format!("DELETE FROM chunks WHERE document_id IN ({})", placeholders),
                    params.as_slice(),
                )
                .map_err(|e| format!("批量删除 chunks 失败: {}", e))?;
            let docs_deleted = tx
                .execute(
                    &format!("DELETE FROM documents WHERE id IN ({})", placeholders),
                    params.as_slice(),
                )
                .map_err(|e| format!("批量删除 documents 失败: {}", e))?;
            Ok(docs_deleted as u64)
        })();
        let docs_deleted = match body {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.rollback();
                return Err(e);
            }
        };
        tx.commit()
            .map_err(|e| format!("提交批量删除事务失败: {}", e))?;

        Ok(docs_deleted)
    }

    /// 获取指定文档 ID 列表的所有 chunk 对应的 vector key。
    /// 用于删除文档时从 usearch 索引中清理向量。
    pub fn get_vector_keys_by_document_ids(
        &self,
        document_ids: &[i64],
    ) -> Result<Vec<i64>, String> {
        if document_ids.is_empty() {
            return Ok(vec![]);
        }
        let placeholders: String = document_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let params: Vec<&dyn rusqlite::types::ToSql> = document_ids
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();

        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT vector_key FROM chunks WHERE document_id IN ({})",
                placeholders
            ))
            .map_err(|e| format!("Failed to prepare vector key query: {}", e))?;

        let keys: Vec<i64> = stmt
            .query_map(params.as_slice(), |row| row.get(0))
            .map_err(|e| format!("Failed to query vector keys: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(keys)
    }

    // ─── Deletion Outbox ───

    /// 插入删除记录到 outbox（预写日志，防止崩溃后丢失）
    pub fn insert_deletion_record(
        &self,
        document_id: i64,
        project_id: Option<i64>,
        vector_keys: &[i64],
    ) -> Result<i64, String> {
        let keys_json = serde_json::to_string(vector_keys)
            .map_err(|e| format!("序列化 vector_keys 失败: {}", e))?;
        self.db
            .execute(
                "INSERT INTO deletion_outbox (document_id, project_id, vector_keys) VALUES (?1, ?2, ?3)",
                params![document_id, project_id, keys_json],
            )
            .map_err(|e| format!("插入删除记录失败: {}", e))?;
        Ok(self.db.last_insert_rowid())
    }

    /// 更新 outbox 记录状态
    pub fn update_deletion_status(
        &self,
        id: i64,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE deletion_outbox SET status = ?1, error = ?2, updated_at = datetime('now') WHERE id = ?3",
                params![status, error, id],
            )
            .map_err(|e| format!("更新删除状态失败: {}", e))?;
        Ok(())
    }

    /// 获取所有待补偿的删除记录（pending 和 failed 状态均重试）
    pub fn get_pending_deletions(&self) -> Result<Vec<(i64, i64, Option<i64>, String)>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, document_id, project_id, vector_keys FROM deletion_outbox WHERE status IN ('pending', 'failed')",
            )
            .map_err(|e| format!("查询 pending 删除记录失败: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| format!("遍历 pending 删除记录失败: {}", e))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("读取 pending 删除记录失败: {}", e))?);
        }
        Ok(results)
    }

    // ─── Chunk operations ───

    /// Get the next globally-unique vector key.
    /// Uses INSERT autoincrement to guarantee uniqueness (fixes MAX+1 concurrency issue).
    pub fn next_vector_key(&self) -> Result<i64, String> {
        self.db
            .execute("INSERT INTO vector_key_seq DEFAULT VALUES", [])
            .map_err(|e| format!("分配向量 key 失败: {}", e))?;
        Ok(self.db.last_insert_rowid())
    }

    /// Ensure the vector_key_seq table exists (called once during app init).
    pub fn ensure_vector_key_seq(&self) -> Result<(), String> {
        self.db
            .execute(
                "CREATE TABLE IF NOT EXISTS vector_key_seq (id INTEGER PRIMARY KEY AUTOINCREMENT)",
                [],
            )
            .map_err(|e| format!("创建向量序列表失败: {}", e))?;
        Ok(())
    }

    /// Insert a chunk linked to a document and vector key
    pub fn insert_chunk(
        &self,
        vector_key: i64,
        document_id: i64,
        content: &str,
        section_path: Option<&str>,
        tags: Option<&[String]>,
        line_no: Option<i64>,
        parent_chunk_id: Option<i64>,
    ) -> Result<i64, String> {
        let tags_json = tags.map(|t| serde_json::to_string(t).unwrap_or_default());

        self.db
            .execute(
                "INSERT INTO chunks (vector_key, document_id, content, section_path, tags, line_no, parent_chunk_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    vector_key,
                    document_id,
                    content,
                    section_path,
                    tags_json,
                    line_no,
                    parent_chunk_id
                ],
            )
            .map_err(|e| format!("Failed to insert chunk: {}", e))?;

        Ok(self.db.last_insert_rowid())
    }

    /// 按 vector key 获取一条 chunk
    pub fn get_chunk_by_vector_key(&self, vector_key: i64) -> Result<Option<ChunkMeta>, String> {
        self.query_one_chunk(
            "SELECT id, vector_key, document_id, content, section_path, tags, line_no, parent_chunk_id, created_at
             FROM chunks WHERE vector_key = ?1",
            params![vector_key],
        )
    }

    /// Get multiple chunks by their vector keys
    pub fn get_chunks_by_vector_keys(&self, keys: &[i64]) -> Result<Vec<ChunkMeta>, String> {
        if keys.is_empty() {
            return Ok(vec![]);
        }

        let placeholders: Vec<String> = keys
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let sql = format!(
            "SELECT id, vector_key, document_id, content, section_path, tags, line_no, parent_chunk_id, created_at
             FROM chunks WHERE vector_key IN ({})",
            placeholders.join(",")
        );

        let mut stmt = self
            .db
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let params: Vec<&dyn rusqlite::types::ToSql> = keys
            .iter()
            .map(|k| k as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), Self::row_to_chunk)
            .map_err(|e| format!("Failed to query chunks: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Failed to read chunk row: {}", e))?);
        }
        Ok(results)
    }

    /// 获取文档的所有 chunk_id（SQLite 行 id）
    pub fn get_chunk_ids_by_document(&self, document_id: i64) -> Result<Vec<i64>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT id FROM chunks WHERE document_id = ?1")
            .map_err(|e| format!("prepare 失败: {}", e))?;
        let ids = stmt
            .query_map(params![document_id], |row| row.get(0))
            .map_err(|e| format!("query 失败: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }

    /// 获取指定文档的所有 chunk
    pub fn get_chunks_by_document(&self, document_id: i64) -> Result<Vec<ChunkMeta>, String> {
        self.query_chunks(
            "SELECT id, vector_key, document_id, content, section_path, tags, line_no, parent_chunk_id, created_at
             FROM chunks WHERE document_id = ?1 ORDER BY line_no, id",
            params![document_id],
        )
    }

    /// 获取指定 chunk 的前后邻居 chunk（句子窗口检索）
    ///
    /// 返回 (prev_chunk, next_chunk)，基于同一文档内的 line_no 排序。
    /// 用于在上下文组装时扩展检索到的 chunk 的周围语境。
    pub fn get_chunk_neighbors(&self, chunk_id: i64) -> Result<(Option<ChunkMeta>, Option<ChunkMeta>), String> {
        // 先获取当前 chunk 的 document_id 和 line_no
        let current = self
            .query_one_chunk(
                "SELECT id, vector_key, document_id, content, section_path, tags, line_no, parent_chunk_id, created_at
                 FROM chunks WHERE id = ?1",
                params![chunk_id],
            )?
            .ok_or_else(|| format!("chunk {} 不存在", chunk_id))?;

        let doc_id = current.document_id;
        let line = current.line_no.unwrap_or(0);

        // 查找前一个邻居（同一文档内 line_no 最大但小于当前 line_no）
        let prev = self.query_one_chunk(
            "SELECT id, vector_key, document_id, content, section_path, tags, line_no, parent_chunk_id, created_at
             FROM chunks WHERE document_id = ?1 AND line_no < ?2
             ORDER BY line_no DESC LIMIT 1",
            params![doc_id, line],
        )?;

        // 查找后一个邻居（同一文档内 line_no 最小但大于当前 line_no）
        let next = self.query_one_chunk(
            "SELECT id, vector_key, document_id, content, section_path, tags, line_no, parent_chunk_id, created_at
             FROM chunks WHERE document_id = ?1 AND line_no > ?2
             ORDER BY line_no ASC LIMIT 1",
            params![doc_id, line],
        )?;

        Ok((prev, next))
    }

    /// 批量获取多个 chunk 的邻居（去重合并，减少 SQL 查询次数）
    ///
    /// 返回 HashMap<chunk_id, (Option<prev_content>, Option<next_content>)>
    pub fn get_chunk_neighbors_batch(
        &self,
        chunk_ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, (Option<String>, Option<String>)>, String> {
        let mut result = std::collections::HashMap::new();
        let mut processed_docs = std::collections::HashSet::new();

        for &chunk_id in chunk_ids {
            // 先从缓存的结果中查找（同一文档批次内）
            if result.contains_key(&chunk_id) {
                continue;
            }

            match self.get_chunk_neighbors(chunk_id) {
                Ok((prev, next)) => {
                    result.insert(
                        chunk_id,
                        (
                            prev.map(|c| c.content),
                            next.map(|c| c.content),
                        ),
                    );
                    // 标记文档已处理（避免同文档内重复全量查询）
                    if let Ok(Some(current)) = self.query_one_chunk(
                        "SELECT document_id FROM chunks WHERE id = ?1",
                        params![chunk_id],
                    ) {
                        processed_docs.insert(current.document_id);
                    }
                }
                Err(e) => {
                    tracing::warn!("获取 chunk {} 邻居失败: {}", chunk_id, e);
                }
            }
        }

        Ok(result)
    }

    /// Delete a chunk by its vector key
    pub fn delete_chunk_by_vector_key(&self, vector_key: i64) -> Result<(), String> {
        self.db
            .execute(
                "DELETE FROM chunks WHERE vector_key = ?1",
                params![vector_key],
            )
            .map_err(|e| format!("Failed to delete chunk: {}", e))?;
        Ok(())
    }

    /// 获取所有聊天附件 chunk_id 列表，用于检索前置过滤
    pub fn get_chat_attachment_chunk_ids(&self) -> Result<Vec<i64>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT c.id FROM chunks c
                 JOIN documents d ON c.document_id = d.id
                 WHERE d.document_scope = 'chat_attachment'",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;
        let ids: Vec<i64> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Failed to query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }

    /// Small-to-Big 检索：将子块 ID 映射为父块
    ///
    /// 给定一组子块 ID，查找其 parent_chunk_id，返回去重后的父块。
    /// 若子块 parent_chunk_id 为 NULL（旧数据），则保留该子块自身作为结果。
    pub fn get_parent_chunks_for_child_ids(
        &self,
        child_ids: &[i64],
    ) -> Result<Vec<ChunkMeta>, String> {
        if child_ids.is_empty() {
            return Ok(vec![]);
        }

        // 先查询所有子块的 parent_chunk_id
        let placeholders: Vec<String> = child_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let sql = format!(
            "SELECT id, parent_chunk_id FROM chunks WHERE id IN ({})",
            placeholders.join(",")
        );

        let mut stmt = self
            .db
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let params: Vec<&dyn rusqlite::types::ToSql> = child_ids
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();

        let rows = stmt
            .query_map(params.as_slice(), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
            })
            .map_err(|e| format!("Failed to query child-parent mappings: {}", e))?;

        // 收集唯一的父块 ID（若为 NULL 则保留子块自身 ID）
        let mut parent_ids: Vec<i64> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for row in rows {
            let (child_id, parent_id) =
                row.map_err(|e| format!("Failed to read child-parent row: {}", e))?;
            let resolved = parent_id.unwrap_or(child_id); // 无父块则用自身
            if seen.insert(resolved) {
                parent_ids.push(resolved);
            }
        }

        // 回退：若无子块有父块（旧数据），无操作
        if parent_ids.is_empty() {
            return Ok(vec![]);
        }

        // 批量查询父块完整内容
        self.get_chunks_by_ids(&parent_ids)
    }

    /// 按 chunk ID 列表批量获取 chunk
    fn get_chunks_by_ids(&self, ids: &[i64]) -> Result<Vec<ChunkMeta>, String> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let placeholders: Vec<String> = ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let sql = format!(
            "SELECT id, vector_key, document_id, content, section_path, tags, line_no, parent_chunk_id, created_at
             FROM chunks WHERE id IN ({})",
            placeholders.join(",")
        );

        let mut stmt = self
            .db
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let params: Vec<&dyn rusqlite::types::ToSql> = ids
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), Self::row_to_chunk)
            .map_err(|e| format!("Failed to query chunks: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Failed to read chunk row: {}", e))?);
        }
        Ok(results)
    }

    // ─── Stats ───

    /// 获取知识库统计信息，可按 project_id 过滤
    pub fn get_stats(&self, project_id: Option<i64>) -> Result<KnowledgeStats, String> {
        let doc_count: i64 = match project_id {
            Some(pid) => self
                .db
                .query_row(
                    "SELECT COUNT(*) FROM documents WHERE project_id = ?1",
                    params![pid],
                    |row| row.get(0),
                )
                .map_err(|e| format!("Failed to count documents: {}", e))?,
            None => self
                .db
                .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
                .map_err(|e| format!("Failed to count documents: {}", e))?,
        };

        let chunk_count: i64 = match project_id {
            Some(pid) => self
                .db
                .query_row(
                    "SELECT COUNT(*) FROM chunks c JOIN documents d ON c.document_id = d.id WHERE d.project_id = ?1",
                    params![pid],
                    |row| row.get(0),
                )
                .map_err(|e| format!("Failed to count chunks: {}", e))?,
            None => self
                .db
                .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
                .map_err(|e| format!("Failed to count chunks: {}", e))?,
        };

        Ok(KnowledgeStats {
            document_count: doc_count,
            chunk_count: chunk_count,
            db_path: self.db_path.to_string_lossy().to_string(),
        })
    }

    // ─── AppConfig ───

    /// 确保 app_config 表存在（幂等）
    fn ensure_app_config_table(&self) -> Result<(), String> {
        self.db
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS app_config (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );",
            )
            .map_err(|e| format!("创建 app_config 表失败: {}", e))
    }

    /// 查询 KB 编译是否启用
    pub fn get_kb_compilation_enabled(&self) -> Result<bool, String> {
        self.ensure_app_config_table()?;
        let result: Result<String, _> = self.db.query_row(
            "SELECT value FROM app_config WHERE key = 'enable_kb_compilation'",
            [],
            |row| row.get(0),
        );
        match result {
            Ok(val) => Ok(val == "true" || val == "1"),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(format!("查询 KB 编译配置失败: {}", e)),
        }
    }

    /// 设置 KB 编译是否启用
    pub fn set_kb_compilation_enabled(&self, enabled: bool) -> Result<(), String> {
        self.ensure_app_config_table()?;
        let val = if enabled { "true" } else { "false" };
        self.db
            .execute(
                "INSERT OR REPLACE INTO app_config (key, value) VALUES ('enable_kb_compilation', ?1)",
                params![val],
            )
            .map_err(|e| format!("设置 KB 编译配置失败: {}", e))?;
        Ok(())
    }

    // ─── Agent Session Ledger ───

    /// 创建 Agent 会话账本
    pub fn create_agent_session(
        &self,
        id: &str,
        project_id: i64,
        slot: &str,
        provider_id: Option<&str>,
        model_id: Option<&str>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        // 用 INSERT OR IGNORE + UPDATE 替代 INSERT OR REPLACE：
        // REPLACE 对同 id 行先 DELETE 再 INSERT，会触发 agent_messages 的
        // ON DELETE CASCADE，把整个会话历史清空——用户每发一条新消息就丢失
        // 之前所有轮次。IGNORE 不触发 CASCADE，旧消息保留；随后 UPDATE 刷新
        // status / updated_at / ended_at。
        self.db
            .execute(
                "INSERT OR IGNORE INTO agent_sessions
                 (id, project_id, slot, status, provider_id, model_id, started_at, updated_at, ended_at)
                 VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6, ?6, NULL)",
                params![id, project_id, slot, provider_id, model_id, now],
            )
            .map_err(|e| format!("创建 Agent 会话账本失败: {}", e))?;
        self.db
            .execute(
                "UPDATE agent_sessions
                 SET status = 'running', updated_at = ?1, ended_at = NULL
                 WHERE id = ?2",
                params![now, id],
            )
            .map_err(|e| format!("更新 Agent 会话状态失败: {}", e))?;
        Ok(())
    }

    /// 更新 Agent 会话状态
    pub fn update_agent_session_status(
        &self,
        session_id: &str,
        status: &str,
        ended: bool,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        if ended {
            self.db
                .execute(
                    "UPDATE agent_sessions SET status = ?1, updated_at = ?2, ended_at = ?2 WHERE id = ?3",
                    params![status, now, session_id],
                )
                .map_err(|e| format!("更新 Agent 会话状态失败: {}", e))?;
        } else {
            self.db
                .execute(
                    "UPDATE agent_sessions SET status = ?1, updated_at = ?2 WHERE id = ?3",
                    params![status, now, session_id],
                )
                .map_err(|e| format!("更新 Agent 会话状态失败: {}", e))?;
        }
        Ok(())
    }

    /// 写入 Agent 消息
    pub fn insert_agent_message(
        &self,
        id: &str,
        session_id: &str,
        role: &str,
        content: &str,
        status: &str,
        parent_message_id: Option<&str>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db
            .execute(
                "INSERT OR REPLACE INTO agent_messages
                 (id, session_id, role, content, status, parent_message_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    session_id,
                    role,
                    content,
                    status,
                    parent_message_id,
                    now
                ],
            )
            .map_err(|e| format!("写入 Agent 消息失败: {}", e))?;
        self.touch_agent_session(session_id)?;
        Ok(())
    }

    /// 更新 Agent 消息正文和状态
    pub fn update_agent_message(
        &self,
        id: &str,
        content: &str,
        status: &str,
    ) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE agent_messages SET content = ?1, status = ?2 WHERE id = ?3",
                params![content, status, id],
            )
            .map_err(|e| format!("更新 Agent 消息失败: {}", e))?;
        if let Some(session_id) = self.session_id_for_agent_message(id)? {
            self.touch_agent_session(&session_id)?;
        }
        Ok(())
    }

    /// 写入 Agent 工具调用
    pub fn insert_agent_tool_call(
        &self,
        id: &str,
        session_id: &str,
        assistant_message_id: Option<&str>,
        tool_name: &str,
        tool_revision: &str,
        effect: &str,
        args_json: &str,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db
            .execute(
                "INSERT OR REPLACE INTO agent_tool_calls
                 (id, session_id, assistant_message_id, tool_name, tool_revision, effect, args_json, status, started_at, ended_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?8, NULL)",
                params![
                    id,
                    session_id,
                    assistant_message_id,
                    tool_name,
                    tool_revision,
                    effect,
                    args_json,
                    now
                ],
            )
            .map_err(|e| format!("写入 Agent 工具调用失败: {}", e))?;
        self.touch_agent_session(session_id)?;
        Ok(())
    }

    /// 更新 Agent 工具调用状态
    pub fn finish_agent_tool_call(&self, id: &str, status: &str) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db
            .execute(
                "UPDATE agent_tool_calls SET status = ?1, ended_at = ?2 WHERE id = ?3",
                params![status, now, id],
            )
            .map_err(|e| format!("更新 Agent 工具调用失败: {}", e))?;
        Ok(())
    }

    /// 写入 Agent 工具结果
    pub fn insert_agent_tool_result(
        &self,
        id: &str,
        tool_call_id: &str,
        result_json: &str,
        preview_text: &str,
        output_ref: Option<&str>,
        status: &str,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db
            .execute(
                "INSERT OR REPLACE INTO agent_tool_results
                 (id, tool_call_id, result_json, preview_text, output_ref, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    tool_call_id,
                    result_json,
                    preview_text,
                    output_ref,
                    status,
                    now
                ],
            )
            .map_err(|e| format!("写入 Agent 工具结果失败: {}", e))?;
        Ok(())
    }

    /// 写入 Agent 事件
    pub fn insert_agent_event(
        &self,
        id: &str,
        session_id: &str,
        event_type: &str,
        payload_json: &str,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db
            .execute(
                "INSERT OR REPLACE INTO agent_events
                 (id, session_id, event_type, payload_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, session_id, event_type, payload_json, now],
            )
            .map_err(|e| format!("写入 Agent 事件失败: {}", e))?;
        self.touch_agent_session(session_id)?;
        Ok(())
    }

    /// 获取指定 Agent 会话快照
    pub fn get_agent_session_snapshot(
        &self,
        session_id: &str,
    ) -> Result<Option<AgentSessionSnapshot>, String> {
        let Some(session) = self.query_agent_session_by_id(session_id)? else {
            return Ok(None);
        };
        Ok(Some(AgentSessionSnapshot {
            messages: self.query_agent_messages(session_id)?,
            tool_calls: self.query_agent_tool_calls(session_id)?,
            tool_results: self.query_agent_tool_results(session_id)?,
            events: self.query_agent_events(session_id)?,
            session,
        }))
    }

    /// 获取项目和 slot 下最近一次 Agent 会话快照
    pub fn get_latest_agent_session_snapshot(
        &self,
        project_id: i64,
        slot: &str,
    ) -> Result<Option<AgentSessionSnapshot>, String> {
        let session_id: Option<String> = self
            .db
            .query_row(
                "SELECT id FROM agent_sessions
                 WHERE project_id = ?1 AND slot = ?2
                 ORDER BY updated_at DESC, started_at DESC
                 LIMIT 1",
                params![project_id, slot],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("查询最近 Agent 会话失败: {}", e))?;
        match session_id {
            Some(id) => self.get_agent_session_snapshot(&id),
            None => Ok(None),
        }
    }

    // ─── Private helpers ───

    fn touch_agent_session(&self, session_id: &str) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db
            .execute(
                "UPDATE agent_sessions SET updated_at = ?1 WHERE id = ?2",
                params![now, session_id],
            )
            .map_err(|e| format!("刷新 Agent 会话时间失败: {}", e))?;
        Ok(())
    }

    fn session_id_for_agent_message(&self, message_id: &str) -> Result<Option<String>, String> {
        self.db
            .query_row(
                "SELECT session_id FROM agent_messages WHERE id = ?1",
                params![message_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("查询 Agent 消息会话失败: {}", e))
    }

    fn query_agent_session_by_id(
        &self,
        session_id: &str,
    ) -> Result<Option<AgentSessionRecord>, String> {
        self.db
            .query_row(
                "SELECT id, project_id, slot, status, provider_id, model_id, started_at, updated_at, ended_at
                 FROM agent_sessions WHERE id = ?1",
                params![session_id],
                Self::row_to_agent_session,
            )
            .optional()
            .map_err(|e| format!("查询 Agent 会话失败: {}", e))
    }

    fn query_agent_messages(&self, session_id: &str) -> Result<Vec<AgentMessageRecord>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, session_id, role, content, status, parent_message_id, created_at
                 FROM agent_messages WHERE session_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(|e| format!("准备查询 Agent 消息失败: {}", e))?;
        let rows = stmt
            .query_map(params![session_id], Self::row_to_agent_message)
            .map_err(|e| format!("查询 Agent 消息失败: {}", e))?;
        collect_sql_rows(rows, "读取 Agent 消息失败")
    }

    fn query_agent_tool_calls(&self, session_id: &str) -> Result<Vec<AgentToolCallRecord>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, session_id, assistant_message_id, tool_name, tool_revision, effect, args_json, status, started_at, ended_at
                 FROM agent_tool_calls WHERE session_id = ?1 ORDER BY started_at ASC",
            )
            .map_err(|e| format!("准备查询 Agent 工具调用失败: {}", e))?;
        let rows = stmt
            .query_map(params![session_id], Self::row_to_agent_tool_call)
            .map_err(|e| format!("查询 Agent 工具调用失败: {}", e))?;
        collect_sql_rows(rows, "读取 Agent 工具调用失败")
    }

    fn query_agent_tool_results(
        &self,
        session_id: &str,
    ) -> Result<Vec<AgentToolResultRecord>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT r.id, r.tool_call_id, r.result_json, r.preview_text, r.output_ref, r.status, r.created_at
                 FROM agent_tool_results r
                 JOIN agent_tool_calls c ON c.id = r.tool_call_id
                 WHERE c.session_id = ?1
                 ORDER BY r.created_at ASC",
            )
            .map_err(|e| format!("准备查询 Agent 工具结果失败: {}", e))?;
        let rows = stmt
            .query_map(params![session_id], Self::row_to_agent_tool_result)
            .map_err(|e| format!("查询 Agent 工具结果失败: {}", e))?;
        collect_sql_rows(rows, "读取 Agent 工具结果失败")
    }

    fn query_agent_events(&self, session_id: &str) -> Result<Vec<AgentEventRecord>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, session_id, event_type, payload_json, created_at
                 FROM agent_events WHERE session_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(|e| format!("准备查询 Agent 事件失败: {}", e))?;
        let rows = stmt
            .query_map(params![session_id], Self::row_to_agent_event)
            .map_err(|e| format!("查询 Agent 事件失败: {}", e))?;
        collect_sql_rows(rows, "读取 Agent 事件失败")
    }

    fn row_to_agent_session(row: &rusqlite::Row) -> SqlResult<AgentSessionRecord> {
        Ok(AgentSessionRecord {
            id: row.get(0)?,
            project_id: row.get(1)?,
            slot: row.get(2)?,
            status: row.get(3)?,
            provider_id: row.get(4)?,
            model_id: row.get(5)?,
            started_at: row.get(6)?,
            updated_at: row.get(7)?,
            ended_at: row.get(8)?,
        })
    }

    fn row_to_agent_message(row: &rusqlite::Row) -> SqlResult<AgentMessageRecord> {
        Ok(AgentMessageRecord {
            id: row.get(0)?,
            session_id: row.get(1)?,
            role: row.get(2)?,
            content: row.get(3)?,
            status: row.get(4)?,
            parent_message_id: row.get(5)?,
            created_at: row.get(6)?,
        })
    }

    fn row_to_agent_tool_call(row: &rusqlite::Row) -> SqlResult<AgentToolCallRecord> {
        Ok(AgentToolCallRecord {
            id: row.get(0)?,
            session_id: row.get(1)?,
            assistant_message_id: row.get(2)?,
            tool_name: row.get(3)?,
            tool_revision: row.get(4)?,
            effect: row.get(5)?,
            args_json: row.get(6)?,
            status: row.get(7)?,
            started_at: row.get(8)?,
            ended_at: row.get(9)?,
        })
    }

    fn row_to_agent_tool_result(row: &rusqlite::Row) -> SqlResult<AgentToolResultRecord> {
        Ok(AgentToolResultRecord {
            id: row.get(0)?,
            tool_call_id: row.get(1)?,
            result_json: row.get(2)?,
            preview_text: row.get(3)?,
            output_ref: row.get(4)?,
            status: row.get(5)?,
            created_at: row.get(6)?,
        })
    }

    fn row_to_agent_event(row: &rusqlite::Row) -> SqlResult<AgentEventRecord> {
        Ok(AgentEventRecord {
            id: row.get(0)?,
            session_id: row.get(1)?,
            event_type: row.get(2)?,
            payload_json: row.get(3)?,
            created_at: row.get(4)?,
        })
    }

    fn row_to_document(row: &rusqlite::Row) -> SqlResult<DocumentMeta> {
        Ok(DocumentMeta {
            id: row.get(0)?,
            title: row.get(1)?,
            source_path: row.get(2)?,
            sha256: row.get(3)?,
            created_at: row.get(4)?,
            project_id: row.get(5)?,
            document_scope: row.get(6)?,
            chat_session_id: row.get(7)?,
            raw_source_identity: row.get(8)?,
        })
    }

    fn row_to_chunk(row: &rusqlite::Row) -> SqlResult<ChunkMeta> {
        Ok(ChunkMeta {
            id: row.get(0)?,
            vector_key: row.get(1)?,
            document_id: row.get(2)?,
            content: row.get(3)?,
            section_path: row.get(4)?,
            tags: row.get(5)?,
            line_no: row.get(6)?,
            parent_chunk_id: row.get(7)?,
            created_at: row.get(8)?,
        })
    }

    fn query_one_document(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Option<DocumentMeta>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let mut rows = stmt
            .query_map(params, Self::row_to_document)
            .map_err(|e| format!("Failed to query documents: {}", e))?;

        match rows.next() {
            Some(Ok(doc)) => Ok(Some(doc)),
            Some(Err(e)) => Err(format!("Failed to read document row: {}", e)),
            None => Ok(None),
        }
    }

    fn query_documents(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<DocumentMeta>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map(params, Self::row_to_document)
            .map_err(|e| format!("Failed to query documents: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Failed to read document row: {}", e))?);
        }
        Ok(results)
    }

    fn query_one_chunk(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Option<ChunkMeta>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let mut rows = stmt
            .query_map(params, Self::row_to_chunk)
            .map_err(|e| format!("Failed to query chunks: {}", e))?;

        match rows.next() {
            Some(Ok(chunk)) => Ok(Some(chunk)),
            Some(Err(e)) => Err(format!("Failed to read chunk row: {}", e)),
            None => Ok(None),
        }
    }

    fn query_chunks(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<ChunkMeta>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map(params, Self::row_to_chunk)
            .map_err(|e| format!("Failed to query chunks: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Failed to read chunk row: {}", e))?);
        }
        Ok(results)
    }
}

fn collect_sql_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> SqlResult<T>>,
    message: &str,
) -> Result<Vec<T>, String> {
    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| format!("{}: {}", message, e))?);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::project_store::ProjectStore;

    fn init_store_with_projects() -> (tempfile::TempDir, MetadataStore, i64, i64) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let project_store = ProjectStore::new(&db_path).unwrap();
        let default_project_id = project_store.ensure_default_project().unwrap();
        let other_project_id = project_store.create_project("测试项目 B", "", "").unwrap();
        drop(project_store);
        let store = MetadataStore::new(db_path).unwrap();
        (tmp, store, default_project_id, other_project_id)
    }

    #[test]
    fn test_metadata_store_crud() {
        let (_tmp, store, default_project_id, _) = init_store_with_projects();

        // Insert document
        let doc_id = store
            .insert_document(
                "测试文档",
                Some("/path/to/doc.md"),
                Some("abc123"),
                default_project_id,
                Some("knowledge"),
                None,
                Some("raw-id-001"),
            )
            .unwrap();
        assert!(doc_id > 0);

        // Check document
        let doc = store.get_document(doc_id).unwrap().unwrap();
        assert_eq!(doc.title, "测试文档");
        assert_eq!(doc.project_id, default_project_id);
        assert_eq!(doc.document_scope, "knowledge");
        assert_eq!(doc.chat_session_id, None);
        assert_eq!(doc.sha256.unwrap(), "abc123");
        assert_eq!(doc.raw_source_identity.unwrap(), "raw-id-001");

        // SHA256 dedup
        let doc_id2 = store
            .insert_document(
                "重复文档",
                None,
                Some("abc123"),
                default_project_id,
                Some("knowledge"),
                None,
                None,
            )
            .unwrap();
        assert_eq!(doc_id2, doc_id); // Should return existing ID

        // Insert chunks
        let chunk_id = store
            .insert_chunk(
                100,
                doc_id,
                "这是测试内容",
                Some("section/1"),
                Some(&["tag1".to_string()]),
                Some(1),
                None,
            )
            .unwrap();
        assert!(chunk_id > 0);

        // Get chunk by vector key
        let chunk = store.get_chunk_by_vector_key(100).unwrap().unwrap();
        assert_eq!(chunk.content, "这是测试内容");
        assert_eq!(chunk.vector_key, 100);

        // Stats
        let stats = store.get_stats(None).unwrap();
        assert_eq!(stats.document_count, 1);
        assert_eq!(stats.chunk_count, 1);

        // Delete chunk
        store.delete_chunk_by_vector_key(100).unwrap();
        assert!(store.get_chunk_by_vector_key(100).unwrap().is_none());

        // Delete document cascades
        store.delete_document(doc_id, None).unwrap();
        assert_eq!(store.get_stats(None).unwrap().document_count, 0);
    }

    #[test]
    fn test_sha256_dedup() {
        let (_tmp, store, default_project_id, _) = init_store_with_projects();

        let id1 = store
            .insert_document(
                "Doc A",
                None,
                Some("sha256_hash_xyz"),
                default_project_id,
                Some("knowledge"),
                None,
                None,
            )
            .unwrap();
        let id2 = store
            .insert_document(
                "Doc B",
                None,
                Some("sha256_hash_xyz"),
                default_project_id,
                Some("knowledge"),
                None,
                None,
            )
            .unwrap();

        assert_eq!(id1, id2);
        assert_eq!(store.get_stats(None).unwrap().document_count, 1);
    }

    #[test]
    fn test_project_id_filtering_and_chat_attachment_scope() {
        let (_tmp, store, default_project_id, other_project_id) = init_store_with_projects();

        store
            .insert_document(
                "Project A doc",
                None,
                Some("hash1"),
                default_project_id,
                Some("knowledge"),
                None,
                None,
            )
            .unwrap();
        store
            .insert_document(
                "Project B doc",
                None,
                Some("hash2"),
                other_project_id,
                Some("knowledge"),
                None,
                None,
            )
            .unwrap();
        let attachment_id = store
            .insert_document(
                "对话附件",
                None,
                Some("hash3"),
                default_project_id,
                Some("chat_attachment"),
                Some("session-123"),
                None,
            )
            .unwrap();

        let docs_a = store.list_documents(Some(default_project_id)).unwrap();
        assert_eq!(docs_a.len(), 1);
        assert_eq!(docs_a[0].project_id, default_project_id);
        assert_eq!(docs_a[0].document_scope, "knowledge");

        let docs_all = store.list_documents(None).unwrap();
        assert_eq!(docs_all.len(), 2);
        assert!(docs_all.iter().all(|doc| doc.document_scope == "knowledge"));

        let attachment = store.get_document(attachment_id).unwrap().unwrap();
        assert_eq!(attachment.project_id, default_project_id);
        assert_eq!(attachment.document_scope, "chat_attachment");
        assert_eq!(attachment.chat_session_id.as_deref(), Some("session-123"));
    }
}
