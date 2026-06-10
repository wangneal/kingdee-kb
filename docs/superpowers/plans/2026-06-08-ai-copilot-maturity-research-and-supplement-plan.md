# 成熟 AI 副驾产品调研与 KingdeeKB 补齐计划

> 日期：2026-06-08  
> 范围：企业 AI 副驾成熟度、Agent 治理、安全、审计、知识 grounding、业务工作流闭环  
> 前置规格：`docs/superpowers/plans/2026-06-01-kingdeekb-technical-spec.md`  
> 衔接文档：`docs/superpowers/plans/2026-06-08-rig-agent-assistant-design-and-gap-analysis.md`

---

## 1. 结论

KingdeeKB 当前已经具备“可运行的 AI 副驾主路径”，但还没有达到成熟产品标准。成熟企业 AI 副驾的共同特征不是单纯的模型能力，而是：

1. **权限继承**：AI 只能访问当前用户、当前项目、当前业务上下文允许访问的数据。
2. **Grounding 可解释**：回答必须能回溯到知识、文件、业务对象或工具结果。
3. **动作可治理**：工具、脚本、外部连接、写入动作都必须有 policy、预算、审批、审计。
4. **会话可恢复**：刷新、取消、失败后，能还原消息、工具调用、结果和副作用状态。
5. **审计可追责**：能回答“谁在什么时候，让哪个 Agent，用什么权限，对什么数据做了什么”。
6. **质量可评估**：有回归集、UAT、验证报告、失败模式沉淀，而不是靠演示效果判断。

因此 KingdeeKB 的补齐方向应从“继续加工具”切换为“先补治理底座，再提升自动化深度”。

---

## 2. 调研对象与成熟做法

### 2.1 Microsoft 365 Copilot

成熟做法：

- 通过 Microsoft Graph 做工作数据 grounding，并受用户已有权限限制。
- 继承 Microsoft 365 的身份、权限、敏感度标签、保留策略和审计能力。
- 提供 Copilot Control System，用于集中管理访问、数据保护、日志、生命周期策略。

对 KingdeeKB 的启发：

- 项目隔离不能只靠 prompt，要在工具层强制绑定 `project_id`。
- 所有检索来源、附件来源、工具来源要进入统一来源链路。
- 管理页需要集中展示 Agent 能力、安全策略、工具启停和审计状态。

### 2.2 Salesforce Agentforce

成熟做法：

- 基于 Einstein Trust Layer 提供安全架构、guardrails、审计与反馈。
- Agent action 依赖 Salesforce 原生 Flow、Apex、Prompt Template 等受控动作。
- 审计追踪覆盖 prompt、response、trust signal、action output。

对 KingdeeKB 的启发：

- 技能脚本不能只是“模型能调用”，必须变成有声明、有授权、有产物协议的受控 action。
- Agent 执行动作时要记录业务身份：session、assistant message、tool call、action、输入、输出。
- 风控、产物、调研写入动作都应有结构化结果，而不是只返回聊天文本。

### 2.3 ServiceNow Now Assist / AI Control Tower

成熟做法：

- 通过 AI Control Tower 管理 AI agent、模型、工作流、策略、生命周期和合规。
- 强调 agent traceability、activity logs、runtime guardrails、role masking。
- 把 AI 运维作为平台能力，而不是散落在每个功能里。

对 KingdeeKB 的启发：

- 需要 Agent 诊断/治理页，集中查看 provider、模型能力、工具调用、失败、预算、权限规则。
- 技能脚本、MCP、外部 API、LLM endpoint 应进入同一外联治理模型。
- 工具执行需要全局预算：每会话工具次数、耗时、输出大小、外部连接次数。

### 2.4 SAP Joule

成熟做法：

- 定位为理解业务系统的企业 copilot，重点在业务应用内完成任务。
- 支持基于 SAP Help、业务系统、客户文档的 grounding。
- 强调客户保留决策控制权和数据隐私。

对 KingdeeKB 的启发：

- KingdeeKB 的成熟方向不是通用聊天，而是“实施顾问业务工作台”。
- 调研、蓝图、风险、产物都应成为业务对象，Agent 只是在对象上协助推进。
- 每个 Agent 建议都应能落到项目阶段、调研问题、风险条目、知识页面或产物记录。

### 2.5 Google Gemini for Workspace / Gemini Enterprise

成熟做法：

- 管理员可控制 Gemini 是否访问 Workspace 应用数据。
- 提供 Gemini for Workspace log events，用于审计用户与 Gemini 的交互。
- 对 prompt injection、恶意内容、数据访问提供管理与安全说明。

对 KingdeeKB 的启发：

- LLM provider、外部连接、知识库检索、附件读取都需要管理员级开关。
- 日志不能只记录错误，还要记录正常交互和工具访问。
- prompt injection 防护要明确“非可信输入”边界：知识库、附件、网页、技能说明都默认不可信。

### 2.6 Glean / Atlassian Rovo / ChatGPT Enterprise

成熟做法：

- Glean 强调企业搜索与 Assistant 统一，遵守数据访问权限，管理员可排除文档源。
- Rovo agent 通过组织管理控制 agent 访问范围，agent 只能访问或修改用户有权限的数据。
- ChatGPT Enterprise/Business connectors 强调 OAuth、按用户权限访问、app 调用日志、合规日志。

对 KingdeeKB 的启发：

- 知识源需要显式启停、排除和安全分级。
- Agent 身份与用户身份要区分：用户授权触发，Agent 按受限权限执行。
- connector/MCP/app calls 都要有统一日志和可回放事件。

---

## 3. 成熟副驾能力模型

| 层级 | 能力 | 成熟标准 | KingdeeKB 当前状态 |
| --- | --- | --- | --- |
| L1 可用 | 对话、流式输出、RAG、基础工具 | 能回答和调用工具 | 已基本具备 |
| L2 可信 | 权限继承、结构化来源、敏感信息保护 | 不越权，回答可追溯 | 部分具备 |
| L3 可治理 | Provider Policy、工具策略、预算、审批 | 管理员可控，动作受限 | 明显不足 |
| L4 可恢复 | 会话 ledger、工具 settlement、副作用记录 | 刷新/取消/失败可恢复 | 不足 |
| L5 可审计 | session/message/tool/action 全链路审计 | 可追责、可合规导出 | 不足 |
| L6 可运营 | UAT、回归集、诊断页、质量指标 | 可持续迭代和上线 | 不足 |
| L7 业务闭环 | 调研、风控、产物、项目阶段对象化 | 不只是聊天，而是推进业务对象 | 部分具备 |

目标：先从 L2/L3 补齐到 L5，再推进 L6/L7。

---

## 4. KingdeeKB 目标设计

### 4.1 Agent Governance Core

新增统一治理核心：

```text
AgentGovernance
  ├─ ProviderPolicy：供应商/模型 allow/deny
  ├─ ModelCapability：工具、多模态、上下文、thinking 能力
  ├─ ToolPolicy：工具启停、预算、effect、审批要求
  ├─ DataPolicy：数据分级、项目隔离、脱敏、来源可信度
  ├─ NetworkPolicy：LLM endpoint、MCP、技能脚本联网 allowlist
  └─ AuditPolicy：日志级别、保留策略、导出范围
```

原则：

- 默认拒绝高风险能力，再逐项允许。
- policy 与配置分离：配置存在不等于允许使用。
- prompt 只能表达意图，强制约束必须在工具/服务层执行。

### 4.2 Agent Session Ledger

新增持久会话账本，作为恢复、审计、验证的唯一事实源：

```sql
agent_sessions(
  id TEXT PRIMARY KEY,
  project_id INTEGER NOT NULL,
  slot TEXT NOT NULL,
  status TEXT NOT NULL,
  provider_id TEXT,
  model_id TEXT,
  started_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  ended_at TEXT
);

agent_messages(
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  status TEXT NOT NULL,
  parent_message_id TEXT,
  created_at TEXT NOT NULL
);

agent_tool_calls(
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  assistant_message_id TEXT,
  tool_name TEXT NOT NULL,
  tool_revision TEXT NOT NULL,
  effect TEXT NOT NULL,
  args_json TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT
);

agent_tool_results(
  id TEXT PRIMARY KEY,
  tool_call_id TEXT NOT NULL,
  result_json TEXT NOT NULL,
  preview_text TEXT NOT NULL,
  output_ref TEXT,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL
);

agent_events(
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);
```

不做旧协议兼容。项目尚未发布，直接采用当前结构。

### 4.3 结构化来源与证据链

所有 grounding 来源统一返回：

```jsonc
{
  "source_type": "kb_chunk | attachment | wiki_page | risk_scope | health_metric | tool_output",
  "source_id": "string",
  "project_id": 1,
  "title": "string",
  "section": "string",
  "snippet": "string",
  "score": 0.82,
  "trusted": false
}
```

要求：

- `search-knowledge` 返回结构化 sources。
- 前端引用不再从中文文本正则解析。
- 风控结论必须携带 evidence。
- 产物必须携带 inputs、source_skill、script、output manifest。

### 4.4 技能与动作协议

技能脚本输出必须包含机器可读 manifest：

```jsonc
{
  "schema_version": "kingdeekb.product_manifest.v1",
  "source_skill": "skill-name",
  "script": "scripts/build_report.py",
  "inputs_hash": "sha256",
  "outputs": [
    {
      "kind": "docx",
      "path": "output/report.docx",
      "title": "项目风险报告",
      "register_as_product": true
    }
  ],
  "warnings": []
}
```

执行规则：

- 必须先 `use-skill` 再 `run-skill-script`。
- 脚本只能来自已扫描 skill 的 `scripts/`。
- 默认禁止联网，默认禁止读取 sandbox、skill_dir、data_dir 外部路径。
- 每次执行绑定 `agent_session_id`、`tool_call_id`、`permission_rule`。

### 4.5 风控证据链

风险结论结构：

```jsonc
{
  "risk_level": "green | yellow | red",
  "confidence": 0.86,
  "decision_type": "rule | llm_suggestion | mixed",
  "evidence": [
    { "type": "scope_item", "id": 12, "quote": "..." },
    { "type": "health_metric", "id": 7, "value": 80 }
  ],
  "summary": "string",
  "recommended_action": "string",
  "product_ref": "optional"
}
```

成熟标准：

- 风险报告可在产物页查看。
- 每条风险都有证据和生成记录。
- LLM 建议和规则判断明确区分。

---

## 5. 补齐计划

### 阶段 A：治理底座闭环（P0，建议 5-7 天）

目标：把“能运行”升级为“可治理、可追责”。

任务：

1. 新增 Provider Policy。
   - 后端强制：Agent 运行、连接测试、模型选择都必须检查 policy。
   - 前端设置页显示 allow/deny 状态。
   - 禁用 provider 后，Agent 不得绕过默认模型继续使用。

2. 新增 Agent Session Ledger。
   - 写入 session、messages、tool_calls、tool_results、events。
   - `question` 澄清、取消、错误都进入 ledger。
   - 前端刷新后能恢复最后一次完整会话。

3. 工具审计绑定会话身份。
   - 审计记录增加 `session_id`、`assistant_message_id`、`tool_call_id`。
   - 工具执行前校验 tool revision，拒绝 stale call。

4. 技能脚本安全策略。
   - 增加 `SkillNetwork`、`SkillExternalRead` 权限规则。
   - 默认拒绝联网和外部路径读取。
   - 执行前展示解释器、脚本、参数、输入、输出目录、权限。

5. 风控 evidence 结构化。
   - `check_scope_creep`、`generate_risk_report` 输出 evidence。
   - 风险报告登记为产物或审计记录。

验收：

- 禁用 provider 后前后端都不可用。
- 一次 Agent 对话可回放消息、工具、结果、错误。
- 技能脚本执行记录能追溯到用户消息和授权规则。
- 风险报告能看到证据链。

### 阶段 B：执行质量闭环（P1，建议 5-8 天）

目标：减少“看起来会做但不可控”的 Agent 行为。

任务：

1. Plan-Execute 单步执行改为 step-scoped Rig agent。
2. 为每个 step 增加 allowed_tools、预算、超时。
3. `search-knowledge` 返回结构化 sources。
4. 前端 sources 从 tool result 读取。
5. 验证报告持久化到 ledger。
6. 增加 Rig live probe 和工具 schema 集成测试。

验收：

- 多步骤产物生成能展示每步计划、工具、结果、最终产物。
- 工具超预算、循环、错配能被明确中断。
- 来源引用不依赖正则解析文本。

### 阶段 C：运营与质量门禁（P1/P2，建议 4-6 天）

目标：让副驾可以持续迭代，而不是靠人工体验判断。

任务：

1. Agent 诊断页。
   - provider、模型能力、工具启停、权限规则、最近失败、预算使用。

2. 技能 UAT。
   - 安装校验、触发样例、脚本执行、产物 manifest 校验。

3. 回归集。
   - 知识问答、技能执行、调研澄清、风险判断、取消恢复。

4. 失败模式库。
   - schema 错误、权限拒绝、超时、无证据回答、幻觉、脚本失败。

验收：

- 每次发版前能跑 Agent 回归集。
- 失败可定位到 provider、工具、权限、数据源或 prompt。
- 设置页可解释当前副驾为什么能做/不能做某件事。

### 阶段 D：业务闭环增强（P2，建议 6-10 天）

目标：从“聊天助手”升级为“实施顾问工作流副驾”。

任务：

1. 调研对象化。
   - 问题推荐、回答记录、纪要、大纲节点、待确认事项进入统一对象。

2. 产物 manifest 注册。
   - 产物页按项目、阶段、技能、输入来源展示。

3. 风控闭环。
   - 风险条目、证据、客户沟通话术、行动项、报告统一关联。

4. 项目阶段联动。
   - Agent 建议可落到项目阶段任务、风险、产物或调研待办。

验收：

- 用户能从一次对话直接沉淀调研记录、风险、产物或项目任务。
- 每个业务对象都能回查来源和生成过程。

---

## 6. 不做事项

- 不做旧模板向导兼容，直接统一到技能产物协议。
- 不做双协议读取或猜测旧格式。
- 不允许 LLM 直接拼 shell 命令执行脚本。
- 不把风控结果只保存为聊天文本。
- 不只靠 prompt 实现项目隔离、权限限制和安全策略。

---

## 7. 参考资料

- Microsoft 365 Copilot grounding：https://support.microsoft.com/en-us/Microsoft-365-Copilot/what-information-does-copilot-use-to-answer-my-prompt
- Microsoft 365 Copilot enterprise data protection：https://learn.microsoft.com/en-us/copilot/microsoft-365/enterprise-data-protection
- Microsoft Copilot Control System：https://www.microsoft.com/en-us/microsoft-365-copilot/copilot-control-system
- Salesforce Agentforce trust：https://help.salesforce.com/s/articleView?id=sf.copilot_trust.htm&language=en_US&type=5
- Salesforce Agentforce security model：https://help.salesforce.com/s/articleView?id=005315874&language=en_US&type=1
- ServiceNow agentic AI security and governance：https://www.servicenow.com/docs/r/platform-security/now-assist-security.html
- ServiceNow AI Control Tower：https://newsroom.servicenow.com/press-releases/details/2025/ServiceNow-Launches-AI-Control-Tower-a-Centralized-Command-Center-to-Govern-Manage-Secure-and-Realize-Value-From-Any-AI-Agent-Model-and-Workflow/
- SAP Joule overview：https://help.sap.com/docs/joule/serviceguide/enable-joule-in-sap-products
- SAP Joule integration architecture：https://architecture.learning.sap.com/docs/ref-arch/06ff6062dc
- Google Gemini Workspace audit logs：https://support.google.com/a/answer/14521388
- Google Gemini Workspace data access control：https://support.google.com/a/answer/16479199
- Google Gemini Workspace privacy hub：https://support.google.com/a/answer/15706919
- Glean Admin Console：https://docs.glean.com/administration/about
- Glean Assistant setup：https://docs.glean.com/get-started/golive/setup-glean-assistant
- Atlassian Rovo agent management：https://support.atlassian.com/organization-administration/docs/manage-agents-in-your-organization/
- Atlassian AI trust：https://www.atlassian.com/trust/ai
- OpenAI Enterprise connectors security：https://help.openai.com/en/articles/11509118-admin-controls-security-and-compliance-in-connectors-enterprise-edu-and-team
- OpenAI enterprise privacy：https://openai.com/enterprise-privacy

---

## 8. 执行记录

### 2026-06-08 P0 治理底座首轮

已完成：

1. Provider Policy。
   - 后端新增 provider/model allow/deny policy，并在默认模型、指定模型、多模态候选、连接配置与运行态列表中强制过滤。
   - 前端设置页显示供应商运行态 allow/deny 状态，Chat 模型选择只展示运行态允许的 provider/model。

2. Agent Session Ledger。
   - `metadata.db` 新增 `agent_sessions`、`agent_messages`、`agent_tool_calls`、`agent_tool_results`、`agent_events`。
   - `agent_chat` 启动、用户消息、assistant 流式内容、工具调用、工具结果、澄清、取消、错误、完成均写入账本。
   - 新增 `get_latest_agent_session`、`get_agent_session` 命令，Chat 页面刷新后优先从账本恢复当前项目最近一次 `chat` 会话，失败时回退本地历史。

3. 工具审计会话绑定。
   - `agent_tool_outputs/tool_calls.jsonl` 审计记录增加 `session_id`、`assistant_message_id`、`tool_call_id` 字段。
   - 运行时工具守卫为每次工具执行生成 `tool_call_id`，并写入当前 Agent `session_id`。
   - 账本中的工具调用记录写入 `tool_revision=rig-tool-profile-v1`，用于后续工具 profile 版本校验。

已验证：

- `cargo check`
- `npx tsc --noEmit`
- `npx biome check src\pages\Chat.tsx src\contexts\AgentContext.tsx src\lib\tauri-commands.ts e2e\mocks\tauri-mock.ts`
- `npx playwright test e2e/chat.spec.ts e2e/settings.spec.ts`
- `cargo test metadata::tests`
- `cargo test rig_tool::tests`

下一步：

1. 技能脚本安全策略：补齐 `SkillNetwork`、`SkillExternalRead` 权限规则，执行前展示解释器、脚本、参数、输入、输出目录和权限。
2. 风控 evidence 结构化：让 `check_scope_creep`、`generate_risk_report` 输出证据链并登记产物或审计记录。
3. 工具 profile 版本校验：将 `tool_revision` 从账本证据推进到工具 schema / runtime guard 的强校验。
