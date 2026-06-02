use regex::Regex;
use std::sync::LazyLock;

use super::types::{CheckResult, Checker, VerificationInput};

static RE_CITATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[来源：([^\]]+\.md)\]|[（(]来源：([^）)]+\.md)[）)]").unwrap()
});

static RE_SRC_SHORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[src:(\d+)\]").unwrap()
});

/// 匹配 [chunk:N] 格式 — 程序化可校验的段落 ID 引用
static RE_CHUNK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[chunk:(\d+)\]").unwrap()
});

pub struct CitationExistenceChecker;

#[async_trait::async_trait]
impl Checker for CitationExistenceChecker {
    fn name(&self) -> &str {
        "citation_existence"
    }

    async fn check(&self, input: &VerificationInput) -> CheckResult {
        let text = &input.generated_text;

        let mut citations: Vec<String> = Vec::new();

        for cap in RE_CITATION.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                citations.push(m.as_str().to_string());
            } else if let Some(m) = cap.get(2) {
                citations.push(m.as_str().to_string());
            }
        }

        let src_count = RE_SRC_SHORT.find_iter(text).count();

        // 解析 [chunk:N] 格式引用并校验 chunk_id 存在性
        let chunk_refs: Vec<i64> = RE_CHUNK
            .captures_iter(text)
            .filter_map(|cap| cap.get(1))
            .filter_map(|m| m.as_str().parse::<i64>().ok())
            .collect();

        let mut missing = Vec::new();

        // 校验 [来源：] 或（来源：）格式
        for citation in &citations {
            let exists = input.chunk_titles.iter().any(|t| {
                t.to_lowercase().contains(&citation.to_lowercase())
                    || input.retrieved_chunks.iter().any(|c| {
                        c.to_lowercase().contains(&citation.to_lowercase())
                    })
            });
            if !exists {
                missing.push(citation.clone());
            }
        }

        // 校验 [chunk:N] 格式
        let mut missing_chunks = Vec::new();
        for chunk_id in &chunk_refs {
            if !input.available_chunk_ids.contains(chunk_id) {
                missing_chunks.push(format!("chunk:{}", chunk_id));
            }
        }
        missing.extend(missing_chunks.iter().cloned());

        let total_refs = citations.len() + src_count + chunk_refs.len();
        let found = total_refs - missing.len();

        if total_refs == 0 {
            // 没有引用标记 → 不是幻觉，但回答可能缺乏溯源
            return CheckResult::pass("citation_existence")
                .with_confidence(0.7)
                .with_evidence(vec!["回答中未包含引用来源标记".to_string()]);
        }

        if missing.is_empty() {
            CheckResult::pass("citation_existence")
                .with_evidence(vec![format!("所有 {} 个引用均在检索结果中找到", total_refs)])
        } else {
            let ratio = found as f32 / total_refs as f32;
            CheckResult::fail("citation_existence", format!("{} 个引用未在检索结果中找到: {:?}", missing.len(), missing))
                .with_confidence(ratio)
                .with_evidence(missing)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::verification::types::ScenarioType;

    #[tokio::test]
    async fn test_all_citations_found() {
        let checker = CitationExistenceChecker;
        let input = VerificationInput {
            generated_text: "金蝶云星空支持多组织架构（来源：金蝶云星空介绍.md）".to_string(),
            retrieved_chunks: vec!["金蝶云星空支持多组织架构和协同业务".to_string()],
            chunk_titles: vec!["金蝶云星空介绍.md".to_string()],
            available_chunk_ids: vec![],
            query: "金蝶云星空特性".to_string(),
            scenario: ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(result.passed, "引用应在检索结果中找到: {}", result.detail);
    }

    #[tokio::test]
    async fn test_citation_not_found() {
        let checker = CitationExistenceChecker;
        let input = VerificationInput {
            generated_text: "K/3 WISE 支持一键迁移到金蝶云星空（来源：不存在文档.md）".to_string(),
            retrieved_chunks: vec!["金蝶云星空支持多组织".to_string()],
            chunk_titles: vec!["金蝶云星空介绍.md".to_string()],
            available_chunk_ids: vec![],
            query: "迁移".to_string(),
            scenario: ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(!result.passed, "不存在的引用应检测失败");
    }

    #[tokio::test]
    async fn test_chunk_ref_found() {
        let checker = CitationExistenceChecker;
        let input = VerificationInput {
            generated_text: "金蝶云星空支持多组织架构[chunk:1]。配置路径详见系统管理[chunk:2]。".to_string(),
            retrieved_chunks: vec![],
            chunk_titles: vec![],
            available_chunk_ids: vec![1, 2, 3],
            query: "金蝶云星空".to_string(),
            scenario: ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(result.passed, "存在的 chunk 引用应通过: {}", result.detail);
    }

    #[tokio::test]
    async fn test_chunk_ref_not_found() {
        let checker = CitationExistenceChecker;
        let input = VerificationInput {
            generated_text: "金蝶云星空支持多组织架构[chunk:999]。".to_string(),
            retrieved_chunks: vec![],
            chunk_titles: vec![],
            available_chunk_ids: vec![1, 2, 3],
            query: "金蝶云星空".to_string(),
            scenario: ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(!result.passed, "不存在的 chunk 引用应检测失败");
    }

    #[tokio::test]
    async fn test_no_citations() {
        let checker = CitationExistenceChecker;
        let input = VerificationInput {
            generated_text: "你好，有什么可以帮你的？".to_string(),
            retrieved_chunks: vec![],
            chunk_titles: vec![],
            available_chunk_ids: vec![],
            query: "你好".to_string(),
            scenario: ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(result.passed, "无引用的问候语应通过验证");
        assert!(result.confidence < 1.0, "无引用的置信度应较低");
    }
}
