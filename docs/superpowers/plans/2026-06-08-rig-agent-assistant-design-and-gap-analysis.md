# 基于 Rig 的 Agent 助手设计与本项目对照

> 日期：2026-06-08  
> 范围：Agent 助手、工具调用、技能执行、知识库检索、风控/安全治理  
> 参考项目：`E:\tmp\rig037-probe`、`E:\tmp\opencode`、`E:\tmp\oh-my-openagent-proxy`、`E:\tmp\superpowers`  
> 前置规格：`docs/superpowers/plans/2026-06-01-kingdeekb-technical-spec.md`

---

## 1. 结论摘要

本项目已经不是“待接入 Rig”的状态，而是已经具备一套基于 `rig-core = 0.37` 的 Agent 主路径：

- `src-tauri/src/services/rig_agent.rs` 已使用 Rig multi-turn stream、流式事件、工具调用、取消、死循环检测、工具速率限制、Plan-Execute 降级。
- `src-tauri/src/services/rig_tool.rs` 已沉淀工具 profile、schema guard、工具审计、输出截断/留存、技能脚本权限、脚本沙箱和路径校验。
- `src-tauri/src/services/rig_provider.rs` 已封装 OpenAI / Anthropic / Ollama 的 Rig client，并处理部分兼容端点。
- 前端 `src/contexts/AgentContext.tsx` 已支持多 slot 会话、流式渲染、工具轨迹、计划事件、澄清问题、取消与验证报告。

因此后续设计重点不是推倒重写，而是将当前“可运行的 Rig Agent”升级为“可治理、可审计、可恢复、可配置、可扩展”的 Agent 平台层。建议采取单一语义升级：保留 Rig 主路径，逐步移除残留的旧式非 Rig 对话/模板路径，不再为旧协议做兼容分支。

---

## 2. 外部成熟项目启发

### 2.1 `rig037-probe`：Rig 接入最小闭环

该项目只有 `Cargo.toml` 和 `src/main.rs`，验证了三个关键点：

- `rig-core = 0.37.0` 可通过 OpenAI-compatible base_url 运行流式 prompt。
- `CompletionClient + StreamingPrompt` 是当前可用主路径。
- 配置从本地 `~/.kingdee-kb/config.json` 读取，适合做端点连通性探针。

对本项目的启发：

- 保留一个最小 live probe，专门验证 Rig 与当前供应商端点是否兼容。
- live probe 应作为忽略测试或诊断命令存在，不应混入业务 Agent 回归测试。
- Rig 版本升级前应先跑探针，再跑工具 schema 和 multi-turn 流式测试。

### 2.2 `opencode`：会话、工具、权限的工程边界

`opencode/specs/v2/tools.md` 和 `session.md` 的核心思想：

- 工具注册必须是具名、作用域化、可替换、可失效拒绝的。
- 工具执行上下文要包含 session、agent、assistant message、tool call 等持久身份。
- 工具输入输出必须过 codec/schema 边界，非法输入不得进入 executor。
- 工具输出应在统一边界做模型可见截断，完整结果由受控存储托管。
- 会话 prompt 入队、可见历史、工具 settlement、取消、恢复要分层处理。
- Provider policy 与 provider 配置分离：配置存在不等于允许使用。

对本项目的启发：

- 当前 `ToolGuardWrapper` 已覆盖 schema guard、审计和输出截断，但缺少“工具调用持久身份”和“stale call 拒绝”这类会话级约束。
- 当前前端 `session_id` 存在，但工具审计记录还没有完整绑定 assistant message / tool call id。
- Provider 当前主要是 CRUD 和默认选择，尚未形成显式 allow/deny 策略层。

### 2.3 `oh-my-openagent-proxy`：Agent 编排与运行时治理

该项目提供了更强的 Agent 运维启发：

- agent/model 匹配、fallback model、模型能力表。
- rules/AGENTS 注入、动态上下文剪枝、compaction。
- tool metadata store、write-existing-file guard、webfetch redirect guard。
- tmux subagent、background task、circuit breaker。
- doctor/status/sandbox 类诊断能力。

对本项目的启发：

- KingdeeKB 的目标用户不是开发者 Agent，而是 ERP 实施顾问，因此无需照搬代码编辑工具链。
- 值得吸收的是“能力元数据”“运行状态诊断”“写入保护”“远程访问限制”“后台任务断路器”。
- 对技能脚本和外部 MCP 的治理应进入统一 Agent 安全策略，而不是散落在单个工具实现里。

### 2.4 `superpowers`：技能与流程治理

`superpowers` 的价值在于技能工作流、计划、审查和验证闭环：

- 技能是显式工作流，不只是 prompt 片段。
- 计划、执行、验证、代码审查有独立阶段。
- 技能测试通过触发用例和多轮对话验证。

对本项目的启发：

- 当前 `use-skill + run-skill-script` 已接近该方向。
- 需要补齐技能质量门禁：安装校验、脚本白名单、产物协议、UAT 样例、失败模式沉淀。
- 项目规则要求用户业务技能安装在项目根目录 `skills/`，不能进入 `.opencode/skills/`。

---

## 3. Rig Agent 助手目标设计

### 3.1 产品定位

Agent 助手面向金蝶 ERP 实施顾问，核心能力是：

1. 基于项目知识库回答产品、实施、调研、方案问题。
2. 读取和使用业务技能，生成交付物、PPT、文档、清单、话术。
3. 协助调研访谈，推荐问题、整理纪要、沉淀大纲。
4. 执行风控分析，识别范围蔓延、项目健康风险、合同条款冲突。
5. 在安全边界内运行工具，不泄露敏感信息，不越权访问项目数据。

### 3.2 架构分层

```text
前端 Chat / 调研 / 风控 / 产物页
  ↓
AgentContext：slot、消息、流式事件、取消、澄清、验证
  ↓
Tauri command：agent_chat / cancel_agent_stream / agent 工具配置
  ↓
Agent Runtime
  ├─ 模式路由：RAG / ReAct / Plan-Execute
  ├─ Rig Client：OpenAI / Anthropic / Ollama
  ├─ Prompt Assembler：系统约束、项目上下文、工具策略、技能规则
  ├─ Tool Registry：内置工具、运行时工具、技能工具、风控工具
  ├─ Tool Guard：schema、权限、速率、审计、截断、留存、沙箱
  ├─ Session Ledger：消息、工具调用、工具结果、取消、恢复
  └─ Verification：回答验证、风险报告验证、产物检查
  ↓
业务服务：知识库、技能、风控、调研、产物、项目、LLM Provider
```

### 3.3 Agent 模式

| 模式 | 适用场景 | 当前状态 | 建议 |
| --- | --- | --- | --- |
| RAG Chat | 简单知识问答、检索解释 | `LLMService::rag_query_to_sender` 存在 | 作为轻量模式保留，但统一模型选择和敏感信息策略 |
| ReAct | 默认多工具任务 | `RigAgent::run` 已实现 | 作为主路径强化治理 |
| Plan-Execute | 多步骤交付物、复杂分析 | `Planner` 已实现但执行阶段偏文本化 | 让步骤执行也走 Rig 工具，而不是普通 `chat_completion` |
| Clarification | 信息不足、脚本授权、业务确认 | `question` 工具存在 | 统一为运行时阻塞工具并进入 session ledger |
| Verification | 事实核验、风险校验、产物检查 | 前端 done 后触发 | 后端应把验证结果作为可追溯事件持久化 |

---

## 4. 核心设计

### 4.1 Provider 与模型能力

设计要求：

- Provider 配置只描述 endpoint、protocol、api key、model、默认项。
- Provider Policy 决定是否允许使用某供应商或模型。
- Model Capability 描述模型是否支持工具调用、多模态、thinking、上下文窗口、最大输出。
- Agent 路由必须先看 policy，再看 capability，最后看默认配置。

建议新增数据结构：

```jsonc
{
  "policies": [
    { "effect": "deny", "action": "provider.use", "resource": "*" },
    { "effect": "allow", "action": "provider.use", "resource": "company-ai" }
  ],
  "model_capabilities": {
    "company-ai:qwen-plus": {
      "supports_tools": true,
      "supports_vision": false,
      "context_window": 32768,
      "max_output_tokens": 8192
    }
  }
}
```

当前对照：

- `LLMProviderConfig` 已有 `protocol`、`api_keys`、`models`、`max_tokens`、`temperature`。
- `ModelConfig` 已有 `context_window`、`max_output_tokens`、`supports_thinking`、`is_multimodal`。
- 缺口是 policy 层和运行时强制执行。

### 4.2 Tool Registry 与工具身份

设计要求：

- 工具定义应有稳定 ID、类别、effect、schema、是否可禁用、是否可重试。
- 每次 provider turn 广告工具时记录 advertised_tool_revision。
- 工具执行时带入 session_id、assistant_message_id、tool_call_id、tool_revision。
- 如果工具被禁用、替换或权限变化，旧调用应拒绝执行，返回 stale tool call。

当前对照：

- `RigToolProfile` 已覆盖 `id/effect/retry/schema_guard/audit/disable_allowed`。
- `filter_disabled_rig_tools` 已支持禁用工具。
- `ToolGuardWrapper` 已覆盖 schema guard、审计、输出边界。
- 缺口是“工具广告与调用的 durable identity”，当前更多依赖 Rig stream 内存态。

### 4.3 知识检索工具

设计要求：

- `search-knowledge` 必须始终绑定当前项目。
- 搜索结果进入 `<context>` 或等价隔离块，明确为参考材料。
- 检索结果要保留 source id、chunk id、score、section、document title。
- Agent 回答必须引用来源或声明没有足够证据。

当前对照：

- `rig_agent.rs` 已注入当前项目名称，并要求工具限定项目范围。
- `LLMService::assemble_context` 已输出 `[chunk:id | title | section]`。
- `build_user_prompt` 已使用 `<context>` 隔离。
- 缺口是工具审计和最终回答之间的来源链路还不够完整，前端从文本中解析 sources，较脆弱。

建议：

- 工具结果返回结构化 JSON，同时提供模型可读摘要。
- 前端 sources 从结构化 tool result 读取，不再用正则解析中文文本。

### 4.4 技能系统与脚本执行

设计要求：

- 技能安装位置固定为项目根目录 `skills/<skill-name>/SKILL.md`。
- Agent 必须先 `use-skill` 再 `run-skill-script`。
- 脚本只能执行已扫描到的 `scripts/` 内文件。
- 执行前展示 plan，并经 `SkillScript(skill:script)` 权限规则判断。
- 每次执行在独立 sandbox，输入文件写入 sandbox，产物写入 output 目录。
- 脚本环境变量只暴露必要目录和上下文。

当前对照：

- `RunSkillScriptTool` 已要求先使用 skill，已有权限询问/持久允许/持久拒绝。
- 已有 sandbox/output、输入文件写入、参数校验、路径校验、shell token 拦截。
- 已有 `setup-skill-env` 处理依赖诊断和安装。
- 缺口是脚本产物协议需要更稳定：哪些文件被登记为产物、如何关联 ProductStore、如何回放执行记录。

建议：

- 产物登记以结构化 manifest 为准：`product_id`、`kind`、`path`、`source_skill`、`script`、`inputs_hash`。
- `run-skill-script` 输出同时包含模型摘要与机器可读 manifest。

### 4.5 Plan-Execute

当前 `Planner` 能生成计划、状态机推进、检测失败关键词和计划漂移，但 `try_plan_execute` 的单步执行是普通 `llm.chat_completion`，没有真正执行工具调用。

建议升级为：

```text
Planner 生成 steps
  ↓
每个 step 构造 step-scoped system prompt
  ↓
Rig agent with allowed_tools = step.tool 或工具集合
  ↓
ToolGuardWrapper + StepConstraintChecker
  ↓
StepResult 持久化
  ↓
Verifier 判断 pass/fail/need_replan
```

这样 Plan-Execute 才能真正用于交付物生成、知识检索、风控分析等多工具任务。

### 4.6 会话与恢复

设计要求：

- 会话消息、工具调用、工具结果、取消、错误应进入可回放日志。
- 流式 delta 可是临时事件，但工具 settlement 和最终消息必须可恢复。
- 取消应停止当前执行链，保留已完成工具结果，不重放不明确副作用。
- 前端刷新后应能恢复最后一次完整会话状态。

当前对照：

- 前端有 `sessionToSlot` 内存映射和取消。
- 后端有 `AgentCancelFlag` 和 `cancel_agent_stream`。
- 工具审计已有 JSONL，但不是完整 session ledger。
- 缺口是 durable session store。

建议新增表：

```sql
agent_sessions(id, project_id, status, provider_id, model_id, created_at, updated_at)
agent_messages(id, session_id, role, content, status, created_at)
agent_tool_calls(id, session_id, assistant_message_id, tool_name, args_json, status, created_at)
agent_tool_results(id, tool_call_id, result_json, preview_text, output_path, created_at)
agent_events(id, session_id, event_type, payload_json, created_at)
```

项目尚未发布，新增时直接采用当前格式，不需要旧格式迁移或双协议读取。

---

## 5. 风控/安全设计

这里必须拆成两类：业务风控和 Agent 运行安全。

### 5.1 业务风控

业务风控目标：

- 合同范围抽取：从合同、SOW、蓝图等文档提取范围内/范围外条目。
- 范围蔓延识别：新需求对照合同范围，输出红黄绿评级。
- 项目健康评分：缺席率、数据延迟、问题积压、客户配合度等指标。
- 防身话术：为顾问生成专业、得体、有据可依的沟通话术。
- 风险报告：生成项目风险摘要、证据、建议行动。

当前对照：

- `RiskControlStore` 已有 `contract_scope_items`、`project_health_metrics`。
- 已有 `check_scope_creep`、`calculate_health_score`、`generate_risk_report`、`generate_defense_script`。
- 已有 LLM JSON 提取、截断数组 salvage、候选条目确认入库。

缺口：

- 风险结论需要证据链：合同条款 chunk、项目指标记录、用户输入、LLM 判断。
- 风险等级算法应区分“规则确定”和“LLM 建议”。
- 风险报告应进入可审计产物，不能只作为聊天文本。

建议：

- 风控工具返回结构化 evidence：

```jsonc
{
  "risk_level": "red",
  "confidence": 0.86,
  "evidence": [
    { "type": "scope_item", "id": 12, "quote": "..." },
    { "type": "metric", "id": 7, "value": 80 }
  ],
  "llm_reasoning_summary": "...",
  "recommended_action": "..."
}
```

### 5.2 Agent 运行安全

运行安全风险清单：

| 风险 | 当前控制 | 缺口 | 建议 |
| --- | --- | --- | --- |
| Prompt 注入 | `<context>` 隔离、工具策略 prompt | 缺结构化来源策略 | 对知识库/附件/skill 内容统一标记 untrusted |
| 越权项目访问 | 系统提示要求当前项目 | 工具层需强制 project_id | 工具参数忽略模型传入项目或只允许当前项目 |
| 工具滥用 | 速率限制、死循环检测、schema guard | 缺全局预算 | 增加每会话工具次数、成本、耗时预算 |
| 工具输出撑爆上下文 | 输出截断、留存 | 已较完善 | 把完整输出路径隐藏为 managed reference |
| 技能脚本越权 | sandbox、权限规则、参数校验 | 不是 OS 级沙箱 | 明确标注为应用级沙箱，补进程/网络/文件访问策略 |
| 供应商误用 | provider 配置 | 缺 provider policy | 加 allow/deny 策略和组织级锁定 |
| API Key 泄露 | keyring 与配置分离部分存在 | `llm_providers.json` 中仍含 key 字段 | 统一迁移到 keyring 或加密存储 |
| 敏感数据出云 | desensitize_messages 存在 | 覆盖范围待核查 | 发送云模型前统一脱敏，并允许本地模型优先 |
| 外部 URL/MCP | 腾讯会议 MCP 存在 | 缺统一外联策略 | 对 MCP、webfetch、模型 endpoint 统一域名 allowlist |
| 产物写入污染 | sandbox output | ProductStore 关联不足 | 只登记 output 内白名单扩展名文件 |
| 会话恢复重放副作用 | 取消存在 | 无 durable activity identity | 已执行副作用工具不得自动重放 |

### 5.3 技能脚本安全边界

当前脚本执行边界是“应用级沙箱”，不是强制 OS 容器沙箱。文档和 UI 必须明确：

- 能限制 Agent 传入的脚本路径、参数、输入文件和输出目录。
- 能要求用户授权某个 skill/script。
- 能审计脚本 stdout/stderr、退出码、输出文件。
- 不能天然阻止脚本访问宿主机其他路径或网络，除非后续引入 OS 级沙箱、受限进程权限、容器或 Windows Job/AppContainer。

短期建议：

- 默认禁止脚本联网，必要时单独授权 `SkillNetwork(skill:script:domain)`。
- 默认禁止脚本读取 sandbox、skill_dir、data_dir 之外路径。
- 对 PowerShell/Bash 加更严格执行参数，不允许 shell 控制字符已经是正确方向。
- 执行前展示脚本解释器、脚本路径、参数、输入文件列表、输出目录、预计权限。

### 5.4 数据安全

建议对数据分级：

| 等级 | 数据 | 默认策略 |
| --- | --- | --- |
| P0 | API Key、客户账号、数据库凭据 | 只进 keyring，不进日志/工具结果/LLM prompt |
| P1 | 合同、SOW、报价、客户组织架构 | 云模型前脱敏，默认只在当前项目检索 |
| P2 | 实施方案、调研纪要、会议转写 | 可入知识库，但需项目隔离 |
| P3 | 通用金蝶产品资料 | 可跨项目复用，但要标明来源 |

---

## 6. 本项目逐项对照

| 设计项 | 成熟项目做法 | 本项目现状 | 差距 | 优先级 |
| --- | --- | --- | --- | --- |
| Rig 接入 | `rig037-probe` 验证 0.37 流式调用 | 已接入 `rig-core = 0.37` | live probe 未产品化 | P1 |
| Provider 兼容 | Rig client builder + custom base_url | `rig_provider.rs` 已实现 | policy 缺失 | P0 |
| 模型能力 | 模型能力表、自动路由 | `ModelConfig` 已有部分字段 | 能力未统一强制 | P1 |
| 工具注册 | opencode 作用域注册 | `all_rig_tools/runtime_rig_tools` | 缺 revision/stale call | P1 |
| 工具 schema | codec 边界 | `ToolGuardWrapper` schema guard | schema 错误可观测性可增强 | P1 |
| 工具输出 | 统一截断与留存 | 已有 `agent_tool_outputs` | 缺 managed reference 抽象 | P2 |
| 工具审计 | tool metadata store | 已有 JSONL 和设置页 | 缺 session/message/tool_call 绑定 | P0 |
| 会话持久化 | durable inbox/history/events | 前端内存态为主 | 缺 session ledger | P0 |
| 取消恢复 | interrupt + settlement | 有 cancel flag | 副作用恢复策略缺失 | P1 |
| 上下文压缩 | 自动 compaction | `compress_conversation` 简化摘要 | 未持久化、未按模型预算 | P2 |
| 规则注入 | AGENTS/rules 源 | 系统 prompt + tool_policy | 缺来源优先级与变更记录 | P2 |
| 技能系统 | superpowers workflow | 已有 skill manager/tools | 缺技能 UAT 和产物 manifest | P1 |
| 脚本权限 | permission ask/save | 已有 SkillScript allow/deny | 缺网络/外部路径权限 | P0 |
| 业务风控 | 领域工具 | 风控服务较完整 | 证据链与审计不足 | P0 |
| 验证层 | reviewer/checker | done 后 runVerification | 验证结果未持久化 | P1 |
| 前端轨迹 | timeline/tool trace | AgentContext 有 trace | 缺可回放历史 | P1 |
| 路由验证 | 先查路由再改页面 | 项目规则已要求 | 本文档无页面修改 | 已满足 |

---

## 7. 推荐实施路线

### 阶段一：治理底座（P0）

1. 新增 Provider Policy，并在模型选择、Agent 运行、连接测试处强制执行。
2. 新增 Agent Session Ledger 表，持久化消息、工具调用、工具结果、最终状态。
3. 工具审计记录绑定 `session_id`、`assistant_message_id`、`tool_call_id`。
4. 风控工具结果返回 evidence，并将风险报告登记为产物或审计记录。
5. 技能脚本补外部路径/网络权限策略，默认拒绝。

验收标准：

- 禁用某 provider 后，前端不可选、后端不可用、Agent 自动路由不可绕过。
- 一次 Agent 运行后，能在本地数据库回放完整消息和工具调用链。
- `run-skill-script` 执行记录能追溯到用户消息、脚本、参数、输出文件和授权规则。

### 阶段二：Agent 执行质量（P1）

1. Plan-Execute 单步执行改为 step-scoped Rig agent。
2. 工具调用加入 allowed_tools、step_id、预算和超时。
3. `search-knowledge` 返回结构化来源，前端移除正则解析依赖。
4. 验证报告持久化，并支持按回答/产物查看。
5. 增加 Rig live probe 和工具 schema 集成测试。

验收标准：

- 多步骤交付物生成能展示计划、每步工具、每步结果和最终产物。
- 工具循环、工具错配、超预算能被明确中断并给出可恢复提示。
- 前端来源引用来自结构化数据而非文本猜测。

### 阶段三：平台化扩展（P2）

1. 引入 managed output reference，隐藏真实审计文件路径。
2. 上下文压缩持久化，支持刷新后恢复。
3. 技能包 UAT：安装、触发、脚本执行、产物校验。
4. Agent 诊断页：Provider、模型能力、工具启停、权限规则、最近失败、预算。
5. MCP/外部连接统一治理。

验收标准：

- 设置页能清楚显示 Agent 当前可用能力和安全策略。
- 技能失败可通过审计和 UAT 定位，不依赖用户复述。
- 外部连接均能被 allowlist/denylist 控制。

---

## 8. 不建议做的事

- 不建议再保留旧模板向导、旧模板生成工具或双协议尝试；项目尚未发布，应直接统一到 skill 产物生成语义。
- 不建议让 LLM 自行拼 shell 命令执行技能脚本；必须固定 `run-skill-script`。
- 不建议只靠 prompt 要求“限定当前项目”；项目隔离必须在工具服务层强制。
- 不建议把完整工具输出直接塞回模型上下文；继续使用截断预览和托管输出。
- 不建议把风控结果只当聊天文本；风险结论必须有证据、置信度、审计记录。

---

## 9. 参考资料

- Rig 官方文档：https://docs.rig.rs/
- Rig 官方站点：https://rig.rs/
- Rust API 文档：https://docs.rs/rig-core/latest/rig/
- 本地 Rig 探针：`E:\tmp\rig037-probe`
- 本地 opencode 规范：`E:\tmp\opencode\specs\v2\tools.md`、`E:\tmp\opencode\specs\v2\session.md`、`E:\tmp\opencode\specs\v2\provider-policy.md`
- 本地 OpenAgent Proxy：`E:\tmp\oh-my-openagent-proxy`
- 本地 Superpowers：`E:\tmp\superpowers`
