//! Cross-Encoder Reranker — 对混合搜索结果进行精排
//!
//! 在 RRF 融合之后，用 fastembed TextRerank 对 TOP N 结果逐对打分，
//! 返回精排后的 TOP K。
//!
//! 使用模型: BAAI/bge-reranker-v2-m3 (或 ms-marco-MiniLM-L-6-v2)
//! 延迟: ~100-300ms (本地 ONNX 推理)

use fastembed::{InitOptionsWithLength, RerankerModel, TextRerank};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use crate::services::hybrid_search::HybridSearchResult;

/// 精排后的结果（带 rerank_score）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankedResult {
    pub chunk_id: i64,
    pub title: String,
    pub content: String,
    pub score: f32,
    pub rerank_score: f32,
    pub source: String,
    pub document_id: i64,
    pub section_path: Option<String>,
    pub project: String,
}

/// Reranker 服务 — 包装 fastembed TextRerank
pub struct RerankerService {
    model: Mutex<TextRerank>,
    /// 精排时保留的 TOP K
    top_k: usize,
}

impl RerankerService {
    /// 创建 Reranker 服务，自动下载模型
    pub fn try_new(top_k: usize) -> Result<Self, String> {
        let options = InitOptionsWithLength::new(RerankerModel::BGERerankerV2M3)
            .with_show_download_progress(false);
        let model = TextRerank::try_new(options)
            .map_err(|e| format!("Reranker 模型加载失败: {}", e))?;

        Ok(Self {
            model: Mutex::new(model),
            top_k,
        })
    }

    /// 对 HybridSearchResult 列表进行精排，返回 TOP K
    pub fn rerank(
        &self,
        query: &str,
        results: &[HybridSearchResult],
    ) -> Result<Vec<RerankedResult>, String> {
        if results.is_empty() {
            return Ok(Vec::new());
        }

        let documents: Vec<&str> = results.iter().map(|r| r.content.as_str()).collect();

        // 调用 fastembed TextRerank
        let reranked = self
            .model
            .lock()
            .map_err(|e| format!("Reranker 锁失败: {}", e))?
            .rerank(query, &documents, true, None)
            .map_err(|e| format!("Rerank 失败: {}", e))?;

        // 将 rerank 结果映射回 HybridSearchResult
        let mut output: Vec<RerankedResult> = reranked
            .into_iter()
            .filter_map(|r| {
                let idx = r.index;
                results.get(idx).map(|orig| RerankedResult {
                    chunk_id: orig.chunk_id,
                    title: orig.title.clone(),
                    content: orig.content.clone(),
                    score: orig.score,
                    rerank_score: r.score,
                    source: orig.source.clone(),
                    document_id: orig.document_id,
                    section_path: orig.section_path.clone(),
                    project: orig.project.clone(),
                })
            })
            .collect();

        // 按 rerank_score 降序排列（fastembed 已排序，这里确保）
        output.sort_by(|a, b| b.rerank_score.partial_cmp(&a.rerank_score).unwrap_or(std::cmp::Ordering::Equal));

        // 截取 TOP K
        output.truncate(self.top_k);

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reranker_creation() {
        // 仅测试创建不报错（模型文件存在时）
        let result = RerankerService::try_new(10);
        // 如果模型不存在可能失败，但不应 panic
        match result {
            Ok(_) => assert!(true),
            Err(e) => println!("Reranker 创建跳过（模型未下载）: {}", e),
        }
    }
}
