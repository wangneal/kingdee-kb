# 防幻觉验证层 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 subagent-driven-development（推荐）或 executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 在 KingdeeKB 中构建跨场景 LLM 输出验证层：引用校验、事实一致性检查、矛盾检测、不确定性标记。

**架构：** 独立的 `verification` 模块，通过 `VerificationPipeline` 编排 4 个 Checker。集成到 `llm_service::rag_query_rig` 之后，不影响已有逻辑。

**技术栈：** Rust + tokio async + serde

---

## 文件结构

| 文件 | 职责 |
|------|------|
| `src-tauri/src/services/verification/mod.rs` | 模块注册 |
| `src-tauri/src/services/verification/types.rs` | 核心类型（VerificationLevel, CheckResult, VerificationReport, VerificationInput） |
| `src-tauri/src/services/verification/citation.rs` | 引用存在性校验器 |
| `src-tauri/src/services/verification/consistency.rs` | 事实一致性检查器（策略 A：基于 chunk 的逐句验证） |
| `src-tauri/src/services/verification/contradiction.rs` | 内部矛盾检测器 |
| `src-tauri/src/services/verification/uncertainty.rs` | 不确定性标记器 |
| `src-tauri/src/services/verification/pipeline.rs` | VerificationPipeline 编排 |
| `src-tauri/src/services/mod.rs` | 新增 `pub mod verification;` |

---

### 任务 1：核心类型定义

**文件：**
- 创建：`src-tauri/src/services/verification/mod.rs`
- 创建：`src-tauri/src/services/verification/types.rs`
- 修改：`src-tauri/src/services/mod.rs`

- [ ] **步骤 1：创建 verification 目录和 mod.rs**

```rust
// src-tauri/src/services/verification/mod.rs
pub mod citation;
pub mod consistency;
pub mod contradiction;
pub mod pipeline;
pub mod types;
pub mod uncertainty;
```

- [ ] **步骤 2：在 types.rs 中定义核心枚举和结构体**

```rust
// src-tauri/src/services/verification/types.rs
use serde::{Deserialize, Serialize};

/// 验证结果等级
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VerificationLevel {
    Confirmed,
    NeedsReview,
    Suspected,
    Failed,
}

/// 单个验证检查项的结果
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

/// 完整验证报告
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

    /// 从检查结果列表计算最终等级和置信度
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

/// 验证场景类型
#[derive(Debug, Clone, PartialEq)]
pub enum ScenarioType {
    Chat,
    SearchQA,
    DocGen,
    Research,
    RiskReport,
    KnowledgeCompilation,
}

/// Checker trait — 所有验证器实现此接口
#[async_trait::async_trait]
pub trait Checker: Send + Sync {
    fn name(&self) -> &str;
    async fn check(&self, input: &VerificationInput) -> CheckResult;
}

/// 验证管线输入
#[derive(Debug, Clone)]
pub struct VerificationInput {
    pub generated_text: String,
    pub retrieved_chunks: Vec<String>,
    pub chunk_titles: Vec<String>,
    pub query: String,
    pub scenario: ScenarioType,
}
```

- [ ] **步骤 3：在 mod.rs 注册 verification 模块**

```rust
// src-tauri/src/services/mod.rs — 在文件顶部添加
pub mod verification;
```

---

### 任务 2：引用存在性校验器

**文件：**
- 创建：`src-tauri/src/services/verification/citation.rs`

- [ ] **步骤 1：实现 CitationExistenceChecker**

```rust
// src-tauri/src/services/verification/citation.rs
use regex::Regex;
use std::sync::LazyLock;

use super::types::{CheckResult, Checker, VerificationInput};

// 匹配 "[来源：xxx.md]" 或 "(来源：[xxx.md])" 格式
static RE_CITATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[来源：([^\]]+\.md)\]|\(来源：([^\)]+\.md)\)").unwrap()
});

// 匹配 "[src:N]" 格式
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

        // 收集所有引用的文档名
        let mut citations: Vec<String> = Vec::new();

        for cap in RE_CITATION.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                citations.push(m.as_str().to_string());
            } else if let Some(m) = cap.get(2) {
                citations.push(m.as_str().to_string());
            }
        }

        // 也匹配 [src:N] 短格式
        let src_count = RE_SRC_SHORT.find_iter(text).count();

        // 检查每个引用是否在检索结果中存在
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

    #[tokio::test]
    async fn test_all_citations_found() {
        let checker = CitationExistenceChecker;
        let input = VerificationInput {
            generated_text: "金蝶云星空支持多组织架构（来源：金蝶云星空介绍.md）".to_string(),
            retrieved_chunks: vec!["金蝶云星空支持多组织架构和协同业务".to_string()],
            chunk_titles: vec!["金蝶云星空介绍.md".to_string()],
            query: "金蝶云星空特性".to_string(),
            scenario: super::types::ScenarioType::Chat,
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
            scenario: super::types::ScenarioType::Chat,
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
            scenario: super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(result.passed, "无引用的问候语应通过验证");
        assert!(result.confidence < 1.0, "无引用的置信度应较低");
    }
}
```

- [ ] **步骤 2：运行测试验证通过**

运行：`cd src-tauri && cargo test citation::tests -- --nocapture`
预期：3 个测试全部 PASS

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/verification/
git add src-tauri/src/services/mod.rs
git commit -m "feat: 引用存在性校验器 + 验证层核心类型"
```

---

### 任务 3：事实一致性检查器

**文件：**
- 创建：`src-tauri/src/services/verification/consistency.rs`

- [ ] **步骤 1：实现 FactualConsistencyChecker（策略 A — 基于 chunk 的逐句验证）**

```rust
// src-tauri/src/services/verification/consistency.rs
use super::types::{CheckResult, Checker, VerificationInput};

pub struct FactualConsistencyChecker;

impl FactualConsistencyChecker {
    /// 将文本按句分割（中英文混合）
    fn split_sentences(text: &str) -> Vec<String> {
        let mut sentences = Vec::new();
        let mut current = String::new();
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            current.push(ch);
            // 句尾标点：。！？.!? 后面跟空格、引号或结束
            if matches!(ch, '。' | '！' | '？' | '.' | '!' | '?') {
                // 检查是否真正句尾（后面是空格、引号或文件结尾）
                let is_end = chars.peek().map_or(true, |&next| {
                    next.is_whitespace() || matches!(next, '"' | '」' | '』' | '）' | ')')
                });
                if is_end {
                    let s = current.trim().to_string();
                    if !s.is_empty() && s.len() > 3 {
                        sentences.push(s);
                    }
                    current.clear();
                }
            }
        }

        // 剩余文本
        let remaining = current.trim().to_string();
        if !remaining.is_empty() && remaining.len() > 3 {
            sentences.push(remaining);
        }

        sentences
    }

    /// 判断一个句子是否需要验证（包含事实性断言）
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

    /// 检查句子中的关键术语是否在上下文中出现
    fn check_terms_in_context(sentence: &str, context: &[String]) -> (bool, Vec<String>) {
        // 提取关键名词短语（简单策略：长度 >= 2 的连续中文字符串）
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

        // 检查术语是否在任一 chunk 中出现
        for term in &terms {
            if term.len() < 4 { continue; } // 跳过太短的二元组
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
            // 没有检索结果，不检查一致性
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
            scenario: super::types::ScenarioType::Chat,
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
            scenario: super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(!result.passed, "矛盾的回答应检测失败");
    }
}
```

- [ ] **步骤 2：运行测试验证通过**

运行：`cd src-tauri && cargo test consistency::tests -- --nocapture`
预期：3 个测试全部 PASS

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/verification/consistency.rs
git commit -m "feat: 事实一致性检查器（策略A — 基于术语的逐句验证）"
```

---

### 任务 4：内部矛盾检测器

**文件：**
- 创建：`src-tauri/src/services/verification/contradiction.rs`

- [ ] **步骤 1：实现 SelfContradictionChecker**

```rust
// src-tauri/src/services/verification/contradiction.rs
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
            scenario: super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        assert!(result.passed, "无矛盾的文本应通过");
    }
}
```

- [ ] **步骤 2：运行测试验证通过**

运行：`cd src-tauri && cargo test contradiction::tests -- --nocapture`
预期：3 个测试全部 PASS

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/verification/contradiction.rs
git commit -m "feat: 内部矛盾检测器（否定词 + 术语匹配）"
```

---

### 任务 5：不确定性标记器

**文件：**
- 创建：`src-tauri/src/services/verification/uncertainty.rs`

- [ ] **步骤 1：实现 UncertaintyMarker**

```rust
// src-tauri/src/services/verification/uncertainty.rs
use super::types::{CheckResult, Checker, VerificationInput};
use super::consistency::FactualConsistencyChecker;

pub struct UncertaintyMarker;

impl UncertaintyMarker {
    /// 需要补充来源的断言模式
    const ASSERTION_WITHOUT_SOURCE: &'static [&'static str] = &[
        "系统支持", "功能包括", "配置路径", "标准功能",
        "金蝶", "K/3", "星空", "苍穹",
    ];

    /// 不确定性信号词
    const UNCERTAINTY_SIGNALS: &'static [&'static str] = &[
        "可能", "应该", "一般来说", "通常", "据说",
        "maybe", "probably", "usually", "generally",
    ];

    /// 来源标记模式
    fn has_citation_marker(sentence: &str) -> bool {
        sentence.contains("[来源：") || sentence.contains("[src:")
            || sentence.contains("(来源：") || sentence.contains("(来源:")
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

            // 检查：包含产品名但没有引用标记的断言
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
            generated_text: "金蝶云星空支持多组织架构（来源：产品介绍.md）。系统支持多语言。".to_string(),
            retrieved_chunks: vec![],
            chunk_titles: vec![],
            query: "test".to_string(),
            scenario: super::types::ScenarioType::Chat,
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
            scenario: super::types::ScenarioType::Chat,
        };
        let result = checker.check(&input).await;
        // 是否通过取决于断言数量 vs 句子总数
        // 2 个句子，1 个断言无引用 → 置信度 = 1 - 1/2 = 0.5 < 0.8 → failed
        assert!(!result.passed, "无来源的产品断言应被标记");
    }
}
```

- [ ] **步骤 2：运行测试验证通过**

运行：`cd src-tauri && cargo test uncertainty::tests -- --nocapture`
预期：2 个测试全部 PASS

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/verification/uncertainty.rs
git commit -m "feat: 不确定性标记器（检测无来源断言 + 模糊措辞）"
```

---

### 任务 6：验证管线编排

**文件：**
- 创建：`src-tauri/src/services/verification/pipeline.rs`

- [ ] **步骤 1：实现 VerificationPipeline**

```rust
// src-tauri/src/services/verification/pipeline.rs
use super::types::{
    CheckResult, Checker, VerificationInput, VerificationLevel, VerificationReport,
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
```

- [ ] **步骤 2：运行测试验证通过**

运行：`cd src-tauri && cargo test pipeline::tests -- --nocapture`
预期：2 个测试全部 PASS

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/verification/pipeline.rs
git commit -m "feat: 验证管线编排（VerificationPipeline + VerificationConfig）"
```

---

### 任务 7：集成到 Chat/Agent 场景

**文件：**
- 修改：`src-tauri/src/services/llm_service.rs`

- [ ] **步骤 1：在 llm_service.rs 的 rag_query_rig 返回后插入验证**

```rust
// 在 src-tauri/src/services/llm_service.rs 中

// 文件顶部新增 import
use crate::services::verification::pipeline::VerificationPipeline;
use crate::services::verification::types::{
    ScenarioType, VerificationInput, VerificationReport,
};

// 在 LLMService struct 定义中新增字段
pub struct LLMService {
    providers: Arc<Mutex<LLMProviderManager>>,
    client: reqwest::Client,
    desensitizer: Option<Arc<crate::services::desensitize::Desensitizer>>,
    /// 可选的验证管线
    verifier: Option<VerificationPipeline>,
}

// 在 new() 和 with_desensitizer() 中设置默认值
impl LLMService {
    pub fn new(providers: Arc<Mutex<LLMProviderManager>>) -> Self {
        Self {
            providers,
            client: reqwest::Client::new(),
            desensitizer: None,
            verifier: Some(VerificationPipeline::default_with_all()), // 默认启用
        }
    }

    pub fn with_desensitizer(
        providers: Arc<Mutex<LLMProviderManager>>,
        desensitizer: Arc<crate::services::desensitize::Desensitizer>,
    ) -> Self {
        Self {
            providers,
            client: reqwest::Client::new(),
            desensitizer: Some(desensitizer),
            verifier: Some(VerificationPipeline::default_with_all()),
        }
    }

    /// 获取验证结果（公开给调用者）
    pub fn last_verification_report(&self) -> Option<VerificationReport> {
        None // 此方法需要改为通过状态共享实现，见下一步
    }
}

// 新增方法：运行 RAG 查询并带验证
impl LLMService {
    /// 执行 RAG 查询 + 验证
    pub async fn verified_rag_query(
        &self,
        config: &LLMProviderConfig,
        system_prompt: &str,
        messages: &[ChatMessage],
        context_chunks: &[crate::services::hybrid_search::HybridSearchResult],
    ) -> Result<(Vec<StreamChunk>, Option<VerificationReport>), String> {
        // 1. 先执行原始 RAG 查询
        let chunks = self.rag_query_rig(config, system_prompt, messages).await?;

        // 2. 如果有验证器，执行验证
        let report = if let Some(ref verifier) = self.verifier {
            // 将完整回答拼起来
            let full_text: String = chunks.iter().map(|c| c.content.as_str()).collect();

            let input = VerificationInput {
                generated_text: full_text,
                retrieved_chunks: context_chunks.iter().map(|c| c.content.clone()).collect(),
                chunk_titles: context_chunks.iter().map(|c| c.title.clone()).collect(),
                query: messages.last().map(|m| m.content.clone()).unwrap_or_default(),
                scenario: ScenarioType::Chat,
            };

            let report = verifier.verify(&input).await;
            Some(report)
        } else {
            None
        };

        Ok((chunks, report))
    }
}
```

- [ ] **步骤 2：验证编译通过**

运行：`cd src-tauri && cargo check`
预期：0 errors, 0 warnings

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/llm_service.rs
git commit -m "feat: 集成验证管线到 RAG Chat 流程（verified_rag_query）"
```

---

### 任务 8：全量测试与清理

**文件：** 无新增

- [ ] **步骤 1：运行所有 verification 模块测试**

运行：`cd src-tauri && cargo test verification -- --nocapture`
预期：全部 PASS

- [ ] **步骤 2：运行 cargo check 确保没有问题**

运行：`cd src-tauri && cargo check 2>&1`
预期：无错误

- [ ] **步骤 3：运行 cargo clippy**

运行：`cd src-tauri && cargo clippy -- -D warnings 2>&1`
预期：无警告

- [ ] **步骤 4：Commit**

```bash
git add -A
git commit -m "chore: 验证层全量测试通过 + clippy 清理"
```
