# 三层 AI 工程范式落地设计

> Context Engineering + Plan-and-Execute + Harness Engineering
>
> 日期：2026-05-31
>
> **更新（2026-06-13）**: 本设计稿中规划的 `context_budget.rs` 模块在创建后未投入使用，已于 2026-06-13 清理。其余模块（`token.rs` / `model_metadata.rs` / `context_compressor.rs` / `agent_router.rs` 等）继续使用。

## 1. 背景与动机

当前项目存在以下架构问题：

1. **上下文管理割裂** — RAG 管道和 Agent 管道有各自独立的截断/压缩逻辑，互不共享
2. **纯 ReAct 模式** — Agent "边想边做"，缺乏全局规划，复杂任务容易走偏或陷入死循环
3. **约束靠 prompt** — Agent 行为约束完全依赖 system prompt 文本，无法程序化强制执行
4. **Token 估算不一致** — `count_tokens`（tiktoken）、`PromptAssembler::estimate_tokens`（启发式）、字符截断三种标准并存
5. **max_tokens 语义歧义** — 配置字段同时被当作"上下文窗口"和"输出 token 限制"使用
6. **模型能力无感知** — 系统不知道当前模型的上下文窗口、最大输出、是否支持 thinking

参考资料描述了 AI 工程的三次范式跃迁：

| 范式 | 优化对象 | 解决问题 |
|------|----------|----------|
| Prompt Engineering | 输入措辞 | 单次对话质量 |
| Context Engineering | 信息输入 | 知识边界与幻觉 |
| Harness Engineering | 运行环境 | Agent 可靠性与可持续性 |

本次改造将三层范式统一落地，分阶段实现。

---

## 2. 总体架构

```
┌───────────────────────────────────────────────────────────┐
│                  Harness Engineering 层                    │
│  ┌─────────────┐  ┌─────────────┐  ┌──────────────────┐  │
│  │ 架构约束     │  │ 反馈循环     │  │ 熵管理            │  │
│  │ ·工具调用限制│  │ ·结果验证    │  │ ·技能过期清理     │  │
│  │ ·输出格式约束│  │ ·质量评分    │  │ ·文档-代码一致性  │  │
│  │ ·禁止循环查询│  │ ·重规划触发  │  │ ·向量化索引刷新   │  │
│  └─────────────┘  └─────────────┘  └──────────────────┘  │
│                                                           │
│  ┌───────────────────────────────────────────────────────┐ │
│  │          Plan-and-Execute 层                            │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐            │ │
│  │  │ Planner  │  │ Executor │  │ Replanner│            │ │
│  │  │ ·任务分解 │→│ ·逐步执行 │→│ ·偏差检测│            │ │
│  │  │ ·技能匹配 │  │ ·工具调用 │  │ ·重规划  │            │ │
│  │  └──────────┘  └──────────┘  └──────────┘            │ │
│  │  自适应模式切换：简单→ReAct，复杂→Plan-Execute         │ │
│  └───────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌───────────────────────────────────────────────────────┐ │
│  │          Context Engineering 层                        │ │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌─────────┐ │ │
│  │  │Token预算  │ │模型元数据│ │分层摘要  │ │渐进披露 │ │ │
│  │  │ ·精确计算 │ │ ·窗口感知│ │ ·关键轮保留│ │ ·按需注入│ │ │
│  │  │ ·动态分配 │ │ ·能力探测│ │ ·LLM摘要 │ │ ·技能延迟│ │ │
│  │  └──────────┘ └──────────┘ └──────────┘ └─────────┘ │ │
│  └───────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Context Engineering 层（P0 — 最先实现）

### 3.1 统一 Token 计数

**现状问题**：
- `llm_service::count_tokens` 使用 tiktoken cl100k_base
- `PromptAssembler::estimate_tokens` 使用独立的启发式估算
- `build_prompt_with_history` 使用字符数截断（12K 字符）
- 三套标准并存，结果不一致

**改造方案**：

1. 删除 `PromptAssembler::estimate_tokens`，所有 token 计算统一使用 `llm_service::count_tokens`
2. 将 `count_tokens` 提取为独立的 `token.rs` 模块，不再挂在 `LLMService` 上
3. 修改 `build_prompt_with_history` 使用 token 预算替代字符截断
4. 对消息结构开销（role 标记、分隔符）添加固定修正值（每条消息 +4 tokens）

```rust
// 新文件: src-tauri/src/services/token.rs

/// Token 计数错误
#[derive(Debug)]
pub enum TokenError {
    TiktokenInitFailed(String),
}

/// 全局 token 计数（基于 tiktoken cl100k_base）
/// ★ 审查修正：tiktoken 初始化失败返回 Result，不再静默降级
pub fn count_tokens(text: &str) -> Result<u32, TokenError> {
    tiktoken_rs::cl100k_base()
        .map(|b| b.encode_with_special_tokens(text).len() as u32)
        .map_err(|e| TokenError::TiktokenInitFailed(e.to_string()))
}

/// 带回退的 token 计数（用于非关键路径）
/// ★ 审查修正：回退公式区分中英文比例，提高精度
pub fn count_tokens_with_fallback(text: &str) -> u32 {
    count_tokens(text).unwrap_or_else(|_| {
        // 统计中文字符和 ASCII 字符比例
        let chinese_chars = text.chars().filter(|c| !c.is_ascii()).count();
        let ascii_chars = text.len() - chinese_chars;
        // 中文约 1.5 字符/token，英文约 4 字符/token（复用 prompt_assembler 的算法）
        (chinese_chars as f32 / 1.5 + ascii_chars as f32 / 4.0) as u32
    })
}

/// 计算消息数组的 token 总量（含结构开销）
/// ★ 优化：优先使用消息上的 token_count 缓存，避免重复分词
/// ★ 复审修正：缓存 miss 时优先使用精确计数，失败才用回退版本
pub fn count_messages_tokens(messages: &[ChatMessage]) -> u32 {
    messages.iter().map(|m| {
        let content_tokens = match m.token_count {
            Some(cached) => cached,
            None => count_tokens(&m.content)
                .unwrap_or_else(|_| count_tokens_with_fallback(&m.content)),
        };
        content_tokens + count_tokens_with_fallback(&m.role) + 4 // +4 结构开销
    }).sum()
}

/// Token 级截断（二分查找，UTF-8 安全）
pub fn truncate_to_tokens(text: &str, max_tokens: u32) -> String {
    // 迁移自 llm_service.rs:461-483
}
```

**Token 缓存设计**：在 `ChatMessage` 中引入 `token_count: Option<u32>` 缓存字段。消息创建时一次性计算并保存，后续 `count_messages_tokens` 遍历时直接读取缓存，避免对历史消息重复调用分词器。

```rust
// llm_service.rs — ChatMessage 扩展
pub struct ChatMessage {
    /// ★ 复审修正：消息唯一标识符，用于增量摘要的位置追踪
    pub id: String,
    pub role: String,
    pub content: String,
    /// ★ 审查优化：token 计数缓存，消息写入时一次性计算
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_count: Option<u32>,
}

impl ChatMessage {
    /// 创建消息并自动计算 token 缓存
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(), // 生成唯一 ID
            role: role.to_string(),
            content: content.to_string(),
            token_count: Some(token::count_tokens_with_fallback(content)),
        }
    }

    /// ★ 审查修正：修改内容时自动失效缓存
    pub fn set_content(&mut self, content: String) {
        self.token_count = None; // 失效缓存
        self.content = content;
    }

    /// ★ 审查修正：懒计算 + 返回缓存
    /// 如果缓存存在则直接返回，否则计算并缓存
    pub fn token_count(&self) -> u32 {
        self.token_count.unwrap_or_else(|| token::count_tokens_with_fallback(&self.content))
    }

    /// ★ 审查修正：批量补算（反序列化后调用）
    /// 解决旧消息反序列化后 token_count 为 None 的问题
    pub fn compute_token_count(&mut self) {
        if self.token_count.is_none() {
            self.token_count = Some(token::count_tokens_with_fallback(&self.content));
        }
    }
}
```

### 3.2 模型元数据自动获取

**现状问题**：
- `LLMProviderConfig.max_tokens` 语义混乱（同时被当作上下文窗口和输出限制）
- 不知道模型的实际上下文窗口大小
- 不知道模型是否支持 thinking/reasoning

**改造方案**：分层获取策略

```rust
// 新文件: src-tauri/src/services/model_metadata.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub context_window: u32,       // 最大输入 tokens
    pub max_output_tokens: u32,    // 最大输出 tokens
    pub supports_thinking: bool,   // 是否支持 thinking/reasoning
    pub supports_vision: bool,     // 是否支持图像输入
    pub supports_tools: bool,      // 是否支持 function calling
}

impl ModelMetadata {
    /// 分层获取模型元数据
    pub async fn resolve(provider: &LLMProviderConfig, model_name: &str) -> Self {
        // 优先级 1: 提供商原生 API
        if let Some(meta) = Self::from_provider_api(provider, model_name).await {
            return meta;
        }
        // 优先级 2: 内置模型数据库（编译时嵌入）
        if let Some(meta) = Self::from_builtin_db(model_name) {
            return meta;
        }
        // 优先级 3: 用户自定义覆盖
        if let Some(meta) = Self::from_user_override(provider, model_name) {
            return meta;
        }
        // 优先级 4: 保守默认值
        Self::fallback(model_name)
    }
}
```

**各提供商获取方式**：

| 提供商 | 获取方式 | 返回字段 |
|--------|----------|----------|
| Anthropic | `GET /v1/models/{id}` | `max_input_tokens`, `max_tokens`, `capabilities.thinking` |
| Google Gemini | `GET /v1beta/models/{id}` | `inputTokenLimit`, `outputTokenLimit`, `thinking` |
| Ollama | `POST /api/show` | `model_info.{arch}.context_length`, `capabilities` |
| OpenAI | 无 API | 内置数据库 + 用户覆盖 |
| DeepSeek | 无 API | 内置数据库 + 用户覆盖 |
| 其他兼容 API | 无标准 | 内置数据库 + 用户覆盖 |

**内置模型数据库**：

编译时嵌入的 JSON 文件，覆盖主流模型系列：

```json
// src-tauri/resources/model_specs.json
{
  "openai": {
    "gpt-4o": { "context_window": 128000, "max_output_tokens": 16384, "supports_thinking": false, "supports_vision": true, "supports_tools": true },
    "gpt-5": { "context_window": 256000, "max_output_tokens": 32768, "supports_thinking": true, "supports_vision": true, "supports_tools": true },
    "o3": { "context_window": 200000, "max_output_tokens": 100000, "supports_thinking": true, "supports_vision": true, "supports_tools": true },
    "o4-mini": { "context_window": 200000, "max_output_tokens": 100000, "supports_thinking": true, "supports_vision": true, "supports_tools": true }
  },
  "deepseek": {
    "deepseek-v4-pro": { "context_window": 1000000, "max_output_tokens": 384000, "supports_thinking": true, "supports_vision": false, "supports_tools": true },
    "deepseek-v4-flash": { "context_window": 1000000, "max_output_tokens": 384000, "supports_thinking": true, "supports_vision": false, "supports_tools": true }
  },
  "anthropic": {
    "claude-opus-4-7": { "context_window": 200000, "max_output_tokens": 128000, "supports_thinking": true, "supports_vision": true, "supports_tools": true },
    "claude-sonnet-4-5": { "context_window": 200000, "max_output_tokens": 64000, "supports_thinking": true, "supports_vision": true, "supports_tools": true }
  }
}
```

**用户自定义覆盖**：在 Settings 页面添加"模型规格"编辑功能，允许用户对未收录的模型手动输入 `context_window` 和 `max_output_tokens`。数据存储在 `ModelConfig` 的新字段中：

```rust
// llm_providers.rs — ModelConfig 新增字段
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    #[serde(default)]
    pub is_multimodal: Option<bool>,
    // ─── 新增字段 ───
    #[serde(default)]
    pub context_window: Option<u32>,     // 用户手动覆盖
    #[serde(default)]
    pub max_output_tokens: Option<u32>,   // 用户手动覆盖
    #[serde(default)]
    pub supports_thinking: Option<bool>,  // 用户手动覆盖
}
```

### 3.3 上下文预算管理器

**现状问题**：
- RAG 管道有预算计算（`budget = max_tokens - system_tokens - RESPONSE_TOKENS - 200`），但 Agent 管道没有
- 两条管道各自截断，不协调

**改造方案**：统一预算管理器

```rust
// 新文件: src-tauri/src/services/context_budget.rs

/// 预算槽优先级（数值越小越先分配，同优先级按比例）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BudgetPriority {
    SystemPrompt = 0,   // 必须完整，不可截断
    UserInput     = 1,   // 当前用户输入，不可截断
    ReservedOutput = 2,  // 模型输出预留
    ToolDefs      = 3,   // 工具定义（高优先级但可压缩）
    Plan          = 4,   // ★ 审查修正：执行计划（优先级高于 History，不可摘要压缩）
    History       = 5,   // 对话历史（可摘要压缩）
    RetrievedCtx  = 6,   // 检索上下文（RAG/技能，可截断）
    Buffer        = 7,   // 安全缓冲
}

/// 预算需求声明
struct BudgetClaim {
    slot: BudgetPriority,
    min_tokens: u32,     // 最低需求
    ideal_tokens: u32,   // 理想需求
    mode_mask: AgentMode, // 哪些模式需要此槽
}

/// 上下文预算分配结果
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
    /// ★ 审查优化：优先级驱动的动态贪婪分配
    /// 按优先级从高到低分配，每个槽先满足 min，剩余空间按比例分给 ideal
    pub fn calculate(metadata: &ModelMetadata, mode: AgentMode) -> Self {
        let total = metadata.context_window;
        // ★ 审查修正：reserved_output 直接使用 metadata.max_output_tokens，不再与 total/4 取 min
        // 对于长输出模型（如 Claude 128K、DeepSeek 384K），total/4 会严重限制输出能力
        // 上下文预算的逻辑应该是：先满足 max_output_tokens，再按优先级分配剩余给各输入槽
        let reserved_output = metadata.max_output_tokens;

        // 声明各槽的需求（按模式区分）
        let claims = Self::build_claims(total, reserved_output, mode);

        // 第一轮：按优先级从高到低，先满足每个槽的 min
        let mut remaining = total;
        let mut min_alloc = HashMap::new();
        for claim in &claims {
            if !claim.mode_mask.contains(mode) { continue; }
            let alloc = claim.min_tokens.min(remaining);
            min_alloc.insert(claim.slot, alloc);
            remaining -= alloc;
        }

        // 第二轮：剩余空间按 ideal 比例贪婪分配
        let total_ideal: u32 = claims.iter()
            .filter(|c| c.mode_mask.contains(mode))
            .map(|c| c.ideal_tokens.saturating_sub(min_alloc[&c.slot]))
            .sum();

        let mut final_alloc = min_alloc.clone();
        if total_ideal > 0 {
            for claim in &claims {
                if !claim.mode_mask.contains(mode) { continue; }
                let deficit = claim.ideal_tokens.saturating_sub(min_alloc[&claim.slot]);
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
        let has_plan = mode == AgentMode::PlanExecute;
        let has_tools = mode != AgentMode::RagChat;
        // ★ 复审修正：严格按 BudgetPriority 从高到低排列（数值越小越优先）
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

bitflags! {
    pub struct AgentMode: u32 {
        const RagChat     = 0b001;
        const ReAct       = 0b010;
        const PlanExecute = 0b100;
    }
}
```

### 3.4 分层摘要压缩

**现状问题**：
- `compress_conversation` 阈值过低（2000 tokens），3-4 轮对话就触发
- `build_prompt_with_history` 直接按字符截断，丢弃旧消息无摘要
- 两条管道互不感知

**改造方案**：统一混合压缩策略，含增量摘要与磁滞防震荡

```rust
// 新文件: src-tauri/src/services/context_compressor.rs

/// 对话历史压缩结果
pub struct CompressedHistory {
    /// 结构化摘要（替代旧消息）
    pub summary: Option<String>,
    /// 保留的关键轮次（永不丢弃）
    pub critical_turns: Vec<ChatMessage>,
    /// 最近的完整对话
    pub recent_turns: Vec<ChatMessage>,
    /// 消耗的 token 总量
    pub tokens_used: u32,
}

/// ★ 审查优化：磁滞回线参数
struct CompressionHysteresis {
    /// 触发压缩的使用率上限（如 80%）
    trigger_threshold_pct: u32,
    /// 压缩释放到的目标使用率（如 50%）
    release_target_pct: u32,
    /// ★ 审查修正：独立的复位阈值（如 30%），低于此值才退出压缩状态
    /// 避免释放到 50% 后立即复位，导致 1-2 轮后又触发压缩
    reset_threshold_pct: u32,
    /// 当前是否处于"已压缩"状态（防止边界震荡）
    is_compressed: bool,
}

impl CompressionHysteresis {
    fn new(trigger_pct: u32, release_pct: u32, reset_pct: u32) -> Self {
        Self {
            trigger_threshold_pct: trigger_pct,
            release_target_pct: release_pct,
            reset_threshold_pct: reset_pct,
            is_compressed: false,
        }
    }

    /// 判断是否需要触发压缩
    fn should_compress(&mut self, usage_pct: u32) -> bool {
        if !self.is_compressed && usage_pct >= self.trigger_threshold_pct {
            self.is_compressed = true;
            return true;
        }
        false
    }

    /// 压缩完成后更新状态，返回释放到的目标 token 预算
    fn on_compressed(&mut self, total_budget: u32) -> u32 {
        total_budget * self.release_target_pct / 100
    }

    /// 当使用率自然降到复位阈值以下时，重置状态
    fn maybe_reset(&mut self, usage_pct: u32) {
        if self.is_compressed && usage_pct < self.reset_threshold_pct {
            self.is_compressed = false;
        }
    }
}

/// ★ 审查优化：增量摘要
/// 不再每次将全量旧消息发给 LLM，而是保留上一代摘要，只将新增消息与旧摘要合并
/// ★ 审查修正：使用消息 ID 而非索引，避免用户删除中间消息时的位置偏移问题
struct IncrementalSummarizer {
    /// 上一代摘要文本
    prev_summary: Option<String>,
    /// 上一代摘要覆盖的最后一条消息 ID（使用 Option<String> 而非索引）
    last_message_id: Option<String>,
}

impl IncrementalSummarizer {
    /// ★ 复审修正：添加 model_tag 参数，指定使用的摘要模型
    async fn summarize(
        &mut self,
        messages: &[ChatMessage],
        budget: u32,
        model_tag: &str,  // "summarization" 或具体模型名
        llm: &LLMService,
    ) -> Result<String, String> {
        // ★ 审查修正：通过消息 ID 查找新增消息的起始位置
        let start_idx = match &self.last_message_id {
            Some(last_id) => {
                // 查找 last_id 在消息列表中的位置
                match messages.iter().position(|m| m.id.as_deref() == Some(last_id.as_str())) {
                    Some(pos) => pos + 1, // 从 last_id 之后开始
                    None => {
                        // 未找到（可能消息被删除），重置摘要器
                        tracing::warn!(
                            "IncrementalSummarizer: 未找到消息 ID={}，重置摘要器",
                            last_id
                        );
                        self.prev_summary = None;
                        0
                    }
                }
            }
            None => 0, // 首次摘要，从头开始
        };
        let new_messages = &messages[start_idx..];

        // 如果没有新增消息，直接返回上一代摘要
        if new_messages.is_empty() {
            return Ok(self.prev_summary.clone().unwrap_or_default());
        }

        let prompt = match &self.prev_summary {
            Some(prev) => format!(
                "以下是之前的对话摘要：\n{prev}\n\n\
                 新增对话：\n{}\n\n\
                 请将以上内容合并为一段结构化摘要，保留关键信息（用户目标、已执行操作、输出结果、遇到错误）。",
                Self::format_messages(new_messages)
            ),
            None => format!(
                "请从以下对话中提取关键上下文，生成结构化摘要：\n\
                 包含：用户目标、Agent做了什么、产生了什么输出、遇到了什么错误。\n\n{}",
                Self::format_messages(new_messages)
            ),
        };

        // ★ 复审修正：使用传入的 model_tag 参数
        let summary = llm.chat_completion_with_model(model_tag, &prompt, budget).await?;

        // 更新增量摘要状态
        self.prev_summary = Some(summary.clone());
        // 记录最后一条消息的 ID
        self.last_message_id = messages.last().and_then(|m| m.id.clone());

        Ok(summary)
    }
}

impl CompressedHistory {
    /// 混合压缩策略（含磁滞防震荡 + 增量摘要）
    pub async fn compress(
        messages: &[ChatMessage],
        budget: u32,
        hysteresis: &mut CompressionHysteresis,
        summarizer: &mut IncrementalSummarizer,
        llm: &LLMService,
    ) -> Result<Self, String> {
        let tokens_used = token::count_messages_tokens(messages);
        let usage_pct = if budget > 0 { tokens_used * 100 / budget } else { 100 };

        // ★ 磁滞回线判断：只有超过 trigger 阈值才触发压缩
        if !hysteresis.should_compress(usage_pct) {
            // 不需要压缩，直接返回
            return Ok(Self {
                summary: summarizer.prev_summary.clone(),
                critical_turns: Self::extract_critical(messages),
                recent_turns: messages.to_vec(),
                tokens_used,
            });
        }

        // 1. 标记关键轮次（system 指令、含工具结果、错误修正轮）
        let critical_indices = Self::mark_critical_turns(messages);

        // 2. 从尾部保留最近消息，直到目标预算的 60%
        let release_budget = hysteresis.on_compressed(budget);
        let recent = Self::retain_recent(messages, release_budget * 60 / 100);

        // 3. 剩余旧消息 = 全量 - 关键轮 - 最近轮
        let old = Self::extract_old(messages, &critical_indices, &recent);

        // 4. ★ 增量摘要：只将新增消息与上一代摘要合并
        // ★ 审查修正：使用绝对值预算（500-2000 tokens），而非百分比
        // 对于 1M 窗口的 DeepSeek V4，百分比会导致 15 万 tokens 给摘要，成本过高
        const SUMMARY_BUDGET: u32 = 1500; // 摘要预算固定 1500 tokens
        
        // ★ 复审修正：使用专用摘要模型，避免调用主模型导致成本翻倍
        // 配置项：summarization_model，默认使用轻量模型（如 gpt-4o-mini 或本地 Ollama）
        let summary = if !old.is_empty() {
            Some(summarizer.summarize(messages, SUMMARY_BUDGET, "summarization", llm).await?)
        } else {
            summarizer.prev_summary.clone()
        };

        // 5. 重置磁滞状态
        hysteresis.maybe_reset(usage_pct);

        // 6. 组装最终结果
        let critical_turns: Vec<_> = critical_indices.iter()
            .filter_map(|&i| messages.get(i).cloned())
            .collect();

        Ok(Self {
            summary,
            critical_turns,
            recent_turns: recent,
            tokens_used: 0, // 调用者填充
        })
    }

    /// 标记关键轮次
    fn mark_critical_turns(messages: &[ChatMessage]) -> Vec<usize> {
        messages.iter().enumerate()
            .filter(|(_, m)| {
                m.role == "system"
                || m.content.contains("【上一轮工具上下文】")
                || m.content.contains("错误") || m.content.contains("失败")
            })
            .map(|(i, _)| i)
            .collect()
    }
}
```

**磁滞回线工作示意**：

```
使用率 0% ───────────────────── 80% ─────→ 触发压缩
                                              │
                                              ▼
                              释放到 50% ←────┘
                                              │
                               后续几轮不会
                               再次触发压缩
                               （因为 < 80%）
                              
                               直到又涨到 80%
                               才再次触发
```

**压缩阈值调整**：从固定 2000 tokens 改为基于预算的动态阈值（默认 80%），压缩时一次性释放到 50%，保证后续多轮对话不会频繁触发压缩。

### 3.5 渐进式披露

**现状问题**：
- `PromptAssembler` 有完整的渐进式披露实现，但未被 Agent 管道使用
- Agent 管道的技能注入在 `risk_blueprint.rs` 中独立实现，直接拼接全部技能列表

**改造方案**：

1. 将 `PromptAssembler` 接入 `RigAgent::run` 的系统提示词构建流程，替换 `risk_blueprint.rs` 中的手动技能注入
2. 技能列表按 token 预算动态注入——只有匹配当前任务的技能以完整模式注入，其余以压缩模式或省略
3. 技能详情在 Agent 通过 `use-skill` 工具调用时才按需加载（当前已实现，无需改动）

### 3.6 合并系统提示词

**现状问题**：
- `llm_service.rs` 有硬编码的 `SYSTEM_PROMPT`
- `prompts.rs` 通过 `include_str!` 导入 `system_prompt.md`
- Agent 管道的系统提示词硬编码在 `rig_agent.rs:171-264`
- `tool_policy::agent_tool_policy_prompt()` 动态生成的工具策略提示词
- ★ 审查修正：实际是**四套**系统提示词并存，不是三套

**改造方案**：

1. 所有系统提示词统一为外部 `.md` 文件，通过 `include_str!` 嵌入
2. 删除 `llm_service.rs` 中的硬编码 `SYSTEM_PROMPT`
3. Agent 系统提示词拆分为基础模板 + 动态片段（技能清单、项目上下文、工具策略）
4. 动态片段通过 `ContextBudget` 控制各部分的 token 预算
5. ★ 审查修正：`tool_policy` 作为动态片段之一，保持其动态生成特性但统一注入点

### 3.7 消除 max_tokens 歧义

**现状问题**：
- `LLMProviderConfig.max_tokens` 被同时用于上下文窗口大小和输出 token 限制

**改造方案**：

1. `ModelConfig` 新增 `context_window` 和 `max_output_tokens` 字段（用户可手动覆盖）
2. `LLMProviderConfig.max_tokens` 保留用于向后兼容，但新代码不再读取它
3. 实际使用的值来源于 `ModelMetadata::resolve()` 的分层获取结果
4. 前端 Settings 页面展示"上下文窗口"和"最大输出"两个独立字段

### 3.8 图像多模态自动路由与回退降级

**现状问题**：
- `process_image` 命令原本只使用单一模型处理图像，缺乏回退机制
- 多模态能力探测与实际请求分离，导致每次调用都重复探测开销

**架构设计**：

```
process_image 调用
    │
    ├─ 1. get_vision_candidates() → 获取所有支持多模态的模型候选列表（按优先级排序）
    │     └─ 基于 LLMProviders 中 supports_vision 或未显式拒绝多模态的配置
    │
    ├─ 2. 逐个尝试候选模型（for loop）
    │     ├─ 成功 → set_llm_multimodal(true) 同步全局状态 → 返回结果
    │     └─ 失败 → 记录 last_api_error → 继续下一个模型
    │
    ├─ 3. 所有 LLM 模型失败 → 检查 OCR 配置
    │     ├─ 有 OCR → 创建纯 OCR 处理器 → 返回 OCR 结果
    │     └─ 无 OCR → 进入步骤 4
    │
    └─ 4. 全部失败 → 返回级联错误信息（包含每个失败模型的具体原因）
```

**关键机制**：

1. **懒探测（Lazy Probing）**：不再在 `vision()` 开始时单独探测多模态能力。而是在实际请求抛错时，捕获 HTTP 400/422 错误或响应体中的 `"image"`/`"vision"` 信号词，标记 `llm_multimodal = false` 并中止后续请求。探测结果缓存到 `AtomicBool`，避免重复开销。

2. **全局状态同步**：某模型成功处理图像后，调用 `global.set_llm_multimodal(true)` 将状态同步回全局 `AppState`。后续调用可跳过探测直接使用已知可行的模型。

3. **错误级联收集**：`last_api_error` 累加记录每次失败的具体信息（HTTP 状态码 + base_url + model + 响应摘要），全部失败时合并为可追溯的错误链。

4. **两级降级**：
   - **第一级（Degradable）**：多模态模型列表自动轮询回退 → 从高优先级模型逐个降级到低优先级
   - **第二级（Degradable）**：所有 LLM 模型失败后回退到纯 OCR 处理（如果配置了 OCR）
   - **Reportable**：OCR 也失败 → 上报给用户

**涉及文件**：
- `src-tauri/src/commands/skill.rs` — `process_image` 命令实现多模型回退循环
- `src-tauri/src/services/image_processor.rs` — `ImageProcessor` 懒探测 + 状态标记 + 错误收集
- `src-tauri/src/services/llm_providers.rs` — `get_vision_candidates()` 候选模型检索

---

## 4. Plan-and-Execute 层（P1）

### 4.1 自适应模式切换

**决策逻辑**：

```rust
// 新文件: src-tauri/src/services/agent_router.rs
// 注意：AgentMode 定义已统一到公共模块 types.rs（见 3.3 节的 bitflags! 定义）

use crate::services::types::AgentMode;

pub fn route_mode(message: &str, skill_matches: &[SkillMatch]) -> AgentMode {
    // 规则引擎判断
    let complexity_score = calculate_complexity(message, skill_matches);

    if complexity_score >= COMPLEXITY_THRESHOLD {
        AgentMode::PlanExecute
    } else {
        AgentMode::ReAct
    }
}

fn calculate_complexity(message: &str, skill_matches: &[SkillMatch]) -> u32 {
    let mut score = 0u32;

    // 多步骤关键词
    let multi_step_keywords = ["分析", "生成", "创建", "开发", "实现", "规划", "对比", "审查"];
    for kw in &multi_step_keywords {
        if message.contains(kw) { score += 10; }
    }

    // 匹配到多个技能 → 更复杂
    score += skill_matches.len() as u32 * 5;

    // 消息长度（长消息通常更复杂）
    if message.len() > 500 { score += 10; }
    if message.len() > 1000 { score += 10; }

    // 包含代码块 → 开发任务
    if message.contains("```") { score += 15; }

    score
}

const COMPLEXITY_THRESHOLD: u32 = 20; // 可配置，存在 app_state 中
```

### 4.2 Planner Agent

```rust
// 新文件: src-tauri/src/services/planner.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub steps: Vec<PlanStep>,
    pub estimated_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: u32,
    pub description: String,
    pub tool: Option<String>,        // 使用的工具，None 表示纯推理
    pub expected_output: String,     // 预期输出描述
    pub depends_on: Vec<u32>,        // 依赖的步骤 ID
}

pub struct Planner;

impl Planner {
    /// 生成执行计划
    pub async fn plan(
        task: &str,
        skill_catalog: &str,     // 匹配的技能清单
        context: &str,           // 项目上下文
        llm: &LLMService,
        metadata: &ModelMetadata,
    ) -> Result<ExecutionPlan, String> {
        let budget = ContextBudget::calculate(metadata, AgentMode::PlanExecute);

        // 使用小模型做规划，节省 token
        let response = llm.chat_completion_with_model(
            "planning",  // 使用轻量模型
            &format!(
                "你是一个任务规划专家。请为以下任务生成执行计划。\n\n\
                 任务：{task}\n\n\
                 可用技能：{skill_catalog}\n\n\
                 项目上下文：{context}\n\n\
                 要求：\n\
                 1. 分解为 3-8 个可执行步骤\n\
                 2. 每步明确工具和预期输出\n\
                 3. 标注步骤间依赖关系\n\
                 4. 以 JSON 格式返回"
            ),
            budget.plan,
        ).await?;

        // 解析 LLM 返回的计划
        Self::parse_plan(&response)
    }
}
```

**★ 审查优化：流式规划（Speculative Step Execution）**

当 Planner 正在生成执行计划时，第一步通常已经可以确定。利用这个特点，可以在 Planner 流式输出的同时，并发启动第一步的执行。

**V2审查补充：流式JSON解析实现方案**

LLM 输出的是连续文本流，生成的 JSON 在流结束前是不完整的。直接用 `serde_json` 反序列化未闭合的 JSON 流会报错。推荐两种实现方案：

**方案A：NDJSON（行分隔JSON）** — 推荐

在 Planner 的 system prompt 中指示 LLM 以 NDJSON 格式输出，每个步骤独占一行：

```json
{"id": 1, "description": "搜索知识库", "tool": "search-knowledge", "expected_output": "相关文档列表"}
{"id": 2, "description": "分析结果", "tool": null, "expected_output": "分析报告"}
```

实现优势：每行都是完整 JSON，可用 `BufRead::lines()` 逐行解析，无需等待流结束。

**方案B：花括号配对计数器（Brace Counting Parser）**

如果必须使用标准 JSON 数组格式，实现一个简单的流式解析器：

```rust
/// 从流中提取第一个完整的 JSON 对象
fn extract_first_json_object(stream: &mut impl Read) -> Option<String> {
    let mut depth = 0;
    let mut in_string = false;
    let mut escape = false;
    let mut buffer = String::new();
    
    for byte in stream.bytes() {
        let b = byte.ok()?;
        buffer.push(b as char);
        
        if escape { escape = false; continue; }
        if b == '\\' && in_string { escape = true; continue; }
        if b == '"' { in_string = !in_string; continue; }
        if in_string { continue; }
        
        match b {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(buffer.clone());
                }
            }
            _ => {}
        }
    }
    None
}
```

**方案C：结构化分割符**（最简单）

指示 LLM 在每个步骤 JSON 前后插入特殊标记：

```
---STEP---
{"id": 1, "description": "...", "tool": "..."}
---STEP---
{"id": 2, "description": "...", "tool": "..."}
---END---
```

实现时按行检测 `---STEP---` 标记，标记之间的内容直接 `serde_json::from_str` 即可。

```rust
// 在 planner.rs 中继续

impl Planner {
    /// 流式规划 + 第一步并发执行
    /// Planner 流式输出时，一旦第一步内容确定，立即启动 Executor
    pub async fn plan_with_speculative_exec<F, Fut>(
        task: &str,
        skill_catalog: &str,
        context: &str,
        llm: &LLMService,
        metadata: &ModelMetadata,
        executor_fn: F,  // 执行单步的闭包
    ) -> Result<(ExecutionPlan, Option<ExecutedStep>), String>
    where
        F: FnOnce(PlanStep) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        // 1. 启动 Planner（流式）
        let plan_stream = llm.chat_completion_stream("planning", &format!(...)).await?;

        // 2. 从流中解析第一步（当 LLM 输出第一个 step 的 JSON 块时）
        let (first_step, remaining_stream) = Self::extract_first_step(plan_stream).await;

        // 3. 并发：启动第一步执行，同时继续解析剩余步骤
        let step_future = if let Some(ref step) = first_step {
            Some(executor_fn(step.clone()))
        } else {
            None
        };

        // 4. 继续解析剩余步骤
        let remaining_steps = Self::parse_remaining_steps(remaining_stream).await;

        // 5. 等待第一步执行结果
        let step_result = if let Some(fut) = step_future {
            Some(ExecutedStep {
                step: first_step.unwrap(),
                result: fut.await?,
            })
        } else {
            None
        };

        let plan = ExecutionPlan {
            steps: {
                let mut steps = vec![];
                if let Some(s) = first_step { steps.push(s); }
                steps.extend(remaining_steps);
                steps
            },
            estimated_tokens: 0,
        };

        Ok((plan, step_result))
    }
}
```

**注意**：如果第一步执行失败且 Replanner 修改了计划，Speculative Execution 的结果会被丢弃。这是可接受的代价——大部分情况下第一步是正确的，节省的延迟值得偶尔的浪费。

**⚠️ V2审查补充：副作用步骤的推测执行限制**

Speculative Execution 只能针对**只读或无副作用**的步骤。如果第一步是写操作（如 `write_file`、`git commit`、`generate-doc`），而后续计划失效被丢弃，已执行的写操作会污染代码库。

**实现方案**：在 `PlanStep` 或工具定义中，标注该步骤是否是 Read-Only / Idempotent（只读或幂等）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: u32,
    pub description: String,
    pub tool: Option<String>,
    pub expected_output: String,
    pub depends_on: Vec<u32>,
    /// ★ V2审查新增：步骤是否有副作用
    /// Read-Only：只读操作，可安全推测执行（如 search-knowledge, grep_search）
    /// Write：写操作，必须等待 Planner 完整解析后执行（如 generate-doc, write_file）
    pub side_effect: SideEffect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SideEffect {
    ReadOnly,   // 只读/幂等，可安全推测执行
    Write,      // 有副作用，必须等待完整计划
}
```

推测执行时的判断逻辑：

```rust
// 在 plan_with_speculative_exec 中
let step_future = if let Some(ref step) = first_step {
    // ★ V2审查新增：只读步骤才推测执行
    if step.side_effect == SideEffect::ReadOnly {
        Some(executor_fn(step.clone()))
    } else {
        // 有副作用的步骤，等待 Planner 完整解析后由状态机控制执行
        tracing::info!("步骤 {} 有副作用，跳过推测执行", step.id);
        None
    }
} else {
    None
};
```

### 4.3 计划状态机与防漂移机制

**核心问题**：LLM 的注意力机制天然"就近关注"——当 Plan 放在上下文开头，执行到第 3 步时窗口已塞满中间结果，LLM 不再看 Plan 的后续步骤，开始合并步骤或自行发挥。

**解决思路**：不让 LLM 自己决定执行哪一步，由 Rust 代码控制步进。每轮 LLM 调用只注入当前步骤上下文，LLM 只负责单步执行。

```rust
// 在 planner.rs 中继续

/// 计划状态机 — 控制步骤执行，防止计划漂移
pub struct PlanStateMachine {
    plan: ExecutionPlan,
    current_index: usize,
    executed: Vec<ExecutedStep>,
    replan_count: u32,
    max_replans: u32,        // 最大重规划次数
    state: PlanState,
}

#[derive(Debug, PartialEq)]
pub enum PlanState {
    /// 等待执行下一步
    Ready,
    /// 当前步骤执行中
    Executing,
    /// 当前步骤完成，准备进入下一步
    StepDone,
    /// 需要重新规划
    NeedsReplan,
    /// 计划全部完成
    Completed,
    /// 计划失败（重规划次数耗尽）
    Failed(String),
}

impl PlanStateMachine {
    pub fn new(plan: ExecutionPlan) -> Self {
        Self {
            plan,
            current_index: 0,
            executed: Vec::new(),
            replan_count: 0,
            max_replans: 3,
            state: PlanState::Ready,
        }
    }

    /// 获取当前步骤
    pub fn current_step(&self) -> Option<&PlanStep> {
        self.plan.steps.get(self.current_index)
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

    /// 请求重新规划
    pub fn request_replan(&mut self, mut new_steps: Vec<PlanStep>) -> PlanState {
        self.replan_count += 1;
        if self.replan_count > self.max_replans {
            self.state = PlanState::Failed("重规划次数超过上限".into());
            return self.state.clone();
        }
        // 替换当前步骤及之后的步骤
        self.plan.steps.truncate(self.current_index);
        
        // ★ 审查修正：校验并清理无效依赖
        // new_steps 中的 depends_on 可能引用已 truncate 删除的步骤 ID
        Self::validate_dependencies(&self.plan.steps, &mut new_steps);
        
        self.plan.steps.extend(new_steps);
        self.state = PlanState::Ready;
        self.state.clone()
    }

    /// 校验依赖合法性，清理无效依赖引用
    fn validate_dependencies(existing_steps: &[PlanStep], new_steps: &mut [PlanStep]) {
        let valid_ids: std::collections::HashSet<u32> = existing_steps.iter()
            .chain(new_steps.iter())
            .map(|s| s.id)
            .collect();
        
        for step in new_steps {
            let original_count = step.depends_on.len();
            step.depends_on.retain(|dep_id| valid_ids.contains(dep_id));
            if step.depends_on.len() < original_count {
                tracing::warn!(
                    "步骤 {} 的依赖被清理：原始 {} 个，有效 {} 个",
                    step.id, original_count, step.depends_on.len()
                );
            }
        }
    }

    /// ★ 关键方法：为当前步骤构建上下文窗口内容
    /// 不是把整个 Plan 塞进去，而是只注入当前步骤需要的最小上下文
    pub fn build_step_context(&self, original_task: &str) -> StepContext {
        StepContext {
            // 原始任务（始终保留，锚定 LLM 的目标感）
            original_task: original_task.to_string(),

            // 当前步骤（这是 LLM 需要执行的具体任务）
            current_step: self.current_step().cloned(),

            // 已执行步骤的摘要（不是完整结果，是结构化摘要）
            executed_summary: Self::summarize_executed(&self.executed),

            // 总进度
            progress: format!("{}/{}", self.current_index, self.plan.steps.len()),

            // ★ 审查修正：只暴露剩余步骤数量，不暴露具体内容
            // 避免 LLM 看到后续步骤后提前"发挥"或合并步骤
            remaining_count: self.plan.steps.len() - self.current_index - 1,
        }
    }

    /// 将已执行步骤压缩为结构化摘要（避免上下文膨胀）
    fn summarize_executed(executed: &[ExecutedStep]) -> String {
        executed.iter().enumerate().map(|(i, s)| {
            // 每步最多保留 150 字符的结果摘要
            let result_preview = if s.result.len() > 150 {
                format!("{}...", &s.result[..150])
            } else {
                s.result.clone()
            };
            format!("步骤{}({}): {}", i + 1, s.step.description, result_preview)
        }).collect::<Vec<_>>().join("\n")
    }
}

/// 单步执行的上下文（注入到 LLM 的 system prompt）
pub struct StepContext {
    pub original_task: String,
    pub current_step: Option<PlanStep>,
    pub executed_summary: String,
    pub progress: String,
    /// ★ 审查修正：只保留剩余步骤数量，不暴露具体内容
    pub remaining_count: usize,
}

impl StepContext {
    /// 渲染为 system prompt 片段
    pub fn to_prompt(&self) -> String {
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
            current_step = self.current_step.as_ref()
                .map(|s| format!("{}: {}", s.id, s.description))
                .unwrap_or_else(|| "无".into()),
            // ★ 审查修正：只显示剩余步骤数量，不暴露具体内容
            remaining = if self.remaining_count == 0 {
                "（这是最后一步）".into()
            } else {
                format!("剩余 {} 步", self.remaining_count)
            },
        )
    }
}
```

**防漂移三重保障**：

| 保障层 | 机制 | 说明 |
|--------|------|------|
| **程序化步进** | `PlanStateMachine` 控制 `current_index` | LLM 无法自行跳步，Rust 代码决定下一步是什么 |
| **最小上下文注入** | `build_step_context()` 只注入当前步骤 | 不暴露完整 Plan，只显示"剩余 N 步"数量 |
| **硬约束提示词** | 4 条"严格约束"规则 | 在每步的 system prompt 中重复强调 |

**额外防护**：输出校验器检测漂移

```rust
// 在 harness/verifier.rs 中新增

/// 检测 LLM 输出是否偏离当前步骤
/// ★ 审查优化：双重检测 — 文本信号 + 结构化 ToolCall 匹配
pub fn detect_step_drift(
    step_description: &str,
    step_tool: Option<&str>,
    llm_output: &str,
    actual_tool_calls: &[ToolCall],
) -> Option<DriftWarning> {
    // ─── 第一层：文本信号检测 ───
    // ★ 审查修正：增加英文信号词，支持中英双语检测
    let drift_signals = [
        // 中文信号词
        "接下来我将",       // 提前预告下一步
        "同时我也会",       // 合并步骤
        "综合以上所有步骤",  // 试图一步完成剩余
        "跳过",             // 跳步
        "我决定改为",       // 自行修改计划
        // 英文信号词
        "Next I will",      // 提前预告下一步
        "Furthermore",      // 合并步骤
        "In summary",       // 试图一步完成剩余
        "skip",             // 跳步
        "I decide to",      // 自行修改计划
        "instead",          // 自行修改计划
        "let me also",      // 合并步骤
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

    // ─── 第二层：结构化 ToolCall 匹配 ───
    // 如果 PlanStep 声明了 tool 但 LLM 实际调用了不同的 tool → 漂移
    if let Some(expected_tool) = step_tool {
        for call in actual_tool_calls {
            if call.name != expected_tool {
                return Some(DriftWarning {
                    signal: format!("工具不匹配：计划要求 '{}'，实际调用 '{}'", expected_tool, call.name),
                    step: step_description.to_string(),
                    suggestion: format!("检测到计划漂移（结构）：请使用 '{}' 工具执行当前步骤，而非 '{}'。", expected_tool, call.name),
                });
            }
        }
    }

    // ─── 第三层：跨步骤工具调用检测 ───
    // 如果 PlanStep 声明 tool=None（纯推理），但 LLM 却发起了 ToolCall → 漂移
    if step_tool.is_none() && !actual_tool_calls.is_empty() {
        return Some(DriftWarning {
            signal: format!("当前步骤为纯推理，但 LLM 调用了工具：{}", actual_tool_calls.iter().map(|c| c.name.clone()).collect::<Vec<_>>().join(", ")),
            step: step_description.to_string(),
            suggestion: "当前步骤不需要工具调用，请仅用推理完成。".into(),
        });
    }

    None
}
```

当 `detect_step_drift` 检测到漂移信号时，不将该输出返回给用户，而是追加修正提示让 LLM 重新生成当前步骤的输出。

### 4.4 Replanner

```rust
// 在 planner.rs 中继续

impl Planner {
    /// 检测是否需要重新规划
    /// ★ 审查修正：增加英文关键词 + 结构化状态字段支持
    pub fn should_replan(
        plan: &ExecutionPlan,
        step_idx: usize,
        step_result: &str,
        expected: &str,
        step_status: Option<&StepStatus>, // ★ 新增：结构化状态
    ) -> bool {
        // 优先检查结构化状态（如果 LLM 输出包含 status 字段）
        if let Some(status) = step_status {
            return matches!(status, StepStatus::Failed | StepStatus::Blocked);
        }

        // 中英文关键词检测
        let failure_signals = [
            // 中文
            "失败", "错误", "不支持", "无法", "遇到问题", "未能完成",
            // 英文
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
        llm: &LLMService,
    ) -> Result<Vec<PlanStep>, String> {
        // 基于已执行步骤和偏差，让 LLM 重新规划
        let history = executed_steps.iter()
            .map(|s| format!("步骤{}（{}）：{}", s.step.id, s.step.description, &s.result[..200.min(s.result.len())]))
            .collect::<Vec<_>>()
            .join("\n");

        let remaining = old_remaining.iter()
            .map(|s| format!("{}. {}", s.id, s.description))
            .collect::<Vec<_>>()
            .join("\n");

        llm.chat_completion_with_model(
            "planning",
            &format!(
                "基于执行偏差，重新规划剩余任务。\n\n\
                 原始任务：{original_task}\n\n\
                 已执行步骤：\n{history}\n\n\
                 原计划的剩余步骤（可能已不适用）：\n{remaining}\n\n\
                 请生成新的剩余步骤。"
            ),
            4000,  // replan budget
        ).await?;

        // 解析新计划...
    }
}
```

### 4.4 Plan-Execute 执行循环

```rust
// 修改: src-tauri/src/services/rig_agent.rs

impl RigAgent {
    pub async fn run(...) {
        // ... 现有的配置获取代码 ...

        // ★ 新增：模式路由
        let mode = agent_router::route_mode(&user_message, &skill_matches);
        tracing::info!("Agent mode: {:?}", mode);

        match mode {
            AgentMode::ReAct => {
                // 现有的 ReAct 逻辑不变
                Self::run_react(...).await
            }
            AgentMode::PlanExecute => {
                Self::run_plan_execute(...).await
            }
        }
    }

    async fn run_plan_execute(...) {
        // 1. 生成执行计划
        let plan = Planner::plan(&user_message, &skill_catalog, &project_context, llm, &metadata).await?;
        Self::emit_plan_event(&sender, &plan);

        // 2. 初始化状态机
        let mut state_machine = PlanStateMachine::new(plan);

        // 3. 逐步执行（由状态机控制步进，非 LLM 自行决定）
        loop {
            match state_machine.state {
                PlanState::Ready => {
                    let step = state_machine.current_step()
                        .expect("Ready state should have a current step");
                    Self::emit_step_start(&sender, step);

                    // ★ 关键：只注入当前步骤的上下文，不是整个 Plan
                    let step_context = state_machine.build_step_context(&user_message);

                    // 执行当前步骤（复用现有 ReAct 循环，但注入步骤约束 prompt）
                    let result = Self::execute_single_step_with_context(
                        step, &step_context, llm, &sender, ...
                    ).await;

                    // ★ 漂移检测（双重：文本信号 + 结构化 ToolCall 匹配）
                    let actual_calls: Vec<ToolCall> = result.extract_tool_calls();
                    if let Some(warning) = verifier::detect_step_drift(
                        &step.description,
                        step.tool.as_deref(),
                        &result,
                        &actual_calls,
                    ) {
                        // 追加修正提示，让 LLM 重新执行当前步骤
                        let corrected = Self::retry_with_drift_correction(
                            step, &warning, llm, &sender, ...
                        ).await;
                        state_machine.record_result(corrected);
                    } else {
                        state_machine.record_result(result);
                    }

                    Self::emit_step_result(&sender, step, &state_machine.executed.last().unwrap().result);
                }

                PlanState::StepDone => {
                    // 推进到下一步
                    match state_machine.advance() {
                        PlanState::Completed => break,
                        PlanState::Ready => continue,
                        _ => break,
                    }
                }

                PlanState::NeedsReplan => {
                    // 重新规划剩余步骤
                    let remaining = &state_machine.plan.steps[state_machine.current_index..];
                    let new_steps = Planner::replan(
                        &user_message,
                        &state_machine.executed,
                        remaining,
                        llm,
                    ).await.unwrap_or_else(|_| remaining.to_vec());

                    state_machine.request_replan(new_steps);
                }

                PlanState::Completed => break,

                PlanState::Failed(reason) => {
                    Self::emit_error(&sender, &reason);
                    break;
                }

                PlanState::Executing => unreachable!(),
            }
        }

        // 4. 综合结果
        let final_answer = Self::synthesize(&user_message, &state_machine.executed, llm).await;
        Self::emit_done(&sender, &final_answer);
    }
}
```

### 4.5 前端 Plan-Execute UI

**★ 审查修正：Plan-Execute 模式的 SSE Event 通道设计**

当前前端通过 SSE 的 `ReActEvent` 接收 Agent 状态。Plan-Execute 需要扩展事件类型：

```rust
// react_agent.rs — ReActEvent 扩展
pub enum ReActEvent {
    // 现有事件
    Thinking { session_id: String, text: String },
    ToolCall { session_id: String, name: String, args: String },
    ToolResult { session_id: String, name: String, result: String },
    TextDelta { session_id: String, delta: String },
    Error { session_id: String, error: String },
    Done { session_id: String, text: String },
    Clarification { session_id: String, question_id: String, question: String },
    
    // ★ 新增：Plan-Execute 事件
    PlanGenerated { session_id: String, plan: ExecutionPlan },
    StepStart { session_id: String, step_index: u32, step: PlanStep },
    StepResult { session_id: String, step_index: u32, result: String, tool_calls: Vec<ToolCall> },
    Replan { session_id: String, reason: String, new_plan: ExecutionPlan },
}
```

```typescript
// 修改: src/contexts/AgentContext.tsx — ReActTrace 扩展

export interface ReActTrace {
    thinking: string;
    toolCalls: { name: string; args: string; result: string }[];
    // ★ 新增：Plan-Execute 状态
    plan?: ExecutionPlan;
    currentStep?: number; // 当前执行到第几步
    stepResult?: string;
    replanReason?: string;
}

export interface ExecutionPlan {
    steps: PlanStep[];
    totalSteps: number;
}

export interface PlanStep {
    id: number;
    description: string;
    tool?: string;
    status: 'pending' | 'running' | 'done' | 'failed' | 'skipped';
}
```

Chat.tsx 中添加 Plan 展示组件：左侧竖线时间线，每步一个节点，显示状态图标和描述。

**★ V2审查补充：执行步骤名称的显式锚定**

即使 Thinking 折叠面板在前端是默认收起的，也建议在 ChatBubble 的头部常态化显示当前执行状态，使用户在思维链流式输出的数秒内，能一目了然地获知 Agent 此时此刻的宏观定位，大幅减少等待焦虑。

```typescript
// Chat.tsx 中的步骤锚定组件
function StepAnchor({ plan, currentStep }: { plan?: ExecutionPlan; currentStep?: number }) {
  if (!plan || currentStep === undefined) return null;
  
  const step = plan.steps[currentStep];
  return (
    <div className="flex items-center gap-2 text-sm text-muted-foreground mb-2">
      <span className="px-2 py-0.5 bg-primary/10 rounded text-primary font-medium">
        规划模式
      </span>
      <span>
        正在执行步骤 ({currentStep + 1}/{plan.steps.length}): {step.description}
      </span>
    </div>
  );
}
```

显示效果示例：
> `[规划模式] 正在执行步骤 (2/5): 分析数据库模式...`

**★ 审查修正：Planner 超时后的前端降级选项**

Planner 生成计划可能耗时较长（2-10s）。前端需要提供降级选项，让用户在超时后可以选择切换到快速模式：

```typescript
// Chat.tsx 中的 Planner 超时降级组件
function PlannerTimeoutFallback({ 
  isPlanning, 
  onSwitchToReact 
}: { 
  isPlanning: boolean; 
  onSwitchToReact: () => void;
}) {
  const [showFallback, setShowFallback] = useState(false);
  
  useEffect(() => {
    if (isPlanning) {
      // 5秒后显示降级选项
      const timer = setTimeout(() => setShowFallback(true), 5000);
      return () => clearTimeout(timer);
    } else {
      setShowFallback(false);
    }
  }, [isPlanning]);
  
  if (!showFallback) return null;
  
  return (
    <div className="flex items-center gap-3 p-3 bg-muted/50 rounded-lg text-sm">
      <span className="text-muted-foreground">
        计划生成中...耗时较长
      </span>
      <Button 
        variant="outline" 
        size="sm" 
        onClick={onSwitchToReact}
      >
        切换为快速模式
      </Button>
    </div>
  );
}
```

后端同步实现：Planner 增加 10s 超时，超时后自动降级为 ReAct 并发送 `PlannerTimeout` 事件通知前端。

---

## 5. Harness Engineering 层（P1 — 与 Plan-Execute 并行推进）

### 5.1 架构约束编码

**现状**：约束完全靠 system prompt 文本（`tool_policy::agent_tool_policy_prompt()`）

**改造方案**：将关键约束从"提示词"升级为"程序化强制执行"

```rust
// 新文件: src-tauri/src/services/harness/constraints.rs

/// 工具调用约束
pub struct ToolConstraints {
    /// 单次会话最大工具调用次数
    pub max_tool_calls: u32,
    /// 连续相同调用最大次数（死循环检测）
    pub max_identical_calls: u32,
    /// ★ 审查优化：滑动窗口大小（用于 Ping-Pong 检测）
    pub ping_pong_window: usize,
    /// ★ 审查优化：窗口内相异调用组合最小数量（低于此判定为死循环）
    /// 
    /// ⚠️ **重要配置规范**（V2审查补充）：
    /// 此值必须 >= 3。如果设为 2，则双工具交替死循环（A→B→A→B）无法被拦截。
    /// 推荐配置：ping_pong_window=6, min_unique_calls_in_window=3
    /// 
    /// 示例：
    /// - A→B→A→B→A→B（窗口=6，相异=2）→ 2 < 3 → 判定为死循环 ✅
    /// - A→B→C→A→D→E（窗口=6，相异=5）→ 5 >= 3 → 正常 ✅
    pub min_unique_calls_in_window: usize,
    /// 禁止连续调用的工具对（如 search → search）
    /// ★ 审查修正：使用 HashSet 提高查找效率（O(1) vs O(n)）
    pub forbidden_sequences: HashSet<(String, String)>,
    /// 每个工具的单次调用最大参数长度
    pub max_param_length: HashMap<String, usize>,
}

/// 输出格式约束
pub struct OutputConstraints {
    /// 要求输出格式（json/markdown/text）
    pub required_format: Option<String>,
    /// 最大输出长度（字符）
    pub max_output_chars: Option<usize>,
    /// 必须包含的字段（JSON 模式）
    pub required_fields: Vec<String>,
}

/// 在 drain_stream 中强制执行约束
pub fn enforce_tool_constraint(
    call: &ToolCall,
    history: &[ToolCall],
    constraints: &ToolConstraints,
) -> Result<(), HarnessViolation> {
    // 1. 总次数检查
    if history.len() >= constraints.max_tool_calls as usize {
        return Err(HarnessViolation::MaxToolCallsExceeded);
    }

    // 2. 连续相同调用检测（简单死循环）
    let identical_count = history.iter().rev()
        .take_while(|c| c.name == call.name && c.args == call.args)
        .count();
    if identical_count >= constraints.max_identical_calls as usize {
        return Err(HarnessViolation::DoomLoopDetected(call.name.clone()));
    }

    // 3. ★ 审查优化：Ping-Pong 交替死循环检测
    //    例：search(A) → search(B) → search(A) → search(B) → ...
    //    检测最近 N 次调用中，相异的 (name, args_hash) 组合是否过少
    if history.len() >= constraints.ping_pong_window {
        let window = &history[history.len() - constraints.ping_pong_window..];
        // ★ 复审修正：使用 normalized_call_key 替代已删除的 hash_args
        let unique_calls: HashSet<String> = window.iter()
            .map(|c| normalized_call_key(&c.name, &c.args))
            .collect();
        if unique_calls.len() < constraints.min_unique_calls_in_window {
            return Err(HarnessViolation::PingPongLoop {
                window_size: constraints.ping_pong_window,
                unique_count: unique_calls.len(),
                pattern: unique_calls.into_iter().collect(),
            });
        }
    }

    // 4. 禁止序列检查
    if let Some(last) = history.last() {
        if constraints.forbidden_sequences.contains(&(last.name.clone(), call.name.clone())) {
            return Err(HarnessViolation::ForbiddenSequence(last.name.clone(), call.name.clone()));
        }
    }

    // 5. 参数长度检查
    if let Some(max_len) = constraints.max_param_length.get(&call.name) {
        if call.args.len() > *max_len {
            return Err(HarnessViolation::ParameterTooLong(call.name.clone()));
        }
    }

    Ok(())
}

/// ★ 审查修正：规范化参数哈希（用于滑动窗口去重）
/// 排除时间戳、随机数、分页偏移量等易变参数，提取核心语义参数
fn normalized_call_key(name: &str, args: &str) -> String {
    use std::hash::{Hash, Hasher};
    
    // 尝试解析 JSON args，提取核心参数
    let core_params = if let Ok(json) = serde_json::from_str::<serde_json::Value>(args) {
        // 提取非时间戳/非随机数的关键字段
        let mut keys: Vec<String> = json.as_object()
            .map(|obj| {
                obj.iter()
                    .filter(|(k, _)| {
                        // 排除易变参数
                        let k_lower = k.to_lowercase();
                        !k_lower.contains("timestamp")
                            && !k_lower.contains("random")
                            && !k_lower.contains("nonce")
                            && !k_lower.contains("page")
                            && !k_lower.contains("offset")
                            && !k_lower.contains("token")
                    })
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect()
            })
            .unwrap_or_default();
        keys.sort(); // 排序确保一致性
        keys.join(",")
    } else {
        // 非 JSON 参数，直接使用原文
        args.to_string()
    };
    
    // ★ 复审修正：直接 hash name 和 core_params，避免冗余 format! 分配
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    core_params.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Harness 违规类型
pub enum HarnessViolation {
    MaxToolCallsExceeded,
    DoomLoopDetected(String),
    PingPongLoop {
        window_size: usize,
        unique_count: usize,
        pattern: Vec<String>,
    },
    ForbiddenSequence(String, String),
    ParameterTooLong(String),
    /// ★ 审查优化：验证重试次数超过上限
    VerificationRetryExhausted {
        tool_name: String,
        retry_count: u32,
        last_error: String,
    },
}

### 5.2 反馈循环：结果验证

```rust
// 新文件: src-tauri/src/services/harness/verifier.rs

/// 工具结果验证器
pub struct ResultVerifier {
    /// ★ 审查优化：同一工具连续验证失败的最大重试次数
    max_consecutive_failures: u32,
    /// 当前连续失败计数器（per tool）
    failure_counts: HashMap<String, u32>,
}

impl ResultVerifier {
    pub fn new(max_consecutive_failures: u32) -> Self {
        Self {
            max_consecutive_failures,
            failure_counts: HashMap::new(),
        }
    }

    /// 验证工具执行结果的质量
    /// ★ 审查优化：超过重试上限时返回 Exhausted 而非 Fail
    pub fn verify(&mut self, tool_name: &str, result: &str) -> VerificationResult {
        let base_result = match tool_name {
            "search-knowledge" => Self::verify_search_result(result),
            "generate-doc" => Self::verify_doc_result(result),
            "check-scope-creep" => Self::verify_scope_result(result),
            _ => VerificationResult::Pass, // 未知工具不验证
        };

        match base_result {
            VerificationResult::Pass => {
                // 成功则重置计数器
                self.failure_counts.remove(tool_name);
                VerificationResult::Pass
            }
            VerificationResult::Fail(reason) | VerificationResult::Partial(reason) => {
                // 失败则递增计数器
                let count = self.failure_counts.entry(tool_name.to_string()).or_insert(0);
                *count += 1;

                if *count > self.max_consecutive_failures {
                    // ★ 超过重试上限，报告耗尽
                    VerificationResult::Exhausted {
                        tool: tool_name.to_string(),
                        retry_count: *count,
                        last_error: reason.clone(),
                        suggestion: format!(
                            "工具 '{}' 连续验证失败 {} 次，已达到上限。建议：1) 切换工具策略 2) 重新规划 3) 报告用户",
                            tool_name, *count
                        ),
                    }
                } else {
                    base_result
                }
            }
            VerificationResult::Exhausted { .. } => base_result, // 已经是 Exhausted
        }
    }

    /// 重置指定工具的失败计数（用于 Replanner 后的新尝试）
    pub fn reset_failure_count(&mut self, tool_name: &str) {
        self.failure_counts.remove(tool_name);
    }

    fn verify_search_result(result: &str) -> VerificationResult {
        if result.is_empty() {
            return VerificationResult::Fail("知识库搜索返回空结果，请尝试更换关键词".into());
        }
        if result.contains("没有找到") || result.contains("no results") {
            return VerificationResult::Partial("搜索结果不足，建议扩大搜索范围或使用不同关键词".into());
        }
        VerificationResult::Pass
    }

    fn verify_doc_result(result: &str) -> VerificationResult {
        if result.len() < 100 {
            return VerificationResult::Fail("生成的文档内容过短，可能未正确执行".into());
        }
        VerificationResult::Pass
    }
}

pub enum VerificationResult {
    Pass,
    Partial(String),   // 部分成功，附带建议
    Fail(String),       // 明确失败，附带原因
    /// ★ 审查优化：验证重试次数耗尽
    Exhausted {
        tool: String,
        retry_count: u32,
        last_error: String,
        suggestion: String,
    },
}
```

**验证重试与 Replanner 联动**：

当 `VerificationResult::Exhausted` 出现时，Plan-Execute 执行循环中的处理逻辑：

```rust
// 在 run_plan_execute 循环中

let verification = verifier.verify(&step.tool.unwrap_or_default(), &result);
match verification {
    VerificationResult::Exhausted { tool, retry_count, last_error, suggestion } => {
        tracing::warn!("工具验证重试耗尽: tool={}, retries={}, error={}", tool, retry_count, last_error);
        // 强制触发 Replanner，而非继续重试当前步骤
        state_machine.state = PlanState::NeedsReplan;
        // 在 Replanner 中重置该工具的失败计数（因为新计划可能不再使用该工具）
        verifier.reset_failure_count(&tool);
        // 将失败信息注入 Replanner 的上下文
        state_machine.last_failure = Some(FailureInfo {
            tool: tool.clone(),
            error: last_error,
            suggestion,
        });
    }
    VerificationResult::Fail(reason) => {
        // 正常的失败处理（注入修正提示，让 LLM 重试当前步骤）
        let corrected = Self::retry_with_correction(step, &reason, llm, &sender, ...).await;
        state_machine.record_result(corrected);
    }
    _ => { /* Pass 或 Partial，继续正常流程 */ }
}
```

**默认配置**：`max_consecutive_failures = 3`，即同一工具连续验证失败 3 次后强制触发 Replanner。
```

在 `drain_stream` 中，ToolResult 到达后调用验证器，如果验证失败，将验证信息追加到下一轮 LLM 调用的上下文中，让 Agent 知道结果有问题并调整策略。

### 5.3 熵管理

```rust
// 新文件: src-tauri/src/services/harness/entropy.rs

/// 熵管理器 — 技术债务清理和文档一致性维护
pub struct EntropyManager {
    data_dir: PathBuf,
}

impl EntropyManager {
    /// 扫描过期技能（超过 90 天未使用）
    pub fn scan_stale_skills(&self, skill_manager: &SkillManager) -> Vec<StaleItem> {
        // 实现技能过期检测
    }

    /// 扫描文档-代码不一致
    pub async fn scan_doc_code_mismatches(
        &self,
        llm: &LLMService,
    ) -> Vec<DocMismatch> {
        // 对比 README/AGENTS.md 与实际代码结构
    }

    /// 扫描向量化索引与源文件不一致
    pub fn scan_index_drift(&self) -> Vec<IndexDrift> {
        // 比较向量库中的文档哈希与磁盘文件
    }

    /// 执行清理（需要用户确认）
    pub async fn run_garbage_collection(&self, items: &[CleanupItem]) -> CleanupReport {
        // 按类别清理
    }
}
```

暴露为 Tauri 命令，前端可在 Settings 或独立页面触发。不自动执行——删除操作需用户确认。

---

## 6. 前端改动

### 6.1 Token 用量指示器

在 Chat.tsx 的 header 区域添加 token 用量显示：

```typescript
// 修改: src/pages/Chat.tsx

interface TokenUsage {
    used: number;     // 已使用 token 数
    total: number;    // 模型上下文窗口
    percent: number;  // 使用百分比
}
```

调用已有的 `countTokens()` Tauri 命令（当前未被使用），在每次消息发送/接收后更新。

### 6.2 Settings 页面扩展

Settings.tsx 添加"上下文工程"section：

- 模型规格表：展示每个模型的 `context_window`、`max_output_tokens`、`supports_thinking`
- 手动覆盖编辑器：允许用户为未收录模型输入规格
- 熵管理入口：触发过期技能扫描、文档一致性检查

### 6.3 Plan-Execute 模式 UI

- Chat.tsx 添加模式切换按钮（快速模式 / 规划模式）
- Plan 时间线组件：竖向步骤列表，每步显示状态图标
- 步骤详情折叠面板：展示 thinking、工具调用、结果

### 6.4 Thinking 展示

当前 ReAct Trace 中已有 thinking 展示（🤔 斜体），但需增强：

- 独立的 `<details>` 块，默认折叠
- 支持深度 thinking（Claude Extended Thinking / DeepSeek R1），展示完整推理链
- Token 消耗标注（thinking tokens vs output tokens）

---

## 7. 数据迁移

### 7.1 ModelConfig 字段扩展

在 `ModelConfig` 中新增字段均有 `#[serde(default)]`，向后兼容：

```rust
pub struct ModelConfig {
    // ... 现有字段
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub supports_thinking: Option<bool>,
}
```

无需数据库迁移——新字段是 Option 类型，缺失时返回 None，由 `ModelMetadata::resolve()` 的分层策略自动填充。

---

## 8. 实现阶段划分

| 阶段 | 内容 | 涉及文件 |
|------|------|----------|
| **P0-a** | 统一 token 计数 + 新建 token.rs | llm_service.rs, prompt_assembler.rs, rig_agent.rs |
| **P0-b** | 模型元数据系统 + model_specs.json | model_metadata.rs（新）, llm_providers.rs, Settings.tsx |
| **P0-c** | 上下文预算管理器 | context_budget.rs（新）, rig_agent.rs, llm_service.rs |
| **P0-d** | 分层摘要压缩 | context_compressor.rs（新）, rig_agent.rs, llm_service.rs |
| **P0-e** | 渐进式披露接入 | prompt_assembler.rs, risk_blueprint.rs |
| **P0-f** | 合并系统提示词 + 消除歧义 | llm_service.rs, prompts.rs, rig_agent.rs |
| **P1-a** | Agent 模式路由 | agent_router.rs（新）, rig_agent.rs |
| **P1-b** | Planner + Replanner | planner.rs（新）, rig_agent.rs |
| **P1-c** | Plan-Execute 循环 | rig_agent.rs, AgentContext.tsx, Chat.tsx |
| **P1-d** | 前端 Plan UI | Chat.tsx, AgentContext.tsx |
| **P2-a** | 架构约束编码 | harness/constraints.rs（新）, rig_agent.rs |
| **P2-b** | 结果验证循环 | harness/verifier.rs（新）, rig_agent.rs |
| **P2-c** | 熵管理 | harness/entropy.rs（新）, Settings.tsx |
| **P2-d** | Token 用量 UI + Thinking 增强 | Chat.tsx, AgentContext.tsx |

---

## 9. 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| tiktoken 对中文不够精确 | 已有回退公式；未来可切换到 o200k_base（GPT-4o 编码器） |
| 模型_specs.json 维护成本 | 支持用户手动覆盖 + Anthropic/Gemini 自动获取 |
| Plan-Execute 增加延迟 | 简单任务自动走 ReAct，不增加开销 |
| 分层摘要丢失关键信息 | 关键轮标记永不丢弃 + 摘要包含结构化信息 |
| 前端改动范围大 | 分阶段交付，每阶段可独立发布 |
| 约束过严导致 Agent 能力受限 | 约束参数可配置，默认值偏宽松 |
| **计划漂移（Plan Drift）** | 三重保障：程序化步进（Rust 控制步进）+ 最小上下文注入（不暴露完整 Plan）+ 漂移信号检测（拦截合并/跳步行为）|

---

## 10. 错误处理与恢复策略

### 10.1 错误分类

| 类别 | 定义 | 处理方式 | 示例 |
|------|------|---------|------|
| **Fatal** | 系统不可恢复的错误 | 直接 panic 或返回严格 Result | tiktoken 初始化失败、状态机内部错误 |
| **Retryable** | 临时性错误，重试可能成功 | 重试 N 次后升级为其他类别 | 网络超时、Planner JSON 解析失败、LLM 速率限制 |
| **Degradable** | 功能降级但仍可用 | 自动切换到次优模式 | Planner 超时→降级为 ReAct、模型不支持 thinking→关闭 thinking |
| **Reportable** | 需要用户决策的错误 | 停止执行，通知用户 | 验证重试耗尽、Replanner 循环、API Key 失效 |
| **Skippable** | 非关键步骤失败 | 标记 skip，继续后续步骤 | 某步骤工具失败但不影响整体目标 |

### 10.2 各层错误边界

| 层/模块 | 错误类型 | 恢复策略 |
|---------|---------|---------|
| **token.rs** | Fatal | 使用 `Result<u32, TokenError>`，不在模块内降级 |
| **context_budget.rs** | Fatal | 使用断言确保分配不超 total |
| **context_compressor.rs** | Retryable | 摘要 LLM 调用失败→重试 1 次→跳过摘要，保留原始消息 |
| **model_metadata.rs** | Degradable | API 探测失败→回退到内置数据库→回退到用户覆盖→保守默认值 |
| **planner.rs** | Degradable | Planner 超时（10s）→自动降级为 ReAct 模式 |
| **planner.rs** | Retryable | JSON 解析失败→重试 1 次→降级为 ReAct |
| **PlanStateMachine** | Reportable | `PlanState::Failed`→生成原因摘要→通知用户 |
| **harness/constraints.rs** | Reportable | HarnessViolation→记录到 Event 流→前端展示 |
| **harness/verifier.rs** | Degradable | 验证重试耗尽→触发 Replanner→如 Replanner 也失败→报告用户 |
| **image_processor.rs** (多模型回退) | Degradable | 当前模型失败→自动尝试下一个候选模型→所有 LLM 模型失败后降级到 OCR |
| **image_processor.rs** (OCR 回退) | Degradable | 所有 LLM 模型失败→回退到纯 OCR 处理 |
| **image_processor.rs** (全部失败) | Reportable | LLM + OCR 均失败→上报级联错误链给用户 |
| **image_processor.rs** (懒探测) | Degradable | 检测到不支持多模态的信号→标记 `llm_multimodal=false`→中止后续请求 |

### 10.3 错误传播链

```
工具执行失败
    ↓
ResultVerifier.verify()
    ↓
VerificationResult::Fail(reason)
    ↓
注入修正提示，重试当前步骤
    ↓ (连续失败 3 次)
VerificationResult::Exhausted
    ↓
触发 Replanner
    ↓ (重规划也失败)
PlanState::Failed
    ↓
ReActEvent::Error → 前端展示
```

---

## 11. 测试策略

### 11.1 单元测试（Rust）

| 模块 | 测试场景 |
|------|---------|
| **token.rs** | 空文本、纯中文、纯英文、中英混合、特殊字符、超长文本 |
| **context_budget.rs** | 窗口极小时各槽分配、优先级顺序验证、不同 AgentMode 差异 |
| **context_compressor.rs** | 磁滞触发/释放/复位、增量摘要重置、历史回滚边界 |
| **agent_router.rs** | 各复杂度评分边界值、英文消息、中英混合 |
| **planner.rs** | NDJSON 流式解析、非法 JSON 恢复、依赖校验 |
| **harness/constraints.rs** | Ping-Pong A→B→A→B→A→B 检测、参数哈希碰撞 |
| **PlanStateMachine** | 所有状态转换路径、重规划耗尽、空计划处理 |

### 11.2 集成测试

| 场景 | 验证点 |
|------|--------|
| Context Budget + Context Compressor 联动 | 预算分配正确，压缩触发时机正确 |
| Agent 模式路由 + Speculative Execution | 复杂任务路由到 Plan-Execute，简单任务走 ReAct |
| SSE Event 通道 + 前端渲染 | Plan 事件正确传递，步骤状态正确显示 |
| Planner + Replanner 联动 | 计划失败后正确触发重规划 |

### 11.3 回归测试

| 场景 | 验证点 |
|------|--------|
| 现有 compress_conversation 与新 compressor 行为对比 | 新实现不丢失关键信息 |
| 现有 count_tokens 精度验证 | tiktoken 计数准确性 |
| ReAct 模式不受影响 | P0/P1 改动不破坏现有功能 |

---

## 12. 实施注意事项

### 12.1 Speculative Execution 并发兼容性（U1）

**问题描述**：Planner 流式输出和第一步推测执行都可能同时调用 `LLMService`。虽然 `reqwest::Client` 默认支持 4 路并发，但：
- Anthropic API 有连接限制（部分账户仅 1 并发）
- `rig` 库的 Agent 内部可能有单代理运行限制

**实施要求**：
1. 在 `plan_with_speculative_exec` 中增加 `LLMService::concurrency_allowed()` 检查
2. 如果不支持并发，降级为顺序执行（先完成 Planner，再执行第一步）
3. 或者使用独立的 LLM 连接（非共享 `reqwest::Client`）

```rust
// 实施时需要添加的检查
if !llm.supports_concurrent() {
    // 降级为顺序执行
    let plan = Planner::plan(...).await?;
    let first_result = if let Some(step) = plan.steps.first() {
        executor_fn(step.clone()).await?
    } else {
        return Ok((plan, None));
    };
    return Ok((plan, Some(first_result)));
}
```

### 12.2 字符级硬上限安全阀（U2）

**问题描述**：Token 预算系统上线后，字符截断不再是主要手段。但极端情况下（如 tiktoken 初始化失败、token 计数器异常），可能导致无限膨胀的消息被发送给 LLM。

**实施要求**：在配置中保留 `max_chars` 硬上限，作为最后的安全阀：

```rust
// 配置项
pub struct ContextConfig {
    /// Token 预算上限（主要手段）
    pub max_tokens: u32,
    /// 字符硬上限（安全阀，防止 token 计数异常）
    pub max_chars_hard_limit: usize,
    /// 硬上限触发时的行为
    pub hard_limit_action: HardLimitAction,
}

pub enum HardLimitAction {
    /// 截断到硬上限
    Truncate,
    /// 返回错误
    Error,
    /// 截断并记录警告
    TruncateAndWarn,
}
```

**默认值**：
- `max_chars_hard_limit`: 500,000 字符（约 125K-250K tokens，视语言而定）
- `hard_limit_action`: `TruncateAndWarn`

**使用位置**：在 `build_prompt_with_history` 和 `ContextBudget::calculate` 的最终输出处，检查总字符数是否超过硬上限。