use super::types::{
    Checker, VerificationInput, VerificationReport,
};

/// 验证策略配置
#[derive(Debug, Clone)]
pub struct VerificationConfig {
    pub enable_citation_check: bool,
    pub enable_consistency_check: bool,
    pub enable_contradiction_check: bool,
    pub enable_uncertainty_marker: bool,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            enable_citation_check: true,
            enable_consistency_check: true,
            enable_contradiction_check: true,
            enable_uncertainty_marker: true,
        }
    }
}

/// 验证管线 — 按序编排多个 Checker
pub struct VerificationPipeline {
    checkers: Vec<Box<dyn Checker>>,
    #[expect(dead_code)]
    config: VerificationConfig,
}

impl VerificationPipeline {
    pub fn new(config: VerificationConfig) -> Self {
        let mut checkers: Vec<Box<dyn Checker>> = Vec::new();

        if config.enable_citation_check {
            checkers.push(Box::new(super::citation::CitationExistenceChecker));
        }
        if config.enable_consistency_check {
            checkers.push(Box::new(super::consistency::FactualConsistencyChecker));
        }
        if config.enable_contradiction_check {
            checkers.push(Box::new(super::contradiction::SelfContradictionChecker));
        }
        if config.enable_uncertainty_marker {
            checkers.push(Box::new(super::uncertainty::UncertaintyMarker));
        }

        Self { checkers, config }
    }

    /// 默认配置（全部开启）
    pub fn default_with_all() -> Self {
        Self::new(VerificationConfig::default())
    }

    /// 仅开启引用校验（最轻量）
    pub fn citation_only() -> Self {
        Self::new(VerificationConfig {
            enable_citation_check: true,
            enable_consistency_check: false,
            enable_contradiction_check: false,
            enable_uncertainty_marker: false,
        })
    }

    /// 对输入执行完整验证管线
    pub async fn verify(&self, input: &VerificationInput) -> VerificationReport {
        let mut checks = Vec::new();

        for checker in &self.checkers {
            let result = checker.check(input).await;
            checks.push(result);
        }

        VerificationReport::from_checks(checks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::verification::types::ScenarioType;

    #[tokio::test]
    async fn test_pipeline_all_checkers() {
        let pipeline = VerificationPipeline::default_with_all();
        let input = VerificationInput {
            generated_text: "金蝶云星空支持多组织架构（来源：产品介绍.md）。K/3 WISE 适用于中小企业。".to_string(),
            retrieved_chunks: vec![
                "金蝶云星空支持多组织架构及协同业务".to_string(),
                "K/3 WISE 适用于中小企业".to_string(),
            ],
            chunk_titles: vec!["产品介绍.md".to_string(), "K3WISE概述.md".to_string()],
            query: "金蝶产品对比".to_string(),
            scenario: ScenarioType::Chat,
        };
        let report = pipeline.verify(&input).await;
        assert_eq!(report.checks.len(), 4, "应运行全部 4 个检查器");
    }

    #[tokio::test]
    async fn test_pipeline_citation_only() {
        let pipeline = VerificationPipeline::citation_only();
        let input = VerificationInput {
            generated_text: "test".to_string(),
            retrieved_chunks: vec![],
            chunk_titles: vec![],
            query: "test".to_string(),
            scenario: ScenarioType::Chat,
        };
        let report = pipeline.verify(&input).await;
        assert_eq!(report.checks.len(), 1, "仅运行引用检查器");
        assert_eq!(report.checks[0].check_name, "citation_existence");
    }
}
