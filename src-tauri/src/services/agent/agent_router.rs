//! Agent 模式路由 — 根据用户消息复杂度自动选择 ReAct 或 Plan-Execute 模式
//!
//! 复杂度评分基于中英文关键词匹配 + 消息长度 + 历史轮次深度。
//! 当复杂度超过阈值时，路由到 Plan-Execute 模式（多步骤规划执行）。

use crate::services::llm_service::ChatMessage;
use crate::services::types::AgentMode;

/// 复杂度阈值：超过此分数触发 Plan-Execute 模式
pub const COMPLEXITY_THRESHOLD: u32 = 24;

/// 计算用户消息的复杂度分数
///
/// 评分维度：
/// - 中英文多步骤关键词（"分步骤"、"step by step"、"先...再..."等）
/// - 消息长度（长消息暗示复杂任务）
/// - 历史对话轮次深度
pub fn score_complexity(user_message: &str, history: &[ChatMessage]) -> u32 {
    let mut score: u32 = 0;

    // 1. 中文多步骤关键词（去除了日常高频、不代表高复杂度的词如 “先”、“所有”、“全部”、“系统”、“多个”）
    let cn_patterns = [
        "分步骤",
        "第一步",
        "第二步",
        "第三步",
        "然后",
        "接着",
        "最后",
        "流程",
        "方案",
        "架构",
        "设计",
        "重构",
        "迁移",
        "集成",
        "部署",
        "整体",
        "规划",
        "计划",
        "阶段",
        "里程碑",
    ];
    for pattern in &cn_patterns {
        if user_message.contains(pattern) {
            score += 3;
        }
    }

    // 2. 英文多步骤关键词
    let en_patterns = [
        "step by step",
        "first",
        "then",
        "finally",
        "next",
        "plan",
        "architect",
        "design",
        "refactor",
        "migrate",
        "batch",
        "multiple",
        "all",
        "entire",
        "system",
        "workflow",
        "pipeline",
        "milestone",
        "phase",
    ];
    let msg_lower = user_message.to_lowercase();
    for pattern in &en_patterns {
        if msg_lower.contains(pattern) {
            score += 3;
        }
    }

    // 3. 消息长度评分（每 200 字符加 1 分，最多加 5 分）
    let len_score = (user_message.len() as u32 / 200).min(5);
    score += len_score;

    // 4. 历史对话深度（每 4 轮加 2 分，最多加 6 分）
    let turns = history.len() as u32 / 2; // user+assistant = 1 turn
    let depth_score = (turns / 4).min(3) * 2;
    score += depth_score;

    // 5. 明确请求简单操作 → 降分
    let simple_patterns = [
        "查询", "搜索", "查找", "什么", "多少", "是否", "search", "find", "what", "how many",
    ];
    for pattern in &simple_patterns {
        if msg_lower.contains(pattern) {
            score = score.saturating_sub(5);
        }
    }

    score
}

/// 根据复杂度路由到合适的 Agent 模式
pub fn route_mode(user_message: &str, history: &[ChatMessage]) -> AgentMode {
    let score = score_complexity(user_message, history);
    if score >= COMPLEXITY_THRESHOLD {
        tracing::info!(
            score,
            threshold = COMPLEXITY_THRESHOLD,
            "路由到 Plan-Execute 模式"
        );
        AgentMode::PlanExecute
    } else {
        tracing::debug!(score, threshold = COMPLEXITY_THRESHOLD, "路由到 ReAct 模式");
        AgentMode::ReAct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_query_low_score() {
        let score = score_complexity("查询金蝶BOS的开发文档", &[]);
        assert!(score < COMPLEXITY_THRESHOLD, "简单查询应该低分: {}", score);
    }

    #[test]
    fn test_complex_plan_high_score() {
        let score = score_complexity(
            "请帮我分步骤设计一个完整的采购到付款流程迁移方案，包括架构设计 and 部署计划",
            &[],
        );
        assert!(score >= COMPLEXITY_THRESHOLD, "复杂规划应该高分: {}", score);
    }

    #[test]
    fn test_route_mode_simple() {
        let mode = route_mode("金蝶云星空怎么配置?", &[]);
        assert_eq!(mode, AgentMode::ReAct);
    }

    #[test]
    fn test_route_mode_complex() {
        let mode = route_mode(
            "请先分析现有系统架构，然后制定迁移方案，最后设计部署流程",
            &[],
        );
        assert_eq!(mode, AgentMode::PlanExecute);
    }
}
