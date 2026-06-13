# Agent 生产就绪 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development 逐任务实现此计划。

> **实施后状态（2026-06-13）：** 本计划规划的 `cost_tracker.rs` 模块在创建后未投入使用，已于 2026-06-13 清理。

**目标：** 将 KingdeeKB agent 系统从"原型级"提升到"生产级" — 补齐超时/重试/取消、可观测性、安全防护、测试覆盖。

**架构：**
- Phase 1（可靠性）：后端添加 tokio::time::timeout + 重试 + 流式取消，前端添加 AbortController
- Phase 2（可观测性）：eprintln 替换为 tracing crate，添加 token 用量统计和审计日志
- Phase 3（安全）：添加 prompt injection 检测、输出内容过滤、工具调用速率限制
- Phase 4（测试）：为安全关键函数和 agent 核心逻辑添加单元测试

**技术栈：** Rust (tokio, tracing, tracing-subscriber) + TypeScript (AbortController)

---

## 文件结构

### 修改文件

| 文件 | 变更 |
|------|------|
| `src-tauri/src/services/rig_agent.rs` | 添加超时、重试、流式取消、tracing 替换 eprintln |
| `src-tauri/src/services/question_tool.rs` | PendingQuestions 添加超时 |
| `src-tauri/src/services/llm_service.rs` | LLM 调用添加 timeout、tracing 替换 eprintln |
| `src-tauri/src/services/rig_tool.rs` | 工具调用添加重试、tracing 替换 eprintln、单元测试 |
| `src-tauri/src/services/rig_provider.rs` | HTTP client 添加超时配置 |
| `src-tauri/src/services/memory.rs` | tracing 替换 eprintln |
| `src-tauri/src/commands/risk_blueprint.rs` | agent_chat 添加流式取消支持 |
| `src/lib/tauri-commands.ts` | agentChat 添加 AbortController、cancelAgentStream 命令 |
| `src/pages/Chat.tsx` | 集成取消按钮、超时处理 |
| `src-tauri/Cargo.toml` | 添加 tracing, tracing-subscriber 依赖 |

### 新建文件

| 文件 | 职责 |
|------|------|
| `src-tauri/src/services/agent_timeout.rs` | 超时配置常量 + 重试策略 |
| `src-tauri/src/services/agent_audit.rs` | 结构化审计日志（谁在何时调用了什么工具） |
| `src-tauri/src/services/cost_tracker.rs` | Token 用量统计 + 成本追踪 |
| `src-tauri/src/services/safety_filter.rs` | Prompt injection 检测 + 输出内容过滤 |

---

## Phase 1：可靠性（超时/重试/取消）

### 任务 1.1：PendingQuestions 超时

**文件：**
- 修改：`src-tauri/src/services/question_tool.rs`

- [ ] **步骤 1：添加超时常量**

在 `question_tool.rs` 顶部添加：

```rust
use std::time::Duration;

/// 用户回答澄清问题的超时时间（秒）
pub const QUESTION_TIMEOUT_SECS: u64 = 300; // 5 分钟
```

- [ ] **步骤 2：修改 await 为 timeout 包裹**

找到 `rx.await` 调用，替换为：

```rust
let answer = tokio::time::timeout(
    Duration::from_secs(QUESTION_TIMEOUT_SECS),
    rx,
)
.await
.map_err(|_| "用户未在规定时间内回答问题".to_string())?
.map_err(|e| format!("接收回答失败: {}", e))?;
```

- [ ] **步骤 3：运行编译验证**

运行：`cargo check`
预期：无错误

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/src/services/question_tool.rs
git commit -m "fix: add timeout to PendingQuestions to prevent agent deadlock"
```

---

### 任务 1.2：LLM 调用超时

**文件：**
- 创建：`src-tauri/src/services/agent_timeout.rs`
- 修改：`src-tauri/src/services/llm_service.rs`
- 修改：`src-tauri/src/services/mod.rs`

- [ ] **步骤 1：创建超时配置模块**

```rust
// src-tauri/src/services/agent_timeout.rs
//! Agent 超时和重试配置

use std::time::Duration;

/// LLM API 调用超时（秒）
pub const LLM_CALL_TIMEOUT_SECS: u64 = 120;

/// LLM 流式调用超时（秒）— 从开始到首个 chunk
pub const LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS: u64 = 30;

/// LLM 流式调用总超时（秒）
pub const LLM_STREAM_TOTAL_TIMEOUT_SECS: u64 = 300;

/// 工具执行超时（秒）
pub const TOOL_EXECUTION_TIMEOUT_SECS: u64 = 120;

/// Agent 总会话超时（秒）
pub const AGENT_SESSION_TIMEOUT_SECS: u64 = 600; // 10 分钟

/// 重试策略
pub const MAX_RETRIES: u32 = 3;
pub const RETRY_BASE_DELAY_MS: u64 = 1000;

/// 计算指数退避延迟
pub fn retry_delay(attempt: u32) -> Duration {
    Duration::from_millis(RETRY_BASE_DELAY_MS * 2u64.pow(attempt))
}
```

- [ ] **步骤 2：在 mod.rs 注册新模块**

在 `src-tauri/src/services/mod.rs` 添加：

```rust
pub mod agent_timeout;
```

- [ ] **步骤 3：为 LLM 非流式调用添加 timeout**

在 `llm_service.rs` 的 `chat_completion` 方法中，找到 HTTP 请求调用，用 `tokio::time::timeout` 包裹：

```rust
use crate::services::agent_timeout::LLM_CALL_TIMEOUT_SECS;

// 在 chat_completion 方法中
let response = tokio::time::timeout(
    Duration::from_secs(LLM_CALL_TIMEOUT_SECS),
    client.send(),
)
.await
.map_err(|_| "LLM 调用超时，请检查网络连接或稍后重试".to_string())?
.map_err(|e| format!("LLM 调用失败: {}", e))?;
```

- [ ] **步骤 4：为 LLM 流式调用添加首 chunk 超时**

在 `llm_service.rs` 的流式方法中，添加首 chunk 超时检测：

```rust
use crate::services::agent_timeout::LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS;

// 在流式循环开始处
let first_chunk = tokio::time::timeout(
    Duration::from_secs(LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS),
    stream.next(),
)
.await
.map_err(|_| "LLM 流式响应超时（未收到首个数据块）".to_string())?;
```

- [ ] **步骤 5：运行编译验证**

运行：`cargo check`
预期：无错误

- [ ] **步骤 6：Commit**

```bash
git add src-tauri/src/services/agent_timeout.rs src-tauri/src/services/mod.rs src-tauri/src/services/llm_service.rs
git commit -m "feat: add LLM call timeout with configurable constants"
```

---

### 任务 1.3：Agent 会话级超时

**文件：**
- 修改：`src-tauri/src/services/rig_agent.rs`

- [ ] **步骤 1：在 agent_chat 入口添加会话超时**

在 `rig_agent.rs` 的主入口函数中，用 `tokio::time::timeout` 包裹整个 agent 循环：

```rust
use crate::services::agent_timeout::AGENT_SESSION_TIMEOUT_SECS;

let result = tokio::time::timeout(
    Duration::from_secs(AGENT_SESSION_TIMEOUT_SECS),
    agent_loop(/* ... */),
)
.await;

match result {
    Ok(inner) => inner,
    Err(_) => {
        let _ = tx.send(ReActEvent::Error {
            message: "会话超时（超过10分钟），请重新开始对话".to_string(),
        });
        let _ = tx.send(ReActEvent::Done);
    }
}
```

- [ ] **步骤 2：运行编译验证**

运行：`cargo check`
预期：无错误

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/rig_agent.rs
git commit -m "feat: add agent session-level timeout (10 min)"
```

---

### 任务 1.4：流式取消机制

**文件：**
- 修改：`src-tauri/src/commands/risk_blueprint.rs`
- 修改：`src-tauri/src/services/rig_agent.rs`
- 修改：`src/lib/tauri-commands.ts`
- 修改：`src/pages/Chat.tsx`

- [ ] **步骤 1：添加取消标志到 agent 状态**

在 `rig_agent.rs` 中添加取消支持：

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Agent 会话取消标志
pub struct AgentCancelFlag {
    cancelled: Arc<AtomicBool>,
}

impl AgentCancelFlag {
    pub fn new() -> (Self, Arc<AtomicBool>) {
        let flag = Arc::new(AtomicBool::new(false));
        (Self { cancelled: flag.clone() }, flag)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}
```

- [ ] **步骤 2：在 agent 循环中检查取消标志**

在 `drain_stream` 的主循环中添加检查：

```rust
if cancel_flag.is_cancelled() {
    let _ = tx.send(ReActEvent::Error {
        message: "用户已取消操作".to_string(),
    });
    let _ = tx.send(ReActEvent::Done);
    return;
}
```

- [ ] **步骤 3：添加 cancel_agent_stream Tauri 命令**

在 `risk_blueprint.rs` 中添加：

```rust
#[tauri::command]
pub async fn cancel_agent_stream(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    // 查找并取消指定会话
    state.cancel_agent_session(&session_id);
    Ok(())
}
```

- [ ] **步骤 4：前端添加取消支持**

在 `tauri-commands.ts` 中添加：

```typescript
export async function cancelAgentStream(sessionId: string): Promise<void> {
  return invoke("cancel_agent_stream", { sessionId });
}
```

在 `Chat.tsx` 中添加取消按钮和 AbortController：

```typescript
const abortControllerRef = useRef<AbortController | null>(null);

const handleCancel = useCallback(async () => {
  if (currentSessionId.current) {
    await cancelAgentStream(currentSessionId.current);
  }
  abortControllerRef.current?.abort();
}, []);
```

- [ ] **步骤 5：运行编译验证**

运行：`cargo check && npx tsc --noEmit`
预期：无错误

- [ ] **步骤 6：Commit**

```bash
git add src-tauri/src/services/rig_agent.rs src-tauri/src/commands/risk_blueprint.rs src/lib/tauri-commands.ts src/pages/Chat.tsx
git commit -m "feat: add streaming cancellation support for agent sessions"
```

---

### 任务 1.5：工具调用重试

**文件：**
- 修改：`src-tauri/src/services/rig_agent.rs`

- [ ] **步骤 1：添加工具调用重试逻辑**

在 `rig_agent.rs` 的工具调用处添加重试：

```rust
use crate::services::agent_timeout::{MAX_RETRIES, retry_delay};

async fn call_tool_with_retry(
    tool: &dyn Tool,
    args: serde_json::Value,
) -> Result<String, ToolError> {
    let mut last_error = String::new();

    for attempt in 0..=MAX_RETRIES {
        match tool.call(args.clone()).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = e.to_string();
                if attempt < MAX_RETRIES {
                    let delay = retry_delay(attempt);
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    Err(ToolError(format!("工具调用失败（重试{}次后）: {}", MAX_RETRIES, last_error)))
}
```

- [ ] **步骤 2：替换直接工具调用为重试版本**

在 agent 循环中，将 `tool.call(args)` 替换为 `call_tool_with_retry(tool, args)`。

- [ ] **步骤 3：运行编译验证**

运行：`cargo check`
预期：无错误

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/src/services/rig_agent.rs
git commit -m "feat: add exponential backoff retry for tool calls"
```

---

## Phase 2：可观测性

### 任务 2.1：引入 tracing crate

**文件：**
- 修改：`src-tauri/Cargo.toml`
- 修改：`src-tauri/src/lib.rs`

- [ ] **步骤 1：添加依赖**

在 `Cargo.toml` 的 `[dependencies]` 中添加：

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

- [ ] **步骤 2：初始化 tracing subscriber**

在 `src-tauri/src/lib.rs` 的 `run()` 函数中，builder.setup 阶段添加：

```rust
tracing_subscriber::fmt()
    .with_env_filter(
        tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("kingdee_kb=info".parse().unwrap())
    )
    .with_target(true)
    .with_thread_ids(true)
    .init();
```

- [ ] **步骤 3：运行编译验证**

运行：`cargo check`
预期：无错误

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/lib.rs
git commit -m "feat: introduce tracing crate for structured logging"
```

---

### 任务 2.2：替换 rig_agent.rs 的 eprintln

**文件：**
- 修改：`src-tauri/src/services/rig_agent.rs`

- [ ] **步骤 1：添加 tracing 导入**

```rust
use tracing::{info, warn, error, debug, instrument};
```

- [ ] **步骤 2：替换所有 eprintln 为 tracing 宏**

替换规则：
- `[RigAgent] start` → `info!(session = %session_id, "agent session started")`
- `[RigAgent] tool_call` → `info!(session = %session_id, tool = %name, "tool call")`
- `[RigAgent] tool_result` → `debug!(session = %session_id, tool = %name, result_len = result.len(), "tool result")`
- `[RigAgent] error` → `error!(session = %session_id, error = %err, "agent error")`
- `[RigAgent] done` → `info!(session = %session_id, elapsed_ms = elapsed, "agent session completed")`

- [ ] **步骤 3：为 agent 入口函数添加 instrument 宏**

```rust
#[instrument(skip_all, fields(session_id = %session_id))]
pub async fn run_agent(/* ... */) {
    // ...
}
```

- [ ] **步骤 4：运行编译验证**

运行：`cargo check`
预期：无错误

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/services/rig_agent.rs
git commit -m "refactor: replace eprintln with tracing in rig_agent"
```

---

### 任务 2.3：替换其余模块的 eprintln

**文件：**
- 修改：`src-tauri/src/services/llm_service.rs`
- 修改：`src-tauri/src/services/rig_tool.rs`
- 修改：`src-tauri/src/services/memory.rs`

- [ ] **步骤 1：批量替换 llm_service.rs**

将所有 `eprintln!("[Compress]` 替换为 `info!(target: "llm", "[Compress]`
将所有 `eprintln!("[RAG]` 替换为 `info!(target: "llm", "[RAG]`
以此类推。

- [ ] **步骤 2：批量替换 rig_tool.rs**

将所有 `eprintln!("[RunSkillScript]` 替换为 `info!(target: "tool", "[RunSkillScript]`

- [ ] **步骤 3：批量替换 memory.rs**

将所有 `eprintln!("[Memory]` 替换为 `info!(target: "memory", "[Memory]`

- [ ] **步骤 4：运行编译验证**

运行：`cargo check`
预期：无错误

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/services/llm_service.rs src-tauri/src/services/rig_tool.rs src-tauri/src/services/memory.rs
git commit -m "refactor: replace eprintln with tracing across all service modules"
```

---

### 任务 2.4：Token 用量统计

**文件：**
- 创建：`src-tauri/src/services/cost_tracker.rs`
- 修改：`src-tauri/src/services/mod.rs`
- 修改：`src-tauri/src/services/rig_agent.rs`

- [ ] **步骤 1：创建成本追踪模块**

```rust
// src-tauri/src/services/cost_tracker.rs
//! Token 用量统计和成本追踪

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Default)]
pub struct CostTracker {
    pub total_input_tokens: AtomicU64,
    pub total_output_tokens: AtomicU64,
    pub total_tool_calls: AtomicU64,
    pub total_llm_calls: AtomicU64,
}

impl CostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_llm_call(&self, input_tokens: u64, output_tokens: u64) {
        self.total_input_tokens.fetch_add(input_tokens, Ordering::Relaxed);
        self.total_output_tokens.fetch_add(output_tokens, Ordering::Relaxed);
        self.total_llm_calls.fetch_add(1, Ordering::Relaxed);

        info!(
            target: "cost",
            input_tokens,
            output_tokens,
            total_input = self.total_input_tokens.load(Ordering::Relaxed),
            total_output = self.total_output_tokens.load(Ordering::Relaxed),
            "LLM call recorded"
        );
    }

    pub fn record_tool_call(&self) {
        self.total_tool_calls.fetch_add(1, Ordering::Relaxed);
    }

    pub fn summary(&self) -> CostSummary {
        CostSummary {
            total_input_tokens: self.total_input_tokens.load(Ordering::Relaxed),
            total_output_tokens: self.total_output_tokens.load(Ordering::Relaxed),
            total_tool_calls: self.total_tool_calls.load(Ordering::Relaxed),
            total_llm_calls: self.total_llm_calls.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CostSummary {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tool_calls: u64,
    pub total_llm_calls: u64,
}
```

- [ ] **步骤 2：在 agent 循环中集成 cost_tracker**

在每次 LLM 调用后记录 token 用量，在每次工具调用后记录计数。

- [ ] **步骤 3：运行编译验证**

运行：`cargo check`
预期：无错误

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/src/services/cost_tracker.rs src-tauri/src/services/mod.rs src-tauri/src/services/rig_agent.rs
git commit -m "feat: add token usage tracking and cost monitoring"
```

---

## Phase 3：安全防护

### 任务 3.1：Prompt Injection 检测

**文件：**
- 创建：`src-tauri/src/services/safety_filter.rs`
- 修改：`src-tauri/src/services/mod.rs`
- 修改：`src-tauri/src/services/rig_agent.rs`

- [ ] **步骤 1：创建安全过滤模块**

```rust
// src-tauri/src/services/safety_filter.rs
//! Prompt injection 检测和输出内容过滤

use tracing::warn;

/// 检测用户输入中可能的 prompt injection
pub fn detect_prompt_injection(input: &str) -> Option<String> {
    let patterns = [
        "ignore previous instructions",
        "ignore above instructions",
        "disregard all prior",
        "you are now",
        "system prompt",
        "act as if",
        "pretend you are",
        "forget your instructions",
        "override your rules",
        "bypass your guidelines",
    ];

    let lower = input.to_lowercase();
    for pattern in &patterns {
        if lower.contains(pattern) {
            warn!(target: "safety", pattern = %pattern, "potential prompt injection detected");
            return Some(format!("检测到可能的指令注入: '{}'", pattern));
        }
    }

    None
}

/// 清理 LLM 输出中的敏感信息
pub fn scrub_output(output: &str) -> String {
    // 移除可能泄露的 system prompt 片段
    let mut result = output.to_string();

    // 移除 <context> 标签（已有逻辑的增强）
    while let Some(start) = result.find("<context>") {
        if let Some(end) = result.find("</context>") {
            let end_pos = end + "</context>".len();
            result = format!("{}{}", &result[..start], &result[end_pos..]);
        } else {
            break;
        }
    }

    result
}
```

- [ ] **步骤 2：在 agent 入口处集成检测**

在 `rig_agent.rs` 的用户消息处理处：

```rust
if let Some(warning) = safety_filter::detect_prompt_injection(&user_message) {
    let _ = tx.send(ReActEvent::Error {
        message: format!("⚠️ {}", warning),
    });
    // 仍然继续处理，但记录警告
}
```

- [ ] **步骤 3：运行编译验证**

运行：`cargo check`
预期：无错误

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/src/services/safety_filter.rs src-tauri/src/services/mod.rs src-tauri/src/services/rig_agent.rs
git commit -m "feat: add prompt injection detection and output scrubbing"
```

---

### 任务 3.2：工具调用速率限制

**文件：**
- 修改：`src-tauri/src/services/rig_agent.rs`

- [ ] **步骤 1：添加速率限制器**

在 `rig_agent.rs` 中添加：

```rust
use std::time::Instant;

/// 工具调用速率限制器
struct ToolRateLimiter {
    /// 每分钟最大工具调用次数
    max_calls_per_minute: u32,
    /// 最近调用时间戳
    recent_calls: Vec<Instant>,
}

impl ToolRateLimiter {
    fn new(max_calls_per_minute: u32) -> Self {
        Self {
            max_calls_per_minute,
            recent_calls: Vec::new(),
        }
    }

    fn check_and_record(&mut self) -> bool {
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);

        // 清理过期记录
        self.recent_calls.retain(|t| *t > one_minute_ago);

        if self.recent_calls.len() >= self.max_calls_per_minute as usize {
            false
        } else {
            self.recent_calls.push(now);
            true
        }
    }
}
```

- [ ] **步骤 2：在工具调用前检查速率**

```rust
if !rate_limiter.check_and_record() {
    let _ = tx.send(ReActEvent::Error {
        message: "工具调用过于频繁（每分钟上限），请稍后重试".to_string(),
    });
    continue;
}
```

- [ ] **步骤 3：运行编译验证**

运行：`cargo check`
预期：无错误

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/src/services/rig_agent.rs
git commit -m "feat: add per-session tool call rate limiting"
```

---

## Phase 4：测试覆盖

### 任务 4.1：安全关键函数单元测试

**文件：**
- 修改：`src-tauri/src/services/rig_tool.rs`（添加 #[cfg(test)] 模块）

- [ ] **步骤 1：为 sanitize_filename 添加测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename_normal() {
        assert_eq!(sanitize_filename("report.txt"), "report.txt");
    }

    #[test]
    fn test_sanitize_filename_special_chars() {
        assert_eq!(sanitize_filename("report<>:\"/\\|?*.txt"), "report.txt");
    }

    #[test]
    fn test_sanitize_filename_long() {
        let long_name = "a".repeat(200);
        let result = sanitize_filename(&long_name);
        assert!(result.len() <= 80);
    }

    #[test]
    fn test_sanitize_filename_empty() {
        let result = sanitize_filename("");
        assert!(!result.is_empty()); // 应该有默认值
    }
}
```

- [ ] **步骤 2：为 is_safe_relative_path 添加测试**

```rust
#[test]
fn test_safe_relative_path_normal() {
    assert!(is_safe_relative_path("scripts/run.sh").is_ok());
}

#[test]
fn test_safe_relative_path_traversal() {
    assert!(is_safe_relative_path("../etc/passwd").is_err());
}

#[test]
fn test_safe_relative_path_absolute() {
    assert!(is_safe_relative_path("/etc/passwd").is_err());
}

#[test]
fn test_safe_relative_path_null_byte() {
    assert!(is_safe_relative_path("script\0.sh").is_err());
}
```

- [ ] **步骤 3：为 contains_shell_control_token 添加测试**

```rust
#[test]
fn test_shell_control_normal() {
    assert!(!contains_shell_control_token("hello world"));
}

#[test]
fn test_shell_control_and() {
    assert!(contains_shell_control_token("cmd1 && cmd2"));
}

#[test]
fn test_shell_control_pipe() {
    assert!(contains_shell_control_token("cmd1 | cmd2"));
}

#[test]
fn test_shell_control_redirect() {
    assert!(contains_shell_control_token("cmd > file"));
}
```

- [ ] **步骤 4：为 validate_skill_script_args 添加测试**

```rust
#[test]
fn test_validate_args_normal() {
    let args = vec!["--verbose".to_string(), "file.txt".to_string()];
    assert!(validate_skill_script_args(&args).is_ok());
}

#[test]
fn test_validate_args_too_many() {
    let args: Vec<String> = (0..40).map(|i| format!("arg{}", i)).collect();
    assert!(validate_skill_script_args(&args).is_err());
}

#[test]
fn test_validate_args_too_long() {
    let args = vec!["a".repeat(2000)];
    assert!(validate_skill_script_args(&args).is_err());
}
```

- [ ] **步骤 5：运行测试**

运行：`cargo test --lib services::rig_tool::tests`
预期：所有测试通过

- [ ] **步骤 6：Commit**

```bash
git add src-tauri/src/services/rig_tool.rs
git commit -m "test: add unit tests for security-critical functions in rig_tool"
```

---

### 任务 4.2：安全过滤模块测试

**文件：**
- 修改：`src-tauri/src/services/safety_filter.rs`

- [ ] **步骤 1：添加 prompt injection 检测测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_injection_ignore_instructions() {
        assert!(detect_prompt_injection("ignore previous instructions and do X").is_some());
    }

    #[test]
    fn test_detect_injection_normal_input() {
        assert!(detect_prompt_injection("帮我搜索金蝶ERP实施文档").is_none());
    }

    #[test]
    fn test_detect_injection_case_insensitive() {
        assert!(detect_prompt_injection("IGNORE Previous Instructions").is_some());
    }

    #[test]
    fn test_scrub_output_removes_context_tags() {
        let input = "前缀<context>敏感内容</context>后缀";
        let result = scrub_output(input);
        assert!(!result.contains("<context>"));
        assert!(result.contains("前缀"));
        assert!(result.contains("后缀"));
    }
}
```

- [ ] **步骤 2：运行测试**

运行：`cargo test --lib services::safety_filter::tests`
预期：所有测试通过

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/safety_filter.rs
git commit -m "test: add unit tests for safety filter module"
```

---

### 任务 4.3：超时配置测试

**文件：**
- 修改：`src-tauri/src/services/agent_timeout.rs`

- [ ] **步骤 1：添加重试延迟测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_delay_exponential() {
        let d0 = retry_delay(0);
        let d1 = retry_delay(1);
        let d2 = retry_delay(2);

        assert_eq!(d0, Duration::from_millis(1000));
        assert_eq!(d1, Duration::from_millis(2000));
        assert_eq!(d2, Duration::from_millis(4000));
    }

    #[test]
    fn test_timeout_constants_sane() {
        assert!(LLM_CALL_TIMEOUT_SECS > 0);
        assert!(LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS > 0);
        assert!(AGENT_SESSION_TIMEOUT_SECS > LLM_CALL_TIMEOUT_SECS);
        assert!(QUESTION_TIMEOUT_SECS > 0);
    }
}
```

- [ ] **步骤 2：运行测试**

运行：`cargo test --lib services::agent_timeout::tests`
预期：所有测试通过

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/agent_timeout.rs
git commit -m "test: add unit tests for timeout configuration"
```

---

## 执行顺序建议

1. **任务 1.1**（PendingQuestions 超时）— 最紧急，防止 agent 死锁
2. **任务 1.2**（LLM 调用超时）— 防止 agent 永久阻塞
3. **任务 1.3**（Agent 会话超时）— 兜底保护
4. **任务 1.4**（流式取消）— 用户体验
5. **任务 1.5**（工具重试）— 可靠性
6. **任务 4.1**（安全函数测试）— 在改动前先锁定行为
7. **任务 2.1-2.3**（tracing 迁移）— 可观测性基础
8. **任务 2.4**（成本追踪）— 运维必需
9. **任务 3.1**（Prompt injection）— 安全防护
10. **任务 3.2**（速率限制）— 防滥用
11. **任务 4.2-4.3**（补充测试）— 质量保证
