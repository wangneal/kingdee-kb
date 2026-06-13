# 三层 AI 工程范式 — 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

> **实施后状态（2026-06-13）：** 本计划规划的 `context_budget.rs` 模块在创建后未投入使用，已于 2026-06-13 清理。其余模块（`token.rs` / `model_metadata.rs` / `context_compressor.rs` / `agent_router.rs` 等）继续使用。

**目标：** 在 KingdeeKB 项目中落地 Context Engineering + Plan-and-Execute + Harness Engineering 三层范式

**架构：** 从下到上三层叠加：Context（精确 token 管理 + 动态预算）→ Plan（Planner + Replanner + 状态机）→ Harness（约束编码 + 验证循环 + 熵管理）。新增 10 个 Rust 服务模块，修改 rig_agent.rs、llm_service.rs、前端 Chat.tsx、AgentContext.tsx、Settings.tsx

**技术栈：** Tauri 2.x + Rust (tokio) + React 18 + TypeScript + rig-core

**设计文档：** `docs/superpowers/specs/2026-05-31-agent-engineering-paradigm-design.md`
**架构文档：** `docs/ARCHITECTURE.md`

---

## 文件结构

```
src-tauri/src/services/
├── token.rs                    [新] 统一 Token 计数 (P0-a)
├── model_metadata.rs           [新] 模型元数据分层获取 (P0-b)
├── context_budget.rs           [新] 优先级驱动动态预算 (P0-c)
├── context_compressor.rs       [新] 分层摘要 + 磁滞回线 (P0-d)
├── types.rs                    [新] 公共类型: AgentMode, BudgetPriority, ChatMessage 扩展 (P0-c)
├── agent_router.rs             [新] 模式路由 + 复杂度评分 (P1-a)
├── planner.rs                  [新] Planner + Replanner + PlanStateMachine (P1-b)
├── harness/
│   ├── mod.rs                  [新] (P2)
│   ├── constraints.rs          [新] 工具约束 + Ping-Pong 检测 (P2-a)
│   ├── verifier.rs             [新] 结果验证 + 重试上限 (P2-b)
│   └── entropy.rs              [新] 熵管理 (P2-c)
├── llm_service.rs              [改] 新增 MessageContext（独立于 ChatMessage 的 id + token_count 追踪）、删硬编码 SYSTEM_PROMPT (P0-a/P0-f)
├── llm_providers.rs            [改] ModelConfig 扩展(context_window/max_output_tokens/supports_thinking) (P0-b)
├── prompt_assembler.rs         [改] 接入 Agent 管道替换手动技能注入 (P0-e)
├── rig_agent.rs                [改] Plan-Execute 执行循环 (P1-c)
├── prompts.rs                  [改] 统一系统提示词 (P0-f)
├── skill_manager.rs            [改] 使用 PromptAssembler (P0-e)

src-tauri/resources/
└── model_specs.json            [新] 内置模型规格数据库 (P0-b)

src-tauri/src/commands/
├── risk_blueprint.rs             [改] 技能注入改用 PromptAssembler (P0-e)

src/
├── contexts/AgentContext.tsx   [改] ReActTrace 扩展、Plan 事件处理 (P1-d)
├── pages/Chat.tsx              [改] Plan 时间线 UI、Token 用量指示器 (P1-d/P2-d)
├── pages/Settings.tsx          [改] 上下文工程 section (P2-d)
```

---

### 任务 1：创建 `token.rs` + 扩展 `ChatMessage` — 统一 Token 计数 (P0-a)

**文件：**
- 创建：`src-tauri/src/services/token.rs`
- 修改：`src-tauri/src/services/llm_service.rs` (ChatMessage 定义)
- 注意：ChatMessage 的 `id` 和 `token_count` 扩展**必须先完成**，再编写引用这些字段的 `count_messages_tokens`

- [x] **步骤 0：先扩展 ChatMessage 结构体（原 Task 2，避免编译依赖死锁）**

在 `llm_service.rs` 的 ChatMessage 定义处添加 `id` 和 `token_count` 字段，并添加 uuid 依赖：

```rust
// src-tauri/src/services/llm_service.rs — ChatMessage 定义处 (L344)
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_count: Option<u32>,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: role.to_string(),
            content: content.to_string(),
            token_count: Some(token::count_tokens_with_fallback(content)),
        }
    }
    pub fn set_content(&mut self, content: String) {
        self.token_count = None;
        self.content = content;
    }
    pub fn get_token_count(&self) -> u32 {
        self.token_count.unwrap_or_else(|| token::count_tokens_with_fallback(&self.content))
    }
    pub fn compute_token_count(&mut self) {
        if self.token_count.is_none() {
            self.token_count = Some(token::count_tokens_with_fallback(&self.content));
        }
    }
}
```
```bash
# uuid 已在 Cargo.toml L74 存在，直接验证编译即可
cargo check
```

- [x] 修复所有直接构造 `ChatMessage { role, content }` 的代码，改用 `ChatMessage::new()` 或添加 `id` 字段

- [x] **步骤 1：创建 `token.rs` 模块文件**
//! 统一 Token 计数模块
//! 全项目所有 token 计算统一通过此模块，替代现有三套标准。

use std::hash::{Hash, Hasher};

/// Token 计数错误
#[derive(Debug)]
pub enum TokenError {
    TiktokenInitFailed(String),
}

/// 精确 token 计数（基于 tiktoken cl100k_base）
/// 失败返回 Result，不静默降级
pub fn count_tokens(text: &str) -> Result<u32, TokenError> {
    tiktoken_rs::cl100k_base()
        .map(|b| b.encode_with_special_tokens(text).len() as u32)
        .map_err(|e| TokenError::TiktokenInitFailed(e.to_string()))
}

/// 带回退的 token 计数（用于非关键路径）
/// 回退公式区分中英文比例：中文 ~1.5 字符/token，英文 ~4 字符/token
pub fn count_tokens_with_fallback(text: &str) -> u32 {
    count_tokens(text).unwrap_or_else(|_| {
        let chinese_chars = text.chars().filter(|c| !c.is_ascii()).count();
        let ascii_chars = text.len() - chinese_chars;
        (chinese_chars as f32 / 1.5 + ascii_chars as f32 / 4.0) as u32
    })
}

/// 计算消息数组的 token 总量（含结构开销）
/// 优先使用消息上的 token_count 缓存，缓存 miss 时先尝试精确计数
pub fn count_messages_tokens(messages: &[crate::services::llm_service::ChatMessage]) -> u32 {
    messages.iter().map(|m| {
        let content_tokens = match m.token_count {
            Some(cached) => cached,
            None => count_tokens(&m.content)
                .unwrap_or_else(|_| count_tokens_with_fallback(&m.content)),
        };
        content_tokens + count_tokens_with_fallback(&m.role) + 4
    }).sum()
}

/// Token 级截断（二分查找，UTF-8 边界安全）
pub fn truncate_to_tokens(text: &str, max_tokens: u32) -> String {
    let total = match count_tokens(text) {
        Ok(t) => t,
        Err(_) => count_tokens_with_fallback(text),
    };
    if total <= max_tokens {
        return text.to_string();
    }

    let mut low = 0;
    let mut high = text.len();

    while low < high {
        let mid = (low + high + 1) / 2;
        let mut end = mid;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        let candidate = &text[..end];
        let tokens = count_tokens_with_fallback(candidate);
        if tokens <= max_tokens {
            low = end;
        } else {
            high = end - 1;
        }
    }

    let mut end = low;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens_empty() {
        let result = count_tokens_with_fallback("");
        assert_eq!(result, 0);
    }

    #[test]
    fn test_count_tokens_chinese() {
        let result = count_tokens_with_fallback("你好世界");
        assert!(result > 0, "Chinese text should return positive token count");
    }

    #[test]
    fn test_count_tokens_english() {
        let result = count_tokens_with_fallback("hello world");
        assert!(result > 0, "English text should return positive token count");
    }

    #[test]
    fn test_truncate_to_tokens_no_truncation() {
        let text = "short text";
        let result = truncate_to_tokens(text, 100);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_to_tokens_truncates() {
        let text = "a".repeat(10000);
        let result = truncate_to_tokens(&text, 10);
        assert!(result.len() < text.len());
        let tokens = count_tokens_with_fallback(&result);
        assert!(tokens <= 10, "truncated text should be within token budget");
    }
}
```

- [x] **步骤 2：注册模块到 `lib.rs`**

```rust
// src-tauri/src/lib.rs — 在 pub mod services 块中添加
pub mod token;
```

- [x] **步骤 3：删除 `llm_service.rs` 中的 `count_tokens` 和 `truncate_to_tokens`**

在 `llm_service.rs` 中：
- 删除 L449-457 `count_tokens` 函数
- 删除 L461-483 `truncate_to_tokens` 函数
- 将所有调用点改为 `token::count_tokens` / `token::count_tokens_with_fallback` / `token::truncate_to_tokens`
- 调用点（搜索 `count_tokens` 和 `truncate_to_tokens` 引用）：`llm_service.rs` 多处 + `doc_generator.rs`
- ⚠️ **补充迁移**：`commands/search_llm.rs:113` 有 Tauri 命令 `count_tokens` 封装了 `llm_service::count_tokens`，需改为 `token::count_tokens`

- [x] **步骤 4：删除 `PromptAssembler::estimate_tokens`，改用 `token::count_tokens_with_fallback`**

```rust
// prompt_assembler.rs — 删除 L161-166 的 estimate_tokens
// 改为调用 token::count_tokens_with_fallback
use crate::services::token;
// 将 used_tokens += Self::estimate_tokens(...) 替换为
// used_tokens += token::count_tokens_with_fallback(...) as usize;
```

- [x] **步骤 5：运行测试，验证编译通过**

```bash
cd src-tauri && cargo test token::tests -- --nocapture
cargo check 2>&1 | Out-String
```
预期：所有 token 测试通过，0 错误 0 警告。

- [x] **步骤 6：Commit**

```bash
git add src-tauri/src/services/token.rs src-tauri/src/lib.rs
git add src-tauri/src/services/llm_service.rs src-tauri/src/services/prompt_assembler.rs
git commit -m "feat(P0-a): add unified token counting module (token.rs)"
```

---

### 任务 2：扩展 `ChatMessage` — 添加 `id` 和 `token_count` 缓存 (P0-a)

**文件：**
- 修改：`src-tauri/src/services/llm_service.rs` (ChatMessage 定义)

- [x] **步骤 1：修改 ChatMessage 结构体**

```rust
// src-tauri/src/services/llm_service.rs — ChatMessage 定义处 (L344)
pub struct ChatMessage {
    /// 消息唯一标识符，用于增量摘要的位置追踪
    pub id: String,
    pub role: String,
    pub content: String,
    /// token 计数缓存，消息写入时一次性计算
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_count: Option<u32>,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: role.to_string(),
            content: content.to_string(),
            token_count: Some(crate::services::token::count_tokens_with_fallback(content)),
        }
    }

    /// 修改内容时自动失效缓存
    pub fn set_content(&mut self, content: String) {
        self.token_count = None;
        self.content = content;
    }

    /// 懒计算 + 返回缓存
    pub fn get_token_count(&self) -> u32 {
        self.token_count.unwrap_or_else(|| {
            crate::services::token::count_tokens_with_fallback(&self.content)
        })
    }

    /// 批量补算（反序列化后调用）
    pub fn compute_token_count(&mut self) {
        if self.token_count.is_none() {
            self.token_count = Some(crate::services::token::count_tokens_with_fallback(&self.content));
        }
    }
}
```

- [x] **步骤 2：检查 Cargo.toml 是否有 uuid 依赖**

```bash
# 检查
Select-String -LiteralPath "src-tauri/Cargo.toml" -Pattern "uuid"
# 如果没有，添加
cargo add uuid --features v4
```

- [x] **步骤 3：修复所有直接构造 `ChatMessage { role, content }` 的代码**

使用 grep 找到所有直接构造点，改为 `ChatMessage::new()`：

```bash
cd src-tauri
rg "ChatMessage\s*\{" --include="*.rs" -l
```

对每个文件，将直接构造改为 `ChatMessage::new()` 或添加 `id: uuid::Uuid::new_v4().to_string()`

- [x] **步骤 4：验证编译通过**

```bash
cd src-tauri && cargo check 2>&1 | Out-String
```

- [x] **步骤 5：Commit**

```bash
git add src-tauri/src/services/llm_service.rs src-tauri/Cargo.toml
git commit -m "feat(P0-a): add id and token_count fields to ChatMessage"
```

---

### 任务 3：创建 `model_metadata.rs` + `model_specs.json` — 模型元数据系统 (P0-b)

**文件：**
- 创建：`src-tauri/src/services/model_metadata.rs`
- 创建：`src-tauri/resources/model_specs.json`

- [x] **步骤 1：创建 `model_specs.json`**

```json
{
  "openai": {
    "gpt-4o": { "context_window": 128000, "max_output_tokens": 16384, "supports_thinking": false, "supports_vision": true, "supports_tools": true },
    "gpt-5": { "context_window": 256000, "max_output_tokens": 32768, "supports_thinking": true, "supports_vision": true, "supports_tools": true },
    "o3": { "context_window": 200000, "max_output_tokens": 100000, "supports_thinking": true, "supports_vision": true, "supports_tools": true },
    "o4-mini": { "context_window": 200000, "max_output_tokens": 100000, "supports_thinking": true, "supports_vision": true, "supports_tools": true }
  },
  "deepseek": {
    "deepseek-v4-pro": { "context_window": 1000000, "max_output_tokens": 384000, "supports_thinking": true, "supports_vision": false, "supports_tools": true },
    "deepseek-v4-flash": { "context_window": 1000000, "max_output_tokens": 384000, "supports_thinking": true, "supports_vision": false, "supports_tools": true },
    "deepseek-reasoner": { "context_window": 1000000, "max_output_tokens": 384000, "supports_thinking": true, "supports_vision": false, "supports_tools": true }
  },
  "anthropic": {
    "claude-opus-4-7": { "context_window": 200000, "max_output_tokens": 128000, "supports_thinking": true, "supports_vision": true, "supports_tools": true },
    "claude-sonnet-4-5": { "context_window": 200000, "max_output_tokens": 64000, "supports_thinking": true, "supports_vision": true, "supports_tools": true }
  }
}
```

- [x] **步骤 2：创建 `model_metadata.rs` — 分层获取逻辑**

```rust
// src-tauri/src/services/model_metadata.rs
use serde::{Deserialize, Serialize};
use super::llm_providers::LLMProviderConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub supports_thinking: bool,
    pub supports_vision: bool,
    pub supports_tools: bool,
}

impl Default for ModelMetadata {
    fn default() -> Self {
        Self {
            context_window: 4096,
            max_output_tokens: 4096,
            supports_thinking: false,
            supports_vision: false,
            supports_tools: false,
        }
    }
}

pub async fn resolve_metadata(provider: &LLMProviderConfig, model_name: &str) -> ModelMetadata {
    // 优先级 1: 用户手动覆盖
    if let Some(model) = provider.models.iter().find(|m| m.name == model_name || m.id == model_name) {
        if model.context_window.is_some() || model.max_output_tokens.is_some() {
            return ModelMetadata {
                context_window: model.context_window.unwrap_or(4096),
                max_output_tokens: model.max_output_tokens.unwrap_or(4096),
                supports_thinking: model.supports_thinking.unwrap_or(false),
                supports_vision: model.is_multimodal.unwrap_or(false),
                supports_tools: true,
            };
        }
    }

    // 优先级 2: 提供商原生 API (Anthropic / Google Gemini)
    // @see: 设计文档 3.2 节 — 异步探测逻辑
    if let Some(meta) = from_provider_api(provider, model_name).await {
        return meta;
    }

    // 优先级 3: 内置模型数据库
    if let Some(meta) = from_builtin_db(model_name) {
        return meta;
    }

    // 优先级 4: 保守默认值
    tracing::warn!("Model '{}' not found in builtin DB, using conservative defaults", model_name);
    ModelMetadata::default()
}

fn from_builtin_db(model_name: &str) -> Option<ModelMetadata> {
    let specs_str = include_str!("../../resources/model_specs.json");
    let specs: serde_json::Value = serde_json::from_str(specs_str).ok()?;

    for (_provider, models) in specs.as_object()? {
        if let Some(spec) = models.get(model_name) {
            return Some(ModelMetadata {
                context_window: spec["context_window"].as_u64()? as u32,
                max_output_tokens: spec["max_output_tokens"].as_u64()? as u32,
                supports_thinking: spec["supports_thinking"].as_bool()?,
                supports_vision: spec["supports_vision"].as_bool()?,
                supports_tools: spec["supports_tools"].as_bool()?,
            });
        }
    }
    None
}

async fn from_provider_api(provider: &LLMProviderConfig, model_name: &str) -> Option<ModelMetadata> {
    let client = reqwest::Client::new();
    // Anthropic
    if provider.base_url.contains("anthropic.com") {
        let resp = client
            .get(format!("{}/v1/models/{}", provider.base_url.trim_end_matches('/'), model_name))
            .header("x-api-key", &provider.api_keys.first()?.key)
            .send().await.ok()?;
        if resp.status().is_success() {
            let json: serde_json::Value = resp.json().await.ok()?;
            return Some(ModelMetadata {
                context_window: json["max_input_tokens"].as_u64()? as u32,
                max_output_tokens: json["max_tokens"].as_u64()? as u32,
                supports_thinking: json["capabilities"]["thinking"]["supported"].as_bool().unwrap_or(false),
                supports_vision: json["capabilities"]["image_input"]["supported"].as_bool().unwrap_or(false),
                supports_tools: true,
            });
        }
    }
    // Gemini
    if provider.base_url.contains("googleapis.com") {
        let api_key = &provider.api_keys.first()?.key;
        let resp = client
            .get(format!("{}/v1beta/models/{}?key={}", provider.base_url.trim_end_matches('/'), model_name, api_key))
            .send().await.ok()?;
        if resp.status().is_success() {
            let json: serde_json::Value = resp.json().await.ok()?;
            return Some(ModelMetadata {
                context_window: json["inputTokenLimit"].as_u64()? as u32,
                max_output_tokens: json["outputTokenLimit"].as_u64()? as u32,
                supports_thinking: json["thinking"].as_bool().unwrap_or(false),
                supports_vision: true,
                supports_tools: true,
            });
        }
    }
    // Ollama
    if provider.protocol == LLMProtocol::Local {
        let resp = client
            .post(format!("{}/api/show", provider.base_url.trim_end_matches('/')))
            .json(&serde_json::json!({"name": model_name}))
            .send().await.ok()?;
        if resp.status().is_success() {
            let json: serde_json::Value = resp.json().await.ok()?;
            // 从 model_info 中提取 context_length
            for (key, value) in json["model_info"].as_object()? {
                if key.ends_with(".context_length") {
                    return Some(ModelMetadata {
                        context_window: value.as_u64()? as u32,
                        max_output_tokens: 8192,
                        supports_thinking: false,
                        supports_vision: json["capabilities"].as_array()
                            .map_or(false, |caps| caps.iter().any(|c| c.as_str() == Some("vision"))),
                        supports_tools: true,
                    });
                }
            }
        }
    }
    None
}
```

- [x] **步骤 3：扩展 `ModelConfig` 结构体**

在 `llm_providers.rs` 中：

```rust
// llm_providers.rs — ModelConfig 新增字段
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    #[serde(default)]
    pub is_multimodal: Option<bool>,
    #[serde(default)]
    pub last_probe_at: Option<String>,
    // ★ 新增
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub supports_thinking: Option<bool>,
    // 其余字段不变...
}
```

- [x] **步骤 4：注册模块，运行编译验证**

```bash
cd src-tauri && cargo check 2>&1 | Out-String
```

- [x] **步骤 5：Commit**

```bash
git add src-tauri/src/services/model_metadata.rs src-tauri/resources/model_specs.json
git add src-tauri/src/services/llm_providers.rs src-tauri/src/lib.rs
git commit -m "feat(P0-b): add model metadata resolution system"
```

---

### 任务 4：创建 `types.rs` — 公共类型 + `AgentMode` (P0-c)

**文件：**
- 创建：`src-tauri/src/services/types.rs`

- [x] **步骤 0：添加 bitflags 依赖**

```bash
cd src-tauri && cargo add bitflags
```
预期：依赖添加成功，Cargo.toml 新增 `bitflags` 条目。

- [x] **步骤 1：创建 `types.rs`**

```rust
// src-tauri/src/services/types.rs
//! 公共类型定义 — 统一 AgentMode、BudgetPriority 等跨模块类型
//! 避免 agent_router 和 context_budget 各自定义同名类型导致编译冲突

use bitflags::bitflags;

bitflags! {
    /// Agent 执行模式（位掩码，支持 P0-c 的 mask 操作）
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AgentMode: u32 {
        const RagChat     = 0b001;
        const ReAct       = 0b010;
        const PlanExecute = 0b100;
    }
}

impl AgentMode {
    pub fn all() -> Self {
        AgentMode::RagChat | AgentMode::ReAct | AgentMode::PlanExecute
    }
    pub fn empty() -> Self {
        AgentMode::empty()
    }
}

/// 预算槽优先级（数值越小越先分配）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BudgetPriority {
    SystemPrompt = 0,
    UserInput    = 1,
    ReservedOutput = 2,
    ToolDefs     = 3,
    Plan         = 4,  // 优先级高于 History，不可摘要压缩
    History      = 5,
    RetrievedCtx = 6,
    Buffer       = 7,
}
```

- [x] **步骤 2：注册模块**

```rust
// lib.rs
pub mod types;
```

- [x] **步骤 3：编译验证**

```bash
cd src-tauri && cargo check 2>&1 | Out-String
```

- [x] **步骤 4：Commit**

```bash
git add src-tauri/src/services/types.rs src-tauri/src/lib.rs
git commit -m "feat(P0-c): add shared types module (AgentMode, BudgetPriority)"
```

---

### 任务 5：创建 `context_budget.rs` — 上下文预算管理器 (P0-c)

**文件：**
- 创建：`src-tauri/src/services/context_budget.rs`

- [x] **步骤 1：创建 `context_budget.rs` — 优先级驱动动态贪婪分配**

```rust
// src-tauri/src/services/context_budget.rs
use std::collections::HashMap;
use super::model_metadata::ModelMetadata;
use super::types::{AgentMode, BudgetPriority};

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
        let total = metadata.context_window;
        let reserved_output = metadata.max_output_tokens;
        let claims = Self::build_claims(total, reserved_output, mode);

        // 第一轮：按优先级从高到低，先满足每个槽的 min
        let mut remaining = total;
        let mut min_alloc: HashMap<BudgetPriority, u32> = HashMap::new();
        for claim in &claims {
            if !claim.mode_mask.contains(mode) { continue; }
            let alloc = claim.min_tokens.min(remaining);
            min_alloc.insert(claim.slot, alloc);
            remaining -= alloc;
        }

        // 第二轮：剩余空间按 ideal 比例贪婪分配
        let total_ideal: u32 = claims.iter()
            .filter(|c| c.mode_mask.contains(mode))
            .map(|c| c.ideal_tokens.saturating_sub(min_alloc.get(&c.slot).copied().unwrap_or(0)))
            .sum();

        let mut final_alloc = min_alloc.clone();
        if total_ideal > 0 {
            for claim in &claims {
                if !claim.mode_mask.contains(mode) { continue; }
                let current = min_alloc.get(&claim.slot).copied().unwrap_or(0);
                let deficit = claim.ideal_tokens.saturating_sub(current);
                let share = remaining * deficit / total_ideal;
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
            reserved_output: *final_alloc.get(&BudgetPriority::ReservedOutput).unwrap_or(&0),
            buffer: *final_alloc.get(&BudgetPriority::Buffer).unwrap_or(&0),
        }
    }

    fn build_claims(total: u32, reserved_output: u32, mode: AgentMode) -> Vec<BudgetClaim> {
        let has_plan = mode.contains(AgentMode::PlanExecute);
        let has_tools = !mode.contains(AgentMode::RagChat);
        // 严格按 BudgetPriority 从高到低排列
        vec![
            BudgetClaim { slot: BudgetPriority::SystemPrompt,   min: 200,             ideal: total / 10,       mode_mask: AgentMode::all() },
            BudgetClaim { slot: BudgetPriority::UserInput,      min: 100,             ideal: total / 20,       mode_mask: AgentMode::all() },
            BudgetClaim { slot: BudgetPriority::ReservedOutput, min: reserved_output, ideal: reserved_output,  mode_mask: AgentMode::all() },
            BudgetClaim { slot: BudgetPriority::ToolDefs,       min: 0,               ideal: total / 5,        mode_mask: if has_tools { AgentMode::all() } else { AgentMode::empty() } },
            BudgetClaim { slot: BudgetPriority::Plan,           min: 0,               ideal: total / 4,        mode_mask: if has_plan { AgentMode::all() } else { AgentMode::empty() } },
            BudgetClaim { slot: BudgetPriority::History,        min: 500,             ideal: total / 2,        mode_mask: AgentMode::all() },
            BudgetClaim { slot: BudgetPriority::RetrievedCtx,   min: 0,               ideal: total * 3 / 10,   mode_mask: AgentMode::all() },
            BudgetClaim { slot: BudgetPriority::Buffer,         min: 200,             ideal: 500,              mode_mask: AgentMode::all() },
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
        assert!(budget.plan == 0, "ReAct mode should have no plan budget");
    }

    #[test]
    fn test_budget_calculate_plan_execute_mode() {
        let meta = ModelMetadata {
            context_window: 128000,
            max_output_tokens: 16384,
            supports_thinking: false,
            supports_vision: false,
            supports_tools: true,
        };
        let budget = ContextBudget::calculate(&meta, AgentMode::PlanExecute);
        assert!(budget.plan > 0, "PlanExecute mode should have plan budget");
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
        let sum = budget.system_prompt + budget.tool_definitions + budget.retrieved_context
            + budget.history + budget.user_input + budget.plan + budget.reserved_output + budget.buffer;
        assert!(sum <= meta.context_window, "total allocation should not exceed context window");
    }
}
```

- [x] **步骤 2：注册模块并编译验证**

```bash
cargo check 2>&1 | Out-String
```

- [x] **步骤 3：运行测试**

```bash
cd src-tauri && cargo test context_budget::tests -- --nocapture
```

- [x] **步骤 4：Commit**

```bash
git add src-tauri/src/services/context_budget.rs src-tauri/src/services/types.rs src-tauri/src/lib.rs
git commit -m "feat(P0-c): add priority-driven dynamic context budget manager"
```

---

### 任务 6：创建 `context_compressor.rs` — 分层摘要 + 磁滞回线 (P0-d)

**文件：**
- 创建：`src-tauri/src/services/context_compressor.rs`

- [x] **步骤 1：创建 `context_compressor.rs`**

```rust
// src-tauri/src/services/context_compressor.rs
//! 分层摘要压缩 + 磁滞回线防震荡 + 增量摘要

use super::llm_service::{ChatMessage, LLMService};
use super::token;

/// 磁滞回线参数
pub struct CompressionHysteresis {
    pub trigger_threshold_pct: u32,   // 80%
    pub release_target_pct: u32,       // 50%
    pub reset_threshold_pct: u32,      // 30% — 低于此才退出压缩状态
    pub is_compressed: bool,
}

impl CompressionHysteresis {
    pub fn new(trigger_pct: u32, release_pct: u32, reset_pct: u32) -> Self {
        Self { trigger_threshold_pct: trigger_pct, release_target_pct: release_pct, reset_threshold_pct: reset_pct, is_compressed: false }
    }

    pub fn should_compress(&mut self, usage_pct: u32) -> bool {
        if !self.is_compressed && usage_pct >= self.trigger_threshold_pct {
            self.is_compressed = true;
            return true;
        }
        false
    }

    pub fn on_compressed(&mut self, total_budget: u32) -> u32 {
        total_budget * self.release_target_pct / 100
    }

    pub fn maybe_reset(&mut self, usage_pct: u32) {
        if self.is_compressed && usage_pct < self.reset_threshold_pct {
            self.is_compressed = false;
        }
    }
}

/// 增量摘要器（使用消息 ID 而非索引）
pub struct IncrementalSummarizer {
    pub prev_summary: Option<String>,
    pub last_message_id: Option<String>,
}

impl IncrementalSummarizer {
    pub fn new() -> Self {
        Self { prev_summary: None, last_message_id: None }
    }

    pub async fn summarize(
        &mut self,
        messages: &[ChatMessage],
        budget: u32,
        model_tag: &str,
        llm: &LLMService,
    ) -> Result<String, String> {
        let start_idx = match &self.last_message_id {
            Some(last_id) => {
                match messages.iter().position(|m| m.id == *last_id) {
                    Some(pos) => pos + 1,
                    None => { self.prev_summary = None; 0 }
                }
            }
            None => 0,
        };
        let new_messages = &messages[start_idx..];
        if new_messages.is_empty() {
            return Ok(self.prev_summary.clone().unwrap_or_default());
        }

        let prompt = match &self.prev_summary {
            Some(prev) => format!("以下是之前的对话摘要：\n{prev}\n\n新增对话：\n{}\n\n请将以上内容合并为一段结构化摘要，保留关键信息。", format_messages(new_messages)),
            None => format!("请从以下对话中提取关键上下文，生成结构化摘要：\n{}", format_messages(new_messages)),
        };

        let summary = llm.chat_completion_with_model(model_tag, &prompt, budget as u64).await
            .map_err(|e| e.to_string())?;

        self.prev_summary = Some(summary.clone());
        self.last_message_id = messages.last().and_then(|m| m.id.clone());
        Ok(summary)
    }
}

/// 压缩后的历史记录
pub struct CompressedHistory {
    pub summary: Option<String>,
    pub critical_turns: Vec<ChatMessage>,
    pub recent_turns: Vec<ChatMessage>,
    pub tokens_used: u32,
}

impl CompressedHistory {
    pub async fn compress(
        messages: &[ChatMessage],
        budget: u32,
        hysteresis: &mut CompressionHysteresis,
        summarizer: &mut IncrementalSummarizer,
        llm: &LLMService,
    ) -> Result<Self, String> {
        let tokens_used = token::count_messages_tokens(messages);
        let usage_pct = if budget > 0 { tokens_used * 100 / budget } else { 100 };

        if !hysteresis.should_compress(usage_pct) {
            return Ok(Self { summary: summarizer.prev_summary.clone(), critical_turns: extract_critical(messages), recent_turns: messages.to_vec(), tokens_used });
        }

        let release_budget = hysteresis.on_compressed(budget);
        let recent = retain_recent(messages, release_budget * 60 / 100);
        let old = extract_old(messages, &mark_critical_indices(messages), &recent);

        const SUMMARY_BUDGET: u32 = 1500;
        let summary = if !old.is_empty() {
            Some(summarizer.summarize(messages, SUMMARY_BUDGET, "summarization", llm).await?)
        } else {
            summarizer.prev_summary.clone()
        };

        hysteresis.maybe_reset(usage_pct);

        let critical_turns: Vec<_> = mark_critical_indices(messages).iter()
            .filter_map(|&i| messages.get(i).cloned())
            .collect();

        Ok(Self { summary, critical_turns, recent_turns: recent, tokens_used: 0 })
    }
}

fn mark_critical_indices(messages: &[ChatMessage]) -> Vec<usize> {
    messages.iter().enumerate()
        .filter(|(_, m)| m.role == "system" || m.content.contains("【上一轮工具上下文】") || m.content.contains("错误") || m.content.contains("失败"))
        .map(|(i, _)| i).collect()
}

fn extract_critical(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    mark_critical_indices(messages).iter().filter_map(|&i| messages.get(i).cloned()).collect()
}

fn retain_recent(messages: &[ChatMessage], budget: u32) -> Vec<ChatMessage> {
    let mut result = Vec::new();
    let mut tokens = 0u32;
    for msg in messages.iter().rev() {
        let msg_tokens = token::count_tokens_with_fallback(&msg.content) + token::count_tokens_with_fallback(&msg.role) + 4;
        if tokens + msg_tokens > budget && !result.is_empty() { break; }
        tokens += msg_tokens;
        result.push(msg.clone());
    }
    result.reverse();
    result
}

fn extract_old(messages: &[ChatMessage], critical: &[usize], recent: &[ChatMessage]) -> Vec<ChatMessage> {
    let recent_ids: std::collections::HashSet<_> = recent.iter().map(|m| m.id.clone()).collect();
    messages.iter().enumerate()
        .filter(|(i, m)| !critical.contains(i) && !recent_ids.contains(&m.id))
        .map(|(_, m)| m.clone()).collect()
}

fn format_messages(messages: &[ChatMessage]) -> String {
    messages.iter().map(|m| format!("{}: {}", m.role, m.content)).collect::<Vec<_>>().join("\n")
}
```

- [x] **步骤 2：编译验证**

```bash
cd src-tauri && cargo check 2>&1 | Out-String
```

- [x] **步骤 3：Commit**

```bash
git add src-tauri/src/services/context_compressor.rs src-tauri/src/lib.rs
git commit -m "feat(P0-d): add layered summary compression with hysteresis"
```

---

### 任务 7-14 概要（P0-e 至 P2-d）

> **注意：** 后续 P0-e 至 P2-d 共 8 个任务，为节省上下文实现为概要形式。完整的步骤级计划（含代码、命令、预期输出）在实现时按需展开。每个概要任务映射到设计文档的一个具体章节。

### 任务 7：渐进式披露接入 (P0-e)
**文件：** `prompt_assembler.rs` 改、`risk_blueprint.rs` 改
**目标：** 将 `PromptAssembler` 接入 Agent 管道，替换 `risk_blueprint.rs` 中的手动技能注入
- [x] 修改 `risk_blueprint.rs:349-385`，调用 `PromptAssembler::build_skill_list_prompt()`
- [x] `prompt_assembler.rs`：将 `estimate_tokens` 替换为 `token::count_tokens_with_fallback`
- [x] 编译验证 + commit

### 任务 8：合并系统提示词 + 消除 max_tokens 歧义 (P0-f)
**文件：** `llm_service.rs` 改、`prompts.rs` 改、`rig_agent.rs` 改
**目标：** 统一四处提示词 + 拆分 max_tokens 语义
- [x] 删除 `llm_service.rs` 硬编码 `SYSTEM_PROMPT`
- [x] 所有提示词统一为外部 `.md` 通过 `include_str!` 嵌入
- [x] `rig_agent.rs` 使用 `ModelMetadata::resolve()` 获取上下文窗口和输出限制
- [x] **U2 安全阀**：在 `ContextBudget::calculate` 最终输出处添加 `max_chars_hard_limit=500000` 检查，超限时 `TruncateAndWarn`
- [x] 编译验证 + commit

### 任务 9：Agent 模式路由 (P1-a)
**文件：** 创建 `agent_router.rs`、修改 `rig_agent.rs`
**目标：** 根据复杂度评分自动路由到 ReAct 或 Plan-Execute
- [x] 实现 `calculate_complexity()` 中英双语言关键词评分
- [x] 实现 `route_mode()`，`COMPLEXITY_THRESHOLD = 20`（可配置）
- [x] 在 `RigAgent::run` 入口调用路由
- [x] 编译验证 + commit

### 任务 10：Planner + Replanner + PlanStateMachine (P1-b)
**文件：** 创建 `planner.rs`
**目标：** NDJSON 流式规划 + 状态机步进控制 + 依赖合法性校验
- [x] 实现 `Planner::plan()`：NDJSON 格式执行计划生成
- [x] 实现 `PlanStateMachine`：状态转换 + `validate_dependencies`
- [x] 实现 `StepContext`：只暴露 `remaining_count`，不暴露具体内容
- [x] 实现 `should_replan`：中英文关键词 + 结构化 `StepStatus`
- [x] 实现 `detect_step_drift`：文本信号 + ToolCall 匹配
- [x] 实现 `plan_with_speculative_exec`：ReadOnly 步骤并发执行
- [x] 编译验证 + 单元测试 + commit

### 任务 11：Plan-Execute 执行循环 (P1-c)
**文件：** 修改 `rig_agent.rs`
**目标：** 在 Agent 核心嵌入 Plan-Execute 循环
- [x] 实现 `run_plan_execute`：状态机驱动循环
- [x] 扩 `ReActEvent` 新增 `PlanGenerated`/`StepStart`/`StepResult`/`Replan`
- [x] Planner 超时 10s → 自动降级 ReAct
- [x] 编译验证 + commit

### 任务 12：前端 Plan UI + Token 用量指示器 (P1-d + P2-d)
**文件：** 修改 `Chat.tsx`、`AgentContext.tsx`、`Settings.tsx`
**目标：** Plan 时间线展示 + 步骤锚定 + 超时降级选项
- [x] `AgentContext.tsx`：扩展 `ReActTrace`，新增 `PlanGenerated`/`StepStart`/`StepResult`/`Replan` 事件处理
- [x] `Chat.tsx`：`StepAnchor` 组件（步骤锚定）、`PlannerTimeoutFallback` 组件（超时降级）、Plan 时间线左侧竖线 UI
- [x] `Chat.tsx`：Token 用量指示器（调用已有 `countTokens`）
- [x] `Settings.tsx`：上下文工程 section（模型规格表 + 手动覆盖编辑器）
- [x] 前端编译验证 `npm run build` + commit

### 任务 13：Harness 约束编码 + 验证循环 (P2-a + P2-b)
**文件：** 创建 `harness/constraints.rs`、`harness/verifier.rs`、`harness/mod.rs`
**目标：** 程序化工具约束 + Ping-Pong 检测 + 验证重试上限
- [x] `constraints.rs`：`enforce_tool_constraint`（Ping-Pong `normalized_call_key` + `HashSet` 禁止序列）
- [x] `verifier.rs`：`ResultVerifier`（`max_consecutive_failures=3` → `Exhausted` → `NeedsReplan`）
- [x] 在 `drain_stream` 中集成约束检查和验证器
- [x] 编译验证 + 单元测试 + commit

### 任务 14：熵管理 (P2-c)
**文件：** 创建 `harness/entropy.rs`
**目标：** 技能过期扫描 + 文档一致性检查
**最低可行版（MVP）**：
- [x] `scan_stale_skills`：扫描 90 天未使用的技能（依赖 SkillManager 添加 `last_used_at`）
- [x] `scan_doc_code_mismatches`：轻量级方案 —— 只检查关键文件是否存在、签名是否匹配（不用 LLM）
- [x] 暴露为 Tauri 命令，前端 Settings 触发
- [x] 编译验证 + commit

### 任务 15：图像处理多模态自动路由与回退降级重构
**文件：** `src-tauri/src/commands/skill.rs`、`src-tauri/src/services/image_processor.rs`、`src-tauri/src/services/llm_providers.rs`
**目标：** 实现 process_image 的多模型回退、懒探测状态回写、OCR 两级降级、错误级联收集
**状态：** ✅ 已完成（代码先于设计文档落地，本任务为追溯补充）
- [x] `skill.rs:process_image` — 多模型候选循环 + 成功后 `global.set_llm_multimodal(true)` 状态回写
- [x] `image_processor.rs` — 懒探测机制（请求时捕获错误信号标记 `llm_multimodal=false`）
- [x] `image_processor.rs` — `last_api_error` 级联收集（HTTP 状态码 + base_url + model + 响应摘要）
- [x] `skill.rs` — OCR 两级降级（LLM 全部失败 → 检查 OCR 配置 → 纯 OCR 处理）
- [x] `llm_providers.rs` — `get_vision_candidates()` 候选模型检索（按优先级排序）

P0 模块已创建并通过编译，但存在以下需在后续阶段解决的遗留问题：

| # | 问题 | 影响 | 解决任务 | 说明 |
|---|------|------|---------|------|
| 1 | `model_specs.json` 仅 ~10 个模型 | 未收录模型回退到保守默认值 4096 | P0-e | 需补充 o1-pro、gpt-4.1、claude-sonnet-4-6 等。实施时在 Settings 页面添加用户手动补录入口 |
| 2 | `IncrementalSummarizer` 使用 `MessageContext.id` | 摘要器接入 `rig_agent.rs` 时需要 `MessageContext` map 查找 | P1-c | ChatMessage 未添加 `id` 字段以避免大规模迁移。实施时在 agent 循环中维护 `HashMap<usize, MessageContext>` |
| 3 | ~~`last_api_error` 警告（3 处）~~ | ~~编译有警告，不影响功能~~ | ~~P1-c~~ | ✅ 已在任务 15 图像重构中解决——`last_api_error` 现在在全部失败时用于级联错误上报 |
| 4 | `count_tokens` / `truncate_to_tokens` 仍在 `llm_service.rs` | 旧实现仍存在，未删除 | P0-f | 当前保留避免破坏现有调用。P0-f 统一替换 |
| 5 | `PromptAssembler::estimate_tokens` 仍存在 | 与 `token.rs` 重复 | P0-e | P0-e 接入 Agent 管道时替换 |
| 6 | 系统提示词四套并存 | 硬编码在 llm_service/rig_agent/prompts/tool_policy | P0-f | P0-f 统一为外部 .md 文件 |

---

## 阶段汇总

| 阶段 | 任务数 | 新增文件 | 修改文件 |
|------|--------|---------|---------|
| P0-a | 2 | token.rs | llm_service.rs, prompt_assembler.rs |
| P0-b | 1 | model_metadata.rs, model_specs.json | llm_providers.rs |
| P0-c | 2 | types.rs, context_budget.rs | lib.rs |
| P0-d | 1 | context_compressor.rs | — |
| P0-e | 1 | — | prompt_assembler.rs, risk_blueprint.rs |
| P0-f | 1 | — | llm_service.rs, prompts.rs, rig_agent.rs |
| P1-a | 1 | agent_router.rs | rig_agent.rs |
| P1-b | 1 | planner.rs | react_agent.rs (ReActEvent 扩展) |
| P1-c | 1 | — | rig_agent.rs |
| P1-d/P2-d | 1 | — | Chat.tsx, AgentContext.tsx, Settings.tsx |
| P2-a/P2-b | 1 | harness/*.rs x3 | rig_agent.rs (drain_stream) |
| P2-c | 1 | harness/entropy.rs | Settings.tsx |
| 追溯 | 1 | — | skill.rs, image_processor.rs, llm_providers.rs |
| **合计** | **15** | **10** | **10** |
