//! 上下文预算管理器
//! 优先级驱动的动态贪婪分配算法

use super::model_metadata::ModelMetadata;
use super::types::{AgentMode, BudgetPriority};
use std::collections::HashMap;

struct BudgetClaim {
    slot: BudgetPriority,
    min_tokens: u32,
    ideal_tokens: u32,
    mode_mask: AgentMode,
}

pub struct ContextBudget {
    pub total: u32,
    pub system_prompt: u32,
    pub tool_definitions: u32,
    pub retrieved_context: u32,
    pub history: u32,
    pub user_input: u32,
    pub plan: u32,
    pub reserved_output: u32,
    pub buffer: u32,
}

impl ContextBudget {
    pub fn calculate(metadata: &ModelMetadata, mode: AgentMode) -> Self {
        // U2 安全阀：硬上限 500K tokens，防止超大上下文窗口导致 OOM
        let total = metadata.context_window.min(500_000);
        let reserved_output = metadata.max_output_tokens;
        let claims = Self::build_claims(total, reserved_output, mode);

        let mut remaining = total;
        let mut min_alloc: HashMap<BudgetPriority, u32> = HashMap::new();
        for claim in &claims {
            if !claim.mode_mask.contains(mode) {
                continue;
            }
            let alloc = claim.min_tokens.min(remaining);
            min_alloc.insert(claim.slot, alloc);
            remaining = remaining.saturating_sub(alloc);
        }

        let total_ideal: u32 = claims
            .iter()
            .filter(|c| c.mode_mask.contains(mode))
            .map(|c| {
                c.ideal_tokens
                    .saturating_sub(min_alloc.get(&c.slot).copied().unwrap_or(0))
            })
            .sum();

        let mut final_alloc = min_alloc.clone();
        if total_ideal > 0 {
            for claim in &claims {
                if !claim.mode_mask.contains(mode) {
                    continue;
                }
                let current = final_alloc.get(&claim.slot).copied().unwrap_or(0);
                let deficit = claim.ideal_tokens.saturating_sub(current);
                let share = if remaining > 0 {
                    // 转换为 u64 进行计算，防止在大上下文窗口下乘法溢出
                    ((remaining as u64 * deficit as u64) / total_ideal as u64) as u32
                } else {
                    0
                };
                *final_alloc.get_mut(&claim.slot).unwrap() += share;
            }
        }

        Self {
            total,
            system_prompt: *final_alloc.get(&BudgetPriority::SystemPrompt).unwrap_or(&0),
            tool_definitions: *final_alloc.get(&BudgetPriority::ToolDefs).unwrap_or(&0),
            retrieved_context: *final_alloc.get(&BudgetPriority::RetrievedCtx).unwrap_or(&0),
            history: *final_alloc.get(&BudgetPriority::History).unwrap_or(&0),
            user_input: *final_alloc.get(&BudgetPriority::UserInput).unwrap_or(&0),
            plan: *final_alloc.get(&BudgetPriority::Plan).unwrap_or(&0),
            reserved_output: *final_alloc
                .get(&BudgetPriority::ReservedOutput)
                .unwrap_or(&0),
            buffer: *final_alloc.get(&BudgetPriority::Buffer).unwrap_or(&0),
        }
    }

    fn build_claims(total: u32, reserved_output: u32, mode: AgentMode) -> Vec<BudgetClaim> {
        let has_plan = mode.contains(AgentMode::PlanExecute);
        let has_tools = !mode.contains(AgentMode::RagChat);
        // 严格按 BudgetPriority 从高到低排列
        vec![
            BudgetClaim {
                slot: BudgetPriority::SystemPrompt,
                min_tokens: 200,
                ideal_tokens: total / 10,
                mode_mask: AgentMode::all(),
            },
            BudgetClaim {
                slot: BudgetPriority::UserInput,
                min_tokens: 100,
                ideal_tokens: total / 20,
                mode_mask: AgentMode::all(),
            },
            BudgetClaim {
                slot: BudgetPriority::ReservedOutput,
                min_tokens: reserved_output,
                ideal_tokens: reserved_output,
                mode_mask: AgentMode::all(),
            },
            BudgetClaim {
                slot: BudgetPriority::ToolDefs,
                min_tokens: 0,
                ideal_tokens: total / 5,
                mode_mask: if has_tools {
                    AgentMode::all()
                } else {
                    AgentMode::empty()
                },
            },
            BudgetClaim {
                slot: BudgetPriority::Plan,
                min_tokens: 0,
                ideal_tokens: total / 4,
                mode_mask: if has_plan {
                    AgentMode::all()
                } else {
                    AgentMode::empty()
                },
            },
            BudgetClaim {
                slot: BudgetPriority::History,
                min_tokens: 500,
                ideal_tokens: total / 2,
                mode_mask: AgentMode::all(),
            },
            BudgetClaim {
                slot: BudgetPriority::RetrievedCtx,
                min_tokens: 0,
                ideal_tokens: total * 3 / 10,
                mode_mask: AgentMode::all(),
            },
            BudgetClaim {
                slot: BudgetPriority::Buffer,
                min_tokens: 200,
                ideal_tokens: 500,
                mode_mask: AgentMode::all(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_calculate_react_mode() {
        let meta = ModelMetadata {
            context_window: 128000,
            max_output_tokens: 16384,
            supports_thinking: false,
            supports_vision: false,
            supports_tools: true,
        };
        let budget = ContextBudget::calculate(&meta, AgentMode::ReAct);
        assert!(budget.system_prompt > 0);
        assert!(budget.history > 0);
        assert_eq!(budget.plan, 0, "ReAct mode should have no plan budget");
    }

    #[test]
    fn test_budget_not_exceed_total() {
        let meta = ModelMetadata {
            context_window: 4096,
            max_output_tokens: 1024,
            supports_thinking: false,
            supports_vision: false,
            supports_tools: false,
        };
        let budget = ContextBudget::calculate(&meta, AgentMode::RagChat);
        let sum = budget.system_prompt
            + budget.tool_definitions
            + budget.retrieved_context
            + budget.history
            + budget.user_input
            + budget.plan
            + budget.reserved_output
            + budget.buffer;
        assert!(sum <= meta.context_window);
    }
}
