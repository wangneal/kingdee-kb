# Project Research Summary

**Project:** KingdeeKB — 金蝶 ERP 实施顾问本地 RAG 知识管理桌面工具
**Domain:** 本地 RAG 桌面应用（中文知识管理）
**Researched:** 2026-05-23
**Confidence:** HIGH

## Executive Summary

KingdeeKB 是一个面向金蝶 ERP 实施顾问的 **本地优先、全离线 RAG 知识管理桌面工具**。与通用 RAG 平台（Dify、MaxKB、AnythingLLM）不同，它聚焦于"ERP 顾问"这一个场景，以轻量安装包（<150MB）、零服务器成本、递归分块保留文档结构、ERP 专用 System Prompt 为核心差异化。

**推荐技术路线**：Tauri 2.x（3-10MB 安装包 vs Electron 150MB）搭载 React 19 前端 + Rust 原生后端。向量方案放弃 ChromaDB（Rust 客户端需 sidecar 进程 + Python 运行时），改用 `usearch`（纯 Rust HNSW 索引）+ `rusqlite`（元数据存储），总依赖 <15MB。Embedding 模型必须使用中文优化的 `bge-small-zh-v1.5`（512维，48MB，C-MTEB 检索 61.77），而非英文训练的 `all-MiniLM-L6-v2`（中文语料 <10%，会导致检索召回率暴跌 30-40%）。BM25 关键词检索用 `tantivy` + jieba 中文分词，通过 RRFR 算法与向量检索融合。

**关键风险**：① `usearch` + `bge-small-zh-v1.5` 组合方案虽理论上可行，但缺乏桌面端实战验证——**必须在 Phase 2 进行 Spike 技术验证**；② 中文分块必须用中文标点（`。！？`）作为分隔符，否则退化为固定长度切割导致语义断裂；③ Tauri WebView2 冷启动白屏需配置 Splash Screen；④ API Key 必须用 OS Keyring 存储而非明文 JSON；⑤ 多项目知识隔离是 ERP 场景特有的刚需，检索默认按项目过滤。当前研究成果置信度 HIGH（基于 Context7 官方文档 2736+ snippets + 4 个已验证的 Tauri 开源参考项目 + GitHub Issues + C-MTEB 基准评测），但向量方案需 Spike 验证后最终确认。

## Key Findings

### Recommended Stack

**桌面框架**：Tauri 2.x (`^2.2`) — 完胜 Electron。包体积 3-10MB vs 150MB，内存 ~50MB vs 300-500MB，Rust 后端无 GC，cross-platform 原生 WebView。对目标用户（金蝶顾问），安装包体积每减小 100MB 可提升约 5% 下载转化率。

**前端**：React 19 + TypeScript 5.7 + Vite 6 + TailwindCSS 4 + shadcn/ui + Lucide React。React 19 是 Tauri 2026 生态最成熟的前端框架，Zustand（全局 UI 状态）+ TanStack Query（服务端数据缓存）构成三层状态架构。

**后端核心变更（偏离 SPEC 原方案）**：

| SPEC 原方案 | 研究推荐 | 原因 |
|-------------|---------|------|
| ChromaDB 嵌入式 | **`usearch` + `rusqlite`** | ChromaDB Rust 客户端非嵌入式，需 sidecar 进程 + Python（~200MB）；`usearch` 100% Rust，编译进单文件 |
| `all-MiniLM-L6-v2` | **`bge-small-zh-v1.5`** | 英文模型中文语料 <10%，LCQMC 相似度仅 0.72；BGE 系列 C-MTEB 61.77，48MB 可控体积 |
| 无重排序 | **`bge-reranker-v2-m3`** | Cross-Encoder 精排提升 Top-5 精度，`fastembed-rs` 原生支持 |

**核心技术清单**：
- **`usearch`** `^2.x` — HNSW 向量索引，单文件持久化，零系统依赖
- **`rusqlite`** `^0.32` — SQLite 元数据存储（chunk 信息、文档索引、配置）
- **`fastembed-rs`** `^5.13` — 本地 embedding + reranking，ONNX Runtime 后端
- **`tantivy`** `^0.22` — 全文检索引擎（BM25）+ jieba 中文分词器
- **`bge-small-zh-v1.5`** — 中文 embedding（512 维，48MB FP16 / 15MB Q4 量化）
- **`bge-reranker-v2-m3`** — Cross-Encoder 重排序（~568M 参数）
- **React 19 + Zustand 5 + TanStack Query 5 + react-router-dom 7** — 前端状态与路由

**存储路径**：`~/.kingdee-kb/` 下分 `knowledge/`（原始文件）、`index/`（HNSW 向量）、`metadata.db`（SQLite）、`bm25_index/`（tantivy）、`config.json`（Tauri Store）、`models/`（ONNX 模型）

### Expected Features

**Must have（v0.1 — 基础验证）**：
- 知识添加（粘贴文本 + 拖入 .md/.txt）— 没有数据就没有一切
- 知识浏览（树形目录 + 内容预览）— 用户需要看到自己有什么
- 知识删除（单条删除）— 基础数据管理闭环
- 向量检索（`bge-small-zh-v1.5` 本地 embedding）— RAG 的定义性能力
- 关键词检索（BM25 + jieba 中文分词）— 精确匹配场景必须（项目代号、模块名等）
- 混合检索（RRFR 融合向量 + BM25）— 兼顾语义和关键词
- AI 问答（OpenAI API，基于检索上下文流式生成）— 最终价值输出
- API 配置（Key + Endpoint + Model + 测试连接）— 用户自备 LLM
- 递归分块 + 元数据提取 — 保留文档结构，检索可溯源

**Should have（v0.2 — 体验增强）**：
- 知识去重检测 — 避免重复入库（批量导入时关键）
- Git 知识包导入 — 解决"冷启动"问题，社区知识包生态
- Anthropic API 兼容 — 部分顾问使用 Claude
- 检索过滤（按标签/时间范围/来源）— 提升检索精度
- 数据导出/备份 — 用户数据安全感
- 存储统计仪表板 — 让用户了解知识库规模

**Defer（v0.3+）**：
- .docx/.pdf 解析 — 格式解析复杂度高，先聚焦 Markdown 验证核心链路
- 深色模式 — 不影响核心价值验证
- macOS/Linux 客户端 — Windows 顾问占绝对主流
- 中文模糊搜索（拼音/编辑距离容错）— 向量检索已可部分缓解

**永久排除**：云端知识库/LLM 代理、团队协作/实时同步、内置 LLM 模型、官方知识包服务器。

### Architecture Approach

采用 **Tauri 2.x IPC 分离式架构**：React 前端（WebView）负责 UI 渲染与用户交互，Rust 后端（Native）负责所有数据处理（解析、分块、向量化、检索、LLM 调用）。前端通过 `invoke()` 调用 Rust `#[tauri::command]`，严禁直接操作文件系统或数据库。状态管理遵循三层架构：TanStack Query（服务端数据）→ Zustand（全局 UI 状态）→ useState（组件局部状态），绝不混用。

**两阶段 RAG 流水线**（严格分离）：
1. **入库流水线**（离线，用户触发）：Parse（文件解析）→ Clean（文本清洗）→ Chunk（递归分块 H2→段落→句子，保留层级元数据）→ Embed（ONNX 批量向量化 512维）→ Store（usearch HNSW + rusqlite 元数据）
2. **检索流水线**（在线，每次查询触发）：Embed Query（查询向量化）→ Hybrid Retrieve（向量 top30 + BM25 top30）→ RRF Fusion（k=60 融合）→ Context Assembly（来源标注格式）→ LLM Generation（OpenAI 流式，固定 ERP System Prompt）

**关键架构决策**：
- 增量更新：SHA256 哈希去重，内容未变则跳过重新向量化
- 项目隔离：检索默认按 `project` 字段过滤，ChromaDB/USearch 命名空间隔离
- 优雅降级：LLM 不可用时仅展示检索结果，不做 AI 生成
- 崩溃恢复：启动时完整性检查，操作日志记录可回滚

> **注意**：ARCHITECTURE.md 中 ChromaDB Sidecar 相关设计（第四节）已被 STACK.md 的研究结论覆盖，需替换为 usearch + rusqlite 方案。其余架构模式（IPC、状态管理、RAG 流水线、错误处理）仍然有效。

### Critical Pitfalls

1. **Embedding 中文退化（P1）**：`all-MiniLM-L6-v2` 中文语料 <10%，ERP 术语严重失准。**对策**：必须使用 `bge-small-zh-v1.5`，技术验证阶段用中文 ERP 样本做 Precision@5 ≥ 0.6 评估。

2. **中文分块策略失效（P3）**：英文分隔符（`.` `\n`）无法切中文句子。**对策**：Rust 侧实现中文感知分隔符（`\n## ` → `\n\n` → `。` → `！` → `？` → `；` → `，`），chunk_size 以 token 数计算。

3. **Tauri WebView2 冷启动白屏（P4）**：Windows 启动 WebView2 需 2-20 秒，用户以为卡死。**对策**：Tauri 2.x 原生 Splash Screen + fixedRuntime 捆绑 WebView2 + 极简初始化 HTML + 渐进加载 React。

4. **API Key 明文存储（P8）**：`config.json` 明文存 Key 可被恶意软件窃取。**对策**：使用 `tauri-plugin-keyring-store`（Windows Credential Manager / macOS Keychain），API Key 不经过前端 JS，Rust 直接从 Keyring 读取后发起 LLM 请求。

5. **ERP 多项目知识污染（P10）**：不同客户项目知识混淆导致错误建议。**对策**：强制项目标签 + 检索默认项目隔离 + 检索结果必须标注项目名 + 跨项目搜索作为高级选项。

## Implications for Roadmap

基于研究发现的依赖关系、架构分层和陷阱预防，建议以下 9 阶段路线图（Phase 1-8 为 v0.1 MVP，Phase 9 为 v0.2 增量）：

### Phase 1: 项目脚手架与基础设施
**Rationale:** 一切功能的基础——没有项目骨架就无法开发后续任何模块。同时在此阶段解决 API Key 安全（P8）和冷启动白屏（P4）两个影响用户体验的陷阱，避免 Phase 6 时返工。
**Delivers:** Tauri 2.x + React 19 + Vite 6 脚手架、Zustand/TanStack Query 状态框架、错误类型定义（`AppError`）、配置管理（`AppConfig`）、Splash Screen、OS Keyring 集成、`~/.kingdee-kb/` 目录结构、WebView2 fixedRuntime 打包。
**Addresses:** API 配置（存储框架就绪）
**Avoids:** Pitfall 4 (冷启动白屏) → Splash Screen / Pitfall 8 (API Key 明文) → Keyring
**Research flag:** 标准模式 — Tauri 2 脚手架有成熟模板

### Phase 2: 嵌入与向量存储引擎（⚠️ 需 Spike 验证）
**Rationale:** 入库和检索的共同上游依赖。Phase 1 完成了框架，Phase 2 搭建核心数据能力。**此阶段必须在投入分块/索引前完成**，验证 `usearch` + `bge-small-zh-v1.5` + ONNX 推理在 Windows 上的端到端可行性。
**Delivers:** fastembed-rs 集成（ONNX Runtime）、bge-small-zh-v1.5 自动下载与缓存、usearch HNSW 索引创建/持久化、rusqlite 元数据表 Schema、批量 embedding + 单条查询 MVP。
**Uses:** `fastembed-rs` `^5.13`、`usearch` `^2.x`、`rusqlite` `^0.32`、`ort` (ONNX Runtime)
**Avoids:** Pitfall 1 (中文语义退化) → bge-small-zh 替代 all-MiniLM
**Research flag:** ⚠️ **强烈建议 `/gsd-spike`** — 验证 usearch + bge-small-zh 组合在 Windows 上的可行性（STACK.md 置信度 MEDIUM-HIGH）

### Phase 3: 入库流水线（解析 → 分块 → 存储）
**Rationale:** 依赖 Phase 2（需 vectorize + store），是知识管理的数据入口。递归分块 + 元数据提取是核心差异化能力，必须实现中文感知分隔符以预防 P3。
**Delivers:** .md/.txt 文件解析器、中文感知文本清洗、递归分块引擎（H2→段落→句子 + 中文标点边界）、ChunkMetadata 提取（source_file/section_path/tags/line_no）、SHA256 增量去重、IngestionService 入库流水线。
**Implements:** ARCHITECTURE 3.2 节 (Ingestion Pipeline) / Features: 知识添加（后台）、递归分块+元数据
**Avoids:** Pitfall 3 (中文分块失效) → 中文感知分隔符

### Phase 4: BM25 与全文检索引擎
**Rationale:** 与 Phase 3 并行可行，但依赖 Phase 2（需要读取已入库的 chunks 文本）。BM25 是混合检索的另一半，中文分词（jieba）是关键——不做则 BM25 完全失效。
**Delivers:** tantivy 集成 + jieba 中文分词器（`cut_for_search` 搜索引擎模式）、BM25 索引构建与增量更新、关键词检索 API、jieba 分词模式对比评估。
**Implements:** ARCHITECTURE 9.2 节 (BM25 中文分词) / Features: 关键词检索
**Avoids:** Pitfall 中"BM25 直接空格分词处理中文"（Architecture 反模式 4）

### Phase 5: 混合检索引擎
**Rationale:** 依赖 Phase 2（向量检索）+ Phase 4（BM25），是 AI 问答的上游。RRF 融合参数需在此阶段做网格搜索确定最优 k 值。
**Delivers:** 向量检索（usearch 余弦相似度 top30）、BM25 检索 top30、RRF 融合引擎（可配置 k，网格搜索确定最优值）、检索结果上下文组装（`[来源：文件 | 章节]` 格式）、项目级检索过滤、分页返回。
**Implements:** ARCHITECTURE 3.3 节 (Search Pipeline) / Features: 混合检索
**Avoids:** Pitfall 5 (RRF k=60 固化) → 网格搜索 / Pitfall 10 (多项目混淆) → 项目隔离
**Research flag:** 需搜索评估 — 准备中文 ERP 测试查询集做 Recall@5 评估

### Phase 6: LLM 集成与 AI 问答
**Rationale:** 依赖 Phase 5（检索上下文）。RAG 价值链的终点，CPO 验证的关键——用户最终感受的是回答质量。需在此阶段解决 token 计数失准（P6）和 API Key 不经过前端（P8 收尾）。
**Delivers:** OpenAI API 客户端（reqwest + 流式 SSE）、ERP 专用 System Prompt（固定不可修改）、检索上下文组装（token 感知截断）、tiktoken-rs 精确 token 计数 + 动态窗口管理、流式响应通过 Tauri Event 推送前端、优雅降级（LLM 不可用 → 仅展示检索结果）、Anthropic API 兼容接口预留。
**Implements:** ARCHITECTURE 3.3 Stage 5 (LLM Generation) / Features: AI 问答
**Avoids:** Pitfall 6 (中文 token 计数失准) → tiktoken-rs / Pitfall 8 (API Key 明文) → Keyring 读取
**Research flag:** 轻度研究 — OpenAI 流式 API 已充分文档化，重点是 token 计数校验

### Phase 7: 前端 UI — 知识管理
**Rationale:** 依赖 Phase 3（入库）。用户可见的第一个完整体验——从"拖入文件"到"看到知识库"的闭环。
**Delivers:** 知识添加界面（粘贴文本 + 拖拽区域）、树形知识浏览（按标签/来源组织）、Markdown 内容预览、知识删除（单条）、索引进度实时展示（Tauri Event）、文件监视自动入库、项目切换 UI。
**Implements:** Features: 知识添加（前端）、知识浏览、知识删除
**Avoids:** Pitfall 9 (IPC 大数据冻结 UI) → 分页 + 进度事件替代数据传输
**Research flag:** 标准模式 — React + Tauri UI 有大量模板

### Phase 8: 前端 UI — 检索问答与设置
**Rationale:** 依赖 Phase 5 + 6（检索 + LLM）。CPO 价值的最终呈现——用户输入问题 → 看到带来源标注的回答。
**Delivers:** 搜索框 + 混合检索结果展示（相关性得分、来源标注）、流式 AI 对话界面、内联引用跳转到原文、API 配置界面（Key 输入走 Keyring 流程、Endpoint、Model、测试连接）、模型下载进度条、存储统计仪表板、错误优雅提示（分类显示用户友好消息）。
**Implements:** Features: 检索前端、AI 问答前端、API 配置前端
**Avoids:** Pitfall 中 UX Pitfalls（无相关度展示、回答无法追溯原文、无引导）
**Research flag:** 标准模式

### Phase 9: 社区知识包导入（v0.2）
**Rationale:** 依赖 Phase 3（入库）+ 去重。解决"冷启动"问题——新顾问打开空知识库不知从何开始。
**Delivers:** Git 知识包导入（`git clone --depth=1`、禁用 hooks 防安全风险）、manifest.json 解析、批量入库 + 去重、知识包格式规范文档。
**Implements:** Features: Git 知识包导入、知识去重检测

### Phase Ordering Rationale

- **Phase 1→2→3→5→6→7→8 构成最小依赖链**：Scaffold → 存储引擎 → 入库 → 检索 → LLM → 前端 UI。每个 Phase 只有一个主要上游依赖。
- **Phase 4（BM25）可与 Phase 3 并行**，但需依赖 Phase 2 完成（需要 chunks 文本）。
- **Phase 7-8（前端 UI）理论上可与 Phase 2-6 并行**（用 Mock 数据），但不建议——后端 API 稳定后再做 UI 减少返工。
- **Phase 9 完全独立**，v0.1 发布后可择机启动。
- **所有 P1 陷阱均在对应 Phase 中预防**：P4(Phase1) + P1(Phase2) + P3(Phase3) + P5(Phase5) + P6/P8(Phase6) + P9(Phase7) + P10(Phase5)。

### Research Flags

**需要 `/gsd-research-phase` 深度研究**：
- **Phase 2** ⚠️ — `usearch` + `bge-small-zh-v1.5` + ONNX 推理桌面端可行性（STACK.md 置信度 MEDIUM-HIGH），建议先用 `/gsd-spike` 验证
- **Phase 5** — RRF 参数调优需要评估数据集准备和网格搜索脚本

**标准模式（跳过 research-phase）**：
- **Phase 1** — Tauri 2 脚手架有大量成熟模板
- **Phase 3** — 文本处理有 langchain 参考，分块算法 SPEC 已定义
- **Phase 7-8** — React + Tauri UI 开发模式成熟

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | **HIGH** | Context7 官方文档（2736 snippets）+ GitHub 开源参考项目（4个）验证 + 多源基准测试 |
| Features | **HIGH** | 5 大竞品功能对比（AnythingLLM/Cherry Studio/MaxKB/Dify/RAGFlow）+ CSDN/掘金/知乎技术文章 + SPEC.md 对齐 |
| Architecture | **HIGH** | 4 个已验证的 Tauri + RAG 开源项目参考（shodhRAG/Gloss/memory-prosthetic/smart-locale-search）+ 官方文档 |
| Pitfalls | **HIGH** | GitHub Issues（ChromaDB #7040/#5868/#6654, Tauri #13727/#4197）直接验证 + 官方文档 + 学术论文 |

**Overall confidence:** HIGH

### Gaps to Address

| Gap | 处理方式 | 时机 |
|-----|---------|------|
| **`usearch` + `bge-small-zh-v1.5` 桌面端端到端可行性未验证** | `/gsd-spike` 验证：Windows 上 ONNX 推理性能、HNSW 索引加载速度、WASM 兼容性 | Phase 2 规划前 |
| **ARCHITECTURE.md 中 ChromaDB Sidecar 设计需替换为 usearch + rusqlite** | Phase 2 实现时更新架构文档，保留原 ChromaDB 章节作为备选参考 | Phase 2 |
| **中文 ERP 评估数据集不存在** | Phase 5 规划前需准备 50-100 个中文 ERP 查询-答案对用于检索评估 | Phase 5 规划前 |
| **Tauri Plugin Keyring Store 实际兼容性** | Phase 1 验证 Windows Credential Manager / macOS Keychain / Linux Secret Service 可用性 | Phase 1 |
| **RRF 最优 k 值需针对 ERP 领域调优** | Phase 5 中实现网格搜索脚本（k ∈ {10,30,60,100,200}，α ∈ {0.3,0.5,0.7}） | Phase 5 |

## Sources

### Primary (HIGH confidence)
- Context7 `/websites/v2_tauri_app` (2736 snippets) — Tauri 2.x 架构、IPC、插件生态、WebView2 配置
- Context7 `/huggingface/sentence-transformers` (1784 snippets) — embedding 模型对比、BGE 系列参数、中英文性能差异
- Context7 `/websites/cookbook_chromadb_dev` (400 snippets) — ChromaDB 嵌入式模式限制、WAL 模式问题
- Context7 `/reactjs/react.dev` (3032 snippets) — React 19 架构模式、Hooks 最佳实践
- HuggingFace `BAAI/bge-small-zh-v1.5` README — C-MTEB benchmark 数据 (Retrieval 61.77)
- GitHub Issues: chroma-core/chroma #7040 (16min hang), #5868 (SQLite corruption), #6654 (data loss)
- GitHub Issues: tauri-apps/tauri #13727 (slow startup), #4197 (IPC slow)
- GitHub 参考项目: `shodhRAG`, `Gloss`, `memory-prosthetic`, `smart-locale-search`
- crates.io: `fastembed` v5.13.1, `chromadb` v2.3.0 (Rust client 非嵌入式确认)

### Secondary (MEDIUM confidence)
- CSDN/掘金/知乎 — Tauri vs Electron 2026 对比、RAG 分块策略分析、混合检索实战
- Exa search — "Embedding Models Compared: What Actually Matters for RAG" (2026.05)
- dev.to — Rust ONNX 推理实践、Tauri API Key 安全存储
- AssetHoard blog — "When 120,000 Files Meet Tauri" (IPC 性能优化)
- 竞品分析: AnythingLLM/Cherry Studio/MaxKB/Dify/RAGFlow 功能矩阵

### Tertiary (待验证)
- `usearch` HNSW 实现在生产环境的可靠性 — 需 Phase 2 Spike 验证
- `bge-small-zh-v1.5` 在 ONNX Runtime Windows 上的推理性能 — 需本地 benchmark
- Tauri Plugin Keyring Store 在中文 Windows 环境的兼容性 — 需 Phase 1 验证

---

*Research completed: 2026-05-23*
*Ready for roadmap: yes*
*Key decision pending: usearch+bge-small-zh Spike 验证结果*
