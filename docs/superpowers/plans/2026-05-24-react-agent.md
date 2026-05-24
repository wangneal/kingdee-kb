# ReAct Agent 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development 逐任务实现此计划。

**目标：** 构建一个 ReAct（Reasoning + Acting）推理引擎，让 KingdeeKB 的 AI 助手能自主思考→调用工具→观察结果→循环推理，最终给出高质量回答。覆盖 Chat、调研助手、文档生成、风险把控全部页面。

**架构：**
- Rust 后端实现 ReAct 循环引擎，所有已有功能注册为工具
- 通过 SSE 事件流（`react_thinking/react_tool_call/react_tool_result/react_done`）实时推送推理过程
- 前端 Chat 组件展示思考链路 + 工具调用卡片，其他页面通过同一引擎驱动

**技术栈：** Rust + Tauri + React + SSE EventStream

---

## 文件结构

### 新建文件
| 文件 | 职责 |
|------|------|
| `src-tauri/src/services/react_agent.rs` | ReAct 循环引擎核心（think→act→observe 循环） |
| `src-tauri/src/services/tool_registry.rs` | 工具注册表 + Tool trait 定义 |
| `src/pages/Chat.tsx` | **重写** — 增加 ReAct 推理展示（思考过程、工具调用卡片） |

### 修改文件
| 文件 | 变更 |
|------|------|
| `src-tauri/src/services/mod.rs` | 添加 `react_agent`、`tool_registry` 模块 |
| `src-tauri/src/services/llm_service.rs` | 添加 `chat_completion_structured` 方法（强制 JSON 输出） |
| `src-tauri/src/app_state.rs` | 添加 `react_agent: ReActAgent` 字段 |
| `src-tauri/src/lib.rs` | 注册新命令 `react_chat` |
| `src/lib/tauri-commands.ts` | 添加 ReAct 类型和前端绑定 |
| `src-tauri/Cargo.toml` | 添加 `serde_json`（已有） |

---

### 任务 1：Tool trait + 工具注册表

**文件：**
- 创建：`src-tauri/src/services/tool_registry.rs`

- [ ] **步骤 1：编写 Tool trait 和工具注册表**

```rust
//! 工具注册表 — ReAct Agent 可调用的所有工具
//!
//! 每个工具是一个实现了 Tool trait 的结构体。
//! 注册表负责工具的注册、发现和调度。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use async_trait::async_trait;

/// 工具参数定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub param_type: String, // "string" | "number" | "boolean"
}

/// 工具调用结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

/// 工具 trait — 所有工具必须实现
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具唯一名称（LLM 通过此名称调用）
    fn name(&self) -> &str;
    /// 工具描述（LLM 理解何时使用）
    fn description(&self) -> &str;
    /// 参数定义
    fn parameters(&self) -> Vec<ToolParam>;
    /// 执行工具调用
    async fn call(&self, args: HashMap<String, String>) -> ToolResult;
}

/// 工具注册表 — 管理所有可用工具
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    /// 注册一个工具
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// 获取所有工具的 OpenAI-compatible function calling 定义
    pub fn get_openai_tools(&self) -> Vec<serde_json::Value> {
        self.tools.values().map(|tool| {
            let params: Vec<serde_json::Value> = tool.parameters().iter().map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "description": p.description,
                    "required": p.required,
                    "type": p.param_type
                })
            }).collect();
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": {
                        "type": "object",
                        "properties": params
                    }
                }
            })
        }).collect()
    }

    /// 调用指定工具
    pub async fn call_tool(&self, name: &str, args: HashMap<String, String>) -> ToolResult {
        match self.tools.get(name) {
            Some(tool) => tool.call(args).await,
            None => ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("工具 '{}' 不存在", name)),
            },
        }
    }

    /// 获取工具描述文本（注入到 System Prompt）
    pub fn get_tool_descriptions(&self) -> String {
        self.tools.values().map(|tool| {
            let params: Vec<String> = tool.parameters().iter().map(|p| {
                format!("  - {}{}: {} ({})", p.name, if p.required { " (必填)" } else { "" }, p.description, p.param_type)
            }).collect();
            format!("## {}\n{}\n参数：\n{}", tool.name(), tool.description(), params.join("\n"))
        }).collect::<Vec<_>>().join("\n\n")
    }
}
```

- [ ] **步骤 2：注册所有已有工具**

在 `tool_registry.rs` 末尾添加具体工具实现，以下是每个工具的框架（完整代码在主文件中）：

```rust
// === 具体工具实现 ===

pub struct SearchKnowledgeTool {
    pub llm: LLMService,
    pub vector_index: Arc<Mutex<VectorIndex>>,
    pub metadata: Arc<Mutex<MetadataStore>>,
    pub bm25: Arc<Mutex<BM25Service>>,
    pub embedding: Arc<Mutex<EmbeddingService>>,
}
#[async_trait]
impl Tool for SearchKnowledgeTool { /* 调用 hybrid_search */ }

pub struct GenerateDocTool {
    pub llm: LLMService,
    /* recipe store etc */
}
#[async_trait]
impl Tool for GenerateDocTool { /* 调用 generate_recipe_doc */ }

pub struct CheckScopeCreepTool {
    pub store: RiskControlStore,
    pub llm: LLMService,
}
#[async_trait]
impl Tool for CheckScopeCreepTool { /* 调用 check_scope_creep */ }

pub struct AnalyzeFitGapTool {
    pub llm: LLMService,
}
#[async_trait]
impl Tool for AnalyzeFitGapTool { /* 调用 analyze_fit_gap */ }

pub struct GetProjectHealthTool {
    pub store: RiskControlStore,
}
#[async_trait]
impl Tool for GetProjectHealthTool { /* 调用 calculate_health_score */ }

pub struct GenerateDefenseScriptTool {
    pub store: RiskControlStore,
    pub llm: LLMService,
}
#[async_trait]
impl Tool for GenerateDefenseScriptTool { /* 调用 generate_defense_script */ }

pub struct ExtractBlueprintTool {
    pub llm: LLMService,
}
#[async_trait]
impl Tool for ExtractBlueprintTool { /* LLM 调用蓝图提炼 */ }

pub struct RecommendQuestionsTool {
    /* question recommender state */
}
#[async_trait]
impl Tool for RecommendQuestionsTool { /* 调用 recommend_questions */ }
```

- [ ] **步骤 3：运行 `cargo check` 验证编译**

- [ ] **步骤 4：Commit**

---

### 任务 2：ReAct 循环引擎核心

**文件：**
- 创建：`src-tauri/src/services/react_agent.rs`

- [ ] **步骤 1：编写 ReAct 引擎**

```rust
//! ReAct 推理引擎 — 思考→行动→观察→循环
//!
//! 核心流程:
//! 1. 组装 System Prompt（角色定义 + 工具描述 + 行为规则）
//! 2. LLM 返回思考 + 动作（工具调用 或 最终回答）
//! 3. 如果是工具调用 → 执行 → 将结果喂回 LLM → 回到 2
//! 4. 如果是最终回答 → 返回给用户
//!
//! 流式事件（SSE）：通过 mpsc channel 发送给前端

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use std::collections::HashMap;
use std::sync::Arc;

use crate::services::llm_service::{ChatMessage, LLMService};
use crate::services::tool_registry::ToolRegistry;

/// ReAct 事件 — 通过 SSE 流式发送给前端
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReActEvent {
    /// AI 的思考过程
    #[serde(rename = "thinking")]
    Thinking { content: String },
    /// AI 决定调用工具
    #[serde(rename = "tool_call")]
    ToolCall { name: String, args: String },
    /// 工具执行结果
    #[serde(rename = "tool_result")]
    ToolResult { name: String, result: String },
    /// 最终回答文本片段
    #[serde(rename = "text_delta")]
    TextDelta { content: String },
    /// 错误
    #[serde(rename = "error")]
    Error { message: String },
    /// 完成
    #[serde(rename = "done")]
    Done,
}

/// ReAct 引擎
pub struct ReActAgent {
    llm: Arc<LLMService>,
    tools: Arc<ToolRegistry>,
    max_iterations: usize,
}

impl ReActAgent {
    pub fn new(llm: Arc<LLMService>, tools: Arc<ToolRegistry>) -> Self {
        Self { llm, tools, max_iterations: 10 }
    }

    /// 运行 ReAct 循环，通过 sender 发送事件
    pub async fn run(
        &self,
        user_message: &str,
        system_extra: &str,
        history: &[ChatMessage],
        sender: mpsc::UnboundedSender<ReActEvent>,
    ) {
        let tool_descriptions = self.tools.get_tool_descriptions();
        let system_prompt = format!(
            "{}你是金蝶ERP实施顾问AI助手。你有权调用以下工具来帮助用户。\n\
             在每次回答前，先思考需要什么信息、需要调用什么工具。\n\
             严格按照以下JSON格式输出你的决策：\n\
             \n\
             如果要调用工具，输出：\n\
             {{\"type\":\"tool_call\",\"thought\":\"你的思考过程\",\"tool\":\"工具名\",\"args\":{{\"参数名\":\"值\"}}}}\n\
             \n\
             如果要直接回答，输出：\n\
             {{\"type\":\"answer\",\"thought\":\"你的思考过程\",\"content\":\"回答内容\"}}\n\
             \n\
             【可用工具】\n\
             {}\n\
             \n\
             【规则】\n\
             - 一次只调用一个工具\n\
             - 观察工具结果后再决定下一步\n\
             - 最多 {} 次工具调用\n\
             - 如果你已经有足够信息，直接回答",
             system_extra, tool_descriptions, self.max_iterations
        );

        let mut messages = vec![
            ChatMessage { role: "system".to_string(), content: system_prompt },
        ];
        // 添加上文历史
        for msg in history {
            messages.push(msg.clone());
        }
        messages.push(ChatMessage { role: "user".to_string(), content: user_message.to_string() });

        for iteration in 0..self.max_iterations {
            // 调用 LLM
            let config = match self.llm.get_config() {
                Ok(c) => c,
                Err(e) => {
                    let _ = sender.send(ReActEvent::Error { message: e });
                    break;
                }
            };

            // 使用 OpenAI function calling API
            let openai_tools = self.tools.get_openai_tools();
            let response = self.llm.chat_completion_with_tools(
                &messages, &config, &openai_tools,
            ).await;

            match response {
                Ok(ReActStep::ToolCall { thought, tool, args }) => {
                    let _ = sender.send(ReActEvent::Thinking { content: thought });
                    let _ = sender.send(ReActEvent::ToolCall {
                        name: tool.clone(),
                        args: serde_json::to_string(&args).unwrap_or_default(),
                    });

                    // 执行工具
                    let result = self.tools.call_tool(&tool, args).await;
                    let result_str = if result.success { result.output } else { format!("错误: {}", result.error.unwrap_or_default()) };
                    let _ = sender.send(ReActEvent::ToolResult {
                        name: tool.clone(),
                        result: result_str.clone(),
                    });

                    // 将工具调用结果加入对话
                    messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: format!("调用工具: {}\n结果: {}", tool, result_str),
                    });
                }
                Ok(ReActStep::Answer { thought, content }) => {
                    let _ = sender.send(ReActEvent::Thinking { content: thought });
                    // 流式发送最终回答
                    for chunk in content.chars() {
                        let _ = sender.send(ReActEvent::TextDelta { content: chunk.to_string() });
                    }
                    let _ = sender.send(ReActEvent::Done);
                    return;
                }
                Err(e) => {
                    let _ = sender.send(ReActEvent::Error { message: e });
                    break;
                }
            }
        }
        let _ = sender.send(ReActEvent::Error { message: "超出最大迭代次数".to_string() });
    }
}

/// LLM 返回的决策
enum ReActStep {
    ToolCall { thought: String, tool: String, args: HashMap<String, String> },
    Answer { thought: String, content: String },
}
```

- [ ] **步骤 2：在 LLMService 中添加 `chat_completion_with_tools` 方法**

`llm_service.rs` 中新增：

```rust
use serde_json::Value;

impl LLMService {
    pub async fn chat_completion_with_tools(
        &self,
        messages: &[ChatMessage],
        config: &LLMConfig,
        tools: &[Value],
    ) -> Result<ReActStep, String> {
        // 构建 OpenAI function calling 请求
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": config.model,
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto",
            "temperature": config.temperature,
            "max_tokens": config.max_tokens,
        });

        let client = reqwest::Client::new();
        let resp = client.post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send().await
            .map_err(|e| format!("LLM 请求失败: {}", e))?;

        let json: Value = resp.json().await
            .map_err(|e| format!("解析响应失败: {}", e))?;

        // 解析 tool_calls 或 content
        let choice = json["choices"][0].clone();
        let msg = &choice["message"];

        if let Some(tool_calls) = msg["tool_calls"].as_array() {
            if let Some(tc) = tool_calls.first() {
                let func = &tc["function"];
                let name = func["name"].as_str().unwrap_or("").to_string();
                let args_str = func["arguments"].as_str().unwrap_or("{}");
                let args: HashMap<String, String> = serde_json::from_str(args_str).unwrap_or_default();
                return Ok(ReActStep::ToolCall {
                    thought: msg["content"].as_str().unwrap_or("").to_string(),
                    tool: name,
                    args,
                });
            }
        }

        let content = msg["content"].as_str().unwrap_or("").to_string();
        Ok(ReActStep::Answer {
            thought: String::new(),
            content,
        })
    }
}
```

- [ ] **步骤 3：运行 `cargo check` 验证**

- [ ] **步骤 4：Commit**

---

### 任务 3：集成到 AppState + Tauri 命令

**文件：**
- 修改：`src-tauri/src/app_state.rs`
- 修改：`src-tauri/src/lib.rs`
- 修改：`src-tauri/src/services/mod.rs`

- [ ] **步骤 1：注册 react_agent 模块 + 初始化**

`mod.rs` 添加：
```rust
pub mod react_agent;
pub mod tool_registry;
```

`app_state.rs` 添加字段：
```rust
pub react_agent: ReActAgent,
```

初始化：
```rust
let tool_registry = Arc::new(ToolRegistry::new()); // 注册所有工具
let react_agent = ReActAgent::new(Arc::new(llm), tool_registry);
```

- [ ] **步骤 2：添加 Tauri 命令**

```rust
#[tauri::command]
async fn react_chat(
    state: State<'_, AppState>,
    message: String,
    context: String,
) -> Result<(), String> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let app_handle = app_handle.clone();
    
    tauri::async_runtime::spawn(async move {
        state.react_agent.run(&message, &context, &[], tx).await;
    });
    
    // 通过 Tauri events 转发给前端
    while let Some(event) = rx.recv().await {
        let _ = app_handle.emit("react-event", &event);
    }
    Ok(())
}
```

- [ ] **步骤 3：运行 `cargo check`**

- [ ] **步骤 4：Commit**

---

### 任务 4：前端 Chat 页面 ReAct 展示

**文件：**
- 修改：`src/pages/Chat.tsx`
- 修改：`src/lib/tauri-commands.ts`

- [ ] **步骤 1：添加 ReAct 类型和事件监听**

`tauri-commands.ts`：
```typescript
export type ReActEventType = 
  | { type: "thinking"; content: string }
  | { type: "tool_call"; name: string; args: string }
  | { type: "tool_result"; name: string; result: string }
  | { type: "text_delta"; content: string }
  | { type: "error"; message: string }
  | { type: "done" };

export async function reactChat(message: string, context?: string): Promise<void> {
  return invoke("react_chat", { message, context: context ?? "" });
}

export async function listenReActEvents(
  handler: (event: ReActEventType) => void,
): Promise<() => void> {
  return listen("react-event", (event) => handler(event.payload as ReActEventType));
}
```

- [ ] **步骤 2：更新 Chat 页面渲染**

在 Chat.tsx 中新增推理过程展示组件：

```tsx
// 在消息气泡中显示 ReAct 推理过程
function ReActTrace({ trace }: { trace: ReActEventType[] }) {
  return (
    <div className="space-y-2 border-l-2 border-amber-200 pl-3 my-2">
      {trace.map((t, i) => {
        if (t.type === "thinking") return (
          <div key={i} className="text-xs text-amber-700 italic">
            🤔 {t.content}
          </div>
        );
        if (t.type === "tool_call") return (
          <div key={i} className="rounded bg-amber-50 border border-amber-200 p-2 text-xs">
            <span className="font-medium text-amber-800">🔧 调用: {t.name}</span>
            <pre className="mt-1 text-amber-700">{t.args}</pre>
          </div>
        );
        if (t.type === "tool_result") return (
          <div key={i} className="rounded bg-green-50 border border-green-200 p-2 text-xs">
            <span className="font-medium text-green-700">✅ {t.name} 结果</span>
            <pre className="mt-1 text-green-600 line-clamp-3">{t.result}</pre>
          </div>
        );
        return null;
      })}
    </div>
  );
}
```

- [ ] **步骤 3：`npm run build` 验证**

- [ ] **步骤 4：Commit**

---

### 任务 5：集成到其他页面

**文件：**
- 修改：`src/pages/ResearchAssistant.tsx`
- 修改：`src/pages/RiskControl.tsx`
- 修改：`src/pages/Settings.tsx`

- [ ] **步骤 1：调研助手 — Agent 辅助回答**

在提问输入框上方加一个"AI 辅助"按钮，调用 ReAct 引擎自动从知识库搜索答案：

```tsx
const handleAIAssist = async () => {
  if (!newQuestion.trim()) return;
  // 调用 ReAct 引擎搜索知识库 + 推荐回答
  const context = `当前调研记录：${JSON.stringify(records)}`;
  await reactChat(`请帮我回答以下调研问题：${newQuestion}`, context);
  // 监听事件，将结果填入 answer 框
};
```

- [ ] **步骤 2：风险把控 — Agent 自主分析**

风险页加"AI 分析"按钮，自动调用多个工具综合分析：
1. 调用 `get_project_health` 获取健康评分
2. 根据评分调用 `generate_defense_script` 或 `check_scope_creep`

- [ ] **步骤 3：`npm run build` 验证**

- [ ] **步骤 4：Commit**

---

### 任务 6：全面测试验证

- [ ] **步骤 1：运行 `cargo test --lib` — 136+ 测试全部通过**

- [ ] **步骤 2：运行 `npm run build` — 前端构建成功**

- [ ] **步骤 3：最终 Commit**

```bash
git add -A && git commit -m "feat: ReAct Agent 引擎 + 全平台集成

- ReAct 推理引擎（思考→工具调用→观察→回答）
- 8 个注册工具（搜索/文档/风险/脱敏/蓝图/推荐）
- Chat 页面 ReAct 推理链路展示
- 调研助手/风险把控 AI 辅助集成"
```

---

## 自检清单

- [ ] 每个任务都有明确的文件路径和代码
- [ ] 没有 TODO/待定占位符
- [ ] 所有类型定义一致（ReActStep, ReActEvent, Tool trait）
- [ ] 前后端事件格式匹配
- [ ] 测试验证步骤完整
