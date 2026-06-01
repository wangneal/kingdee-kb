# 知识库重构 + 调研大纲编辑器及脑图视图 — 实现计划

> **状态**：实现计划草案。**未获用户明确通知，不得执行任何代码修改、安装依赖或数据库迁移。**  
> **面向 AI 代理的工作者**：使用 subagent-driven-development 逐任务实现。步骤使用复选框 `- [ ]` 跟踪进度。  
> **目标**：按设计决策文档完成 5 阶段实现，涵盖全部 10 项修正。  
> **技术栈**：Rust + Tauri v2 + React 19 + SQLite + markmap  
> **参考文档**：`docs/superpowers/plans/2026-06-01-kb-refactor-design-decisions.md`  
> **工期说明**：各阶段工期按细化任务重新估算，与概要/设计文档略有差异（阶段一 2.5→4、二 2→3、三 4→5、四 6→9、五 4→单独计划），以本计划为准。总工期从 22 天延长至约 25 天。

**每项任务完成标准**：
- 编译检查：`cargo check`（Rust）+ `npx tsc --noEmit`（TypeScript）零错误
- 验收测试：按阶段具体定义
- 提交：仅在用户要求时执行 `git add + commit`

**阶段验收测试**：

| 阶段 | 验收项 |
|------|--------|
| 一 | ① `cargo check` + `npx tsc` 零错误；② 并发导入 5 个文件不产生向量 key 冲突；③ 删除文档中途进程终止后三索引一致（SQLite/usearch/BM25 均无残留） |
| 二 | ① 迁移 SQL 可在现有数据库上执行且不破坏已有数据；② 导入文件后原始副本保留在 `raw/{project}/sources/`；③ 队列进程终止后重启，processing 状态任务恢复为 pending；④ `raw_source_identity` 正确关联 |
| 三 | ① `enable_kb_compilation=true` 时导入文件后生成 `wiki_pages`（含 `content_candidate`，`content` 不变）；② `approve_candidate` 后 `content` 更新、候选字段清空；③ 增量缓存命中时跳过 LLM 调用；④ 差异 diff >30% 标记为 `conflict` |
| 四 | ① 大纲节点 CRUD 完整；② Fractional Indexing 排序正确（新增/移动后 `ORDER BY sort_order` 一致）；③ 移动父节点到子孙节点时被拒绝（防成环）；④ 删除节点后孤儿 QA 被清理；⑤ `Ctrl+Z` Undo 后数据正确；⑥ 脑图视图能正常渲染；⑦ `export_outline` 两种格式输出正确 |
| 五 | 知识图谱构建 <5 秒；图扩展检索能发现关键词不匹配的相关页面 |

---

## 文件清单

| 文件 | 操作 | 阶段 |
|------|------|------|
| `src-tauri/src/app_state.rs` | 修改 | 一 |
| `src-tauri/src/services/metadata.rs` | 修改 | 一 |
| `src-tauri/src/services/vector_index.rs` | 修改 | 一 |
| `src-tauri/src/services/bm25_service.rs` | 修改 | 一 |
| `src-tauri/src/commands/document.rs` | 修改 | 一 |
| `src-tauri/src/commands/ingestion.rs` | 修改 | 一 |
| `src-tauri/src/commands/search_llm.rs` | 修改 | 一 |
| `src-tauri/src/services/ingestion_queue.rs` | 新建 | 二 |
| `src-tauri/src/services/raw_source.rs` | 新建 | 二 |
| `src-tauri/src/services/wiki_page.rs` | 新建 | 三 |
| `src-tauri/src/services/frontmatter.rs` | 新建 | 三 |
| `src-tauri/src/services/outline.rs` | 新建 | 四 |
| `src-tauri/src/commands/outline.rs` | 新建 | 四 |
| `src-tauri/src/services/mod.rs` | 修改 | 各阶段 |
| `src-tauri/src/lib.rs` | 修改 | 各阶段 |
| `src/contexts/OutlineContext.tsx` | 新建 | 四 |
| `src/contexts/AudioContext.tsx` | 新建 | 四 |
| `src/lib/audio.ts` | 新建 | 四 |
| `src/components/outliner/OutlineTree.tsx` | 新建 | 四 |
| `src/components/outliner/OutlineNode.tsx` | 新建 | 四 |
| `src/components/outliner/NodeDetailPanel.tsx` | 新建 | 四 |
| `src/components/outliner/MindmapView.tsx` | 新建 | 四 |
| `src/pages/ResearchAssistant.tsx` | 修改 | 四 |
| `src/App.tsx` | 修改 | 四 | 路由 + 导航菜单入口 + ProjectProvider/OutlineProvider/AudioProvider 层级 |
| `src/components/Layout.tsx` | 修改 | 四 | 导航菜单入口 |

---

## 阶段一：基础设施加固（4 天）

### 任务 1.1：分级锁 + 锁顺序等级

- [ ] AppState 中读多写少服务改为 RwLock
- [ ] 在文档中定义并注释全局锁获取顺序（metadata→bm25→vector_index→embedding→其他）
- [ ] 全局替换 `.lock().map_err(|e| e.to_string())?` 为 `.read()` / `.write()`
- [ ] `cargo check` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

### 任务 1.2：安全向量 key（INSERT 模式，修正 6）

- [ ] 创建 `vector_key_seq` 表（`id INTEGER PRIMARY KEY AUTOINCREMENT`）
- [ ] 实现 `next_vector_key()` ：`INSERT INTO vector_key_seq DEFAULT VALUES` + `last_insert_rowid()`
- [ ] 替换 ingestion.rs 中的 `MAX+1` 为 `next_vector_key()`
- [ ] `cargo check` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

### 任务 1.3：删除补偿机制（outbox + BM25 commit + usearch 异步 compact）

- [ ] 实现 BM25 `remove_chunks()`：`delete_term` 循环后追加 `writer.commit() + reader.reload()`。大量删除时（>100 条）触发异步延迟 commit。**安全保证**：outbox 删除记录只有在 BM25 commit + reader.reload + validate（检索确认已删除）成功后才标记完成；若进程在 commit 前崩溃，启动时 outbox 补偿会重试 BM25 删除。
- [ ] 实现 VectorIndex `remove_keys()` + `compact()`（异步 double-buffering，20% 阈值 + 5 分钟冷却）
- [ ] 实现 metadata 中的 outbox 记录逻辑（删除前记录待删 chunk_id）
- [ ] 实现 `delete_document` 事务编排（outbox→usearch→BM25→metadata，校验补偿）
- [ ] `cargo check` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

### 任务 1.4：消除重复 ensure_embedding_ready 代码

- [ ] 提取公共方法到 AppState
- [ ] 替换三处重复代码
- [ ] `cargo check` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

---

## 阶段二：原始资料层 + 持久化队列（3 天，依赖阶段一）

### 任务 2.1：raw_sources 表 + 源文件管理

- [ ] 实现 `raw_sources` 表（project, identity UNIQUE, storage_path, sha256, status CHECK active/deleted, deleted_at）
- [ ] 实现 Rust CRUD：`create_source`、`list_sources`、`soft_delete_source`、`get_source_by_identity`
- [ ] 修改导入流程：复制文件到 `raw/{project}/sources/` → 写入 raw_sources → 走现有 ingest
- [ ] `documents` 表新增 `raw_source_identity TEXT` 字段
- [ ] 迁移脚本：为已有 document 填充 raw_source_identity（匹配 sha256）
- [ ] `cargo check` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

### 任务 2.2：持久化摄入队列

- [ ] 实现 `IngestionQueue`：JSON 文件持久化，状态机 pending→processing→done/failed
- [ ] 项目级互斥锁（串行处理，防 index.md 竞争）
- [ ] 崩溃恢复：启动时 processing→pending
- [ ] 重试机制：最多 3 次
- [ ] `cargo check` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

---

## 阶段三：编译知识层（5 天，依赖阶段二，可与阶段四并行）

### 任务 3.1：wiki_pages 表 + CRUD

- [ ] 实现 `wiki_pages` 表（含 `content_candidate`、`candidate_status`、`candidate_version`、`page_status`、组合约束）
- [ ] 实现 `analysis_cache` 表（UNIQUE project, source_identity, sha256）
- [ ] 实现 `ingest_cache` 表（UNIQUE project, source_identity, sha256）
- [ ] 实现 Rust CRUD：create/update/get/delete/set_candidate/approve_candidate
- [ ] 注册 Tauri 命令
- [ ] `cargo check` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

### 任务 3.2：Step 1 双引擎（修正 7）

- [ ] 实现 `DocumentAnalysis` 结构体（兼容 LLM 和 Rust 两种输出）
- [ ] 实现 Rust 引擎：TF-IDF 关键词、标题层级树、词频统计
- [ ] 实现 LLM 引擎调用：prompt 组装 → LLM 流式调用 → 解析实体/概念/引用/矛盾
- [ ] 实现引擎选择逻辑：`enable_kb_compilation=true` 且 LLM 可用时用 LLM。LLM 超时（30s，含网络/API 延迟）或无可用 LLM 时自动降级为 Rust 引擎。降级后在前端提示"当前使用快速分析模式（非 LLM）"，用户可通过设置手动切换。
- [ ] 实现 `analysis_cache` 读写（写入分析结果 + 检查缓存）
- [ ] `cargo check` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

### 任务 3.3：两步摄入集成

- [ ] 实现 `process_with_kb_compilation(SourceInfo) -> Result<WikiPages>`：Step1 → Step2(可选) → 写入 → 更新 ingest_cache
- [ ] 在摄入队列处理流程中追加 KB 编译步骤：**保留现有 ingest**（写入 documents/chunks/vector/BM25），在其后追加 raw/wiki 编译流程。不替换现有 ingest。
- [ ] 增量缓存三重验证：SHA256 + files_written 文件存在性检查 + cache 命中跳过
- [ ] LLM 候选发布：写入 `content_candidate`，标记 `candidate_status='pending'`，不覆盖 `content`
- [ ] 差异检测：确定性 diff，≤30% → auto，>30% → conflict
- [ ] 后端 `approve_candidate` 命令：candidate→content，清空候选字段
- [ ] `cargo check` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

---

## 阶段四：调研大纲编辑器 + 脑图视图（9-10 天，可与阶段三并行）

**依赖注意**：任务 4.5 依赖 4.1 的 `outline_nodes` 表及 `session_qa_records` 字段迁移完成，不依赖阶段三。

### 任务 4.1：outline_nodes 表 + 后端 CRUD（含全部修正）

- [ ] `session_qa_records` 表迁移：`ALTER TABLE ADD COLUMN source TEXT CHECK('auto','manual')` + `is_bookmarked INTEGER`
- [ ] 实现 `outline_nodes` 表（`sort_order REAL`，`parent_id` 无级联，`question_id ON DELETE SET NULL`，无 `left_id`）
- [ ] 实现 Rust CRUD：
  - `create_node(session_id, parent_id, content)` — Fractional Indexing，首个 1.0，默认追加 `MAX+1.0`
  - `update_node(id, fields)` — 支持部分更新
  - `delete_node_subtree(id)` — 递归删除 + 孤儿 QA 清理（含 `source!='manual' AND is_bookmarked=0` 过滤）
  - `move_node(id, new_parent_id, new_sort_order)` — Fractional Indexing 平均值 + 防成环校验
  - `get_tree(session_id)` — `ORDER BY parent_id, sort_order`
  - `export_outline(session_id, format)` — markdown_list / markdown_headings
- [ ] 所有写操作成功后 `app_handle.emit("outline:changed", session_id)`
- [ ] 注册 Tauri 命令
- [ ] `cargo check` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

### 任务 4.2：OutlineContext 状态管理

- [ ] 实现 OutlineContext（OutlineTree 构建、选中节点、展开/折叠状态）
- [ ] `listen<number>("outline:changed")` 监听 + session_id 过滤 + 防抖刷新（300ms）
- [ ] Undo/Redo 栈。采用**操作日志模式**（存储 diff 而非全量快照），仅当节点数 ≤ 100 时使用全量快照作为降级方案。50 层上限。`Ctrl+Z` 取消 saveTimer → 恢复快照 → 重新触发保存
- [ ] `Ctrl+Shift+Z` Redo，同上竞态保护
- [ ] 800ms 防抖持久化
- [ ] `cargo check` + `npx tsc` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

### 任务 4.3：大纲树 UI 组件

- [ ] `OutlineTree`：递归渲染，`ORDER BY sort_order`
- [ ] `OutlineNode`：拖拽手柄、折叠/展开按钮、contentEditable 编辑、状态标记图标
- [ ] 快捷键实现：导航态 vs 编辑态区分（见设计决策快捷键映射表）
- [ ] ASR 语音按钮：调用 `src/lib/audio.ts` 统一接口，追加转写文本到 `content`
- [ ] `NodeDetailPanel`：备注编辑、标签编辑、Q&A 关联按钮
- [ ] **路由集成**：在 `src/App.tsx` 中注册大纲视图路由（`/research/:sessionId/outline`），在导航菜单添加入口，绑定 `ProjectContext` 的项目过滤
- [ ] `npx tsc` 零错误
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

### 任务 4.4：脑图视图

- [ ] 安装 `markmap-lib` + `markmap-view`
- [ ] 实现 `MindmapView`：`treeToMarkdown(tree, format="headings")` → markmap 渲染
- [ ] 大纲/脑图一键切换（共享 OutlineContext 数据源）
- [ ] 导出按钮（markmap SVG → PNG）

### 任务 4.5：历史数据迁移（依赖 outline_nodes 表就绪，不依赖阶段三）

> **依赖**：此任务依赖 4.1 的 `outline_nodes` 表及 `session_qa_records` 的字段迁移（`source`/`is_bookmarked`）完成，**不依赖阶段三**的 wiki_pages。因此可在阶段四内与任务 4.1-4.4 并行执行。

- [ ] 迁移脚本 Step 1：为有 QA 的 session 创建大纲根节点
- [ ] 迁移脚本 Step 2：历史 QA 转为子节点（`ROW_NUMBER()` 生成唯一 sort_order）
- [ ] 编译通过
- [ ] 验收测试通过后可按需提交

---

## 阶段五：知识图谱 + 图检索（独立计划）

阶段五已从本计划中拆分，转为独立的 `2026-06-01-kb-refactor-phase5-plan.md`。本阶段先行**占位**，不包含具体任务。

**前提**：阶段三上线后有足够的 `wiki_pages` 数据（建议 ≥ 50 页）。

**届时需确定的决策**：
- 图存储选型：SQLite 递归 CTE（适合 ≤ 5000 节点）vs 独立图数据库（适合更大规模）
- 性能指标：<5 秒指"构建全图"而非检索（检索应 <500ms）
- wikilink 编辑器是否与大纲编辑器合并

**相关设计文档**：设计决策文档 §2.5（4 信号知识图谱）+ §2.6（图扩展检索管道）

---

## 并行策略

```
阶段一 ████████████████████
阶段二 ████████████████████  ← 依赖阶段一
阶段三 ████████████████████  ← 依赖阶段二，与四并行
阶段四 ████████████████████  ← 不依赖二/三；任务 4.5 在 4.1 完成后可滞后执行
阶段五 → 已拆分为独立计划
```
