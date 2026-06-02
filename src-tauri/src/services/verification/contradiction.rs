use super::types::{CheckResult, Checker, VerificationInput};
use super::consistency::FactualConsistencyChecker;

pub struct SelfContradictionChecker;

impl SelfContradictionChecker {
    /// 检测两句话是否矛盾（基于否定词 + 关键术语匹配）
    fn are_contradictory(s1: &str, s2: &str) -> bool {
        // 提取两句话中的关键名词短语
        let terms1 = Self::extract_terms(s1);
        let terms2 = Self::extract_terms(s2);

        // 如果没有共同主题，不算矛盾
        let common: Vec<&String> = terms1.iter().filter(|t| terms2.contains(*t)).collect();
        if common.is_empty() {
            return false;
        }

        // 检查一方是否否定另一方
        let s1_neg = Self::has_negation(s1);
        let s2_neg = Self::has_negation(s2);

        // 如果共同主题被否定，且另一方是肯定 → 矛盾
        // "支持多组织" vs "不支持多组织"
        if s1_neg != s2_neg {
            return true;
        }

        false
    }

    fn extract_terms(s: &str) -> Vec<String> {
        s.chars()
            .collect::<Vec<_>>()
            .windows(3)
            .filter(|w| w.iter().all(|c| *c > '\u{4e00}' && *c < '\u{9fff}'))
            .map(|w| w.iter().collect::<String>())
            .filter(|t| t.len() >= 3)
            .collect()
    }

    fn has_negation(s: &str) -> bool {
        s.contains("不") || s.contains("没有") || s.contains("无法")
            || s.contains("不支持") || s.contains("不能")
    }
}

#[async_trait::async_trait]
impl Checker for SelfContradictionChecker {
    fn name(&self) -> &str {
        "self_contradiction"
    }

    async fn check(&self, input: &VerificationInput) -> CheckResult {
        let text = &input.generated_text;
        let sentences = FactualConsistencyChecker::split_sentences(text);

        let mut contradictions = Vec::new();

        for i in 0..sentences.len() {
            for j in i + 1..sentences.len() {
                if Self::are_contradictory(&sentences[i], &sentences[j]) {
                    contradictions.push(format!(
                        "句子 {} 和 {} 可能矛盾: 「{}」vs 「{}」",
                        i + 1, j + 1, sentences[i], sentences[j]
                    ));
                }
            }
        }

        if contradictions.is_empty() {
            CheckResult::pass("self_contradiction")
                .with_confidence(1.0)
                .with_evidence(vec![format!("{} 个句子间未检测到矛盾", sentences.len())])
        } else {
            CheckResult::fail("self_contradiction", format!("检测到 {} 处潜在矛盾", contradictions.len()))
                .with_confidence(0.0)
                .with_evidence(contradictions)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_contradiction() {
        let result = SelfContradictionChecker::are_contradictory(
            "金蝶云星空支持多组织架构",
            "金蝶云星空不支持多组织架构",
        );
        assert!(result, "肯定 vs 否定应检测为矛盾");
    }

    #[test]
    fn test_no_false_positive() {
        let result = SelfContradictionChecker::are_contradictory(
            "金蝶云星空支持多组织架构",
            "K/3 WISE 适用于中小企业",
        );
        assert!(!result, "不同主题不应检测为矛盾");
    }

    #[tokio::test]
    async fn test_no_contradiction_in_good_answer() {
        let checker = SelfContradictionChecker;
        let input = VerificationInput {
            generated_text: "金蝶云星空支持多组织架构。它适用于大型企业。系统支持多语言。".to_string(),
            retrieved_chunks: vec![],
            chunk_titles: vec![],
            query: "test".to_string(),
            scenario: super::super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(result.passed, "无矛盾的文本应通过");
    }
}
