//! 自一致性验证（策略 B — LLM 判官模式）
//!
//! 将生成的文本 + 检索源文档发给 LLM，让其判断是否有编造或矛盾。
//! 仅在文档生成等一次性重要产物场景启用。

use super::types::{CheckResult, Checker, VerificationInput};
use crate::services::llm_providers::LLMProviderConfig;
use crate::services::llm_service::LLMService;
use std::sync::Arc;

pub struct SelfConsistencyChecker {
    llm: Arc<LLMService>,
}

impl SelfConsistencyChecker {
    pub fn new(llm: Arc<LLMService>) -> Self {
        Self { llm }
    }
}

#[async_trait::async_trait]
impl Checker for SelfConsistencyChecker {
    fn name(&self) -> &str {
        "self_consistency"
    }

    async fn check(&self, input: &VerificationInput) -> CheckResult {
        let text = &input.generated_text;

        // 没有检索源且非知识编译场景时跳过
        if input.retrieved_chunks.is_empty() && input.scenario != super::types::ScenarioType::KnowledgeCompilation {
            return CheckResult::pass("self_consistency")
                .with_confidence(0.5)
                .with_evidence(vec!["无知识库内容可进行一致性验证".to_string()]);
        }

        // 构建验证 prompt
        let context = input.retrieved_chunks.join("\n---\n");
        let prompt = format!(
            "你是一个事实核查助手。以下是一段 AI 生成的回答和它参考的知识库内容。\n\
            请仔细检查回答中的每个事实性陈述是否符合知识库内容。\n\
            如果发现任何编造、矛盾或知识库中不存在的断言，请逐条列出。\n\
            如果没有问题，请仅回复「验证通过」。\n\n\
            【知识库内容】\n{}\n\n\
            【AI 回答】\n{}",
            context, text
        );

        let config = match self.llm.get_active_config() {
            Ok(c) => c,
            Err(_) => {
                return CheckResult::pass("self_consistency")
                    .with_confidence(0.5)
                    .with_evidence(vec!["无法获取 LLM 配置，跳过验证".to_string()]);
            }
        };

        let messages = vec![
            crate::services::llm_service::ChatMessage {
                role: "system".to_string(),
                content: "你是一个严谨的事实核查助手。只基于事实回答，不推测。".to_string(),
            },
            crate::services::llm_service::ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];

        match self.llm.chat_completion(&messages, &config).await {
            Ok(response) => {
                let trimmed = response.trim();
                if trimmed.contains("验证通过") || trimmed.contains("没有发现") || trimmed.contains("无矛盾") {
                    CheckResult::pass("self_consistency")
                        .with_confidence(0.9)
                        .with_evidence(vec![trimmed.to_string()])
                } else {
                    let issues: Vec<String> = trimmed.lines().map(|l| l.to_string()).collect();
                    CheckResult::fail("self_consistency", "LLM 判官发现潜在问题".to_string())
                        .with_confidence(0.3)
                        .with_evidence(issues)
                }
            }
            Err(e) => {
                CheckResult::fail("self_consistency", "LLM 验证调用失败，验证未完成".to_string())
                    .with_confidence(0.0)
                    .with_evidence(vec![format!("LLM 调用失败: {}", e)])
            }
        }
    }
}
