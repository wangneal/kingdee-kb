use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VerificationLevel {
    Confirmed,
    NeedsReview,
    Suspected,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check_name: String,
    pub passed: bool,
    pub confidence: f32,
    pub detail: String,
    pub evidence: Vec<String>,
}

impl CheckResult {
    pub fn pass(name: impl Into<String>) -> Self {
        Self {
            check_name: name.into(),
            passed: true,
            confidence: 1.0,
            detail: String::new(),
            evidence: Vec::new(),
        }
    }

    pub fn fail(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            check_name: name.into(),
            passed: false,
            confidence: 0.0,
            detail: reason.into(),
            evidence: Vec::new(),
        }
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn with_evidence(mut self, evidence: Vec<String>) -> Self {
        self.evidence = evidence;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub level: VerificationLevel,
    pub checks: Vec<CheckResult>,
    pub overall_confidence: f32,
    pub corrected_output: Option<String>,
    pub suggested_labels: Vec<String>,
}

impl VerificationReport {
    pub fn confirmed() -> Self {
        Self {
            level: VerificationLevel::Confirmed,
            checks: Vec::new(),
            overall_confidence: 1.0,
            corrected_output: None,
            suggested_labels: Vec::new(),
        }
    }

    pub fn from_checks(checks: Vec<CheckResult>) -> Self {
        let total = checks.len() as f32;
        if total == 0.0 {
            return Self::confirmed();
        }

        let passed_count = checks.iter().filter(|c| c.passed).count() as f32;
        let avg_confidence: f32 = checks.iter().map(|c| c.confidence).sum::<f32>() / total;

        let level = if passed_count == total {
            if avg_confidence >= 0.8 {
                VerificationLevel::Confirmed
            } else {
                VerificationLevel::NeedsReview
            }
        } else if passed_count == 0.0 {
            VerificationLevel::Failed
        } else if passed_count >= total * 0.5 {
            VerificationLevel::NeedsReview
        } else {
            VerificationLevel::Suspected
        };

        let labels = Self::compute_labels(&checks);

        Self {
            level,
            overall_confidence: avg_confidence,
            checks,
            corrected_output: None,
            suggested_labels: labels,
        }
    }

    fn compute_labels(checks: &[CheckResult]) -> Vec<String> {
        let mut labels = Vec::new();
        for check in checks {
            if !check.passed && check.check_name == "citation_existence" {
                labels.push("部分引用未在知识库中找到，请核实数据来源".to_string());
            }
            if !check.passed && check.check_name == "factual_consistency" {
                labels.push("回答与知识库内容存在不一致".to_string());
            }
            if !check.passed && check.check_name == "self_contradiction" {
                labels.push("回答中存在前后矛盾".to_string());
            }
        }
        if checks.iter().any(|c| c.confidence < 0.5) {
            labels.push("部分内容可信度较低，建议核查".to_string());
        }
        labels
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScenarioType {
    Chat,
    SearchQA,
    DocGen,
    Research,
    RiskReport,
    KnowledgeCompilation,
}

#[async_trait::async_trait]
pub trait Checker: Send + Sync {
    fn name(&self) -> &str;
    async fn check(&self, input: &VerificationInput) -> CheckResult;
}

#[derive(Debug, Clone)]
pub struct VerificationInput {
    pub generated_text: String,
    pub retrieved_chunks: Vec<String>,
    pub chunk_titles: Vec<String>,
    pub query: String,
    pub scenario: ScenarioType,
}
