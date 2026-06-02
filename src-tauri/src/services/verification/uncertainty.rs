use super::types::{CheckResult, Checker, VerificationInput};
use super::consistency::FactualConsistencyChecker;

pub struct UncertaintyMarker;

impl UncertaintyMarker {
    const ASSERTION_WITHOUT_SOURCE: &'static [&'static str] = &[
        "系统支持", "功能包括", "配置路径", "标准功能",
        "金蝶", "K/3", "星空", "苍穹",
    ];

    const UNCERTAINTY_SIGNALS: &'static [&'static str] = &[
        "可能", "应该", "一般来说", "通常", "据说",
        "maybe", "probably", "usually", "generally",
    ];

    fn has_citation_marker(sentence: &str) -> bool {
        sentence.contains("[来源：") || sentence.contains("[src:")
            || sentence.contains("(来源：") || sentence.contains("（来源：")
    }
}

#[async_trait::async_trait]
impl Checker for UncertaintyMarker {
    fn name(&self) -> &str {
        "uncertainty_marker"
    }

    async fn check(&self, input: &VerificationInput) -> CheckResult {
        let text = &input.generated_text;
        let sentences = FactualConsistencyChecker::split_sentences(text);

        let mut unmarked_assertions = Vec::new();
        let mut uncertain_count = 0;

        for sentence in &sentences {
            let has_signal = Self::UNCERTAINTY_SIGNALS.iter().any(|s| sentence.contains(s));
            if has_signal {
                uncertain_count += 1;
            }

            let is_assertion = Self::ASSERTION_WITHOUT_SOURCE.iter().any(|s| sentence.contains(s));
            if is_assertion && !Self::has_citation_marker(sentence) && !has_signal {
                unmarked_assertions.push(sentence.clone());
            }
        }

        let total_issues = unmarked_assertions.len() + uncertain_count;
        if total_issues == 0 {
            return CheckResult::pass("uncertainty_marker")
                .with_confidence(1.0)
                .with_evidence(vec!["未检测到不确定性内容".to_string()]);
        }

        let mut evidence = Vec::new();
        if !unmarked_assertions.is_empty() {
            evidence.push(format!("{} 个事实断言缺少来源引用标记", unmarked_assertions.len()));
            evidence.extend(unmarked_assertions.iter().take(3).map(|s| format!("  - {}", s)));
        }
        if uncertain_count > 0 {
            evidence.push(format!("{} 个句子使用不确定性措辞", uncertain_count));
        }

        CheckResult::fail("uncertainty_marker", format!("检测到 {} 个潜在可信度问题", total_issues))
            .with_confidence(1.0 - (total_issues as f32 / (sentences.len().max(1) as f32)))
            .with_evidence(evidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_well_sourced_response() {
        let checker = UncertaintyMarker;
        let input = VerificationInput {
            generated_text: "金蝶云星空支持多组织架构（来源：产品介绍.md）。这是一个集成解决方案。".to_string(),
            retrieved_chunks: vec![],
            chunk_titles: vec![],
            query: "test".to_string(),
            scenario: super::super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(result.passed, "有来源引用的回答应通过");
    }

    #[tokio::test]
    async fn test_unmarked_assertion() {
        let checker = UncertaintyMarker;
        let input = VerificationInput {
            generated_text: "金蝶云星空支持多组织架构。这是一个很好的产品。".to_string(),
            retrieved_chunks: vec![],
            chunk_titles: vec![],
            query: "test".to_string(),
            scenario: super::super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(!result.passed, "无来源的产品断言应被标记");
    }
}
