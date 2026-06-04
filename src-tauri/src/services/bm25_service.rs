//! BM25 全文搜索服务，使用 tantivy + jieba-rs
//!
//! 通过 tantivy 的 BM25 评分和 jieba `cut_for_search` 分词，
//! 提供中文感知的全文搜索。
//!
//! 索引持久化到 `~/.kingdee-kb/bm25_index/`。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::*;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};

use jieba_rs::Jieba;

fn project_allowed(project: &str, project_id: Option<&str>, extra_project_ids: &[String]) -> bool {
    if extra_project_ids.iter().any(|p| p == project) {
        return true;
    }
    project_id.map_or(true, |pid| project == pid)
}

// ─── tantivy 的 Jieba 分词器 ───

/// 使用 jieba `cut_for_search` 进行中文文本分词的分词器
pub struct JiebaTokenizer {
    jieba: Arc<Jieba>,
}

/// 金蝶/ERP 领域专业术语 — 确保这些词不被 jieba 误拆分
const KINGDEE_DOMAIN_WORDS: &[(&str, usize)] = &[
    // 财务会计
    ("科目", 10),
    ("凭证", 10),
    ("核算", 10),
    ("辅助核算", 15),
    ("损益", 10),
    ("资产负债", 12),
    ("应收应付", 12),
    ("应收账款", 12),
    ("应付账款", 12),
    ("余额", 10),
    ("借方", 10),
    ("贷方", 10),
    ("总账", 10),
    ("明细账", 10),
    ("日记账", 10),
    ("试算平衡", 12),
    ("结转", 10),
    ("折旧", 10),
    ("摊销", 10),
    ("计提", 10),
    ("成本核算", 12),
    ("存货核算", 12),
    ("固定资产", 12),
    // 金蝶平台
    ("苍穹", 10),
    ("金蝶苍穹", 15),
    ("表单插件", 12),
    ("工作流", 10),
    ("基础资料", 12),
    ("动态表单", 12),
    ("单据", 10),
    ("单据转换", 12),
    ("操作插件", 12),
    ("报表插件", 12),
    ("菜单", 10),
    ("权限", 10),
    ("组织架构", 12),
    ("角色权限", 12),
    ("数据权限", 12),
    ("业务对象", 12),
    ("实体", 10),
    ("字段", 10),
    ("分录", 10),
    ("树形分录", 12),
    ("审批流程", 12),
    ("消息中心", 12),
    // 供应链
    ("采购订单", 12),
    ("销售订单", 12),
    ("入库单", 10),
    ("出库单", 10),
    ("库存调拨", 12),
    ("物料", 10),
    ("供应商", 10),
    ("客户", 10),
    ("BOM", 5),
    ("供应链", 10),
    ("物料清单", 12),
    // 其他 ERP
    ("ERP", 5),
    ("财务报表", 12),
    ("利润表", 10),
    ("现金流量", 12),
    ("多核算维度", 15),
    ("核算维度", 12),
    ("预算管理", 12),
    ("资金管理", 12),
    ("银企互联", 12),
    ("税务", 10),
    ("增值税", 10),
    ("发票", 10),
    ("报销", 10),
];

impl JiebaTokenizer {
    /// 使用默认词典和金蝶领域专业术语创建 JiebaTokenizer
    pub fn with_domain_dict() -> Self {
        let mut jieba = Jieba::new();
        for (word, freq) in KINGDEE_DOMAIN_WORDS {
            jieba.add_word(word, Some(*freq), None);
        }
        Self {
            jieba: Arc::new(jieba),
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

/// jieba cut_for_search 的 token 流
pub struct JiebaTokenStream {
    token: tantivy::tokenizer::Token,
    tokens: Vec<(usize, usize, String)>, // （字节偏移起始、字节偏移结束、文本）
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
            // jieba 返回 &str 切片；在原始文本中查找其字节偏移量
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

// ─── BM25 搜索结果 ───

/// BM25 搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BM25SearchResult {
    /// 来自元数据存储的 Chunk ID
    pub chunk_id: i64,
    /// 文档标题
    pub title: String,
    /// Chunk 内容（片段）
    pub content: String,
    /// BM25 相关性评分
    pub score: f32,
    /// 可选的章节路径
    pub section_path: Option<String>,
    /// 项目名称
    pub project: String,
}

// ─── BM25 服务 ───

/// BM25 内部引擎（tantivy + jieba）
struct InnerBM25 {
    index: Index,
    reader: IndexReader,
    writer: Arc<Mutex<IndexWriter>>,
    field_chunk_id: Field,
    field_content: Field,
    field_title: Field,
    field_section_path: Field,
    field_project: Field,
}

impl InnerBM25 {
    /// 在指定目录创建或打开 BM25 索引
    fn build(index_dir: &std::path::Path) -> Result<Self, String> {
        std::fs::create_dir_all(index_dir)
            .map_err(|e| format!("Failed to create BM25 index directory: {}", e))?;

        let mut schema_builder = Schema::builder();

        // chunk_id：已索引的 i64 字段，用于删除和检索
        let field_chunk_id = schema_builder.add_i64_field("chunk_id", STORED | INDEXED);

        // content：使用 jieba 分词器进行全文索引，存储用于片段显示
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

        // title：使用 jieba 进行全文索引，已存储
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

        // section_path：仅存储
        let field_section_path = schema_builder.add_text_field("section_path", STORED);

        // project：使用原始分词器索引用于精确匹配过滤，已存储
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

        // 打开现有索引或创建新索引
        let index = if index_dir.join("meta.json").exists() {
            Index::open_in_dir(index_dir)
                .map_err(|e| format!("Failed to open BM25 index: {}", e))?
        } else {
            Index::create_in_dir(index_dir, schema.clone())
                .map_err(|e| format!("Failed to create BM25 index: {}", e))?
        };

        // 注册分词器（jieba 使用金蝶领域词典 + 原始分词器）
        index
            .tokenizers()
            .register("jieba", JiebaTokenizer::with_domain_dict());
        index
            .tokenizers()
            .register("raw", tantivy::tokenizer::RawTokenizer::default());

        // 使用手动重载策略的读取器
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e| format!("Failed to create BM25 reader: {}", e))?;

        // 使用 15MB 堆内存的写入器（tantivy BM25 要求至少 15MB）
        let writer = index
            .writer(15_000_000)
            .map_err(|e| format!("Failed to create BM25 writer: {}", e))?;

        Ok(Self {
            index,
            reader,
            writer: Arc::new(Mutex::new(writer)),
            field_chunk_id,
            field_content,
            field_title,
            field_section_path,
            field_project,
        })
    }
}

/// 基于 tantivy 和 jieba 分词的 BM25 全文搜索服务（支持懒加载）
pub struct BM25Service {
    inner: Option<InnerBM25>,
    index_dir: PathBuf,
}

impl BM25Service {
    /// 在指定目录创建或打开 BM25 索引（立即初始化）
    pub fn new(index_dir: PathBuf) -> Result<Self, String> {
        let inner = InnerBM25::build(&index_dir)?;
        Ok(Self {
            inner: Some(inner),
            index_dir,
        })
    }

    /// 创建懒加载 BM25 服务——首次使用时才初始化
    pub fn empty(index_dir: PathBuf) -> Self {
        Self {
            inner: None,
            index_dir,
        }
    }

    /// BM25 引擎是否已初始化
    pub fn is_ready(&self) -> bool {
        self.inner.is_some()
    }

    /// 确保 BM25 引擎已初始化（幂等安全）
    pub fn ensure_initialized(&mut self) -> Result<(), String> {
        if self.inner.is_some() {
            return Ok(());
        }
        let inner = InnerBM25::build(&self.index_dir)?;
        self.inner = Some(inner);
        Ok(())
    }

    /// 获取内部引擎引用（写操作调用，未初始化时返回错误）
    fn inner(&self) -> Result<&InnerBM25, String> {
        self.inner
            .as_ref()
            .ok_or_else(|| "BM25 全文搜索服务尚未初始化".to_string())
    }

    /// 索引单个 chunk
    pub fn add_chunk(
        &self,
        chunk_id: i64,
        title: &str,
        content: &str,
        section_path: Option<&str>,
        project: &str,
    ) -> Result<(), String> {
        let inner = self.inner()?;
        let writer = inner.writer.lock().map_err(|e| e.to_string())?;

        let mut doc = TantivyDocument::new();
        doc.add_i64(inner.field_chunk_id, chunk_id);
        doc.add_text(inner.field_content, content);
        doc.add_text(inner.field_title, title);
        doc.add_text(inner.field_project, project);
        if let Some(sp) = section_path {
            doc.add_text(inner.field_section_path, sp);
        }

        writer
            .add_document(doc)
            .map_err(|e| format!("Failed to index chunk {}: {}", chunk_id, e))?;

        Ok(())
    }

    /// 批量索引多个 chunk
    pub fn add_chunks(
        &self,
        chunks: &[(i64, String, String, Option<String>, String)],
    ) -> Result<(), String> {
        let inner = self.inner()?;
        let writer = inner.writer.lock().map_err(|e| e.to_string())?;
        for (chunk_id, title, content, section_path, project) in chunks {
            let mut doc = TantivyDocument::new();
            doc.add_i64(inner.field_chunk_id, *chunk_id);
            doc.add_text(inner.field_content, content.as_str());
            doc.add_text(inner.field_title, title.as_str());
            doc.add_text(inner.field_project, project.as_str());
            if let Some(sp) = section_path {
                doc.add_text(inner.field_section_path, sp.as_str());
            }

            writer
                .add_document(doc)
                .map_err(|e| format!("Failed to index chunk {}: {}", chunk_id, e))?;
        }

        Ok(())
    }

    /// 根据 chunk_id 从索引中移除 chunk
    pub fn remove_chunk(&self, chunk_id: i64) -> Result<(), String> {
        let inner = self.inner()?;
        let writer = inner.writer.lock().map_err(|e| e.to_string())?;
        let term = tantivy::Term::from_field_i64(inner.field_chunk_id, chunk_id);
        writer.delete_term(term);
        Ok(())
    }

    /// 移除多个 chunk 并提交（批量删除并持久化）
    pub fn remove_chunks(&self, chunk_ids: &[i64]) -> Result<(), String> {
        let inner = self.inner()?;
        let mut writer = inner.writer.lock().map_err(|e| e.to_string())?;
        for cid in chunk_ids {
            let term = tantivy::Term::from_field_i64(inner.field_chunk_id, *cid);
            writer.delete_term(term);
        }
        writer
            .commit()
            .map_err(|e| format!("BM25 批量删除提交失败: {}", e))?;
        drop(writer);
        inner
            .reader
            .reload()
            .map_err(|e| format!("BM25 重载失败: {}", e))?;
        Ok(())
    }

    /// 移除项目中的所有 chunk
    pub fn remove_project(&self, project: &str) -> Result<(), String> {
        let inner = self.inner()?;
        let writer = inner.writer.lock().map_err(|e| e.to_string())?;
        let term = tantivy::Term::from_field_text(inner.field_project, project);
        writer.delete_term(term);
        Ok(())
    }

    /// 提交待处理的更改并重新加载读取器
    pub fn commit(&self) -> Result<(), String> {
        let inner = self.inner()?;
        let mut writer = inner.writer.lock().map_err(|e| e.to_string())?;
        writer
            .commit()
            .map_err(|e| format!("Failed to commit BM25 index: {}", e))?;
        drop(writer);

        inner
            .reader
            .reload()
            .map_err(|e| format!("Failed to reload BM25 reader: {}", e))?;

        Ok(())
    }

    /// 搜索匹配查询的 chunk，可选按项目过滤。
    /// 支持在 tantivy 查询级别预过滤排除 chunk ID
    /// （修复聊天附件项目的"搜索劫持"问题）。
    /// 未初始化时返回空结果（不阻塞搜索流程）。
    pub fn search(
        &self,
        query: &str,
        project_id: Option<&str>,
        extra_project_ids: &[String],
        top_k: u32,
        exclude_chunk_ids: &[i64],
    ) -> Result<Vec<BM25SearchResult>, String> {
        let inner = match self.inner.as_ref() {
            Some(i) => i,
            None => return Ok(Vec::new()),
        };
        let searcher = inner.reader.searcher();

        let query_parser =
            QueryParser::for_index(&inner.index, vec![inner.field_content, inner.field_title]);

        // 构建用户查询
        let user_query_str = if let Some(proj) = project_id {
            let mut project_terms = vec![format!("project:\"{}\"", proj)];
            project_terms.extend(
                extra_project_ids
                    .iter()
                    .map(|project| format!("project:\"{}\"", project)),
            );
            format!("{} AND ({})", query, project_terms.join(" OR "))
        } else {
            query.to_string()
        };

        let parsed_user_query = query_parser
            .parse_query(&user_query_str)
            .map_err(|e| format!("无法解析查询 '{}': {}", query, e))?;

        // 前置过滤：将排除的 chunk_id 作为 MustNot 子句加入 BooleanQuery
        let final_query: Box<dyn tantivy::query::Query> = if exclude_chunk_ids.is_empty() {
            parsed_user_query
        } else {
            let mut subqueries: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::new();
            subqueries.push((Occur::Must, parsed_user_query));
            for cid in exclude_chunk_ids {
                let term = tantivy::Term::from_field_i64(inner.field_chunk_id, *cid);
                subqueries.push((
                    Occur::MustNot,
                    Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
                ));
            }
            Box::new(BooleanQuery::new(subqueries))
        };

        let top_docs = searcher
            .search(&final_query, &TopDocs::with_limit(top_k as usize))
            .map_err(|e| format!("搜索失败: {}", e))?;

        let mut results = Vec::new();

        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| format!("Failed to retrieve doc: {}", e))?;

            let chunk_id = doc
                .get_first(inner.field_chunk_id)
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let title = doc
                .get_first(inner.field_title)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let content = doc
                .get_first(inner.field_content)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let section_path = doc
                .get_first(inner.field_section_path)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let project = doc
                .get_first(inner.field_project)
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();

            if !project_allowed(&project, project_id, extra_project_ids) {
                continue;
            }

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

    /// 获取已索引文档数量（未初始化时返回 0）
    pub fn doc_count(&self) -> usize {
        match self.inner.as_ref() {
            Some(inner) => inner.reader.searcher().num_docs() as usize,
            None => 0,
        }
    }

    /// 获取索引目录路径
    pub fn index_dir(&self) -> &PathBuf {
        &self.index_dir
    }

    /// 使用提供的 chunk 重建整个索引
    pub fn rebuild(
        &self,
        chunks: &[(i64, String, String, Option<String>, String)],
    ) -> Result<(), String> {
        let inner = self.inner()?;
        let mut writer = inner.writer.lock().map_err(|e| e.to_string())?;
        // 清除所有现有文档
        writer
            .delete_all_documents()
            .map_err(|e| format!("Failed to clear BM25 index: {}", e))?;

        // 重新索引所有 chunk
        for (chunk_id, title, content, section_path, project) in chunks {
            let mut doc = TantivyDocument::new();
            doc.add_i64(inner.field_chunk_id, *chunk_id);
            doc.add_text(inner.field_content, content.as_str());
            doc.add_text(inner.field_title, title.as_str());
            doc.add_text(inner.field_project, project.as_str());
            if let Some(sp) = section_path {
                doc.add_text(inner.field_section_path, sp.as_str());
            }

            writer
                .add_document(doc)
                .map_err(|e| format!("Failed to index chunk {}: {}", chunk_id, e))?;
        }

            writer
                .commit()
                .map_err(|e| format!("Failed to commit BM25 rebuild: {}", e))?;
        drop(writer);

        inner
            .reader
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

        // 索引一些中文内容
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

        // 搜索中文内容
        let results = service.search("表单插件", None, &[], 10, &[]).unwrap();
        assert!(!results.is_empty(), "Should find results for '表单插件'");
        assert_eq!(results[0].chunk_id, 2, "Chunk 2 should be top result");

        // 使用项目过滤搜索
        let results_a = service
            .search("工作流", Some("project_a"), &[], 10, &[])
            .unwrap();
        assert!(!results_a.is_empty(), "Should find '工作流' in project_a");
        assert_eq!(results_a[0].chunk_id, 3);

        // 在错误的项目中搜索
        let results_b = service
            .search("工作流", Some("project_b"), &[], 10, &[])
            .unwrap();
        assert!(
            results_b.is_empty(),
            "Should not find '工作流' in project_b"
        );
    }

    #[test]
    fn test_bm25_incremental_update() {
        let tmp = tempfile::tempdir().unwrap();
        let index_dir = tmp.path().join("bm25");
        let service = BM25Service::new(index_dir).unwrap();

        // 索引
        service
            .add_chunk(1, "测试文档", "这是一个测试内容", None, "default")
            .unwrap();
        service.commit().unwrap();
        assert_eq!(service.doc_count(), 1);

        // 删除
        service.remove_chunk(1).unwrap();
        service.commit().unwrap();

        let results = service.search("测试内容", None, &[], 10, &[]).unwrap();
        assert!(
            results.is_empty(),
            "Deleted chunk should not appear in search"
        );
    }

    #[test]
    fn test_extra_project_ids_are_explicitly_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let index_dir = tmp.path().join("bm25");
        let service = BM25Service::new(index_dir).unwrap();

        service
            .add_chunk(1, "项目一资料", "项目关键词", None, "1")
            .unwrap();
        service
            .add_chunk(2, "项目二资料", "项目关键词", None, "2")
            .unwrap();
        service.commit().unwrap();

        let unrestricted = service.search("项目关键词", None, &[], 10, &[]).unwrap();
        assert_eq!(unrestricted.len(), 2);

        let scoped_project = "1".to_string();
        let scoped = service
            .search("项目关键词", Some("999"), &[scoped_project], 10, &[])
            .unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].project, "1");
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

        // 使用新内容重建
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

        // 旧内容已消失，新内容已存在
        let results = service.search("内容A", None, &[], 10, &[]).unwrap();
        assert!(
            results.is_empty(),
            "Old content should be gone after rebuild"
        );

        let results = service.search("新内容", None, &[], 10, &[]).unwrap();
        assert!(
            !results.is_empty(),
            "New content should be searchable after rebuild"
        );
    }
}
