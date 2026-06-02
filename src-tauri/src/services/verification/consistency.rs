use super::types::{CheckResult, Checker, VerificationInput};

pub struct FactualConsistencyChecker;

impl FactualConsistencyChecker {
    fn split_sentences(text: &str) -> Vec<String> {
        let mut sentences = Vec::new();
        let mut current = String::new();
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            current.push(ch);
            let is_end = if matches!(ch, '。' | '！' | '？') {
                chars.peek().map_or(true, |&next| {
                    !matches!(next, '」' | '』' | '）' | ')')
                })
            } else if matches!(ch, '.' | '!' | '?') {
                chars.peek().map_or(true, |&next| {
                    next.is_whitespace() || matches!(next, '"' | '」' | '』' | '）' | ')')
                })
            } else {
                false
            };
            if is_end {
                let s = current.trim().to_string();
                if !s.is_empty() && s.len() > 3 {
                    sentences.push(s);
                }
                current.clear();
            }
        }

        let remaining = current.trim().to_string();
        if !remaining.is_empty() && remaining.len() > 3 {
            sentences.push(remaining);
        }

        sentences
    }

    fn is_factual_claim(sentence: &str) -> bool {
        let skip_prefixes = [
            "你好", "请问", "抱歉", "谢谢", "可以", "请", "您好",
            "总结", "根据", "综上所述",
        ];
        for prefix in &skip_prefixes {
            if sentence.starts_with(prefix) {
                return false;
            }
        }
        true
    }

    fn check_terms_in_context(sentence: &str, context: &[String]) -> (bool, Vec<String>) {
        let terms: Vec<String> = sentence
            .chars()
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[0] > '\u{4e00}' && w[0] < '\u{9fff}' && w[1] > '\u{4e00}' && w[1] < '\u{9fff}')
            .map(|w| w.iter().collect::<String>())
            .collect();

        if terms.is_empty() {
            return (true, Vec::new());
        }

        let mut missing = Vec::new();

        for term in &terms {
            if term.len() < 4 { continue; }
            let found = context.iter().any(|chunk| chunk.contains(term));
            if !found {
                missing.push(term.clone());
            }
        }

        let threshold = (terms.len() as f32 * 0.6) as usize;
        (missing.len() <= threshold, missing)
    }
}

#[async_trait::async_trait]
impl Checker for FactualConsistencyChecker {
    fn name(&self) -> &str {
        "factual_consistency"
    }

    async fn check(&self, input: &VerificationInput) -> CheckResult {
        let text = &input.generated_text;
        let context = &input.retrieved_chunks;

        if context.is_empty() {
            return CheckResult::pass("factual_consistency")
                .with_confidence(0.5)
                .with_evidence(vec!["无知识库内容可进行比较".to_string()]);
        }

        let sentences = Self::split_sentences(text);
        let mut inconsistencies = Vec::new();
        let mut verified_count = 0;
        let mut total_claims = 0;

        for sentence in &sentences {
            if !Self::is_factual_claim(sentence) {
                continue;
            }
            total_claims += 1;

            let (consistent, missing_terms) = Self::check_terms_in_context(sentence, context);
            if consistent {
                verified_count += 1;
            } else {
                inconsistencies.push(format!("句子中包含上下文未出现的内容: {} (缺失: {:?})", sentence, missing_terms));
            }
        }

        if total_claims == 0 {
            return CheckResult::pass("factual_consistency")
                .with_confidence(1.0)
                .with_evidence(vec!["未检测到事实性陈述".to_string()]);
        }

        let ratio = verified_count as f32 / total_claims as f32;

        if ratio >= 0.8 {
            CheckResult::pass("factual_consistency")
                .with_confidence(ratio)
                .with_evidence(vec![format!("{}/{} 个事实陈述与知识库一致", verified_count, total_claims)])
        } else if ratio >= 0.5 {
            CheckResult::fail("factual_consistency", format!("{}/{} 个事实陈述与知识库一致", verified_count, total_claims))
                .with_confidence(ratio)
                .with_evidence(inconsistencies)
        } else {
            CheckResult::fail("factual_consistency", "多数事实陈述与知识库内容不一致".to_string())
                .with_confidence(ratio)
                .with_evidence(inconsistencies)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_sentences_chinese() {
        let text = "金蝶云星空支持多组织架构。K/3 WISE 适用于中小企业。这是结论。";
        let sentences = FactualConsistencyChecker::split_sentences(text);
        assert_eq!(sentences.len(), 3);
    }

    #[tokio::test]
    async fn test_consistent_answer() {
        let checker = FactualConsistencyChecker;
        let input = VerificationInput {
            generated_text: "金蝶云星空支持多组织架构。它适用于大型企业。".to_string(),
            retrieved_chunks: vec![
                "金蝶云星空支持多组织架构及协同业务".to_string(),
                "金蝶云星空适用于大型企业集团".to_string(),
            ],
            chunk_titles: vec!["doc1.md".to_string(), "doc2.md".to_string()],
            query: "金蝶云星空".to_string(),
            scenario: super::super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(result.passed, "一致的回答应通过: {}", result.detail);
    }

    #[tokio::test]
    async fn test_inconsistent_answer() {
        let checker = FactualConsistencyChecker;
        let input = VerificationInput {
            generated_text: "金蝶云星空不支持多组织架构。它只适用于小微企业。".to_string(),
            retrieved_chunks: vec![
                "金蝶云星空支持多组织架构及协同业务".to_string(),
                "金蝶云星空适用于大型企业集团".to_string(),
            ],
            chunk_titles: vec!["doc1.md".to_string(), "doc2.md".to_string()],
            query: "金蝶云星空".to_string(),
            scenario: super::super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(!result.passed, "矛盾的回答应检测失败");
    }
}
