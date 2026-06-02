use regex::Regex;
use std::sync::LazyLock;

use super::types::{CheckResult, Checker, VerificationInput};

static RE_CITATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[来源：([^\]]+\.md)\]|[（(]来源：([^）)]+\.md)[）)]").unwrap()
});

static RE_SRC_SHORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[src:(\d+)\]").unwrap()
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

        let mut missing = Vec::new();
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

        let total_refs = citations.len() + src_count;
        let found = total_refs - missing.len();

        if total_refs == 0 {
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
            query: "迁移".to_string(),
            scenario: ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(!result.passed, "不存在的引用应检测失败");
    }

    #[tokio::test]
    async fn test_no_citations() {
        let checker = CitationExistenceChecker;
        let input = VerificationInput {
            generated_text: "你好，有什么可以帮你的？".to_string(),
            retrieved_chunks: vec![],
            chunk_titles: vec![],
            query: "你好".to_string(),
            scenario: ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(result.passed, "无引用的问候语应通过验证");
        assert!(result.confidence < 1.0, "无引用的置信度应较低");
    }
}
