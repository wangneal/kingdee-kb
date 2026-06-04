//! SQLite metadata store for chunk↔vector mapping
//!
//! Manages documents and chunks tables with SHA256 dedup,
//! project_id filtering, and WAL journal mode.

use rusqlite::{params, Connection, Result as SqlResult};
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

/// Chunk metadata (one per vector in the HNSW index)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMeta {
    pub id: i64,
    pub vector_key: i64,
    pub document_id: i64,
    pub content: String,
    pub section_path: Option<String>,
    pub tags: Option<String>,
    pub line_no: Option<i64>,
    pub created_at: String,
}

/// Knowledge base statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeStats {
    pub document_count: i64,
    pub chunk_count: i64,
    pub db_path: String,
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
                created_at TEXT DEFAULT (datetime('now'))
            );

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
            ",
            )
            .map_err(|e| format!("Failed to initialize schema: {}", e))?;

        // 兼容已有数据库：先补齐列，再创建依赖这些列的索引。
        self.ensure_column("documents", "raw_source_identity", "TEXT DEFAULT NULL")?;
        self.ensure_column("documents", "project_id", "INTEGER")?;
        self.ensure_column(
            "documents",
            "document_scope",
            "TEXT NOT NULL DEFAULT 'knowledge'",
        )?;
        self.ensure_column("documents", "chat_session_id", "TEXT")?;
        self.ensure_column("chunks", "line_no", "INTEGER")?;
        self.ensure_column("deletion_outbox", "project_id", "INTEGER")?;
        self.backfill_document_project_id()?;

        self.db
            .execute_batch(
                "
            CREATE INDEX IF NOT EXISTS idx_chunks_vector_key ON chunks(vector_key);
            CREATE INDEX IF NOT EXISTS idx_chunks_document_id ON chunks(document_id);
            CREATE INDEX IF NOT EXISTS idx_documents_sha256 ON documents(sha256);
            CREATE INDEX IF NOT EXISTS idx_documents_project_id ON documents(project_id);
            CREATE INDEX IF NOT EXISTS idx_documents_scope ON documents(document_scope);
            CREATE INDEX IF NOT EXISTS idx_documents_chat_session_id ON documents(chat_session_id);
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

    fn backfill_document_project_id(&self) -> Result<(), String> {
        let default_project_id = match self.db.query_row(
            "SELECT id FROM projects WHERE status = 'active' ORDER BY id ASC LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        ) {
            Ok(id) => id,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(()),
            Err(e) => return Err(format!("读取默认项目失败: {}", e)),
        };

        self.db
            .execute(
                "UPDATE documents SET project_id = ?1 WHERE project_id IS NULL",
                params![default_project_id],
            )
            .map_err(|e| format!("回填文档默认项目失败: {}", e))?;
        Ok(())
    }

    // ─── Document operations ───

    /// Insert a new document. Returns the document ID.
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

        // If sha256 was provided and already existed, return existing ID
        if let Some(hash) = sha256 {
            if let Some(doc) = self.get_document_by_sha256(hash)? {
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

    /// Delete a document and its associated chunks
    /// If project_id is specified, verify the document belongs to that project before deleting
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
            .map_err(|e| format!("Failed to begin transaction: {}", e))?;

        let _chunks_deleted = tx
            .execute(
                &format!("DELETE FROM chunks WHERE document_id IN ({})", placeholders),
                params.as_slice(),
            )
            .map_err(|e| format!("Failed to batch-delete chunks: {}", e))?;

        let docs_deleted = tx
            .execute(
                &format!("DELETE FROM documents WHERE id IN ({})", placeholders),
                params.as_slice(),
            )
            .map_err(|e| format!("Failed to batch-delete documents: {}", e))?;

        tx.commit()
            .map_err(|e| format!("Failed to commit batch delete: {}", e))?;

        Ok(docs_deleted as u64)
    }

    /// Get vector keys for all chunks belonging to the given document IDs.
    /// Used to remove vectors from the usearch index when deleting documents.
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
    ) -> Result<i64, String> {
        let tags_json = tags.map(|t| serde_json::to_string(t).unwrap_or_default());

        self.db
            .execute(
                "INSERT INTO chunks (vector_key, document_id, content, section_path, tags, line_no)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    vector_key,
                    document_id,
                    content,
                    section_path,
                    tags_json,
                    line_no
                ],
            )
            .map_err(|e| format!("Failed to insert chunk: {}", e))?;

        Ok(self.db.last_insert_rowid())
    }

    /// Get a chunk by its vector key
    pub fn get_chunk_by_vector_key(&self, vector_key: i64) -> Result<Option<ChunkMeta>, String> {
        self.query_one_chunk(
            "SELECT id, vector_key, document_id, content, section_path, tags, line_no, created_at
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
            "SELECT id, vector_key, document_id, content, section_path, tags, line_no, created_at
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

    /// Get all chunks for a document
    pub fn get_chunks_by_document(&self, document_id: i64) -> Result<Vec<ChunkMeta>, String> {
        self.query_chunks(
            "SELECT id, vector_key, document_id, content, section_path, tags, line_no, created_at
             FROM chunks WHERE document_id = ?1 ORDER BY line_no, id",
            params![document_id],
        )
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

    // ─── Stats ───

    /// Get knowledge base statistics, optionally filtered by project_id
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

    // ─── Private helpers ───

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
            created_at: row.get(7)?,
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
    fn migrates_legacy_documents_table_before_creating_indexes() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");

        {
            let db = Connection::open(&db_path).unwrap();
            db.execute_batch(
                "
                CREATE TABLE documents (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    source_path TEXT,
                    sha256 TEXT UNIQUE,
                    project TEXT,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                CREATE TABLE chunks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    vector_key INTEGER UNIQUE,
                    document_id INTEGER REFERENCES documents(id),
                    content TEXT NOT NULL,
                    section_path TEXT,
                    tags TEXT,
                    line_no INTEGER,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                INSERT INTO documents (title, source_path, sha256, project)
                VALUES ('旧文档', '/legacy.md', 'legacy-sha', '默认项目');
                ",
            )
            .unwrap();
        }

        let project_store = ProjectStore::new(&db_path).unwrap();
        let default_project_id = project_store.ensure_default_project().unwrap();
        drop(project_store);

        let store = MetadataStore::new(db_path).unwrap();
        let docs = store.list_documents(None).unwrap();

        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].title, "旧文档");
        assert_eq!(docs[0].project_id, default_project_id);
        assert_eq!(docs[0].document_scope, "knowledge");
        assert_eq!(docs[0].chat_session_id, None);
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
