# Phase 10: 文档生成核心 — CONTEXT.md

**Phase:** 10 — 文档生成核心
**Created:** 2026-05-24
**Mode:** Auto (smart discuss — all grey areas auto-proposed)

---

## Phase Goal

LLM 填充模板 → 标准化 .docx/.xlsx 输出，包括调研报告/纪要自动生成。

---

## Codebase Context (Pre-Existing)

### Already Implemented (from Phase 9 + early Phase 10 scaffolding)

Phase 10 的核心代码**已大部分实现**：

1. **`doc_generator.rs`** — 完整的 `generate_document()` 流水线：
   - `GenerateDocRequest` 含 `template_path`, `fields`, `schema_fields`, `project_name`, `context`
   - `GeneratedDoc` 含 `output_path`, `fields_filled`, `user_fields`, `ai_fields`, `missing_fields`, `missing_fields_detail`
   - `generate_llm_fields()` — LLM 字段生成（JSON 输出）
   - `fill_template()` — 简单同步填充（无 LLM）
   - 支持 .docx + .xlsx 格式路由

2. **`deliverable_recipes.rs`** — 7 个交付物配方：
   - `DeliverableRecipe` 含 `template_id`, `field_overrides`, `system_prompt`
   - `all_recipes()`, `get_recipe_by_template_id()`, `apply_recipe_overrides()`
   - 7 配方：调研报告、周报/月报、业务蓝图、PCR、上线单、验收单、会议纪要

3. **`smart_completion.rs`** — KB+LLM 智能补全：
   - `SmartFillRequest` → hybrid_search → LLM fill → `SmartFillResult`
   - 支持 source citations（知识库来源追溯）

4. **`docx_filler.rs`** — Word split-run 正则替换，保留样式
5. **`xlsx_filler.rs`** — umya-spreadsheet 单元格级替换
6. **`template_schema.rs`** — YAML sidecar 加载器，`SchemaField` 含 fill_strategy
7. **`template_scanner.rs`** — 8 阶段目录扫描器
8. **`product_store.rs`** — SQLite CRUD（products + versions），WAL 模式

### Tauri Commands Registered (lib.rs)

- `fill_template` — 简单模板填充
- `generate_doc` — 完整文档生成流水线
- `smart_fill` — 智能补全
- `probe_missing_fields` — 缺失字段探针
- `get_deliverable_recipe` — 获取配方
- Product 管理系列：`list_products`, `get_product`, `delete_product`, `export_product`, `regenerate_product`

---

## Implementation Gaps (What's Missing)

### GAP-1: Recipe-Aware `generate_document()` Integration

**现状**: `generate_doc` 命令直接调用 `doc_generator::generate_document(request, &state.llm)`，不集成配方。

**问题**: 
- `generate_llm_fields()` 使用通用 system_prompt："你是一个文档字段填充助手"
- 配方的 domain-specific system_prompt（如"你是一位资深的金蝶ERP实施顾问，正在撰写项目调研报告"）未被使用
- `apply_recipe_overrides()` 函数存在但未在 generate 流程中调用
- `field_overrides`（如"调研背景" strategy 从 "user" → "ai"）未被应用

**影响**: DGEN-01/DGEN-02/DGEN-03/DGEN-04 的核心体验——模板选择后进入向导式填写流程，需要配方来决定哪些字段用 AI、哪些用 KB、哪些必须用户填写。

### GAP-2: KB Fill Strategy ("kb") Not Implemented in generate_document()

**现状**: `generate_llm_fields()` 只处理 `fill_strategy == "ai"` 或 `"llm"` 的字段。"kb" strategy 字段被跳过。

**问题**: 配方中多个字段标注为 `strategy: "kb"`（如"企业概况"、"业务现状"、"系统功能映射"），但 `generate_document()` 中没有 KB 检索 → LLM 总结的 pipeline。

**影响**: DGEN-05（系统从知识库检索相关历史内容辅助填充）无法实现。

### GAP-3: Research Report Generation Pipeline

**现状**: 调研报告配方（`recipe_investigation_report`）存在，但没有"调研记录 → 报告"的端到端流程。

**问题**: DLVR-01 要求"用户提供调研记录 → 输出调研报告.docx"，但目前：
- 没有 ResearchSession → 调研记录聚合的机制
- 没有"将 Q&A 记录作为 LLM context 传入报告生成"的 pipeline
- 调研报告的字段（调研背景、企业概况、业务现状、问题分析、建议方案）需要从调研会话中提取

**影响**: DLVR-01 核心交付物无法生成。

### GAP-4: Meeting Minutes Generation Pipeline

**现状**: 会议纪要配方（`recipe_meeting_minutes`）存在，但没有"会议记录 → 纪要"的端到端流程。

**问题**: DLVR-07 要求"用户提供会议记录 → 输出会议纪要.docx"，但目前：
- 没有从会议记录（文本/Whisper 转写）提取议题、决议、待办的 pipeline
- 会议纪要字段需要从原始记录中结构化提取

**影响**: DLVR-07 无法实现。

### GAP-5: Product Store Status Lifecycle

**现状**: `product_store.rs` 中 `create_product()` 硬编码 `status: 'completed'`。

**问题**: PROD-03（产物重新生成）需要 draft → completed 状态流转，但所有产物直接标记为 completed。

**影响**: 低影响——v0.2 可先接受"所有产物都是 completed"，重新生成时直接创建新版本。v0.3 再加 draft 状态。

---

## Grey Area Proposals (Auto-Decided)

### Grey Area 1: Recipe Integration Strategy

**问题**: 如何将 DeliverableRecipe 集成到 generate_document() 流程？

**选项**:
- A: 在 `GenerateDocRequest` 新增 `recipe_id: Option<String>` 字段，generate_document 内部查找配方 → apply_overrides → 替换 system_prompt
- B: 前端先调用 `get_deliverable_recipe` → 拿到配方 → apply_overrides 到 schema_fields → 传入 generate_doc（后端不变）
- C: 新建 `generate_recipe_doc()` 函数，封装配方查找 + generate_document 调用

**决策**: **选项 C** — 新建 `generate_recipe_doc()` 封装函数
- 优势：generate_document() 保持通用（不依赖配方），新增函数封装配方逻辑
- 前端可以：选模板 → get_deliverable_recipe → generate_recipe_doc
- 后端内聚：配方查找 + overrides 应用 + system_prompt 替换 + KB 检索整合都在一个函数中
- 不破坏现有 GenerateDocRequest 结构

### Grey Area 2: KB Fill Strategy Implementation

**问题**: 如何实现 `strategy: "kb"` 字段的填充？

**选项**:
- A: 在 generate_document 中为 kb 字段调用 hybrid_search → 把搜索结果作为 LLM context → LLM 总结填充
- B: 在 generate_recipe_doc 中，kb 字段先检索 → 检索结果合并到 LLM prompt 的 context section
- C: kb 字段直接调用 smart_fill（已有 KB+LLM pipeline）

**决策**: **选项 B** — 在 generate_recipe_doc 中统一处理
- generate_recipe_doc 已经封装了配方逻辑，KB 检索也应在此层
- 流程：kb 字段 → hybrid_search(query=project_name+field_hint, top_k=3) → 搜索结果作为 context 注入 LLM prompt
- 与 smart_fill 不同——smart_fill 是单字段级 KB+LLM，这里是整文档级的批量 KB 检索 + LLM 生成

### Grey Area 3: Research Report Pipeline

**问题**: 调研记录如何传入报告生成？

**选项**:
- A: 新增 `ResearchSession` → Q&A 聚合 → 传入 generate_recipe_doc 的 context 参数
- B: 前端直接将调研 Q&A 文本作为 context 传入（Phase 13 再做 Session 管理）
- C: 在 ResearchOutline 上新增"已完成问题"追踪 → 自动聚合

**决策**: **选项 B** — 前端聚合传入
- Phase 13 才做 ResearchSession CRUD，Phase 10 不需要后端 Session 管理
- 前端在调研过程中收集 Q&A → 结束时将所有 Q&A 文本合并为 context → 传入 generate_recipe_doc
- 后端只需在 generate_recipe_doc 中接收 context，配方 system_prompt 指导 LLM 从 context 中提取调研报告内容

### Grey Area 4: Meeting Minutes Pipeline

**问题**: 会议记录 → 纪要的生成流程？

**选项**:
- A: 与调研报告相同——前端传入会议记录文本作为 context，配方 system_prompt 指导 LLM 结构化提取
- B: 专门新建 `generate_meeting_minutes()` 函数

**决策**: **选项 A** — 与调研报告共享同一 generate_recipe_doc 流程
- 会议纪要配方已有合适的 system_prompt（"你是一位金蝶ERP项目助理，正在整理会议纪要"）
- 只需前端传入会议记录文本（或 Whisper 转写文本）作为 context
- 不需要额外后端函数——generate_recipe_doc 的 recipe_id="meeting_minutes" 即可

### Grey Area 5: Product Store Status

**问题**: 是否在 Phase 10 实现 draft 状态？

**决策**: **暂不实现** — v0.2 所有产物都是 completed
- 重新生成时直接创建新版本（已有 regenerate_product 命令）
- 状态生命周期在 Phase 13（产物管理后端）再做

---

## Architectural Summary

### New Code to Add

1. **`generate_recipe_doc()`** — 封装配方 + KB + LLM 的文档生成函数
   - 输入：`RecipeDocRequest` (recipe_id + template_path + fields + project_name + context)
   - 流程：查找配方 → 加载 schema → apply_overrides → KB 检索(kb 字段) → LLM 生成(ai/kb 字段，使用配方 system_prompt) → 填充模板 → 保存产物
   - 输出：`GeneratedDoc` + `ProductMeta`

2. **KB Fill in generate_recipe_doc** — kb 字段的填充逻辑
   - 对每个 kb strategy 字段：调用 hybrid_search(query=project_name+hint, top_k=3)
   - 搜索结果注入 LLM context section

3. **Tauri command `generate_recipe_doc`** — 前端调用入口

4. **Tauri command `generate_from_research`** — 调研报告专用入口（聚合 Q&A → generate_recipe_doc）

5. **Tauri command `generate_from_meeting`** — 会议纪要专用入口（传入会议记录 → generate_recipe_doc）

### Existing Code to Modify

1. **`doc_generator.rs`** — 新增 `generate_recipe_doc()` + `RecipeDocRequest`
2. **`lib.rs`** — 新增 3 个 Tauri commands
3. **`product_store.rs`** — `create_product()` 添加 `status` 参数（默认 "completed"，为 Phase 13 预留）

### No Changes Needed

- `deliverable_recipes.rs` — 配方数据已完整，只需被调用
- `smart_completion.rs` — 独立功能，Phase 11 再增强
- `docx_filler.rs` / `xlsx_filler.rs` — 底层渲染不变
- `template_schema.rs` / `template_scanner.rs` — Phase 9 遗产不变

---

## Requirements Coverage Mapping

| Req | Status | Implementation |
|-----|--------|----------------|
| DGEN-01 向导式填写流程 | NEW | generate_recipe_doc + 前端向导（Phase 14） |
| DGEN-02 自动识别字段 | ✅ Already | probe_missing_fields + template_schema |
| DGEN-03 缺失追问 | ✅ Already | missing_fields_detail in GeneratedDoc |
| DGEN-04 LLM 填充 | ✅ Already | generate_llm_fields |
| DGEN-05 KB 辅助填充 | NEW | generate_recipe_doc 中 kb strategy |
| DGEN-06 .docx 输出 | ✅ Already | docx_filler |
| DGEN-07 .xlsx 输出 | ✅ Already | xlsx_filler |
| DGEN-08 产物存储 | ✅ Already | product_store |
| DLVR-01 调研报告 | NEW | generate_from_research command |
| DLVR-02 周报 | ✅ Covered | generate_recipe_doc(recipe_id="weekly_monthly_report") |
| DLVR-03 业务蓝图 | ✅ Covered | generate_recipe_doc(recipe_id="business_blueprint") |
| DLVR-04 PCR | ✅ Covered | generate_recipe_doc(recipe_id="pcr") |
| DLVR-05 上线单 | ✅ Covered | generate_recipe_doc(recipe_id="go_live") |
| DLVR-06 验收单 | ✅ Covered | generate_recipe_doc(recipe_id="acceptance") |
| DLVR-07 会议纪要 | NEW | generate_from_meeting command |
| PROD-01~04 | ✅ Already | product_store commands |

---

## Dependencies

- **Phase 9** (✅ Complete) — template_schema, template_scanner, research_indexer
- **Phase 11** (parallel) — smart_completion 增强不在 Phase 10 scope
- **Phase 13** — ResearchSession CRUD 不在 Phase 10 scope

---

*CONTEXT.md created: 2026-05-24 — Phase 10 smart discuss auto-proposed*