//! Harness 模块 — 约束编码 + 验证循环 + 熵管理 + 活文档
//!
//! 为 Agent 执行提供程序化约束和结果验证机制：
//! - constraints: 工具约束 + Ping-Pong 检测
//! - verifier: 结果验证 + 重试上限
//! - entropy: 技术债务清理和文档一致性维护
//! - agents_log: Agent 失败学习记录（活文档）

pub mod agents_log;
pub mod constraints;
pub mod entropy;
pub mod verifier;
