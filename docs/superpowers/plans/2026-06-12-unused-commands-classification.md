# 未用命令分类报告

**生成日期**：2026-06-12
**审计范围**：`src-tauri/src/lib.rs::invoke_handler` 注册的 216 个命令 vs 前端 `invoke()` 调用点
**审计方法**：扫描 `src-tauri/src/lib.rs` 注册表 + 扫描 `src/**/*.{ts,tsx}` 的 `invoke("xxx")` 模式；再用 Rust 全量 grep 排除注册/定义自身，找出真正无任何调用方的命令。

> 实际数量为 **26 个**（用户口语化估计为 23）。

## 分类总览

| 分类 | 数量 | 处理建议 |
|---|---|---|
| A. 死代码：前端无 UI，后端无内部调用 | 21 | **删除** |
| B. 死命令，但底层服务方法被内部使用 | 3 | 删命令，保服务方法 |
| C. 死命令，但底层服务方法也未被使用 | 2 | 全部删除（含服务模块） |
| 合计 | 26 | |

## A 类：彻底死代码（21 个）

| # | 命令 | 注册位置 | 说明 |
|---|---|---|---|
| 1 | `greet` | `commands/core.rs:21` | 模板测试桩，仅返回 Hello 字符串 |
| 2 | `get_data_dir` | `commands/core.rs:93` | 前端从未询问数据目录路径 |
| 3 | `get_api_key` | `commands/core.rs:111` | 前端只用 `set_api_key` / `delete_api_key`，不读出 |
| 4 | `scan_stale_skills` | `commands/core.rs:383` | 健康检查型命令，UI 不存在 |
| 5 | `scan_index_drift` | `commands/core.rs:408` | 健康检查型命令，UI 不存在（核心循环中 356/410 行调用的是 `EntropyManager::scan_index_drift` 方法，不是命令本身） |
| 6 | `get_available_providers` | `commands/embedding.rs:222` | 前端从未获取可用 provider 列表 |
| 7 | `get_index_stats` | `commands/embedding.rs:204` | UI 不存在；与 `get_knowledge_stats` 重复 |
| 8 | `get_knowledge_stats` | `commands/embedding.rs:212` | 与 `document::get_stats` 重复；UI 不存在 |
| 9 | `search_similar` | `commands/embedding.rs:186` | UI 不存在 |
| 10 | `load_index` | `commands/embedding.rs:197` | 启动期自动加载，无须命令入口 |
| 11 | `force_recompile_kb_source` | `commands/kb_compilation.rs:653` | UI 不存在；`start_kb_recompile` 接受 `force` 参数已覆盖 |
| 12 | `retry_failed_ingestions` | `commands/ingestion_queue.rs:33` | 与 `retry_project_failed_ingestions` 重复，UI 仅调用后者 |
| 13 | `process_ingestion_queue` | `commands/ingestion_queue.rs:57` | 与 `process_project_ingestion_queue` 重复，UI 仅调用后者 |
| 14 | `get_product` | `commands/product.rs:21` | 前端 `list_products` / `delete_product` / `export_product` 已满足；按 id 获取未用 |
| 15 | `get_investigation_recipe` | `commands/research.rs:17` | 配方字符串在 `prompts::RECIPE_INVESTIGATION`，UI 未拉取 |
| 16 | `get_current_edition` | `commands/research.rs:23` | 调研版本切换 UI 不存在 |
| 17 | `set_edition` | `commands/research.rs:30` | 同上 |
| 18 | `list_research_modules` | `commands/research.rs:38` | 同上 |
| 19 | `import_research_outlines` | `commands/research.rs:47` | 同上 |
| 20 | `seed_demo_wiki_pages` | `commands/wiki_page.rs:292` | 演示数据，发布前必须删除 |
| 21 | `traverse_graph` | `commands/knowledge_graph.rs:48` | UI 不存在；`graph_expand_search` 已替代多跳遍历 |

## B 类：删命令保留服务方法（3 个）

| # | 命令 | 注册位置 | 底层服务方法 | 调用方 |
|---|---|---|---|---|
| 22 | `embed_text` | `commands/embedding.rs:137` | `EmbeddingEngine::embed_text` (`services/embedding.rs:1012`) | `services/hybrid_search.rs:151`、`services/memory.rs:240`、`services/skill_trigger.rs:167` |
| 23 | `embed_batch` | `commands/embedding.rs:159` | `EmbeddingEngine::embed_batch` (`services/embedding.rs:1034`) | `services/ingestion.rs:279`、`services/research_indexer.rs:375` |

**说明**：服务方法（`emb.embed_text()` / `emb.embed_batch()`）是异步进程内调用，**不是** Tauri 命令调用——命令仅是多此一举的 IPC 包装。前端从未调用过这两个命令。

## C 类：删命令同时删服务模块（2 个命令 + 整链服务）

| # | 命令 | 注册位置 | 关联死链 |
|---|---|---|---|
| 24 | `recommend_questions` | `commands/research.rs:72` | → `services/question_recommend.rs::recommend_questions` (无其他调用方) |
| 25 | `generate_followup_questions` | `commands/research.rs:90` | → `services/question_recommend.rs::generate_followup_questions` (无其他调用方) |
| 26 | `smart_fill_for_question` | `commands/research.rs:107` | → `services/question_recommend.rs::smart_fill_for_question` (无其他调用方) |

**说明**：三个命令是 `question_recommend` 服务模块**唯一**的入口。`question_recommend` 模块本身也仅被 `commands/research.rs` 引用——整链可整段删除。

进一步可连带删除（属于 C 类的"无主"服务）：
- `services/question_recommend.rs`（全部）
- `services/edition_config.rs`（仅 `research.rs` 死命令用）
- `services/research_indexer.rs`（仅 `research.rs` 死命令用）

## 删除动作清单

### 阶段 1：删 21 个 A 类命令
- `commands/core.rs`：删除 `greet`、`get_data_dir`、`get_api_key`、`scan_stale_skills`、`scan_index_drift`
- `commands/embedding.rs`：删除 `get_available_providers`、`get_index_stats`、`get_knowledge_stats`、`search_similar`、`load_index`
- `commands/kb_compilation.rs`：删除 `force_recompile_kb_source`
- `commands/ingestion_queue.rs`：删除 `retry_failed_ingestions`、`process_ingestion_queue`
- `commands/product.rs`：删除 `get_product`
- `commands/research.rs`：删除 `get_investigation_recipe`、`get_current_edition`、`set_edition`、`list_research_modules`、`import_research_outlines`
- `commands/wiki_page.rs`：删除 `seed_demo_wiki_pages`
- `commands/knowledge_graph.rs`：删除 `traverse_graph`
- `lib.rs`：同步删除 21 个 `invoke_handler` 注册行

### 阶段 2：删 3 个 B 类命令（保留服务方法）
- `commands/embedding.rs`：删除 `embed_text`、`embed_batch` 命令函数；保留 `services/embedding.rs` 方法
- `lib.rs`：同步删除 2 个注册行

### 阶段 3：删 3 个 C 类命令 + 整链
- `commands/research.rs`：删除 `recommend_questions`、`generate_followup_questions`、`smart_fill_for_question`
- 删除 `services/question_recommend.rs`
- `services/mod.rs`：删除 `pub mod question_recommend;`
- 删除 `services/edition_config.rs`、`services/research_indexer.rs`（如确认无其他业务逻辑依赖）
- `app_state.rs`：清理 `edition_config` / `research_indexer` 字段及其初始化（约 9 处）
- `lib.rs`：同步删除 3 个注册行
- `services/mod.rs`：删除 `pub mod edition_config;` / `pub mod research_indexer;`

## 风险评估

- **无功能回归**：26 个命令均无前端 UI 入口，也无后端内部 Rust 流程依赖。
- **A/B 类无副作用**：纯命令注册 + 函数定义删除。
- **C 类需谨慎**：`app_state.rs` 字段清理需确认初始化顺序无副作用；建议改完跑 `cargo check` + 全量测试。
- **seed_demo_wiki_pages**：若在 `wiki_page.rs` 中还存在仅供演示的 fixture 函数，需一并清。

## 验收标准

1. `grep "commands::" src-tauri/src/lib.rs` 行数从 219（含注释行）减至约 192。
2. `cargo build --release` 通过。
3. `npm run tsc --noEmit` 通过。
4. 全量测试套件通过。
5. 启动后设置页、知识图谱、调研、维基等核心路径行为不变。
