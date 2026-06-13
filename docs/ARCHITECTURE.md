# 顾问工作台 架构文档

> 软件名：顾问工作台（内部代号 KingdeeKB）
> 适用版本：v0.1.x
> 范围：技术栈、代码规范、技术选型、功能清单

---

## 1. 技术栈

### 1.1 运行时

| 层 | 技术 | 版本 |
|---|---|---|
| 桌面壳 | Tauri | 2.2 |
| 后端语言 | Rust | stable（edition 2021） |
| 前端框架 | React | 19.1 |
| 前端语言 | TypeScript | 5.8 |
| 前端构建 | Vite | 7.x |
| 包管理 | pnpm | — |

### 1.2 核心依赖（Rust）

| 类别 | 库 | 用途 |
|------|----|----|
| LLM 框架 | `rig-core` 0.37 | ReAct Agent、Provider 抽象 |
| HTTP 客户端 | `reqwest` 0.12 + `ureq` 3 | LLM 流式调用 |
| 异步运行时 | `tokio` 1 | 多任务调度 |
| 向量嵌入 | `fastembed` 5 | 本地 BGE-Small-ZH |
| 向量索引 | `usearch` 2 | HNSW 近似最近邻 |
| 全文检索 | `tantivy` 0.22 | BM25 索引 |
| 中文分词 | `jieba-rs` 0.7 | 全文检索中文分词 |
| 数据库 | `rusqlite` 0.32（bundled） | 元数据、文档、对话持久化 |
| 序列化 | `serde` 1 + `serde_json` + `serde_yaml` | 配置、数据交换 |
| 文档解析 | `pdf-extract` / `calamine` / `umya-spreadsheet` / `cfb` / `quick-xml` / `zip` | PDF/Excel/Visio 解析 |
| Token 计数 | `tiktoken-rs` 0.6 | 上下文窗口管理 |
| 语音识别 | `whisper-rs` 0.16 + `cpal` 0.15 + `hound` 3 | 本地 Whisper 推理与录音 |
| 视频转写 | `ffmpeg-sidecar` 2 | 从视频提取音频 |
| 加密/编码 | `hmac` 0.12 / `sha1` 0.10 / `base64` / `hex` | 腾讯云 ASR/MCP 签名 |
| 工具 | `regex` / `chrono` / `uuid` / `rand` / `urlencoding` / `bitflags` | 通用工具 |
| 错误处理 | `thiserror` 2 + `anyhow` 1 | 结构化错误与顶层错误传播 |
| 日志 | `tracing` 0.1 + `tracing-subscriber` 0.3 | 分级日志 |
| 异步抽象 | `async-trait` 0.1 | Service Trait |
| 凭证 | `keyring` 3 + `tauri-plugin-keyring-store` 0.2 | API Key 安全存储 |

### 1.3 核心依赖（前端）

| 类别 | 库 | 用途 |
|------|----|----|
| UI | `react` 19 / `react-dom` 19 | 视图层 |
| 路由 | `react-router-dom` 7 | 客户端路由 |
| 样式 | `tailwindcss` 4 + `@tailwindcss/vite` | 原子化 CSS |
| Markdown | `react-markdown` 10 + `remark-gfm` 4 | Markdown 渲染 |
| 思维导图 | `markmap-lib` / `markmap-view` / `markmap-common` 0.18 | 调研大纲可视化 |
| 图标 | `lucide-react` | UI 图标 |
| 差异 | `diff` 9 | 文档对比 |
| 排版 | `@tailwindcss/typography` | Markdown 排版 |
| Tauri 桥 | `@tauri-apps/api` 2.11 + `@tauri-apps/plugin-dialog/fs/opener/global-shortcut` | IPC / 文件 / 全局快捷键 |
| 测试 | `vitest` 4 + `@testing-library/react` 16 + `happy-dom` 20 | 单元/组件测试 |
| E2E | `@playwright/test` 1.60 | 端到端测试 |
| 规范 | `@biomejs/biome` 2.4 | 格式化与 lint |
| 类型检查 | `typescript` 5.8 | 静态类型 |

---

## 2. 代码规范

### 2.1 通用原则

- **注释语言**：所有源代码注释必须使用中文
- **行内解释优先**：用代码本身表达意图，仅在逻辑不显然时添加注释
- **避免 AI 味道**：禁止堆砌「这是一个 XXX 函数」「用于 YYY 用途」「返回 ZZZ」式冗余注释，禁止「如果 …… 则 …… 否则 ……」「这一步很重要」式说教
- **避免过度防御**：信任类型系统和框架保证，仅在系统边界（用户输入、外部 API）做校验
- **避免兼容代码**：项目尚未发布，重写时直接替换旧实现，不保留双协议/迁移/回退逻辑

### 2.2 Rust 规范

- 使用 `cargo fmt` 默认风格
- 使用 `cargo clippy --all-targets -- -D warnings` 检查
- 错误处理：`thiserror` 定义结构化错误，`anyhow` 仅用于顶层一次性传播
- 模块组织：命令层（`commands/`）只做参数校验和转发，业务逻辑放 `services/`
- 公开 API 必填文档注释（`///` 中文）
- 异步函数统一返回 `Result<T, AppError>`（`AppError` 来自 `src-tauri/src/error.rs`）

### 2.3 TypeScript / React 规范

- 使用 Biome 格式化与 lint（`pnpm lint` / `pnpm lint:fix`）
- 严格模式 `strict: true`，禁止 `any`（除显式标注的跨 IPC 边界）
- 组件：函数组件 + Hooks，状态就近用 `useState` 提升到 Context 需说明理由
- 跨 IPC 边界的数据用 `lib/` 下独立模块导出，类型与命令一一对应
- 命名：组件 PascalCase，Hook `useXxx`，工具函数 camelCase，常量 UPPER_SNAKE
- 注释：仅在「为什么」层面写注释，不复述代码

### 2.4 文件命名

- Rust：`snake_case.rs`
- TypeScript 组件：`PascalCase.tsx`
- TypeScript 工具/类型：`camelCase.ts`
- 技能资源：`SKILL.md`（按官方约定大写）
- 文档：`SCREAMING_SNAKE.md`（如 `USER-GUIDE.md`、`ARCHITECTURE.md`）

### 2.5 提交规范

- Conventional Commits 风格：`feat: ...` / `fix: ...` / `refactor: ...` / `docs: ...` / `chore: ...`
- 一个提交只做一件事；提交前必须 `cargo check` + `pnpm typecheck` + `pnpm lint` 通过

---

## 3. 技术选型

### 3.1 为什么用 Tauri 而不是 Electron？

- 安装包体积小一个数量级（Rust 编译产物 + 系统 WebView，无 Chromium 捆绑）
- 启动更快、内存占用更低
- Rust 后端原生支持细粒度并发（tokio），适合 LLM 流式调用与并行文档处理
- 跨平台一致的系统集成（钥匙串、文件对话框、全局快捷键、侧边栏）

### 3.2 为什么用 SQLite + 文件存储？

- 单用户本地工具，无需引入数据库服务
- 嵌入式 rusqlite 编译进二进制，零依赖部署
- SQLite 用于元数据、文档、对话、配置的结构化存储
- 嵌入向量、原始文件、产物文件、嵌入模型分目录存放文件系统，路径由 SQLite 引用

### 3.3 为什么用 BGE-Small-ZH + FastEmbed？

- 中文检索效果好，模型仅 ~90 MB
- FastEmbed 提供 ONNX 推理，无需 Python 环境
- 完全离线，避免每次检索都要走云端
- 模型可通过 Hugging Face 镜像预下载，离线场景也能用

### 3.4 为什么用 tantivy + usearch 双索引？

- 全文检索（tantivy BM25）解决关键词精确匹配
- 向量检索（usearch HNSW）解决语义相似
- RRF（Reciprocal Rank Fusion）融合两路结果，对中文支持好、参数简单
- 两者都是纯 Rust 实现，零外部依赖

### 3.5 为什么用 rig-core 而不是 LangChain？

- 纯 Rust 框架，与 Tokio 异步运行时无缝集成
- 编译期类型检查，LLM 调用更安全
- Provider 抽象统一 OpenAI / Anthropic / Ollama
- 体积小、依赖少，不会拖慢 Tauri 启动

### 3.6 为什么用 Whisper 本地推理？

- 完全离线，无网络即可语音转写
- 多语种支持，中文识别效果可接受
- 配合 ffmpeg-sidecar 还能从视频提取音频
- 替代方案（腾讯云/讯飞）需要 API Key 和上传音频流，仅作可选

### 3.7 为什么把 Skills 做成 SKILL.md 文件系统？

- 技能本质上是「约定优于配置」的内容 + 脚本，文件系统可读性最强
- 用户可直接编辑 `skills/<name>/SKILL.md` 自定义
- 通过 git 版本管理，与代码一致
- 技能运行时再加载、解析、执行，避免启动时全部解析

### 3.8 为什么用系统钥匙串存 API Key？

- 操作系统提供加密保护（Windows DPAPI / macOS Keychain / Linux Secret Service）
- Tauri 官方插件 `tauri-plugin-keyring-store` 桥接，零额外依赖
- 避免在 SQLite/配置文件中明文存储密钥，符合安全最佳实践

### 3.9 为什么状态管理用 React Context 而不是 Redux/Zustand？

- 当前页面数有限，Context 完全够用
- 没有复杂的跨切片状态更新（多数状态与当前项目绑定）
- 避免引入额外的心智负担和样板代码
- 若未来出现性能瓶颈，优先考虑拆分 Context 而非引入状态库

---

## 4. 项目结构

```
KingdeeKB/
├── src/                          # 前端 (React + TypeScript)
│   ├── components/               # 通用 UI 组件
│   │   ├── Layout.tsx            # 全局布局 + 侧边栏 + 状态栏
│   │   ├── Spotlight.tsx         # 全局快捷提问浮层
│   │   ├── ProjectSwitcher.tsx   # 项目切换器
│   │   ├── ImportModal.tsx       # 文档导入对话框
│   │   ├── ContextMenu.tsx       # 右键菜单
│   │   ├── ErrorBoundary.tsx     # 错误边界
│   │   ├── Toast.tsx             # 通知 Toast
│   │   ├── VerificationBadge.tsx # 验证徽标
│   │   ├── outliner/             # 调研大纲（编辑器/脑图/详情）
│   │   └── wiki/                 # Wiki 链接与图谱统计
│   ├── contexts/                 # React Context
│   │   ├── AgentContext.tsx      # Agent 槽位 + 消息流
│   │   ├── AppErrorContext.tsx   # 全局错误提示
│   │   ├── AudioContext.tsx      # 录音状态
│   │   ├── OutlineContext.tsx    # 调研大纲
│   │   └── ProjectContext.tsx    # 当前项目
│   ├── hooks/                    # 自定义 Hook
│   ├── lib/                      # 工具函数与 IPC 封装
│   │   ├── tauri-commands.ts     # 核心 Tauri 命令封装
│   │   ├── project-commands.ts   # 项目/阶段/源数据
│   │   ├── skill-commands.ts     # 技能系统
│   │   ├── outline-commands.ts   # 调研大纲
│   │   ├── wiki-commands.ts      # Wiki 页面
│   │   ├── app-error.ts          # 错误解析与格式化
│   │   ├── audio.ts              # 音频工具
│   │   ├── kingdee-qa.ts         # 金蝶领域问答数据
│   │   ├── keyring.ts            # 钥匙串封装
│   │   ├── clipboard-files.ts    # 剪贴板文件处理
│   │   ├── dialog-options.ts     # 文件对话框默认路径
│   │   └── skill-types.ts        # 技能类型
│   ├── pages/                    # 页面
│   │   ├── Home.tsx              # 概览
│   │   ├── Browse.tsx            # 知识库浏览
│   │   ├── Search.tsx            # 混合检索
│   │   ├── Chat.tsx              # AI 对话
│   │   ├── ResearchAssistant.tsx # 调研助手
│   │   ├── RiskControl.tsx       # 风险把控
│   │   ├── Products.tsx          # 产物管理
│   │   ├── ProjectManagement.tsx # 项目管理
│   │   ├── Skills.tsx            # 技能体系
│   │   ├── KnowledgeGraph.tsx    # 知识图谱
│   │   ├── Import.tsx            # 文档导入
│   │   └── Settings.tsx          # 设置
│   ├── sidebar/                  # 侧边栏独立入口（腾讯会议）
│   ├── App.tsx                   # 路由表
│   ├── main.tsx                  # 入口
│   └── index.css                 # 全局样式
│
├── src-tauri/                    # 后端 (Rust)
│   ├── src/
│   │   ├── main.rs               # 入口
│   │   ├── lib.rs                # 命令注册 + 模块导出
│   │   ├── app_state.rs          # 全局共享状态
│   │   ├── error.rs              # AppError 类型
│   │   ├── commands/             # Tauri 命令层（参数校验 + 转发）
│   │   │   ├── agent.rs
│   │   │   ├── core.rs
│   │   │   ├── document.rs
│   │   │   ├── embedding.rs
│   │   │   ├── ingestion.rs
│   │   │   ├── ingestion_queue.rs
│   │   │   ├── kb_compilation.rs
│   │   │   ├── knowledge_graph.rs
│   │   │   ├── llm_provider.rs
│   │   │   ├── media.rs
│   │   │   ├── outline.rs
│   │   │   ├── product.rs
│   │   │   ├── project.rs
│   │   │   ├── raw_source.rs
│   │   │   ├── research.rs
│   │   │   ├── risk_blueprint.rs
│   │   │   ├── search_llm.rs
│   │   │   ├── skill.rs
│   │   │   ├── tencent_meeting.rs
│   │   │   ├── verification.rs
│   │   │   └── wiki_page.rs
│   │   └── services/             # 业务实现
│   │       ├── rig_agent.rs      # ReAct Agent 引擎
│   │       ├── rig_tool.rs       # Agent 工具定义
│   │       ├── rig_provider.rs   # Provider 客户端
│   │       ├── react_agent.rs    # ReActEvent 事件枚举
│   │       ├── agent_router.rs   # Agent 模式路由
│   │       ├── planner.rs        # Plan-and-Execute
│   │       ├── llm_service.rs    # LLM 调用封装
│   │       ├── llm_providers.rs  # 多供应商管理
│   │       ├── prompt_assembler.rs
│   │       ├── prompts.rs        # 提示词常量
│   │       ├── tool_policy.rs    # 工具策略
│   │       ├── token.rs          # 统一 Token 计数
│   │       ├── model_metadata.rs # 模型能力元数据
│   │       ├── model_downloader.rs
│   │       ├── embedding.rs      # fastembed 包装
│   │       ├── vector_index.rs   # usearch 索引
│   │       ├── bm25_service.rs   # tantivy 全文
│   │       ├── hybrid_search.rs  # 混合检索融合
│   │       ├── chunker.rs        # 文档切片
│   │       ├── text_cleaner.rs
│   │       ├── chinese_postprocess.rs
│   │       ├── desensitize.rs    # 脱敏
│   │       ├── safety_filter.rs
│   │       ├── rerank.rs         # 重排
│   │       ├── memory.rs         # 聊天记忆
│   │       ├── file_extractor.rs # 多格式文本提取
│   │       ├── document_analysis.rs
│   │       ├── ingestion.rs / ingestion_pipeline.rs / ingestion_helpers.rs
│   │       ├── ingestion_queue.rs
│   │       ├── ingest_cache.rs
│   │       ├── analysis_cache.rs
│   │       ├── knowledge_graph.rs
│   │       ├── project_store.rs
│   │       ├── product_store.rs
│   │       ├── raw_source.rs
│   │       ├── wiki_page.rs
│   │       ├── wikilink_parser.rs
│   │       ├── research_session.rs
│   │       ├── research_outline.rs
│   │       ├── outline.rs
│   │       ├── risk_control.rs
│   │       ├── signal_writer.rs
│   │       ├── missing_detection.rs
│   │       ├── verification/     # 答案验证（引用/一致性/矛盾/不确定性）
│   │       ├── harness/          # 工程护栏（约束/熵/日志/验证）
│   │       ├── skill_manager.rs
│   │       ├── skill_loader.rs
│   │       ├── skill_trigger.rs
│   │       ├── skill_types.rs
│   │       ├── skill_executor.rs
│   │       ├── template_manager.rs / template_schema.rs
│   │       ├── template_docx.rs / template_xlsx.rs
│   │       ├── docx_filler.rs / xlsx_filler.rs
│   │       ├── image_processor.rs
│   │       ├── media.rs
│   │       ├── audio_capture.rs
│   │       ├── whisper_service.rs
│   │       ├── tencent_asr.rs
│   │       ├── tencent_meeting_mcp.rs
│   │       ├── video_transcriber.rs
│   │       ├── metadata.rs
│   │       ├── traits.rs
│   │       ├── spawn_safe.rs
│   │       ├── agent_timeout.rs
│   │       ├── question_tool.rs
│   │       └── types.rs
│   ├── resources/
│   │   ├── models/               # 本地嵌入模型（git LFS 或下载）
│   │   ├── prompts/              # 系统提示词与产物模板
│   │   └── model_specs.json      # 模型能力元数据
│   ├── capabilities/
│   │   └── default.json          # Tauri 权限声明
│   ├── tests/                    # 集成测试
│   ├── tauri.conf.json
│   ├── Cargo.toml
│   └── build.rs
│
├── skills/                       # 内置技能
│   ├── _shared/                  # 技能间共享脚本
│   ├── acceptance-pack/          # 验收包
│   ├── blueprint-tools/          # 蓝图工具
│   ├── build-tracker/            # 实施跟踪
│   ├── change-manager/           # 变更管理
│   ├── claude-req-analysis/      # 需求分析
│   ├── data-auditor/             # 数据审计
│   ├── data-cleaner/             # 数据清洗
│   ├── doc-sanitizer/            # 文档脱敏
│   ├── doc-tools/                # 文档工具
│   ├── drafter-diagram/          # 图形草稿
│   ├── golive-pack/              # 上线包
│   ├── humanizer/                # 人性化改写（英文）
│   ├── humanizer-zh/             # 人性化改写（中文）
│   ├── kdclub-ai-product-qa/     # 金蝶社区问答
│   ├── kickoff-pack/             # 启动包
│   ├── kingdee-ppt/              # 金蝶 PPT
│   ├── openai-whisper/           # OpenAI Whisper
│   ├── project-dashboard/        # 项目仪表盘
│   ├── project-init/             # 项目初始化
│   ├── project-sync/             # 项目同步
│   ├── qa-root-cause-analysis/   # 根因分析
│   ├── risk-manager/             # 风险管理
│   ├── skill-updater/            # 技能更新
│   ├── stakeholder-comms/        # 干系人沟通
│   ├── survey-assistant/         # 调研助手
│   ├── test-manager/             # 测试管理
│   ├── ux-flow-designer/         # 流程设计
│   └── weekly-report/            # 周报
│
├── e2e/                          # Playwright E2E 测试
│   ├── browse.spec.ts
│   ├── chat.spec.ts
│   ├── home.spec.ts
│   ├── import.spec.ts
│   ├── navigation.spec.ts
│   ├── risk.spec.ts
│   ├── search.spec.ts
│   └── settings.spec.ts
│
├── docs/
│   ├── USER-GUIDE.md             # 终端用户使用说明书
│   └── ARCHITECTURE.md           # 本文件
│
├── public/                       # 静态资源
├── AGENTS.md                     # 仓库级 AI 协作规则
├── README.md                     # 项目说明
└── package.json / pnpm-lock.yaml
```

---

## 5. 功能清单

### 5.1 知识库

| 功能 | 前端入口 | 后端模块 |
|------|---------|---------|
| 文档导入（单文件/文件夹/腾讯会议） | `pages/Import.tsx`、`components/ImportModal.tsx` | `commands/ingestion.rs`、`commands/raw_source.rs`、`services/ingestion*.rs` |
| 多格式文本提取 | — | `services/file_extractor.rs`（PDF/DOCX/XLSX/VSDX/HTML/MD/视频/音频/图片） |
| 切片与嵌入 | — | `services/chunker.rs`、`services/embedding.rs` |
| BM25 全文索引 | — | `services/bm25_service.rs`（tantivy + jieba） |
| 向量索引 | — | `services/vector_index.rs`（usearch HNSW） |
| 混合检索（RRF 融合） | `pages/Search.tsx` | `commands/search_llm.rs`、`services/hybrid_search.rs` |
| 知识库浏览与详情 | `pages/Browse.tsx` | `commands/document.rs` |
| 知识图谱 | `pages/KnowledgeGraph.tsx`、`components/wiki/GraphStatsBanner.tsx` | `services/knowledge_graph.rs` |
| 知识编译（Wiki 候选） | — | `commands/kb_compilation.rs` |
| 文档脱敏 | — | `services/desensitize.rs` |
| 文档重排 | — | `services/rerank.rs` |

### 5.2 AI 对话与 Agent

| 功能 | 前端入口 | 后端模块 |
|------|---------|---------|
| 流式 ReAct 对话 | `pages/Chat.tsx`、`contexts/AgentContext.tsx` | `commands/risk_blueprint.rs`、`services/rig_agent.rs` |
| 多槽位对话 | `contexts/AgentContext.tsx` | `services/rig_agent.rs` |
| Agent 工具系统 | — | `services/rig_tool.rs`（12 个工具） |
| 运行时工具 | — | `services/question_tool.rs`（澄清）、技能脚本执行 |
| 工具策略 | — | `services/tool_policy.rs` |
| 提示词组装 | — | `services/prompt_assembler.rs` + `prompts.rs` |
| Agent 模式路由 | — | `services/agent_router.rs`（ReAct / Plan-Execute） |
| Plan-and-Execute | — | `services/planner.rs` |
| 答案验证 | — | `services/verification/`（引用、一致性、矛盾、不确定性、self-consistency） |
| 工程护栏 | — | `services/harness/`（约束、熵、日志、验证器） |
| Token 精确计数 | — | `services/token.rs` |
| 模型能力感知 | — | `services/model_metadata.rs` + `resources/model_specs.json` |
| Agent 超时 | — | `services/agent_timeout.rs` |

### 5.3 调研助手

| 功能 | 前端入口 | 后端模块 |
|------|---------|---------|
| 调研会话 CRUD | `pages/ResearchAssistant.tsx` | `commands/research.rs`、`services/research_session.rs` |
| QA 记录 | `pages/ResearchAssistant.tsx` | `services/research_session.rs` |
| 调研大纲 | `components/outliner/*` | `services/research_outline.rs`、`services/outline.rs` |
| 思维导图 | `components/outliner/MindmapView.tsx` | （前端 markmap 渲染） |
| 蓝图提炼 | `pages/ResearchAssistant.tsx` | `services/rig_tool.rs`（`extract-blueprint`） |
| 导出 Markdown/CSV | `pages/ResearchAssistant.tsx` | `commands/research.rs` |
| 腾讯会议导入 | `pages/ResearchAssistant.tsx` | `services/tencent_meeting_mcp.rs` |

### 5.4 风险把控

| 功能 | 前端入口 | 后端模块 |
|------|---------|---------|
| 合同范围提取 | `pages/RiskControl.tsx` | `services/risk_control.rs` |
| 需求蔓延比对 | `pages/RiskControl.tsx` | `services/rig_tool.rs`（`check-scope-creep`） |
| 项目健康度 | `pages/RiskControl.tsx` | `services/rig_tool.rs`（`get-project-health`） |
| Fit/Gap 分析 | `pages/RiskControl.tsx` | `services/rig_tool.rs`（`analyze-fit-gap`） |
| 防身话术 | `pages/RiskControl.tsx` | `services/rig_tool.rs`（`generate-defense-script`） |
| 项目报告导出 | `pages/RiskControl.tsx` | `commands/risk_blueprint.rs`（`export_report`） |

### 5.5 交付物生成

| 功能 | 前端入口 | 后端模块 |
|------|---------|---------|
| 技能注册与匹配 | — | `services/skill_manager.rs`、`services/skill_trigger.rs` |
| 技能加载 | — | `services/skill_loader.rs` |
| 技能脚本沙箱 | — | `services/skill_executor.rs`（`spawn_safe.rs` 安全子进程） |
| 模板解析 | — | `services/template_manager.rs`、`template_schema.rs` |
| DOCX 模板填充 | — | `services/template_docx.rs`、`docx_filler.rs` |
| XLSX 模板填充 | — | `services/template_xlsx.rs`、`xlsx_filler.rs` |
| 产物持久化 | `pages/Products.tsx` | `services/product_store.rs`、`commands/product.rs` |
| 报告/周报/纪要/蓝图/上线/验收 | AI 对话中触发 | 各技能 SKILL.md（`skills/`） |
| PPT 套件 | `skills/kingdee-ppt/` | `commands/skill.rs` |

### 5.6 语音与媒体

| 功能 | 前端入口 | 后端模块 |
|------|---------|---------|
| 本地录音 | `contexts/AudioContext.tsx`、`lib/audio.ts` | `services/audio_capture.rs`（cpal） |
| 本地 Whisper 转写 | `pages/ResearchAssistant.tsx` | `services/whisper_service.rs` |
| 腾讯云 ASR | `pages/ResearchAssistant.tsx` | `services/tencent_asr.rs` |
| 视频转音频 | — | `services/video_transcriber.rs`（ffmpeg-sidecar） |
| 图片理解 | — | `services/image_processor.rs` |

### 5.7 项目管理

| 功能 | 前端入口 | 后端模块 |
|------|---------|---------|
| 项目 CRUD | `pages/ProjectManagement.tsx`、`components/ProjectSwitcher.tsx` | `commands/project.rs`、`services/project_store.rs` |
| 实施阶段 | `pages/ProjectManagement.tsx` | `commands/project.rs` |
| 源数据管理 | `pages/ProjectManagement.tsx` | `commands/raw_source.rs` |
| 导入队列与重试 | `pages/ProjectManagement.tsx` | `commands/ingestion_queue.rs`、`services/ingestion_queue.rs` |
| 项目仪表盘 | `pages/Home.tsx` | （组合多个查询） |

### 5.8 设置

| 功能 | 前端入口 | 后端模块 |
|------|---------|---------|
| LLM 供应商管理 | `pages/Settings.tsx` | `commands/llm_provider.rs`、`services/llm_providers.rs` |
| 嵌入模型下载与加载 | `pages/Settings.tsx` | `commands/embedding.rs`、`services/model_downloader.rs` |
| ASR 配置 | `pages/Settings.tsx` | `commands/media.rs` |
| 腾讯会议 MCP（Token 配置） | `pages/Settings.tsx` | `commands/tencent_meeting.rs` |
| 腾讯会议预约/管理/转写 | `pages/Meetings.tsx`、`pages/Home.tsx` | `services/tencent_meeting_mcp.rs` + `skills/tencent-meeting-mcp/` |
| 全局快捷键 | `pages/Settings.tsx`、`components/Spotlight.tsx` | （`tauri-plugin-global-shortcut`） |
| 数据导入/导出 | `pages/Settings.tsx` | `commands/core.rs` |

---

## 6. 关键流程

### 6.1 文档导入流程

```
用户触发导入
  → commands/ingestion.rs::enqueue_ingestion
  → 写入 ingestion_queue（pending）
  → 异步：services/ingestion_pipeline.rs
      ├── file_extractor 提取文本（含视频/音频转写）
      ├── text_cleaner 清理
      ├── desensitize 脱敏
      ├── chinese_postprocess 中文后处理
      ├── chunker 切片
      ├── embedding 计算向量（fastembed）
      ├── vector_index 写入 usearch
      └── bm25_service 写入 tantivy
  → 更新 ingestion_queue 状态（done / failed）
```

### 6.2 混合检索流程

```
用户查询
  → commands/search_llm.rs::hybrid_search
  → 并行：
      ├── bm25_service::search（关键词命中）
      └── vector_index::search（语义相似）
  → hybrid_search RRF 融合
  → rerank（可选）二次重排
  → 返回片段 + 来源文档
```

### 6.3 Agent 对话流程

```
用户输入（Chat / RiskControl / ResearchAssistant）
  → AgentContext.sendMessage(slotId, text)
  → invoke("agent_chat", { message, history, session_id })
  → commands/risk_blueprint.rs::agent_chat
      ├── agent_router 决定模式（ReAct / Plan-Execute）
      ├── skill_manager 匹配最佳技能，构建 skill_catalog
      ├── rig_agent::run
      │     ├── 提示词组装（系统提示词 + skill_catalog + 上下文 + 历史）
      │     ├── build_provider（OpenAI / Anthropic / Ollama）
      │     ├── agent.stream_prompt()
      │     └── drain_stream() 循环产出 ReActEvent
      └── Tauri emit("react-event", ReActEvent) → 前端 SSE
  → AgentContext 监听事件，更新 messages + currentTrace
```

### 6.4 技能执行流程

```
Agent 决定调用 use-skill 工具
  → services/skill_executor.rs
      ├── skill_manager 解析技能
      ├── 生成执行计划（含脚本命令）
      ├── 前端授权（用户确认）
      ├── spawn_safe 子进程执行脚本
      ├── 读取产物（DOCX/PPTX/MD/...）
      └── product_store 登记产物
  → 产物在「产物管理」页可访问
```

---

## 7. 安全与稳定性约束

| 机制 | 阈值 | 位置 |
|------|------|------|
| 死循环检测 | 连续 3 次相同工具调用 | `services/rig_agent.rs` |
| 工具速率限制 | 30 次/分钟 | `services/rig_agent.rs` |
| 会话超时 | 10 分钟 | `services/agent_timeout.rs` |
| LLM 调用超时 | 120s | `services/agent_timeout.rs` |
| 流式首字节超时 | 30s | `services/agent_timeout.rs` |
| 工具执行超时 | 120s | `services/agent_timeout.rs` |
| 最大工具调用轮数 | 10,000（名义） | `services/rig_agent.rs` |
| 输出 token 钳位 | 16K-32K | `services/llm_service.rs` |
| API Key 存储 | 操作系统钥匙串 | `tauri-plugin-keyring-store` |
| 技能脚本执行 | 需用户授权 | `services/skill_executor.rs` |
| 文档脱敏 | 导入前处理 | `services/desensitize.rs` |

---

## 8. 数据流与持久化

| 数据 | 存储位置 | 形式 |
|------|---------|------|
| 项目元数据、阶段 | SQLite | 结构化表 |
| 原始文档内容 | SQLite（content 表） | TEXT/BLOB |
| 切片文本 | SQLite | 关联到 raw_source |
| 嵌入向量 | usearch 索引文件 | 二进制（usearch 自有格式） |
| BM25 索引 | 文件系统 | tantivy 索引目录 |
| 对话历史 | SQLite | per session |
| 配置文件 | SQLite | kv 表 |
| 产物文件 | 文件系统 | 路径由 SQLite 引用 |
| 嵌入模型 | `resources/models/` | ONNX 文件 |
| API Key | 系统钥匙串 | DPAPI/Keychain/Secret Service |
| 提示词模板 | `resources/prompts/` | 文本文件 |

应用数据根目录：
- Windows：`%APPDATA%/kingdee-kb/`
- macOS：`~/Library/Application Support/kingdee-kb/`
- Linux：`~/.config/kingdee-kb/`（遵循 XDG）

---

## 9. 测试策略

| 层级 | 工具 | 范围 |
|------|------|------|
| 单元测试 | Rust 内置 `#[test]`、`vitest` | 函数、组件 |
| 集成测试 | `src-tauri/tests/` | 持久化、嵌入、视觉 |
| E2E | `playwright` + `tauri-mock` | 页面流程 |
| Lint | `biome check src/` + `cargo clippy` | 风格、错误 |
| 类型 | `tsc --noEmit` + `cargo check` | 类型 |

运行命令：
```bash
# 前端
pnpm typecheck
pnpm lint
pnpm test

# 后端
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test

# E2E
pnpm test:e2e
```

---

## 10. 构建与发布

```bash
# 开发
pnpm tauri dev

# 生产构建
pnpm tauri build
```

CI：`.github/workflows/build.yml`（跨平台矩阵构建）。

---

## 11. 已实现与未实现（透明化记录）

| 模块 | 已实现 | 未实现（已知限制） |
|------|-------|------------------|
| 腾讯会议预约 / 取消 / 查询 | ✅ 全部经 MCP（v1.0.10） | 无 |
| 会议转写 / AI 智能纪要 | ✅ 一键拉取 | 自动定时同步缺失（需手动） |
| 会议纪要落盘到 `00_项目管理/会议纪要/*.md` | ❌ 需在 AI 对话中触发 stakeholder-comms 技能 | 自动落盘 + 待办提取 缺失 |
| 会议预约的 Agent 工具注册 | ⚠️ MCP 透传已就绪，未在 Rig Agent 工具注册 | 智能对话中说"明天上午 10 点开会"尚不能自动预约 |
| 操作手册 / 培训材料 截图 | ❌ 不实现 | 真实界面截图由顾问从知识库中挑图或人工补图 |
| Mermaid 流程图 → PNG 渲染管线 | ❌ 不实现 | 当前为文本占位，导出时需手动渲染 |
| 蓝图 / 流程图 | ⚠️ 调用 `ux-flow-designer` 生成 Mermaid 文本 | 渲染嵌 Word/PPT 缺失 |
| 调研助手 → stakeholder-comms 技能 | ⚠️ 技能 SKILL.md 完整 | 未被 video_transcriber.rs 的硬编码 prompt 替换（待 P2） |

**已实现闭环**：

```
用户 AI 对话 → Rig Agent 工具 → MCP 透传 → 腾讯会议服务端
                  ↓
           MCP 返回会议号 / 详情
                  ↓
           前端渲染
```

**未完整闭环**（优先级 P2）：

```
会议结束 → 自动检测 → 拉转写 → stakeholder-comms 技能 → 落盘 00_项目管理/会议纪要/*.md → 提取待办 → 写入活动日志
         ↑ 这条链当前断在第一步
```
