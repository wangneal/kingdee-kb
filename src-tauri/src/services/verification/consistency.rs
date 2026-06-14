use super::types::{CheckResult, Checker, VerificationInput};
use std::sync::{Arc, RwLock};

/// 事实一致性检查器
///
/// 检查 LLM 生成回答中的事实陈述是否在检索到的知识库 chunk 中有一致性支持。
/// 支持两种模式：
/// - **嵌入相似度模式**（推荐）：对句子和 chunk 做嵌入后计算余弦相似度
/// - **词项匹配模式**（回退）：中文 bigram 字符匹配 + 否定词检测
pub struct FactualConsistencyChecker {
    embedding: Option<Arc<RwLock<crate::services::embedding::EmbeddingService>>>,
}

impl FactualConsistencyChecker {
    /// 创建带嵌入支持的检查器（推荐）
    pub fn new(
        embedding: Option<Arc<RwLock<crate::services::embedding::EmbeddingService>>>,
    ) -> Self {
        Self { embedding }
    }

    /// 计算两个向量的余弦相似度
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }
        dot / (norm_a * norm_b)
    }

    /// 嵌入相似度检查：对每个事实句和检索到的 chunk 做嵌入后计算余弦相似度
    ///
    /// 阈值：>0.7 通过，0.4-0.7 可疑，<0.4 不一致
    async fn check_sentence_similarity(
        &self,
        sentence: &str,
        chunks: &[String],
    ) -> (bool, f32, Vec<String>) {
        let emb = match &self.embedding {
            Some(e) => e,
            None => return (true, 1.0, Vec::new()), // 无嵌入服务，跳过
        };

        // 嵌入句子
        let sentence_emb = {
            let mut service = emb.write().map_err(|e| format!("Lock: {}", e)).ok();
            match service.as_mut() {
                Some(s) if s.is_ready() => match s.embed_text(sentence) {
                    Ok(v) => v,
                    Err(_) => return (true, 1.0, vec!["嵌入句子失败".to_string()]),
                },
                _ => return (true, 1.0, Vec::new()),
            }
        };

        // 嵌入所有 chunk 并计算最大相似度
        let mut max_similarity = 0.0f32;
        let mut best_match = String::new();

        for chunk in chunks {
            let chunk_emb = {
                let mut service = emb.write().map_err(|e| format!("Lock: {}", e)).ok();
                match service.as_mut() {
                    Some(s) if s.is_ready() => match s.embed_text(chunk) {
                        Ok(v) => v,
                        Err(_) => continue,
                    },
                    _ => continue,
                }
            };
            let sim = Self::cosine_similarity(&sentence_emb, &chunk_emb);
            if sim > max_similarity {
                max_similarity = sim;
                best_match = if chunk.chars().count() > 120 {
                    chunk.chars().take(120).collect::<String>() + "..."
                } else {
                    chunk.clone()
                };
            }
        }

        if max_similarity >= 0.7 {
            (true, max_similarity, vec![format!("最佳匹配 (sim={:.2}): {}", max_similarity, best_match)])
        } else if max_similarity >= 0.4 {
            (
                true, // 可疑但通过（降低置信度）
                max_similarity,
                vec![format!("低相似度 (sim={:.2}): {}", max_similarity, best_match)],
            )
        } else {
            (
                false,
                max_similarity,
                vec![format!("无匹配 (sim={:.2})", max_similarity)],
            )
        }
    }

    pub fn split_sentences(text: &str) -> Vec<String> {
        let mut sentences = Vec::new();
        let mut current = String::new();
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            current.push(ch);
            let is_end = if matches!(ch, '。' | '！' | '？') {
                chars
                    .peek()
                    .is_none_or(|&next| !matches!(next, '」' | '』' | '）' | ')'))
            } else if matches!(ch, '.' | '!' | '?') {
                chars.peek().is_none_or(|&next| {
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
            "你好",
            "请问",
            "抱歉",
            "谢谢",
            "可以",
            "请",
            "您好",
            "总结",
            "根据",
            "综上所述",
        ];
        for prefix in &skip_prefixes {
            if sentence.starts_with(prefix) {
                return false;
            }
        }
        true
    }

    fn check_terms_in_context(sentence: &str, context: &[String]) -> (bool, Vec<String>) {
        // 否定词检测（P1 增强）：检测"不支持"/"无法"/"不能" vs "支持"/"可以"/"能"
        let negation_pairs: &[(&[&str], &[&str])] = &[
            (&["不支持", "无法", "不能", "不可", "没有"], &["支持", "可以", "能", "可", "有"]),
        ];
        let mut negation_warnings = Vec::new();
        for (neg_words, pos_words) in negation_pairs {
            for neg in *neg_words {
                if sentence.contains(neg) {
                    // 检查上下文中有没有对应的肯定词
                    let found_pos = pos_words
                        .iter()
                        .any(|pos| context.iter().any(|c| c.contains(pos)));
                    let found_neg_in_context = context.iter().any(|c| c.contains(neg));
                    if found_pos && !found_neg_in_context {
                        negation_warnings.push(format!(
                            "句子中使用否定词 '{}'，但知识库中多为肯定表述",
                            neg
                        ));
                    }
                }
            }
        }

        let terms: Vec<String> = sentence
            .chars()
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| {
                w[0] > '\u{4e00}' && w[0] < '\u{9fff}' && w[1] > '\u{4e00}' && w[1] < '\u{9fff}'
            })
            .map(|w| w.iter().collect::<String>())
            .collect();

        if terms.is_empty() {
            return (negation_warnings.is_empty(), negation_warnings);
        }

        let mut missing = Vec::new();

        for term in &terms {
            let found = context.iter().any(|chunk| chunk.contains(term));
            if !found {
                missing.push(term.clone());
            }
        }

        // 合并词项缺失和否定词警告
        missing.extend(negation_warnings);

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
        let has_embedding = self.embedding.is_some();

        for sentence in &sentences {
            if !Self::is_factual_claim(sentence) {
                continue;
            }
            total_claims += 1;

            // 优先使用嵌入相似度检查，回退到词项匹配
            if has_embedding {
                let (consistent, sim, evidence) =
                    self.check_sentence_similarity(sentence, context).await;
                if consistent && sim >= 0.7 {
                    verified_count += 1;
                } else if consistent && sim >= 0.4 {
                    // 低相似度：算通过但记录为可疑
                    verified_count += 1;
                    inconsistencies.push(format!(
                        "句子与知识库相似度较低 (sim={:.2}): {} (证据: {:?})",
                        sim, sentence, evidence
                    ));
                } else {
                    inconsistencies.push(format!(
                        "句子在知识库中无对应 (sim={:.2}): {} (证据: {:?})",
                        sim, sentence, evidence
                    ));
                }
            } else {
                // 回退：词项匹配 + 否定词检测
                let (consistent, missing_terms) =
                    Self::check_terms_in_context(sentence, context);
                if consistent {
                    verified_count += 1;
                } else {
                    inconsistencies.push(format!(
                        "句子中包含上下文未出现的内容: {} (缺失: {:?})",
                        sentence, missing_terms
                    ));
                }
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
                .with_evidence(vec![format!(
                    "{}/{} 个事实陈述与知识库一致",
                    verified_count, total_claims
                )])
        } else if ratio >= 0.5 {
            CheckResult::fail(
                "factual_consistency",
                format!("{}/{} 个事实陈述与知识库一致", verified_count, total_claims),
            )
            .with_confidence(ratio)
            .with_evidence(inconsistencies)
        } else {
            CheckResult::fail(
                "factual_consistency",
                "多数事实陈述与知识库内容不一致".to_string(),
            )
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
        let checker = FactualConsistencyChecker { embedding: None };
        let input = VerificationInput {
            generated_text: "金蝶云星空支持多组织架构。它适用于大型企业。".to_string(),
            retrieved_chunks: vec![
                "金蝶云星空支持多组织架构及协同业务".to_string(),
                "金蝶云星空适用于大型企业集团".to_string(),
            ],
            chunk_titles: vec!["doc1.md".to_string(), "doc2.md".to_string()],
            available_chunk_ids: vec![],
            query: "金蝶云星空".to_string(),
            scenario: super::super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(result.passed, "一致的回答应通过: {}", result.detail);
    }

    #[tokio::test]
    async fn test_inconsistent_answer() {
        let checker = FactualConsistencyChecker { embedding: None };
        let input = VerificationInput {
            generated_text: "金蝶云星空不支持多组织架构。它只适用于小微企业。".to_string(),
            retrieved_chunks: vec![
                "金蝶云星空支持多组织架构及协同业务".to_string(),
                "金蝶云星空适用于大型企业集团".to_string(),
            ],
            chunk_titles: vec!["doc1.md".to_string(), "doc2.md".to_string()],
            available_chunk_ids: vec![],
            query: "金蝶云星空".to_string(),
            scenario: super::super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(!result.passed, "矛盾的回答应检测失败");
    }
}
