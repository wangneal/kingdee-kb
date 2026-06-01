//! Harness 模块 — 约束编码 + 验证循环
//!
//! 为 Agent 执行提供程序化约束和结果验证机制：
//! - constraints: 工具约束 + Ping-Pong 检测
//! - verifier: 结果验证 + 重试上限

pub mod constraints;
pub mod verifier;
