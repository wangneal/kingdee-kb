//! Planner + Replanner + PlanStateMachine
//!
//! NDJSON 流式规划 + 状态机步进控制 + 依赖合法性校验。
//! 配合 agent_router 的模式路由，为复杂任务提供 Plan-Execute 执行路径。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ─── 数据结构 ────────────────────────────────────────────

/// 执行计划
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub steps: Vec<PlanStep>,
    pub estimated_tokens: u32,
}

/// 计划步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: u32,
    pub description: String,
    pub tool: Option<String>,
    pub expected_output: String,
    pub depends_on: Vec<u32>,
}

/// 已执行步骤记录
#[derive(Debug, Clone)]
pub struct ExecutedStep {
    pub step: PlanStep,
    pub result: String,
}

/// 步骤执行状态（LLM 结构化输出）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StepStatus {
    Success,
    Failed,
    Blocked,
    Partial(String),
}

/// 计划状态机的状态
#[derive(Debug, Clone, PartialEq)]
pub enum PlanState {
    /// 等待执行下一步
    Ready,
    /// 当前步骤执行中
    Executing,
    /// 当前步骤完成
    StepDone,
    /// 需要重新规划
    NeedsReplan,
    /// 计划全部完成
    Completed,
    /// 计划失败
    Failed(String),
}

/// 单步执行的上下文（注入到 LLM 的 system prompt）
#[derive(Debug, Clone)]
pub struct StepContext {
    pub original_task: String,
    pub current_step: Option<PlanStep>,
    pub executed_summary: String,
    pub progress: String,
    pub remaining_count: usize,
}

/// 漂移警告
#[derive(Debug, Clone)]
pub struct DriftWarning {
    pub signal: String,
    pub step: String,
    pub suggestion: String,
}

// ─── Planner ─────────────────────────────────────────────

pub struct Planner;

impl Planner {
    /// 生成执行计划（使用 LLM）
    ///
    /// 向 LLM 发送规划 prompt，解析返回的 JSON/NDJSON 格式计划。
    /// 当 LLM 不可用时，回退到基于关键词的启发式规划。
    pub async fn plan(
        task: &str,
        skill_catalog: &str,
        context: &str,
        llm: &crate::services::llm_service::LLMService,
        _plan_budget: u32,
    ) -> Result<ExecutionPlan, String> {
        let prompt = format!(
            "你是一个任务规划专家。请为以下任务生成执行计划。\n\n\
             任务：{task}\n\n\
             可用技能：{skill_catalog}\n\n\
             项目上下文：{context}\n\n\
             要求：\n\
             1. 分解为 3-8 个可执行步骤\n\
             2. 每步明确工具（tool 字段）和预期输出\n\
             3. 标注步骤间依赖关系（depends_on）\n\
             4. 以 NDJSON 格式输出，每个步骤独占一行 JSON\n\
             5. 每行格式：{{\"id\":N,\"description\":\"...\",\"tool\":\"...\",\"expected_output\":\"...\",\"depends_on\":[]}}"
        );

        let messages = vec![
            crate::services::llm_service::ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];
        let config = llm
            .get_active_config()
            .map_err(|e| format!("Planner config error: {}", e))?;
        let response = llm
            .chat_completion(&messages, &config)
            .await
            .map_err(|e| format!("Planner LLM 调用失败: {}", e))?;

        Self::parse_plan(&response)
    }

    /// 解析 LLM 返回的计划（支持 JSON 数组和 NDJSON）
    pub fn parse_plan(response: &str) -> Result<ExecutionPlan, String> {
        let trimmed = response.trim();

        // 尝试 JSON 数组格式
        if let Ok(steps) = serde_json::from_str::<Vec<PlanStep>>(trimmed) {
            if !steps.is_empty() {
                let estimated_tokens = crate::services::token::count_tokens_with_fallback(trimmed);
                return Ok(ExecutionPlan { steps, estimated_tokens });
            }
        }

        // 尝试 NDJSON（逐行解析）
        let mut steps = Vec::new();
        for line in trimmed.lines() {
            let line = line.trim();
            if line.is_empty() || !line.starts_with('{') {
                continue;
            }
            if let Ok(step) = serde_json::from_str::<PlanStep>(line) {
                steps.push(step);
            }
        }

        if !steps.is_empty() {
            let estimated_tokens = crate::services::token::count_tokens_with_fallback(trimmed);
            Ok(ExecutionPlan { steps, estimated_tokens })
        } else {
            // 回退：启发式单步计划
            tracing::warn!("Planner 无法解析 LLM 输出，回退到启发式计划");
            Ok(ExecutionPlan {
                steps: vec![PlanStep {
                    id: 1,
                    description: trimmed.to_string(),
                    tool: None,
                    expected_output: "任务完成".to_string(),
                    depends_on: vec![],
                }],
                estimated_tokens: crate::services::token::count_tokens_with_fallback(trimmed),
            })
        }
    }

    /// 检测是否需要重新规划
    pub fn should_replan(
        _plan: &ExecutionPlan,
        _step_idx: usize,
        step_result: &str,
        _expected: &str,
        step_status: Option<&StepStatus>,
    ) -> bool {
        // 优先检查结构化状态
        if let Some(status) = step_status {
            return matches!(status, StepStatus::Failed | StepStatus::Blocked);
        }

        // 中英文关键词检测
        let failure_signals = [
            "失败", "错误", "不支持", "无法", "遇到问题", "未能完成",
            "failed", "error", "not supported", "unable", "cannot", "blocked", "exception",
        ];
        let result_lower = step_result.to_lowercase();
        for signal in &failure_signals {
            if result_lower.contains(&signal.to_lowercase()) {
                return true;
            }
        }
        false
    }

    /// 重新规划剩余步骤
    pub async fn replan(
        original_task: &str,
        executed_steps: &[ExecutedStep],
        old_remaining: &[PlanStep],
        llm: &crate::services::llm_service::LLMService,
    ) -> Result<Vec<PlanStep>, String> {
        let history = executed_steps
            .iter()
            .map(|s| {
                let preview = if s.result.len() > 200 {
                    format!("{}...", &s.result[..200])
                } else {
                    s.result.clone()
                };
                format!("步骤{}（{}）：{}", s.step.id, s.step.description, preview)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let remaining = old_remaining
            .iter()
            .map(|s| format!("{}. {}", s.id, s.description))
            .collect::<Vec<_>>()
            .join("\n");

        let replan_messages = vec![
            crate::services::llm_service::ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "基于执行偏差，重新规划剩余任务。\n\n\
                     原始任务：{original_task}\n\n\
                     已执行步骤：\n{history}\n\n\
                     原计划的剩余步骤（可能已不适用）：\n{remaining}\n\n\
                     请以 NDJSON 格式生成新的剩余步骤。"
                ),
            },
        ];
        let replan_config = llm
            .get_active_config()
            .map_err(|e| format!("Replanner config error: {}", e))?;
        let response = llm
            .chat_completion(&replan_messages, &replan_config)
            .await
            .map_err(|e| format!("Replanner LLM 调用失败: {}", e))?;

        let plan = Self::parse_plan(&response)?;
        Ok(plan.steps)
    }

    /// 检测 LLM 输出是否偏离当前步骤
    pub fn detect_step_drift(
        step_description: &str,
        step_tool: Option<&str>,
        llm_output: &str,
        actual_tool_calls: &[String], // 简化：工具名列表
    ) -> Option<DriftWarning> {
        // 第一层：文本信号检测
        let drift_signals = [
            "接下来我将",
            "同时我也会",
            "综合以上所有步骤",
            "跳过",
            "我决定改为",
            "Next I will",
            "Furthermore",
            "In summary",
            "skip",
            "I decide to",
            "instead",
            "let me also",
        ];

        for signal in &drift_signals {
            if llm_output.contains(signal) {
                return Some(DriftWarning {
                    signal: signal.to_string(),
                    step: step_description.to_string(),
                    suggestion: "检测到计划漂移信号（文本），已拦截。请只执行当前步骤。".into(),
                });
            }
        }

        // 第二层：结构化 ToolCall 匹配
        if let Some(expected_tool) = step_tool {
            for call_name in actual_tool_calls {
                if call_name != expected_tool {
                    return Some(DriftWarning {
                        signal: format!(
                            "工具不匹配：计划要求 '{}'，实际调用 '{}'",
                            expected_tool, call_name
                        ),
                        step: step_description.to_string(),
                        suggestion: format!(
                            "检测到计划漂移（结构）：请使用 '{}' 工具执行当前步骤。",
                            expected_tool
                        ),
                    });
                }
            }
        }

        // 第三层：纯推理步骤不应有工具调用
        if step_tool.is_none() && !actual_tool_calls.is_empty() {
            return Some(DriftWarning {
                signal: format!(
                    "当前步骤为纯推理，但调用了工具：{}",
                    actual_tool_calls.join(", ")
                ),
                step: step_description.to_string(),
                suggestion: "当前步骤不需要工具调用，请仅用推理完成。".into(),
            });
        }

        None
    }
}

// ─── PlanStateMachine ─────────────────────────────────────

/// 计划状态机 — 控制步骤执行，防止计划漂移
pub struct PlanStateMachine {
    plan: ExecutionPlan,
    current_index: usize,
    executed: Vec<ExecutedStep>,
    replan_count: u32,
    max_replans: u32,
    state: PlanState,
}

impl PlanStateMachine {
    pub fn new(plan: ExecutionPlan) -> Self {
        if plan.steps.is_empty() {
            return Self {
                plan,
                current_index: 0,
                executed: Vec::new(),
                replan_count: 0,
                max_replans: 3,
                state: PlanState::Failed("空计划".into()),
            };
        }
        Self {
            plan,
            current_index: 0,
            executed: Vec::new(),
            replan_count: 0,
            max_replans: 3,
            state: PlanState::Ready,
        }
    }

    pub fn state(&self) -> &PlanState {
        &self.state
    }

    pub fn current_step(&self) -> Option<&PlanStep> {
        self.plan.steps.get(self.current_index)
    }

    pub fn current_step_index(&self) -> usize {
        self.current_index
    }

    pub fn plan(&self) -> &ExecutionPlan {
        &self.plan
    }

    pub fn executed(&self) -> &[ExecutedStep] {
        &self.executed
    }

    pub fn replan_count(&self) -> u32 {
        self.replan_count
    }

    /// 设置当前状态为 Executing
    pub fn begin_step(&mut self) {
        self.state = PlanState::Executing;
    }

    /// 记录当前步骤执行结果
    pub fn record_result(&mut self, result: String) -> PlanState {
        if let Some(step) = self.current_step() {
            self.executed.push(ExecutedStep {
                step: step.clone(),
                result,
            });
        }
        self.state = PlanState::StepDone;
        self.state.clone()
    }

    /// 推进到下一步
    pub fn advance(&mut self) -> PlanState {
        self.current_index += 1;
        if self.current_index >= self.plan.steps.len() {
            self.state = PlanState::Completed;
        } else {
            self.state = PlanState::Ready;
        }
        self.state.clone()
    }

    /// 请求重新规划
    pub fn request_replan(&mut self, mut new_steps: Vec<PlanStep>) -> PlanState {
        self.replan_count += 1;
        if self.replan_count > self.max_replans {
            self.state = PlanState::Failed("重规划次数超过上限".into());
            return self.state.clone();
        }
        self.plan.steps.truncate(self.current_index);
        Self::validate_dependencies(&self.plan.steps, &mut new_steps);
        self.plan.steps.extend(new_steps);
        self.state = PlanState::Ready;
        self.state.clone()
    }

    /// 校验依赖合法性，清理无效依赖引用
    fn validate_dependencies(existing_steps: &[PlanStep], new_steps: &mut [PlanStep]) {
        let valid_ids: HashSet<u32> = existing_steps
            .iter()
            .chain(new_steps.iter())
            .map(|s| s.id)
            .collect();

        for step in new_steps {
            let original_count = step.depends_on.len();
            step.depends_on.retain(|dep_id| valid_ids.contains(dep_id));
            if step.depends_on.len() < original_count {
                tracing::warn!(
                    "步骤 {} 的依赖被清理：原始 {} 个，有效 {} 个",
                    step.id,
                    original_count,
                    step.depends_on.len()
                );
            }
        }
    }

    /// 为当前步骤构建最小上下文
    pub fn build_step_context(&self, original_task: &str) -> StepContext {
        StepContext {
            original_task: original_task.to_string(),
            current_step: self.current_step().cloned(),
            executed_summary: Self::summarize_executed(&self.executed),
            progress: format!(
                "{}/{}",
                self.current_index,
                self.plan.steps.len()
            ),
            remaining_count: self.plan.steps.len() - self.current_index - 1,
        }
    }

    fn summarize_executed(executed: &[ExecutedStep]) -> String {
        if executed.is_empty() {
            return "（无已执行步骤）".into();
        }
        executed
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let result_preview = if s.result.len() > 150 {
                    // 找到安全的 UTF-8 边界
                    let mut end = 150;
                    while end > 0 && !s.result.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &s.result[..end])
                } else {
                    s.result.clone()
                };
                format!("步骤{}({}): {}", i + 1, s.step.description, result_preview)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 获取剩余步骤
    pub fn remaining_steps(&self) -> &[PlanStep] {
        if self.current_index < self.plan.steps.len() {
            &self.plan.steps[self.current_index..]
        } else {
            &[]
        }
    }
}

// ─── StepContext 渲染 ─────────────────────────────────────

impl StepContext {
    /// 渲染为 system prompt 片段
    pub fn to_prompt(&self) -> String {
        let current_step_str = self
            .current_step
            .as_ref()
            .map(|s| format!("{}: {}", s.id, s.description))
            .unwrap_or_else(|| "无".into());

        let remaining_str = if self.remaining_count == 0 {
            "（这是最后一步）".into()
        } else {
            format!("剩余 {} 步", self.remaining_count)
        };

        format!(
            "## 任务规划执行\n\n\
             **原始任务**: {original_task}\n\n\
             **进度**: {progress}\n\n\
             **已执行步骤摘要**:\n{executed_summary}\n\n\
             **当前步骤**: {current_step}\n\n\
             **后续步骤预览**: {remaining}\n\n\
             ⚠️ 严格约束：\n\
             1. 你只执行「当前步骤」描述的任务，不要跳步或合并步骤\n\
             2. 不要自行修改计划——如果当前步骤无法执行，报告障碍即可\n\
             3. 不要执行后续步骤的内容\n\
             4. 完成当前步骤后，输出结果即可，不要继续",
            original_task = self.original_task,
            progress = self.progress,
            executed_summary = self.executed_summary,
            current_step = current_step_str,
            remaining = remaining_str,
        )
    }
}

// ─── 单元测试 ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plan_ndjson() {
        let ndjson = r#"{"id":1,"description":"搜索知识库","tool":"search-knowledge","expected_output":"文档列表","depends_on":[]}
{"id":2,"description":"分析结果","tool":null,"expected_output":"分析报告","depends_on":[1]}"#;
        let plan = Planner::parse_plan(ndjson).unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].id, 1);
        assert_eq!(plan.steps[1].depends_on, vec![1]);
    }

    #[test]
    fn test_state_machine_advance() {
        let plan = ExecutionPlan {
            steps: vec![
                PlanStep { id: 1, description: "Step 1".into(), tool: None, expected_output: "ok".into(), depends_on: vec![] },
                PlanStep { id: 2, description: "Step 2".into(), tool: None, expected_output: "ok".into(), depends_on: vec![1] },
            ],
            estimated_tokens: 100,
        };
        let mut sm = PlanStateMachine::new(plan);
        assert_eq!(*sm.state(), PlanState::Ready);
        assert_eq!(sm.current_step().unwrap().id, 1);

        sm.begin_step();
        assert_eq!(*sm.state(), PlanState::Executing);

        sm.record_result("done".into());
        assert_eq!(*sm.state(), PlanState::StepDone);

        sm.advance();
        assert_eq!(*sm.state(), PlanState::Ready);
        assert_eq!(sm.current_step().unwrap().id, 2);

        sm.begin_step();
        sm.record_result("done 2".into());
        sm.advance();
        assert_eq!(*sm.state(), PlanState::Completed);
    }

    #[test]
    fn test_state_machine_replan() {
        let plan = ExecutionPlan {
            steps: vec![
                PlanStep { id: 1, description: "Step 1".into(), tool: None, expected_output: "ok".into(), depends_on: vec![] },
                PlanStep { id: 2, description: "Step 2".into(), tool: None, expected_output: "ok".into(), depends_on: vec![1] },
            ],
            estimated_tokens: 100,
        };
        let mut sm = PlanStateMachine::new(plan);
        sm.record_result("done".into());
        sm.advance();

        let new_steps = vec![
            PlanStep { id: 3, description: "New step".into(), tool: None, expected_output: "ok".into(), depends_on: vec![1] },
        ];
        let state = sm.request_replan(new_steps);
        assert_eq!(state, PlanState::Ready);
        assert_eq!(sm.replan_count(), 1);
        assert_eq!(sm.current_step().unwrap().id, 3);
    }

    #[test]
    fn test_should_replan_on_failure() {
        assert!(Planner::should_replan(
            &ExecutionPlan { steps: vec![], estimated_tokens: 0 },
            0,
            "执行失败：连接超时",
            "成功",
            None,
        ));
    }

    #[test]
    fn test_should_not_replan_on_success() {
        assert!(!Planner::should_replan(
            &ExecutionPlan { steps: vec![], estimated_tokens: 0 },
            0,
            "查询成功，返回 5 条结果",
            "返回结果列表",
            None,
        ));
    }

    #[test]
    fn test_detect_drift_text_signal() {
        let warning = Planner::detect_step_drift(
            "搜索知识库",
            Some("search-knowledge"),
            "接下来我将直接生成文档",
            &[],
        );
        assert!(warning.is_some());
        assert!(warning.unwrap().signal.contains("接下来"));
    }

    #[test]
    fn test_detect_drift_tool_mismatch() {
        let warning = Planner::detect_step_drift(
            "搜索知识库",
            Some("search-knowledge"),
            "好的，我来搜索",
            &["generate-doc".to_string()],
        );
        assert!(warning.is_some());
    }

    #[test]
    fn test_no_drift() {
        let warning = Planner::detect_step_drift(
            "分析结果",
            None,
            "基于搜索结果，分析如下...",
            &[],
        );
        assert!(warning.is_none());
    }

    #[test]
    fn test_step_context_to_prompt() {
        let ctx = StepContext {
            original_task: "测试任务".into(),
            current_step: Some(PlanStep { id: 1, description: "Step 1".into(), tool: None, expected_output: "ok".into(), depends_on: vec![] }),
            executed_summary: "（无已执行步骤）".into(),
            progress: "0/3".into(),
            remaining_count: 2,
        };
        let prompt = ctx.to_prompt();
        assert!(prompt.contains("测试任务"));
        assert!(prompt.contains("剩余 2 步"));
        assert!(prompt.contains("严格约束"));
    }

    #[test]
    fn test_validate_dependencies_cleans_invalid() {
        let existing = vec![PlanStep { id: 1, description: "A".into(), tool: None, expected_output: "ok".into(), depends_on: vec![] }];
        let mut new_steps = vec![
            PlanStep { id: 2, description: "B".into(), tool: None, expected_output: "ok".into(), depends_on: vec![1, 99] },
        ];
        PlanStateMachine::validate_dependencies(&existing, &mut new_steps);
        assert_eq!(new_steps[0].depends_on, vec![1]); // 99 被清理
    }

    #[test]
    fn test_empty_plan_fails() {
        let plan = ExecutionPlan { steps: vec![], estimated_tokens: 0 };
        let sm = PlanStateMachine::new(plan);
        assert!(matches!(sm.state(), PlanState::Failed(_)));
    }
}
