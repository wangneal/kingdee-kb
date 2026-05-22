//! BM25 full-text search service using tantivy + jieba-rs
//!
//! Provides Chinese-aware full-text search via tantivy's BM25 scoring
//! with jieba `cut_for_search` tokenization.
//!
//! Index persisted to `~/.kingdee-kb/bm25_index/`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};

use jieba_rs::Jieba;

// ─── Jieba Tokenizer for tantivy ───

/// Tokenizer that uses jieba `cut_for_search` for Chinese text segmentation
pub struct JiebaTokenizer {
    jieba: Arc<Jieba>,
}

impl JiebaTokenizer {
    pub fn with_default_dict() -> Self {
        Self {
            jieba: Arc::new(Jieba::new()),
        }
    }
}

impl Clone for JiebaTokenizer {
    fn clone(&self) -> Self {
        Self {
            jieba: Arc::clone(&self.jieba),
        }
    }
}

/// Token stream from jieba cut_for_search
pub struct JiebaTokenStream {
    token: tantivy::tokenizer::Token,
    tokens: Vec<(usize, usize, String)>, // (byte_offset_from, byte_offset_to, text)
    index: usize,
}

impl tantivy::tokenizer::TokenStream for JiebaTokenStream {
    fn advance(&mut self) -> bool {
        if self.index < self.tokens.len() {
            let (offset_from, offset_to, ref text) = self.tokens[self.index];
            self.token.offset_from = offset_from;
            self.token.offset_to = offset_to;
            self.token.position = self.index;
            self.token.text.clear();
            self.token.text.push_str(text);
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn token(&self) -> &tantivy::tokenizer::Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut tantivy::tokenizer::Token {
        &mut self.token
    }
}

impl tantivy::tokenizer::Tokenizer for JiebaTokenizer {
    type TokenStream<'a> = JiebaTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let words = self.jieba.cut_for_search(text, false);
        let mut tokens = Vec::new();
        let mut search_from = 0usize;

        for word in words {
            // jieba returns &str slices; find their byte offsets in the original text
            if let Some(byte_start) = text[search_from..].find(word) {
                let byte_offset_from = search_from + byte_start;
                let byte_offset_to = byte_offset_from + word.len();
                tokens.push((byte_offset_from, byte_offset_to, word.to_string()));
                search_from = byte_offset_to;
            }
        }

        JiebaTokenStream {
            token: tantivy::tokenizer::Token::default(),
            tokens,
            index: 0,
        }
    }
}

// ─── BM25 Search Result ───

/// A result from BM25 search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BM25SearchResult {
    /// Chunk ID from the metadata store
    pub chunk_id: i64,
    /// Document title
    pub title: String,
    /// Chunk content (snippet)
    pub content: String,
    /// BM25 relevance score
    pub score: f32,
    /// Optional section path
    pub section_path: Option<String>,
    /// Project name
    pub project: String,
}

// ─── BM25 Service ───

/// BM25 full-text search service backed by tantivy with jieba tokenization
pub struct BM25Service {
    index: Index,
    reader: IndexReader,
    writer: Arc<Mutex<IndexWriter>>,
    index_dir: PathBuf,
    // Field handles (cached for performance)
    field_chunk_id: Field,
    field_content: Field,
    field_title: Field,
    field_section_path: Field,
    field_project: Field,
}

impl BM25Service {
    /// Create or open a BM25 index at the given directory
    pub fn new(index_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&index_dir)
            .map_err(|e| format!("Failed to create BM25 index directory: {}", e))?;

        let mut schema_builder = Schema::builder();

        // chunk_id: indexed i64 for deletion and retrieval
        let field_chunk_id = schema_builder.add_i64_field("chunk_id", STORED | INDEXED);

        // content: full-text indexed with jieba tokenizer, stored for snippets
        let field_content = schema_builder.add_text_field(
            "content",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("jieba")
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored(),
        );

        // title: full-text indexed with jieba, stored
        let field_title = schema_builder.add_text_field(
            "title",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("jieba")
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored(),
        );

        // section_path: stored only
        let field_section_path = schema_builder.add_text_field("section_path", STORED);

        // project: indexed with raw tokenizer for exact-match filtering, stored
        let field_project = schema_builder.add_text_field(
            "project",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("raw")
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored(),
        );

        let schema = schema_builder.build();

        // Open existing or create new index
        let index = if index_dir.join("meta.json").exists() {
            Index::open_in_dir(&index_dir)
                .map_err(|e| format!("Failed to open BM25 index: {}", e))?
        } else {
            Index::create_in_dir(&index_dir, schema.clone())
                .map_err(|e| format!("Failed to create BM25 index: {}", e))?
        };

        // Register tokenizers
        index
            .tokenizers()
            .register("jieba", JiebaTokenizer::with_default_dict());
        index
            .tokenizers()
            .register("raw", tantivy::tokenizer::RawTokenizer::default());

        // Reader with manual reload policy
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e| format!("Failed to create BM25 reader: {}", e))?;

        // Writer with 50MB heap
        let writer = index
            .writer(50_000_000)
            .map_err(|e| format!("Failed to create BM25 writer: {}", e))?;

        Ok(Self {
            index,
            reader,
            writer: Arc::new(Mutex::new(writer)),
            index_dir,
            field_chunk_id,
            field_content,
            field_title,
            field_section_path,
            field_project,
        })
    }

    /// Index a single chunk
    pub fn add_chunk(
        &self,
        chunk_id: i64,
        title: &str,
        content: &str,
        section_path: Option<&str>,
        project: &str,
    ) -> Result<(), String> {
        let mut writer = self.writer.lock().map_err(|e| e.to_string())?;

        let mut doc = TantivyDocument::new();
        doc.add_i64(self.field_chunk_id, chunk_id);
        doc.add_text(self.field_content, content);
        doc.add_text(self.field_title, title);
        doc.add_text(self.field_project, project);
        if let Some(sp) = section_path {
            doc.add_text(self.field_section_path, sp);
        }

        writer
            .add_document(doc)
            .map_err(|e| format!("Failed to index chunk {}: {}", chunk_id, e))?;

        Ok(())
    }

    /// Index multiple chunks in batch
    pub fn add_chunks(
        &self,
        chunks: &[(i64, String, String, Option<String>, String)],
    ) -> Result<(), String> {
        let mut writer = self.writer.lock().map_err(|e| e.to_string())?;

        for (chunk_id, title, content, section_path, project) in chunks {
            let mut doc = TantivyDocument::new();
            doc.add_i64(self.field_chunk_id, *chunk_id);
            doc.add_text(self.field_content, content.as_str());
            doc.add_text(self.field_title, title.as_str());
            doc.add_text(self.field_project, project.as_str());
            if let Some(sp) = section_path {
                doc.add_text(self.field_section_path, sp.as_str());
            }

            writer
                .add_document(doc)
                .map_err(|e| format!("Failed to index chunk {}: {}", chunk_id, e))?;
        }

        Ok(())
    }

    /// Remove a chunk from the index by its chunk_id
    pub fn remove_chunk(&self, chunk_id: i64) -> Result<(), String> {
        let mut writer = self.writer.lock().map_err(|e| e.to_string())?;
        let term = tantivy::Term::from_field_i64(self.field_chunk_id, chunk_id);
        writer.delete_term(term);
        Ok(())
    }

    /// Remove all chunks for a project
    pub fn remove_project(&self, project: &str) -> Result<(), String> {
        let mut writer = self.writer.lock().map_err(|e| e.to_string())?;
        let term = tantivy::Term::from_field_text(self.field_project, project);
        writer.delete_term(term);
        Ok(())
    }

    /// Commit pending changes and reload the reader
    pub fn commit(&self) -> Result<(), String> {
        let mut writer = self.writer.lock().map_err(|e| e.to_string())?;
        writer
            .commit()
            .map_err(|e| format!("Failed to commit BM25 index: {}", e))?;
        drop(writer);

        self.reader
            .reload()
            .map_err(|e| format!("Failed to reload BM25 reader: {}", e))?;

        Ok(())
    }

    /// Search for chunks matching the query, optionally filtered by project
    pub fn search(
        &self,
        query: &str,
        project_id: Option<&str>,
        top_k: u32,
    ) -> Result<Vec<BM25SearchResult>, String> {
        let searcher = self.reader.searcher();

        let query_parser =
            QueryParser::for_index(&self.index, vec![self.field_content, self.field_title]);

        // Add project filter if provided
        let full_query = if let Some(proj) = project_id {
            format!("{} AND project:\"{}\"", query, proj)
        } else {
            query.to_string()
        };

        let parsed_query = query_parser
            .parse_query(&full_query)
            .map_err(|e| format!("Failed to parse query '{}': {}", query, e))?;

        let top_docs = searcher
            .search(&parsed_query, &TopDocs::with_limit(top_k as usize))
            .map_err(|e| format!("Search failed: {}", e))?;

        let mut results = Vec::new();

        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| format!("Failed to retrieve doc: {}", e))?;

            let chunk_id = doc
                .get_first(self.field_chunk_id)
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let title = doc
                .get_first(self.field_title)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let content = doc
                .get_first(self.field_content)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let section_path = doc
                .get_first(self.field_section_path)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let project = doc
                .get_first(self.field_project)
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();

            results.push(BM25SearchResult {
                chunk_id,
                title,
                content,
                score,
                section_path,
                project,
            });
        }

        Ok(results)
    }

    /// Get the number of indexed documents
    pub fn doc_count(&self) -> usize {
        self.reader.searcher().num_docs() as usize
    }

    /// Get index directory path
    pub fn index_dir(&self) -> &PathBuf {
        &self.index_dir
    }

    /// Rebuild the entire index from provided chunks
    pub fn rebuild(
        &self,
        chunks: &[(i64, String, String, Option<String>, String)],
    ) -> Result<(), String> {
        let mut writer = self.writer.lock().map_err(|e| e.to_string())?;

        // Clear all existing documents
        writer
            .delete_all_documents()
            .map_err(|e| format!("Failed to clear BM25 index: {}", e))?;

        // Re-index all chunks
        for (chunk_id, title, content, section_path, project) in chunks {
            let mut doc = TantivyDocument::new();
            doc.add_i64(self.field_chunk_id, *chunk_id);
            doc.add_text(self.field_content, content.as_str());
            doc.add_text(self.field_title, title.as_str());
            doc.add_text(self.field_project, project.as_str());
            if let Some(sp) = section_path {
                doc.add_text(self.field_section_path, sp.as_str());
            }

            writer
                .add_document(doc)
                .map_err(|e| format!("Failed to index chunk {}: {}", chunk_id, e))?;
        }

        writer
            .commit()
            .map_err(|e| format!("Failed to commit BM25 rebuild: {}", e))?;
        drop(writer);

        self.reader
            .reload()
            .map_err(|e| format!("Failed to reload BM25 reader: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bm25_service_create_and_search() {
        let tmp = tempfile::tempdir().unwrap();
        let index_dir = tmp.path().join("bm25");
        let service = BM25Service::new(index_dir).unwrap();

        // Index some Chinese content
        service
            .add_chunk(
                1,
                "金蝶苍穹开发指南",
                "金蝶苍穹是一个企业级PaaS平台，提供了丰富的API和开发框架",
                Some("第一章/概述"),
                "default",
            )
            .unwrap();

        service
            .add_chunk(
                2,
                "表单插件开发",
                "表单插件是金蝶苍穹中最常用的扩展方式，可以通过Java代码自定义表单行为",
                Some("第二章/插件"),
                "default",
            )
            .unwrap();

        service
            .add_chunk(
                3,
                "工作流配置",
                "工作流引擎支持复杂的审批流程配置，包括条件分支、并行审批等",
                Some("第三章/工作流"),
                "project_a",
            )
            .unwrap();

        service.commit().unwrap();

        // Search for Chinese content
        let results = service.search("表单插件", None, 10).unwrap();
        assert!(!results.is_empty(), "Should find results for '表单插件'");
        assert_eq!(results[0].chunk_id, 2, "Chunk 2 should be top result");

        // Search with project filter
        let results_a = service.search("工作流", Some("project_a"), 10).unwrap();
        assert!(!results_a.is_empty(), "Should find '工作流' in project_a");
        assert_eq!(results_a[0].chunk_id, 3);

        // Search in wrong project
        let results_b = service.search("工作流", Some("project_b"), 10).unwrap();
        assert!(results_b.is_empty(), "Should not find '工作流' in project_b");
    }

    #[test]
    fn test_bm25_incremental_update() {
        let tmp = tempfile::tempdir().unwrap();
        let index_dir = tmp.path().join("bm25");
        let service = BM25Service::new(index_dir).unwrap();

        // Index
        service
            .add_chunk(1, "测试文档", "这是一个测试内容", None, "default")
            .unwrap();
        service.commit().unwrap();
        assert_eq!(service.doc_count(), 1);

        // Delete
        service.remove_chunk(1).unwrap();
        service.commit().unwrap();

        let results = service.search("测试内容", None, 10).unwrap();
        assert!(
            results.is_empty(),
            "Deleted chunk should not appear in search"
        );
    }

    #[test]
    fn test_jieba_tokenizer() {
        let jieba = Jieba::new();
        let words = jieba.cut_for_search("金蝶苍穹企业级PaaS平台", false);
        assert!(words.len() > 1, "jieba should produce multiple tokens");
        let combined: String = words.join("");
        assert!(
            combined.contains("金蝶") || combined.contains("苍穹"),
            "Should contain expected segments"
        );
    }

    #[test]
    fn test_bm25_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let index_dir = tmp.path().join("bm25");
        let service = BM25Service::new(index_dir).unwrap();

        service
            .add_chunk(1, "Doc A", "内容A", None, "default")
            .unwrap();
        service.commit().unwrap();
        assert_eq!(service.doc_count(), 1);

        // Rebuild with new content
        let chunks = vec![
            (
                10,
                "New Doc".to_string(),
                "新内容".to_string(),
                None,
                "default".to_string(),
            ),
            (
                11,
                "Another Doc".to_string(),
                "另一篇文档".to_string(),
                None,
                "default".to_string(),
            ),
        ];
        service.rebuild(&chunks).unwrap();

        // Old content gone, new content present
        let results = service.search("内容A", None, 10).unwrap();
        assert!(
            results.is_empty(),
            "Old content should be gone after rebuild"
        );

        let results = service.search("新内容", None, 10).unwrap();
        assert!(
            !results.is_empty(),
            "New content should be searchable after rebuild"
        );
    }
}
