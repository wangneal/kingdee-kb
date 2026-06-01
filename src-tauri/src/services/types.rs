//! 公共类型定义
//! 统一 AgentMode、BudgetPriority 等跨模块类型，避免重复定义导致编译冲突

use bitflags::bitflags;

bitflags! {
    /// Agent 执行模式（位掩码，支持 context_budget 的 mask 操作）
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AgentMode: u32 {
        const RagChat     = 0b001;
        const ReAct       = 0b010;
        const PlanExecute = 0b100;
    }
}

/// 预算槽优先级（数值越小越先分配，同优先级按比例）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BudgetPriority {
    SystemPrompt = 0,
    UserInput = 1,
    ReservedOutput = 2,
    ToolDefs = 3,
    Plan = 4, // 优先级高于 History，不可摘要压缩
    History = 5,
    RetrievedCtx = 6,
    Buffer = 7,
}

/// 聊天附件元数据
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttachmentInfo {
    pub name: String,
    pub path: String,
    pub kind: String, // "image" | "document"
}
