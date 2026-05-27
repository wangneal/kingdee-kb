//! 核心服务 trait 定义
//!
//! 为 VectorIndex、MetadataStore、BM25Service、LLMService 定义抽象接口，
//! 使得：
//! 1. 可以编写 mock 实现进行单元测试
//! 2. 替换底层实现时无需修改所有调用点
//! 3. 函数签名不再依赖具体类型

use crate::services::bm25_service::BM25SearchResult;
use crate::services::llm_service::LLMConfig;
use crate::services::metadata::{ChunkMeta, DocumentMeta, KnowledgeStats};
use crate::services::vector_index::SearchResult;

// ─── 向量索引 trait ───

/// 向量相似性搜索抽象
pub trait VectorSearch {
    /// 添加单个向量
    fn add(&mut self, key: u64, vector: &[f32]) -> Result<(), String>;

    /// 批量添加向量
    fn add_batch(&mut self, keys: &[u64], vectors: &[Vec<f32>]) -> Result<(), String>;

    /// 搜索最相似的 top_k 个向量
    fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<SearchResult>, String>;

    /// 删除指定 key 的向量
    fn remove(&self, key: u64) -> Result<usize, String>;

    /// 持久化索引到磁盘
    fn save(&self) -> Result<(), String>;

    /// 当前向量数量
    fn len(&self) -> usize;

    /// 是否为空
    fn is_empty(&self) -> bool;
}

// ─── 元数据存储 trait ───

/// 文档和分块元数据存储抽象
pub trait MetadataStore {
    /// 插入文档记录，返回文档 ID
    fn insert_document(
        &self,
        title: &str,
        source_path: Option<&str>,
        sha256: Option<&str>,
        project: Option<&str>,
    ) -> Result<i64, String>;

    /// 根据 ID 获取文档
    fn get_document(&self, id: i64) -> Result<Option<DocumentMeta>, String>;

    /// 根据 SHA256 获取文档（去重用）
    fn get_document_by_sha256(&self, sha256: &str) -> Result<Option<DocumentMeta>, String>;

    /// 列出项目下的所有文档
    fn list_documents(&self, project: Option<&str>) -> Result<Vec<DocumentMeta>, String>;

    /// 删除文档及其所有分块
    fn delete_document(&self, id: i64) -> Result<(), String>;

    /// 批量删除文档
    fn delete_documents_batch(&self, document_ids: Vec<i64>) -> Result<u64, String>;

    /// 获取下一个可用的向量 key
    fn next_vector_key(&self) -> Result<i64, String>;

    /// 插入分块记录
    fn insert_chunk(
        &self,
        vector_key: i64,
        document_id: i64,
        content: &str,
        section_path: Option<&str>,
        tags: Option<&[String]>,
        line_no: Option<i64>,
    ) -> Result<i64, String>;

    /// 根据向量 key 获取分块
    fn get_chunk_by_vector_key(&self, vector_key: i64) -> Result<Option<ChunkMeta>, String>;

    /// 批量获取分块
    fn get_chunks_by_vector_keys(&self, keys: &[i64]) -> Result<Vec<ChunkMeta>, String>;

    /// 获取文档下的所有分块
    fn get_chunks_by_document(&self, document_id: i64) -> Result<Vec<ChunkMeta>, String>;

    /// 删除分块
    fn delete_chunk_by_vector_key(&self, vector_key: i64) -> Result<(), String>;

    /// 获取知识库统计
    fn get_stats(&self) -> Result<KnowledgeStats, String>;
}

// ─── BM25 全文搜索 trait ───

/// BM25 全文搜索抽象
pub trait BM25Search {
    /// 添加单个分块到索引
    fn add_chunk(
        &self,
        chunk_id: i64,
        title: &str,
        content: &str,
        section_path: Option<&str>,
        project: &str,
    ) -> Result<(), String>;

    /// 批量添加分块
    fn add_chunks(
        &self,
        chunks: &[(i64, String, String, Option<String>, String)],
    ) -> Result<(), String>;

    /// 删除分块
    fn remove_chunk(&self, chunk_id: i64) -> Result<(), String>;

    /// 删除项目下的所有分块
    fn remove_project(&self, project: &str) -> Result<(), String>;

    /// 提交变更到索引
    fn commit(&self) -> Result<(), String>;

    /// 全文搜索
    fn search(
        &self,
        query: &str,
        project: Option<&str>,
        top_k: u32,
    ) -> Result<Vec<BM25SearchResult>, String>;

    /// 当前文档数量
    fn doc_count(&self) -> usize;

    /// 重建索引
    fn rebuild(
        &self,
        chunks: &[(i64, String, String, Option<String>, String)],
    ) -> Result<(), String>;
}

// ─── LLM 服务 trait ───

/// LLM 服务抽象（同步部分）
///
/// 注意：rag_query 等异步方法因与其他服务深度耦合（EmbeddingService、VectorIndex 等），
/// 暂不纳入 trait。如需测试这些方法，请使用集成测试。
pub trait LLMServiceSync {
    /// 是否已配置
    fn is_configured(&self) -> bool;

    /// 获取当前配置
    fn get_config(&self) -> Result<LLMConfig, String>;

    /// 更新配置
    fn set_config(&self, config: LLMConfig) -> Result<(), String>;

    /// 同步生成文本（非流式）
    fn generate_text_sync(&self, system_prompt: &str, user_message: &str)
        -> Result<String, String>;
}

#[cfg(test)]
mod tests {
    // Trait 定义测试：确保所有 trait 能作为 bound 使用

    #[test]
    fn test_traits_are_usable_as_bounds() {
        // 编译期验证：trait 定义正确可引用
        fn _check_search(_: &mut dyn super::VectorSearch) {}
        fn _check_bm25(_: &dyn super::BM25Search) {}
        fn _check_meta(_: &dyn super::MetadataStore) {}
        let _ = _check_search as fn(&mut dyn super::VectorSearch);
        let _ = _check_bm25 as fn(&dyn super::BM25Search);
        let _ = _check_meta as fn(&dyn super::MetadataStore);
    }
}
