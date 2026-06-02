//! SQLite metadata store for chunk↔vector mapping
//!
//! Manages documents and chunks tables with SHA256 dedup,
//! project filtering, and WAL journal mode.

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
    pub project: String,
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
                project TEXT DEFAULT 'default'
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

            CREATE INDEX IF NOT EXISTS idx_chunks_vector_key ON chunks(vector_key);
            CREATE INDEX IF NOT EXISTS idx_chunks_document_id ON chunks(document_id);
            CREATE INDEX IF NOT EXISTS idx_documents_sha256 ON documents(sha256);
            CREATE INDEX IF NOT EXISTS idx_documents_project ON documents(project);

            CREATE TABLE IF NOT EXISTS vector_key_seq (
                id INTEGER PRIMARY KEY AUTOINCREMENT
            );
            ",
            )
            .map_err(|e| format!("Failed to initialize schema: {}", e))?;

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

    /// Insert a new document. Returns the document ID.
    pub fn insert_document(
        &self,
        title: &str,
        source_path: Option<&str>,
        sha256: Option<&str>,
        project: Option<&str>,
    ) -> Result<i64, String> {
        self.db
            .execute(
                "INSERT OR IGNORE INTO documents (title, source_path, sha256, project)
                 VALUES (?1, ?2, ?3, ?4)",
                params![title, source_path, sha256, project.unwrap_or("default")],
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

    /// Get a document by its SHA256 hash
    pub fn get_document_by_sha256(&self, sha256: &str) -> Result<Option<DocumentMeta>, String> {
        self.query_one_document(
            "SELECT id, title, source_path, sha256, created_at, project
             FROM documents WHERE sha256 = ?1",
            params![sha256],
        )
    }

    /// Get a document by its ID
    pub fn get_document(&self, id: i64) -> Result<Option<DocumentMeta>, String> {
        self.query_one_document(
            "SELECT id, title, source_path, sha256, created_at, project
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
            "SELECT id, title, source_path, sha256, created_at, project
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
    pub fn list_documents(&self, project: Option<&str>) -> Result<Vec<DocumentMeta>, String> {
        if let Some(proj) = project {
            self.query_documents(
                "SELECT id, title, source_path, sha256, created_at, project
                 FROM documents WHERE project = ?1 ORDER BY created_at DESC",
                params![proj],
            )
        } else {
            self.query_documents(
                "SELECT id, title, source_path, sha256, created_at, project
                 FROM documents ORDER BY created_at DESC",
                [],
            )
        }
    }

    /// Delete a document and its associated chunks
    /// If project is specified, verify the document belongs to that project before deleting
    pub fn delete_document(&self, id: i64, project: Option<&str>) -> Result<(), String> {
        // Verify project ownership if project is specified
        if let Some(pid) = project {
            let doc_project: String = self
                .db
                .query_row(
                    "SELECT project FROM documents WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|e| format!("Document {} not found: {}", id, e))?;
            if doc_project != pid {
                return Err(format!(
                    "Document {} belongs to project '{}', not '{}'",
                    id, doc_project, pid
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
    /// If project is specified, verify all documents belong to that project before deleting
    pub fn delete_documents_batch(
        &self,
        document_ids: Vec<i64>,
        project: Option<&str>,
    ) -> Result<u64, String> {
        if document_ids.is_empty() {
            return Ok(0);
        }

        // Verify project ownership if project is specified
        if let Some(pid) = project {
            for &doc_id in &document_ids {
                let doc_project: String = self
                    .db
                    .query_row(
                        "SELECT project FROM documents WHERE id = ?1",
                        params![doc_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("Document {} not found: {}", doc_id, e))?;
                if doc_project != pid {
                    return Err(format!(
                        "Document {} belongs to project '{}', not '{}'",
                        doc_id, doc_project, pid
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

    /// 获取所有 chat-attachments 项目的 chunk_id 列表，用于检索前置过滤
    pub fn get_chat_attachment_chunk_ids(&self) -> Result<Vec<i64>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT c.id FROM chunks c
                 JOIN documents d ON c.document_id = d.id
                 WHERE d.project LIKE 'chat-attachments:%'",
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

    /// Get knowledge base statistics, optionally filtered by project
    pub fn get_stats(&self, project: Option<&str>) -> Result<KnowledgeStats, String> {
        let doc_count: i64 = match project {
            Some(pid) => self
                .db
                .query_row(
                    "SELECT COUNT(*) FROM documents WHERE project = ?1",
                    [pid],
                    |row| row.get(0),
                )
                .map_err(|e| format!("Failed to count documents: {}", e))?,
            None => self
                .db
                .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
                .map_err(|e| format!("Failed to count documents: {}", e))?,
        };

        let chunk_count: i64 = match project {
            Some(pid) => self
                .db
                .query_row(
                    "SELECT COUNT(*) FROM chunks c JOIN documents d ON c.document_id = d.id WHERE d.project = ?1",
                    [pid],
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

    // ─── Private helpers ───

    fn row_to_document(row: &rusqlite::Row) -> SqlResult<DocumentMeta> {
        Ok(DocumentMeta {
            id: row.get(0)?,
            title: row.get(1)?,
            source_path: row.get(2)?,
            sha256: row.get(3)?,
            created_at: row.get(4)?,
            project: row.get(5)?,
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

    #[test]
    fn test_metadata_store_crud() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let store = MetadataStore::new(db_path).unwrap();

        // Insert document
        let doc_id = store
            .insert_document("测试文档", Some("/path/to/doc.md"), Some("abc123"), None)
            .unwrap();
        assert!(doc_id > 0);

        // Check document
        let doc = store.get_document(doc_id).unwrap().unwrap();
        assert_eq!(doc.title, "测试文档");
        assert_eq!(doc.sha256.unwrap(), "abc123");

        // SHA256 dedup
        let doc_id2 = store
            .insert_document("重复文档", None, Some("abc123"), None)
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
        let tmp = tempfile::tempdir().unwrap();
        let store = MetadataStore::new(tmp.path().join("test.db")).unwrap();

        let id1 = store
            .insert_document("Doc A", None, Some("sha256_hash_xyz"), None)
            .unwrap();
        let id2 = store
            .insert_document("Doc B", None, Some("sha256_hash_xyz"), None)
            .unwrap();

        assert_eq!(id1, id2);
        assert_eq!(store.get_stats(None).unwrap().document_count, 1);
    }

    #[test]
    fn test_project_filtering() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MetadataStore::new(tmp.path().join("test.db")).unwrap();

        store
            .insert_document("Project A doc", None, Some("hash1"), Some("project_a"))
            .unwrap();
        store
            .insert_document("Project B doc", None, Some("hash2"), Some("project_b"))
            .unwrap();

        let docs_a = store.list_documents(Some("project_a")).unwrap();
        assert_eq!(docs_a.len(), 1);
        assert_eq!(docs_a[0].project, "project_a");

        let docs_all = store.list_documents(None).unwrap();
        assert_eq!(docs_all.len(), 2);
    }
}
