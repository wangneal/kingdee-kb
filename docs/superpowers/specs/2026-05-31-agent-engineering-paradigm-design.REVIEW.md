# 三层 AI 工程范式落地设计 — 审查报告

**审查人**: Sisyphus
**审查日期**: 2026-05-31（终审）
**覆盖文档**:
- 设计规格：`docs/superpowers/specs/2026-05-31-agent-engineering-paradigm-design.md`
- 架构文档：`docs/ARCHITECTURE.md`
- 实现计划：`docs/superpowers/plans/2026-05-31-agent-engineering-paradigm.md`
- 实际代码：`src-tauri/src/`

---

## 审查范围

- 架构合理性评估（设计文档）
- 与现有代码库的一致性验证（设计文档 + 架构文档）
- 三层设计缺陷与风险（设计文档）
- 跨层集成问题（设计文档）
- 设计文档与架构文档的一致性（交叉比对）
- 实现计划的正确性、完整性与可执行性（计划文档）
- 实施策略评估（计划文档）
- 测试与错误处理策略（设计文档）

---

## 第一部分：设计规格审查

## 1. 总体评估

### 评分矩阵

| 维度 | 评分 | 说明 |
|------|------|------|
| 架构设计 | ⭐⭐⭐⭐½ | 三层边界清晰，抽象合理；Context → Plan → Harness 的依赖链正确 |
| 代码一致性 | ⭐⭐⭐⭐ | 初始两处文件路径错误已修正；ChatMessage 无 id/token_count 正确识别为 P0-a 任务 |
| 异常覆盖 | ⭐⭐⭐⭐ | 新增第 10 章后已有完整错误分类和恢复策略 |
| 测试策略 | ⭐⭐⭐⭐ | 新增第 11 章覆盖单元/集成/回归测试 |
| 可落地性 | ⭐⭐⭐⭐ | 分阶段划分合理，每阶段有明确文件清单 |
| 审查迭代 | ⭐⭐⭐⭐⭐ | 经过 3 轮审查、27/28 项问题已修复 |

### 核心强度

1. **层抽象合理** — Context → Plan → Harness 的依赖链清晰，每层解决不同维度的问题
2. **防漂移三重保障** — 程序化步进 + 最小上下文注入 + 漂移信号检测，设计用心
3. **渐进式实现** — P0 基础设施 → P1 核心逻辑 → P2 加固，阶段划分清晰
4. **流式规划（Speculative Execution）** — NDJSON 方案设计仔细，考虑了 ReadOnly vs Write 的区分

### 核心短板

1. **中英文混合 case 覆盖不足** — 关键词检测、失败信号检测等信号词覆盖不够全面
2. **Speculative Execution 并发兼容性未处理** — 部分 LLM 提供商有连接限制
3. **字符级硬上限安全阀未添加** — 防止 token 计数器极端异常

---

## 2. 代码一致性验证

基于 `src-tauri/src/` 实际代码交叉验证：

| 文档声称 | 实际代码 | 状态 |
|---------|---------|------|
| `ChatMessage` 有 `token_count` 字段 | 只有 `role` + `content`，无 `token_count` | ❌ 需在 P0-a 新增 |
| `PromptAssembler::estimate_tokens` 用独立启发式 | ✅ 确有两个实现：llm_service 用 tiktoken(u32)，prompt_assembler 用字符比例(usize) | ✅ 准确 |
| `llm_service::count_tokens` 用 tiktoken | ✅ `tiktoken_rs::cl100k_base`，回退 `chars/2.5` | ✅ 准确 |
| `build_prompt_with_history` 按字符截断 | ✅ `const MAX_HISTORY_CHARS: usize = 12_000` | ✅ 准确 |
| `compress_conversation` 阈值 2000 tokens | ✅ `COMPRESS_THRESHOLD = 2000` | ✅ 准确 |
| `LLMProviderConfig.max_tokens` 语义混乱 | ✅ `max_tokens: u32` 确实被同时用于上下文和输出 | ✅ 准确 |
| Tauri Command 在 `risk_control.rs` | ✅ 命令文件是 `commands/risk_blueprint.rs`，服务文件是 `services/risk_control.rs` | ⚠️ 设计文档已全部修正为 `risk_control.rs`，但实际命令文件是 `risk_blueprint.rs`（详见交叉审查） |

---

## 3. Context Engineering 层审查

### 3.1 Token 计算模块（P0-a）

| 设计决策 | 评价 |
|---------|------|
| 统一 `token.rs` 模块 | ✅ 正确。当前三套标准需要统一 |
| `count_tokens` 返回 `Result<u32, TokenError>` | ✅ 关键路径不静默降级 |
| `count_tokens_with_fallback` 区分中英文 | ✅ 1.5ch/token(中文) + 4ch/token(英文) 比当前 `chars/2.5` 更精确 |
| `truncate_to_tokens` 字节级二分查找 | ⚠️ 需确保 UTF-8 边界安全（计划已处理） |
| `count_messages_tokens` 缓存优先 | ✅ 缓存 miss → 精确计数 → 回退计数 |

**潜在问题**：`count_messages_tokens` 的 4 个结构开销 token 是硬编码的，对于多字段消息可能不准确。建议文档化这个假设。

### 3.2 模型元数据系统（P0-b）

| 设计决策 | 评价 |
|---------|------|
| 四层优先级（用户覆盖 > API 探测 > 内置 DB > 默认值） | ✅ 合理的渐进式降级策略 |
| 内置 `model_specs.json` | ✅ 覆盖主�的模型规格 |
| `from_provider_api` 支持 Anthropic/Gemini/Ollama | ✅ 覆盖主流 API 类型 |
| 默认值 4096 | ✅ 保守但安全，防止内存溢出 |

**潜在问题**：
- `from_provider_api` 中 `LLMProtocol::Local` 的引用（L483）未导入。`llm_providers.rs` 中该枚举名称需确认。👉 **实施时需检查枚举路径**。
- Ollama 的 `show` API 返回的 `model_info` 格式因模型而异，`context_length` 字段名可能变化。建议增加 `context_length` 同义词匹配。

### 3.3 上下文预算分配（P0-c）

| 设计决策 | 评价 |
|---------|------|
| 优先级驱动（数值越小越优先） | ✅ 设计合理 |
| 两轮分配（min 满足 → ideal 比例分配） | ✅ 正确。先保底线，再按理想比例分剩余 |
| `mode_mask` 支持不同模式不同预算 | ✅ 灵活设计 |
| Plan 优先级(4) > History(5) | ✅ Plan 应优先于历史记录 |
| `ContextBudget::calculate` 最终输出含 plan 字段 | ✅ 结构清晰 |

**潜在问题**：
- `build_claims` 的 `min_tokens` 和 `ideal_tokens` 值（如 `total / 20` 给 UserInput）尚未经过测试验证。这些默认值可能需要在实施时调整。
- 对于小窗口模型（如 4096），`total / 4` 给 Plan 只有 1024，可能不够复杂任务的执行计划。

### 3.4 分层摘要压缩（P0-d）

| 设计决策 | 评价 |
|---------|------|
| 磁滞回线三阈值（触发 80% / 释放 50% / 复位 30%） | ✅ 防震荡设计 |
| 增量摘要用消息 ID（`last_message_id: Option<String>`） | ✅ 比索引更鲁棒 |
| 摘要预算绝对值 1500 tokens | ✅ 避免百分比在大窗口下的成本失控 |
| 专用摘要模型 `model_tag: "summarization"` | ✅ 用轻量模型避免主模型成本翻倍 |
| `CompressionHysteresis::maybe_reset` 使用 `reset_threshold_pct` | ✅ 正确使用独立复位阈值 |

**潜在问题**：
- `"summarization"` 模型标签如何映射到实际模型？需要在 `llm_providers.rs` 或配置中增加 `summarization_model` 字段，建议在 P0-f 说明。
- `mark_critical_indices` 的信号词硬编码（`"错误"`, `"失败"`），缺乏英文版本。

---

## 4. Plan-and-Execute 层审查

### 4.1 模式路由（P1-a）

| 设计决策 | 评价 |
|---------|------|
| 复杂度评分指标覆盖语言类型、步骤数、条件分支、外部依赖 | ✅ 多维评分合理 |
| 阈值 20 可配置 | ✅ 便于调优 |
| 简单任务走 ReAct，复杂走 Plan-Execute | ✅ 合理权衡 |

### 4.2 Planner + PlanStateMachine（P1-b）

| 设计决策 | 评价 |
|---------|------|
| NDJSON 流式规划 | ✅ 支持逐步展示，用户体验好 |
| PlanStateMachine 四状态（Planned/Executing/Replanning/Completed） | ✅ 状态机设计紧凑 |
| `validate_dependencies` 依赖校验 | ✅ 防止步进死锁 |
| `should_replan` 中英文关键词 + 结构化 `StepStatus` | ✅ 已覆盖中英文 |
| `detect_step_drift` 文本信号 + ToolCall 匹配 | ✅ 双重检测 |
| `plan_with_speculative_exec` | ✅ ReadOnly 步骤并发执行提升效率 |

**潜在问题**：
- **U1：Speculative Execution 并发兼容性**（原 4.4，未完全解决）。Planner 流式输出和第一步推测执行都可能调用 LLMService。`reqwest::Client` 默认支持 4 路并发，但 Anthropic API 有连接限制。**建议**在实施 P1-b 时增加 `LLMService::concurrency_allowed()` 检查或使用独立连接。

### 4.3 Plan-Execute 执行循环（P1-c）

| 设计决策 | 评价 |
|---------|------|
| 状态机驱动循环 | ✅ 防止失控 |
| SSE 事件扩展：`PlanGenerated`/`StepStart`/`StepResult`/`Replan` | ✅ 前端可实时展示 |
| Planner 超时 10s → 自动降级 ReAct | ✅ 优雅降级 |
| `StepContext.remaining_count` 不暴露后续步骤 | ✅ 安全设计 |

---

## 5. Harness Engineering 层审查

### 5.1 程序化约束（P2-a）

| 设计决策 | 评价 |
|---------|------|
| `enforce_tool_constraint` 限制最大工具调用次数 | ✅ 防止失控 |
| `DOOM_LOOP_THRESHOLD = 3` 次相同调用中断 | ✅ 参考现有代码 `rig_agent.rs` |
| Ping-Pong 检测用 `normalized_call_key` + `HashSet` 禁止序列 | ✅ 防止 A→B→A→B 循环 |
| `normalized_call_key` 排除时间戳/随机数 | ✅ 参数规范化正确 |

### 5.2 验证循环（P2-b）

| 设计决策 | 评价 |
|---------|------|
| `ResultVerifier` 的 `max_consecutive_failures=3` → `Exhausted` → `NeedsReplan` | ✅ 有明确的状态转换 |
| 持续重试耗尽后触发 Replanner | ✅ 防止无限重试 |

### 5.3 熵管理（P2-c）

| 设计决策 | 评价 |
|---------|------|
| MVP 范围合理：技能过期扫描 + 文档-代码一致性 | ✅ 不过度设计 |
| 不用 LLM 做一致性检查 | ✅ 成本可控 |

---

## 6. 错误处理策略（第 10 章）

| 类别 | 评价 |
|------|------|
| 错误分类（Fatal/Retryable/Degradable/Reportable/Skippable） | ✅ 5 类定义清晰 |
| 各层边界 | ✅ 每层明确了错误类型和恢复策略 |
| 传播链 | ✅ 从工具失败到前端展示的完整路径 |

**建议补充**: 定义 Replanner 循环检测标准——同一步骤连续 replan 超过 2 次 = 循环。

---

## 7. 测试策略（第 11 章）

| 类型 | 评价 |
|------|------|
| 单元测试 | ✅ 模块覆盖全面，测试场景描述具体 |
| 集成测试 | ✅ 4 个关键联动场景已覆盖 |
| 回归测试 | ✅ 新旧 compressor 对比 + count_tokens 精度验证 |

**建议补充**: 为每个模块的测试添加"最低行覆盖率"目标（如 token.rs: 90%+, PlanStateMachine: 85%+）。

---

## 8. 设计文档三轮审查变更总结

| 轮次 | 发现总数 | 已修复 | 遗留 |
|------|---------|--------|------|
| 第 1 轮（初稿） | 21 | 21 | 0 |
| 第 2 轮（修正后） | 6 | 6 | 0 |
| 第 3 轮（终审） | 1 | — | 1 |

**唯一遗留项**：
- `maybe_reset` 未用 `reset_threshold_pct`（低优先级，不影响正确性）

---

---

## 第二部分：实现计划审查

## 9. 计划文档总体评估

| 维度 | 评分 | 说明 |
|------|------|------|
| 阶段划分 | ⭐⭐⭐⭐⭐ | P0→P1→P2 递进清晰，每阶段有明确文件清单 |
| 任务粒度 | ⭐⭐⭐⭐ | 多数任务分解到原子步骤，含代码片段和命令 |
| 代码正确性 | ⭐⭐⭐⭐ | 原 3 处阻断/高优先级错误已修正 2 处（见下文 10.1-10.3） |
| 依赖完整性 | ⭐⭐⭐ | 2 处遗漏依赖和文件引用 |
| 可执行性 | ⭐⭐⭐⭐ | 按步骤执行可到达编译通过（修正缺陷后） |

---

## 10. 阻断性缺陷（实施前必须修复）

### ~~🔴 10.1 命令文件名错误（阻断）~~ ✅ 已修正

**位置**：计划 L49
**原问题**：计划引用 `risk_control.rs`，但实际文件为 `risk_blueprint.rs`

**修正状态**：✅ 已修正
- 计划 L49 及后续引用已改为 `risk_blueprint.rs`
- 架构文档序列图已同步修正

---

### 🔴 10.2 文件结构重复条目（阻断）

**位置**：计划 L32-L36
**问题**：在 `├── harness/entropy.rs`（L31）之后，以下 6 行重复出现：

```
├── model_metadata.rs      ← 已在 L21
├── context_budget.rs      ← 已在 L23
├── context_compressor.rs  ← 已在 L24
├── types.rs               ← 已在 L25
├── agent_router.rs        ← 已在 L26
├── planner.rs             ← 已在 L27
```

**修复**：删除 L32-L36 的重复行。

---

### ~~🔴 10.3 缺失 `search_llm.rs` 中 `count_tokens` Tauri 命令的迁移（阻断）~~ ✅ 已修正

**位置**：计划 Task 1 Step 3（L221-226）
**原问题**：计划迁移 `count_tokens` 时遗漏了 `commands/search_llm.rs:113` 的 Tauri 命令封装

**修正状态**：✅ 已修正
- 计划 L236 已补充迁移说明：修改 `commands/search_llm.rs:113`，将 `llm_service::count_tokens` 改为 `token::count_tokens`

---

## 11. 高风险问题

### ~~🟡 11.1 `bitflags` 依赖添加时序错误~~ ✅ 已修正

**位置**：Task 4（L557-595）vs Task 5（L766）
**原问题**：`bitflags` 依赖在 Task 5 才添加，但 Task 4 已使用

**修正状态**：✅ 已修正
- 计划 L565 已将 `cargo add bitflags` 移到 Task 4 之前（前置依赖步骤）

---

### 🟡 11.2 Task 1 与 Task 2 的 ChatMessage 扩展重叠

**位置**：Task 1 Step 0（L66-93）vs Task 2（L256-338）
**问题**：两个 Task 都扩展 `ChatMessage`，但实现不一致：

| 方法 | Task 1 Step 0 | Task 2 |
|------|--------------|--------|
| `new()` | `token_count: Some(0)` ❌ 空缓存 | `token_count: Some(token::count_tokens_with_fallback(content))` ✅ |
| `set_content()` | ❌ 缺 | ✅ |
| `get_token_count()` | ❌ 缺 | ✅ |
| `compute_token_count()` | ❌ 缺 | ✅ |

**影响**：如果先执行 Task 1（Step 0）再执行 Task 2，Step 0 中 `new()` 创建的 `ChatMessage` 有空的 `token_count`（`Some(0)`），后续不会自动补算。

**修复**：两方案选一：
1. **合并**两个 Task 为一个
2. **删除** Task 1 Step 0（L66-93→已冗余），保留 Task 2 为唯一 `ChatMessage` 扩展任务
3. 或将 Task 1 Step 0 的 `new()` 改为与 Task 2 一致：`token_count: Some(token::count_tokens_with_fallback(content))`

---

### 🟡 11.3 `uuid` 和 `tiktoken-rs` 依赖已存在

| 依赖 | Cargo.toml | 计划中操作 | 实际需要 |
|------|-----------|-----------|---------|
| `uuid = { version = "1", features = ["v4"] }` | ✅ L74 | `cargo add uuid --features v4`（L92） | ❌ 冗余 |
| `tiktoken-rs = "0.6"` | ✅ L48 | 未显式提 | ✅ `token.rs` 直接可用 |

**修复**：删除 Task 1 Step 0 的 `cargo add uuid --features v4` 命令（L92）。`uuid` 已在 L74 存在。如担心版本兼容性，改为仅 `cargo check` 验证。

---

## 12. 中风险问题

### 🟢 12.1 计划中 `truncate_to_tokens` 实现与当前有差异

| 维度 | 当前实现（`llm_service.rs:461`） | 计划中的实现 |
|------|-------------------------------|-------------|
| 搜索方式 | `char` 级二分：`chars[..mid]` 收集为 String | `byte` 级二分：`text[..end]` 切片 |
| UTF-8 安全 | ✅ 通过 `chars` 迭代 | ✅ 通过 `is_char_boundary` 回退 |
| 性能 | 每次迭代都分配新 String | ✅ 更高效（字节切片） |

两个实现都正确，不改变可用性。建议在实施时用 `cargo test` 验证回归兼容性。

### 🟢 12.2 `git add -A` 过于宽泛

**位置**：Task 2 Step 5（L336）
**问题**：`git add -A` 会添加所有未跟踪文件和变更，可能误添加缓存文件或生成物。

**修复**：改为显式路径：
```bash
git add src-tauri/src/services/llm_service.rs
```

### 🟢 12.3 `model_specs.json` 缺少 `gpt-4.1` 和 `claude-sonnet-4-6`

OpenAI 和 Anthropic 的模型更新频繁。`model_specs.json` 内置数据库应注明更新频率或提供自动同步机制。建议在注释中说明数据来源日期。

### 🟢 12.4 计划中 line number 引用将漂移

Task 1 Step 3 引用的行号（L502, L1334, L1369, L1947）在 `ChatMessage` 扩展后会发生位移。建议：
- 保留 line number 作为参考
- 同时标注函数名：`count_tokens` 调用点、`truncate_to_tokens` 调用点
- 实施时用 grep 确认实际位置

---

## 13. 实施顺序建议

### 13.1 修正后的 P0-a 执行流程

1. **Task 4（types.rs + bitflags）** ← 移到最前面，因为 Task 5 需要它
2. **依赖检查**：`cargo add bitflags`（移至 Task 4）
3. **Task 2（ChatMessage 扩展）** ← 合并 Task 1 的 Step 0
4. **Task 1（token.rs）** ← 补充 `search_llm.rs` 迁移
5. **Task 3（model_metadata.rs）**
6. **Task 5（context_budget.rs）**
7. **Task 6（context_compressor.rs）**
8. 后续 P0-e 至 P2-c

### 13.2 建议新增 P0.5：集成测试阶段

后端基础设施（P0）完成后、核心逻辑（P1）开始前，增加一个测试阶段：

- Context Budget 分配算法的单元测试（边界值、优先级顺序）
- 磁滞回线行为的状态测试
- `PlanStateMachine` 状态转换测试
- `count_tokens` 缓存正确性测试
- 现有 `compress_conversation` 与新 `context_compressor` 的行为比对测试

---

---

## 第三部分：架构文档审查

## 14. ARCHITECTURE.md 评估

### 14.1 整体评价

`ARCHITECTURE.md` 整体准确，目录结构和核心流程描述正确。但经过三层范式设计后，部分内容已过时。

### 14.2 需更新的内容

| 位置 | 当前内容 | 问题 | 建议更新 |
|------|---------|------|---------|
| L179-182 | `ChatMessage { role, content }` | 未反映新增的 `id` 和 `token_count` 字段 | 按设计文档 3.1 节更新结构体 |
| L191 | `max_tokens: u32 // ⚠️ 语义混乱` | 文档已指出问题但未提供修复后的结构 | 拆分为 `context_window: u32` + `max_output_tokens: u32`，参考 `ModelMetadata` |
| L292-295 | "详见设计文档" + 三层架构图 | 缺少对 P0-a 等新增模块的引用 | 模块表新增 `types.rs`、`token.rs` |
| L313-323 | 未来模块表 | 缺少 `services/types.rs`（共享类型模块） | 新增一行 `types.rs` |
| L315 | `model_metadata.rs` | 无 `services/` 前缀 | 统一为 `services/model_metadata.rs` |
| L273 | 序列图 `risk_control.rs` | 应改为 `risk_blueprint.rs` | 修正文件路径 |

### 14.3 内部不一致

| 位置 | 内容 A | 内容 B | 冲突 |
|------|--------|--------|------|
| L51 | 目录结构：`commands/risk_blueprint.rs` | L273 序列图：`(risk_control.rs)` | 同一文件在架构文档内引用两个不同名称 |

---

---

## 第四部分：交叉一致性问题

## 15. 三文档一致性矩阵

| 项 | 设计文档 | 架构文档 | 实现计划 | 实际代码 | 一致？ |
|---|---------|---------|---------|---------|--------|
| Tauri 命令文件 | `risk_control.rs` ❌ | L51: `risk_blueprint.rs` ✅ / L273: `risk_control.rs` ❌ | `risk_blueprint.rs` ✅ | `risk_blueprint.rs` ✅ | ✅ 计划已修正 |
| `ChatMessage` 结构 | 含 `id` + `token_count` ✅ | 不含 ❌ | 含 `id` + `token_count` ✅ | 不含（待实施） ⚠️ | ⚠️ |
| `AgentMode` | `bitflags!` ✅ | 未提及 | `bitflags!` ✅ | 不存在（待实施） ⚠️ | ⚠️ |
| 新增模块路径 | `token.rs`, `model_metadata.rs` 等 | 未来架构表 9 模块 | `token.rs`, `model_metadata.rs` 等 10+ 模块 | 不存在（待实施） | ⚠️ |
| `max_tokens` 歧义 | 已识别需拆分 | 已识别（L191 注释） | P0-f 解决 | 存在 | ✅ 诊断一致 |

---

## 16. 最终遗留项清单

| # | 问题 | 优先级 | 涉及文档 | 状态 |
|---|------|--------|---------|------|
| 1 | 命令文件名错乱（`risk_control.rs` ↔ `risk_blueprint.rs`） | ~~🔴 阻断~~ | 设计文档、架构文档、计划 | ✅ 已修正 |
| 2 | 计划 L32-L36 重复条目 | 🔴 阻断 | 计划 | ⚠️ 待修复 |
| 3 | 计划缺失 `search_llm.rs` 迁移 | ~~🔴 阻断~~ | 计划 | ✅ 已修正 |
| 4 | `bitflags` 依赖时序 | ~~🟡 高~~ | 计划 | ✅ 已修正 |
| 5 | Task 1 Step 0 与 Task 2 重叠 | 🟡 高 | 计划 | ⚠️ 待修复 |
| 6 | `uuid` / `tiktoken-rs` 冗余添加 | 🟡 高 | 计划 | ⚠️ 待修复 |
| 7 | `maybe_reset` 未用 `reset_threshold_pct` | 🟢 低 | 设计文档 | ⚠️ 待修复 |
| 8 | U1: Speculative Execution 并发 | 🟢 低 | 设计文档、架构文档 | ⚠️ 待修复 |
| 9 | U2: 字符硬上限安全阀 | 🟢 低 | 设计文档、架构文档、计划 | ⚠️ 待修复 |
| 10 | `"summarization"` 模型标签映射未定义 | 🟢 低 | 设计文档、计划 | ⚠️ 待修复 |
| 11 | 架构文档 ChatMessage 未更新 | 🟢 低 | 架构文档 | ⚠️ 待修复 |
| 12 | 计划中 line number 漂移风险 | 🟢 低 | 计划 | ⚠️ 待修复 |
| 13 | `git add -A` 过于宽泛 | 🟢 低 | 计划 | ⚠️ 待修复 |

**当前状态**：3 个阻断缺陷已修正 2 个，剩余 1 个（L32-L36 重复条目）+ 2 个高优先级 + 7 个低优先级待修复。

---

## 17. 实施就绪状态

### 可立即开始（不受审查缺陷影响）

| 任务 | 前置条件 | 风险 |
|------|---------|------|
| Task 4: `types.rs` + `bitflags` | 无（`bitflags` 已在 Task 4 前添加） | 无 |
| Task 3: `model_metadata.rs` + `model_specs.json` | 无（纯新增） | 低 |
| Task 1: `token.rs`（含 Step 3 search_llm.rs 迁移） | 无 | 低 |
| P0-e: 渐进式披露 | 无（`risk_blueprint.rs` 已正确引用） | 低 |

### 需修复缺陷后才能开始

| 任务 | 阻塞 | 修复要求 |
|------|------|---------|
| Task 1 Step 0 + Task 2: ChatMessage 扩展 | 两任务重叠 + line number 漂移 | 合并或重排顺序 |
| 计划 L32-L36: 文件结构重复条目 | 重复行影响可读性 | 删除重复行 |

### 建议实施顺序（修正后）

```
Task 4 → Task 3 → Task 2 → Task 1 → Task 5 → Task 6 → P0-e ... P2-c
（types.rs）（metadata）（ChatMessage）（token.rs）（budget）（compressor）
```
前置修复 → Task 4 → Task 3 → Task 2 → Task 1 → Task 5 → Task 6 → P0-e ... P2-c
（types.rs）  （metadata） （ChatMessage）（token.rs）  （budget）  （compressor）
```

---

## 附录：审查历史

| 轮次 | 审查对象 | 发现问题 | 当前状态 |
|------|---------|---------|---------|
| 第 1 轮 | 设计文档初稿 | 21（+2 代码验证） | ✅ 全部修复 |
| 第 2 轮 | 设计文档修正版 | 6 个新增问题 | ✅ 全部修复 |
| 第 3 轮 | 设计文档二次修正版 | 1 个遗留问题 | ⚠️ 低优先级未修复 |
| 第 4 轮（本轮） | 架构文档 + 实现计划 | 13 项（3 阻断、3 高、7 低） | ✅ 阻断 2/3 已修正，高 1/3 已修正 |

**结束** — 文档可进入实施阶段（1 个阻断缺陷待修复：L32-L36 重复条目）。
