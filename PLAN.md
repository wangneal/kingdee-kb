# Plan: Add Agent Session Timeout to rig_agent.rs

## Task
Wrap the agent streaming loop in `RigAgent::run()` with `tokio::time::timeout` using `AGENT_SESSION_TIMEOUT_SECS` (600s / 10 min) from `agent_timeout.rs`.

## File
`src-tauri/src/services/rig_agent.rs`

## Changes

### 1. Add imports (after line 9)

```rust
use std::time::Duration;
```

And add to the crate imports section (after line 23 or nearby):

```rust
use crate::services::agent_timeout::AGENT_SESSION_TIMEOUT_SECS;
```

### 2. Wrap match block with timeout (lines 183-280)

**Before the match block** (after line 181, the system_prompt format closing), insert:

```rust
        // 会话超时保护：整个 agent 流式循环最长运行 AGENT_SESSION_TIMEOUT_SECS
        let timeout_sender = sender.clone();
        let timeout_sid = sid.clone();

        let result = tokio::time::timeout(
            Duration::from_secs(AGENT_SESSION_TIMEOUT_SECS),
            async {
```

**Wrap the existing match block** (lines 183-280) inside the async block. The match block content stays unchanged but is indented one additional level (4 spaces).

**After the match block closing `}`** (after line 280), insert:

```rust
            },
        )
        .await;

        if result.is_err() {
            let _ = timeout_sender.send(ReActEvent::Error {
                session_id: timeout_sid.clone(),
                message: "会话超时（超过10分钟），请重新开始对话".to_string(),
            });
            let _ = timeout_sender.send(ReActEvent::Done {
                session_id: timeout_sid,
            });
        }
```

### Why this works

- `sender` and `sid` are used in early-return error paths (lines 86-90) BEFORE the async block — those paths `return` so the variables are available after line 92
- `timeout_sender` (clone) is kept outside the async block for the timeout handler
- `timeout_sid` (clone) is kept outside the async block for the timeout handler
- The original `sender` and `sid` are captured by the async block (moved in) for use inside the match arms
- `config`, `model`, `temperature`, `max_tokens`, `prompt`, `started_at` and all `Arc<Mutex<...>>` params are captured by the async block — fine since they're only needed inside
- References (`llm: &LLMService`, `user_message: &str`, etc.) are valid because the async block's lifetime is bounded by the function's lifetime

### What NOT to change
- Function signature stays identical
- No logic inside the match block changes
- `agent_timeout.rs` untouched
- No new dependencies

## Verification
```bash
cd src-tauri && cargo check
```
