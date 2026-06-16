//! 缺失检测 — 评估检索结果质量，不足时触发补充检索
//!
//! 流程：
//! 1. 首轮检索后检查 TOP 3 结果分数
//! 2. 如果分数低于阈值 → 缺信息
//! 3. 自动用查询扩展/同义词改写重试
//! 4. 合并两轮结果去重

use crate::services::hybrid_search::HybridSearchResult;

/// 检索质量评估结果
#[derive(Debug, Clone)]
pub enum RetrievalQuality {
    /// 检索充分，可直接使用
    Sufficient(Vec<HybridSearchResult>),
    /// 检索不足，需要补充
    Insufficient {
        primary: Vec<HybridSearchResult>,
        reason: String,
    },
}

/// 评估检索结果质量
///
/// 检查逻辑：
/// - 如果结果为 0 → 缺失
/// - 如果 TOP 1 分数 < 0.3 → 可能缺失（低分意味着相关性不足）
/// - 如果 TOP 3 平均分 < 0.2 → 可能缺失
pub fn assess_quality(results: &[HybridSearchResult]) -> RetrievalQuality {
    if results.is_empty() {
        return RetrievalQuality::Insufficient {
            primary: vec![],
            reason: "检索结果为 0".to_string(),
        };
    }

    let top_score = results.first().map(|r| r.score).unwrap_or(0.0);
    let top3_avg: f32 =
        results.iter().take(3).map(|r| r.score).sum::<f32>() / results.len().min(3) as f32;

    let mut reasons = Vec::new();
    if top_score < 0.3 {
        reasons.push(format!("TOP 1 分数偏低 ({:.2})", top_score));
    }
    if top3_avg < 0.2 {
        reasons.push(format!("TOP 3 平均分偏低 ({:.2})", top3_avg));
    }

    if reasons.is_empty() {
        RetrievalQuality::Sufficient(results.to_vec())
    } else {
        RetrievalQuality::Insufficient {
            primary: results.to_vec(),
            reason: reasons.join("; "),
        }
    }
}

/// 对查询进行简单扩展（同义词改写），用于补充检索
///
/// 简单策略：在查询前/后添加扩展词
/// 更复杂的策略可使用 LLM（后续可扩展）
pub fn expand_query(query: &str) -> Vec<String> {
    let mut expanded = Vec::new();

    // 策略 1：原查询
    expanded.push(query.to_string());

    // 策略 2：添加金蝶ERP上下文
    let prefixes = ["金蝶", "ERP"];
    for prefix in &prefixes {
        if !query.contains(prefix) {
            expanded.push(format!("{} {}", prefix, query));
        }
    }

    // 策略 3：去掉特定词
    let stripped = query
        .replace("有什么区别", " 差异")
        .replace("如何配置", " 设置")
        .replace("怎么用", " 操作");
    if stripped != query {
        expanded.push(stripped);
    }

    expanded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_results() {
        let quality = assess_quality(&[]);
        assert!(matches!(quality, RetrievalQuality::Insufficient { .. }));
    }

    #[test]
    fn test_good_results() {
        let results = vec![
            HybridSearchResult {
                chunk_id: 1,
                title: "".to_string(),
                content: "".to_string(),
                score: 0.8,
                source: "vector".to_string(),
                document_id: 1,
                section_path: None,
                project: "test".to_string(),
                parent_chunk_id: None,
            },
            HybridSearchResult {
                chunk_id: 2,
                title: "".to_string(),
                content: "".to_string(),
                score: 0.6,
                source: "vector".to_string(),
                document_id: 1,
                section_path: None,
                project: "test".to_string(),
                parent_chunk_id: None,
            },
        ];
        let quality = assess_quality(&results);
        assert!(matches!(quality, RetrievalQuality::Sufficient(_)));
    }
}
