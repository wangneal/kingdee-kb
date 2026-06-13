# KingdeeKB 整体技术规格书

> **版本**: v1.1
> **日期**: 2026-06-03
> **范围**: 涵盖全项目架构、所有子系统、数据流、命令注册表、数据库 schema
> **前置文档**: 各子系统详细设计见 `docs/superpowers/plans/` 下对应文档

---

## 1. 项目概述

KingdeeKB 是一个面向金蝶ERP实施顾问的 **AI 辅助知识库桌面应用**，使用 Tauri v2 (Rust) + React 19 + TypeScript 构建。

**核心价值**：帮助实施顾问快速检索金蝶产品知识、管理调研访谈、生成实施文档、控制项目风险。

---

## 2. 技术栈

| 层 | 技术 | 版本 |
|----|------|------|
| 桌面框架 | Tauri v2 | ^2.11.0 |
| Rust 后端 | rustc stable | — |
| 前端框架 | React | ^19.1.0 |
| 构建工具 | Vite | ^7.0.0 |
| 类型系统 | TypeScript | ~5.8 |
| 样式 | Tailwind CSS v4 | ^4.3.0 |
| UI 图标 | lucide-react | ^1.16.0 |
| 路由 | react-router-dom | ^7.15.0 |
| Markdown 渲染 | react-markdown + remark-gfm | ^10.1.0 |
| Tauri 插件 | dialog, fs, opener, global-shortcut | — |

**Rust 关键依赖**：

| Crate | 用途 |
|-------|------|
| `tantivy` ^0.22 | BM25 全文搜索引擎 |
| `jieba-rs` | 中文分词 |
| `usearch` | HNSW 向量索引 |
| `fastembed` | 本地 ONNX Embedding 模型 |
| `rusqlite` | SQLite 数据库 |
| `reqwest` | HTTP 客户端（LLM API 调用） |
| `serde` / `serde_json` | 序列化 |
| `keyring` | 系统凭据存储 |
| `tokio` | 异步运行时 |
| `tracing` | 日志 |
| `chrono` | 日期时间 |

---

## 3. 目录结构

```
KingdeeKB/
├── src/                            # React 前端
│   ├── main.tsx                    # 入口
│   ├── App.tsx                     # 路由 + Provider 层级
│   ├── App.css / index.css         # 全局样式
│   │
│   ├── pages/                      # 页面组件
│   │   ├── Home.tsx                # 首页仪表盘
│   │   ├── Chat.tsx                # AI 对话
│   │   ├── Browse.tsx              # 知识库浏览
│   │   ├── Search.tsx              # 搜索
│   │   ├── Import.tsx              # 文档导入
│   │   ├── ResearchAssistant.tsx   # 调研助手
│   │   ├── RiskControl.tsx         # 风险控制
│   │   ├── Settings.tsx            # 设置
│   │   ├── Skills.tsx              # 技能管理
│   │   ├── Products.tsx            # 产物管理
│   │   └── Browse.tsx              # 文档浏览
│   │
│   ├── components/                 # 通用组件
│   │   ├── Layout.tsx              # 布局 + 侧边栏
│   │   ├── Spotlight.tsx           # 全局搜索覆盖层
│   │   ├── Toast.tsx               # 消息提示
│   │   ├── ErrorBoundary.tsx       # 错误边界
│   │   ├── ContextMenu.tsx         # 通用右键菜单组件
│   │   ├── ImportModal.tsx         # 轻量导入弹窗
│   │   │
│   │   └── outliner/               # 大纲组件
│   │       └── OutlineTree.tsx     # 大纲树组件
│   │
│   ├── contexts/                   # React Context 状态管理
│   │   ├── AgentContext.tsx         # AI Agent 会话管理
│   │   └── ProjectContext.tsx       # 当前项目上下文
│   │
│   ├── hooks/                      # 自定义 Hooks
│   │   └── useImport.ts            # 可复用文档导入 Hook
│   │
│   └── lib/                        # 工具库
│       ├── tauri-commands.ts        # Tauri 命令封装（~1120 行）
│       ├── skill-commands.ts        # 技能系统命令封装
│       ├── skill-types.ts           # 技能系统类型
│       └── clipboard-files.ts       # 剪贴板文件提取
│
├── src-tauri/                      # Rust 后端
│   ├── src/
│   │   ├── lib.rs                  # Tauri app builder + 命令注册
│   │   ├── main.rs                 # 入口
│   │   ├── app_state.rs            # 全局状态容器（22+ 服务）
│   │   │
│   │   ├── commands/               # Tauri 命令（~12 个模块）
│   │   │   ├── core.rs             # 核心（数据目录、凭据、文件操作）
│   │   │   ├── embedding.rs        # Embedding 模型管理
│   │   │   ├── document.rs         # 文档 CRUD
│   │   │   ├── ingestion.rs        # 文档摄入
│   │   │   ├── search_llm.rs       # 搜索
│   │   │   ├── media.rs            # 语音/视频
│   │   │   ├── research.rs         # 调研管理
│   │   │   ├── product.rs          # 产物管理
│   │   │   ├── risk_blueprint.rs   # 风控/蓝图
│   │   │   ├── skill.rs            # 技能系统
│   │   │   └── llm_provider.rs     # LLM 供应商管理
│   │   │
│   │   └── services/               # 业务服务（~50+ 模块）
│   │       ├── mod.rs              # 模块注册
│   │       ├── metadata.rs         # SQLite 元数据存储
│   │       ├── embedding.rs        # Embedding 服务
│   │       ├── vector_index.rs     # usearch HNSW 索引
│   │       ├── bm25_service.rs     # BM25 全文搜索
│   │       ├── hybrid_search.rs    # 混合搜索
│   │       ├── chunker.rs          # 文本分块
│   │       ├── ingestion*.rs       # 摄入管道
│   │       ├── file_extractor.rs   # 文件格式提取
│   │       ├── text_cleaner.rs     # 文本清洗
│   │       ├── llm_service.rs      # LLM 调用
│   │       ├── llm_providers.rs    # LLM 供应商
│   │       ├── rig_agent.rs        # ReAct Agent 引擎
│   │       ├── planner.rs          # Plan-Execute 规划器
│   │       ├── prompts.rs          # 系统提示词
│   │       ├── research_session.rs # 调研会话
│   │       ├── research_outline.rs # 调研大纲
│   │       ├── research_indexer.rs # 大纲索引
│   │       ├── risk_control.rs     # 风控服务
│   │       ├── skill*.rs           # 技能系统（~10模块）
│   │       ├── template_*.rs       # DOCX/XLSX 填充与技能模板清单
│   │       ├── whisper_service.rs  # Whisper 语音识别
│   │       ├── audio_capture.rs    # 音频采集
│   │       ├── tencent_asr.rs      # 腾讯语音识别
│   │       ├── xfyun_asr.rs        # 讯飞语音识别
│   │       ├── asr_provider.rs     # ASR 抽象层
│   │       ├── video_transcriber.rs# 视频转写
│   │       └── image_processor.rs  # 图片处理
│   │
│   └── capabilities/
│       └── default.json            # Tauri 权限配置
│
├── package.json                     # 前端依赖
├── vite.config.ts                   # Vite 配置
├── tsconfig.json                    # TypeScript 配置
└── index.html                       # HTML 入口
```

---

## 4. 子系统架构

### 4.1 知识库子系统（Knowledge Base）

**职责**：文档摄入、存储、检索

```
文档 → file_extractor → text_cleaner → chunker → embedding → vector_index (usearch)
                                                               ↓
                                                         metadata (SQLite)
                                                               ↓
                                                         bm25_index (tantivy)

检索: 用户查询 → embedding → vector_search + BM25 → RRFR fusion → 结果
```

**核心表**：`documents`, `chunks`, `vector_key_seq`
**项目隔离**：`project TEXT` 字段贯穿所有表
**搜索**：hybrid_search (RRF, vector_weight=2, bm25_weight=1, k=60, TOP_N=200)

### 4.2 AI Agent 子系统

**职责**：多轮对话、知识库检索、工具调用、文档生成

```
用户输入 → AgentContext.sendMessage → agentChat (Tauri)
  → rig_agent (ReAct 引擎) → LLM 流式调用
  → 工具调用 (search-knowledge, use-skill, run-skill-script, question 等)
  → SSE 流式事件 → 前端渲染
```

**事件类型**：thinking, tool_call, tool_result, text_delta, done, error, clarification, plan_generated
**Agent 模式**：ReAct（默认）| Plan-Execute（复杂任务）

### 4.3 调研子系统

**职责**：调研会话管理、Q&A 记录、大纲结构化

```
大纲导入 (.docx) → ResearchIndexer → research_outlines + research_questions 表
调研会话 → ResearchSession → research_sessions + session_qa_records 表
AI 辅助 → AgentContext.sendMessage("research") → 知识库检索 + LLM 生成
```

### 4.4 风控子系统

**职责**：风险项目管理、范围蔓延检测、健康指标

```
风险项目 → risk_projects 表
范围条目 → scope_items 表（含合同范围/实际范围对比）
健康指标 → health_metrics 表
报告生成 → LLM 生成风险报告/话术
```

### 4.5 技能子系统

**职责**：外部技能包管理、脚本执行

```
技能包 (.zip) → skill_manager → skills/ 目录
技能匹配 → keyword/semantic/path 三种模式
脚本执行 → run-skill-script → 沙箱目录输出
```

### 4.6 技能交付物生成子系统

**职责**：通过官方技能生成文档、PPT、清单等实施交付物

```
用户请求 → AI 对话 → use-skill 匹配官方技能
技能指引 → run-skill-script 受控执行 → 沙箱输出目录
产物记录 → ProductStore / 产物管理页
```

### 4.7 语音/视频子系统

**职责**：录音转写、视频转录、ASR 服务管理

```
录音 → AudioCapture (PCM f32) → Whisper / 腾讯 ASR → 文本
视频 → 音频提取 → Whisper 转写 → 会议纪要生成
```

---

## 5. 数据库 Schema 总览

所有数据存储在 `~/.kingdee-kb/metadata.db`（单 SQLite 文件，WAL 模式）。

```sql
-- 已有表
documents              (id, title, source_path, sha256, project)
chunks                 (id, vector_key, document_id, content, section_path, tags)
vector_key_seq         (id INTEGER PRIMARY KEY AUTOINCREMENT)
app_config             (key, value) -- 键值配置

research_outlines      (id, edition, module_code, module_name, ...)
research_questions     (id, outline_id, edition, section, category, question_text, ...)
research_sessions      (id, title, edition, module_code, interviewee, session_date, project)
session_qa_records     (id, session_id, question_id, question_text, answer_text, sort_order)
research_editions      (id, name, label, is_active)

risk_projects          (id, name, kb_project, ...)
scope_items            (id, risk_project_id, source, type, description, ...)
health_metrics         (id, risk_project_id, metric_type, score, ...)

sensitive_keywords     (id, keyword)
skills                 (id, name, category, phase, ...)

products               (id, project, template_id, fields, status, ...)

asr_config             (tencent_secret_id, tencent_secret_key, ...) -- JSON 文件存储

-- 计划新增的表（详见设计决策文档）
raw_sources            (id, project, identity, original_path, storage_path, sha256, status)
wiki_pages             (id, project, slug, title, page_type, content, content_candidate, ...)
analysis_cache         (id, project, source_identity, sha256, analysis_json, ...)
ingest_cache           (id, project, source_identity, sha256, files_written, ...)
outline_nodes          (id, session_id, parent_id, content, note, collapsed, sort_order, question_id)
ingest_queue           (id, project, source_identity, status, retry_count, ...)
```

---

## 6. 数据流全景

```
┌─────────────────────────────────────────────────────────────────┐
│                        用户操作                                    │
├────────┬────────┬────────┬────────┬────────┬────────┬──────────┤
│ 导入   │ 对话   │ 搜索   │ 调研   │ 生成   │ 风控   │ 技能    │
│ 文档   │ Chat   │ Search │Research│ Chat   │ Risk   │ Skills  │
└───┬────┴───┬────┴───┬────┴───┬────┴───┬────┴───┬────┴───┬──────┘
    │        │        │        │        │        │        │
    ▼        ▼        ▼        ▼        ▼        ▼        ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Tauri Invoke Layer                            │
│  (src/lib/tauri-commands.ts + skill-commands.ts)                 │
└───────────┬─────────────────────────────────────┬────────────────┘
            │ IPC (JSON serialized)                │
            ▼                                      ▼
┌───────────────────────┐          ┌──────────────────────────────┐
│  Rust Backend          │          │  File System                 │
│  (commands/*.rs)       │          │  ~/.kingdee-kb/              │
│    ↓                   │          │  ├─ metadata.db              │
│  Services (services/*) │          │  ├─ index/vectors.usearch    │
│    ↓                   │          │  ├─ bm25_index/              │
│  External APIs         │          │  ├─ models/                  │
│  (LLM, ASR, ...)       │          │  ├─ products.db              │
└───────────────────────┘          │  ├─ skills/                   │
                                   │  └─ templates/               │
                                   └──────────────────────────────┘
```

---

## 7. Tauri 命令注册表

lib.rs 中注册了约 **130+** 命令，按子系统分组：

| 子系统 | 命令数 | 注册位置 |
|--------|--------|---------|
| Core (数据目录/凭据/文件) | 8 | `commands::core::*` |
| Embedding | 9 | `commands::embedding::*` |
| Ingestion | 4 | `commands::ingestion::*` |
| KB Compilation | 2 | `commands::kb_compilation::*` |
| Document | 5 | `commands::document::*` |
| Search | 3 | `commands::search_llm::*` |
| Product | 5 | `commands::product::*` |
| Media (Whisper/ASR/Video) | 9 | `commands::media::*` |
| Research | 17 | `commands::research::*` |
| Risk Blueprint | 20 | `commands::risk_blueprint::*` |
| Skill System | 16 | `commands::skill::*` |
| LLM Provider | 22 | `commands::llm_provider::*` |

---

## 8. AppState 全局状态容器

```rust
pub struct AppState {
    // 读多写少（RwLock）
    pub metadata: RwLock<MetadataStore>,
    pub bm25: RwLock<BM25Service>,
    pub product_store: RwLock<ProductStore>,
    pub risk_store: RwLock<RiskControlStore>,
    pub asr_config: RwLock<AsrConfigStore>,

    // 写密集（Mutex）
    pub embedding: Mutex<EmbeddingService>,
    pub vector_index: Mutex<VectorIndex>,
    pub skill_manager: Mutex<SkillManager>,
    pub llm_providers: Mutex<LLMProviderManager>,
    pub whisper_service: Mutex<WhisperService>,
    pub audio_capture: Mutex<AudioCapture>,
    pub image_processor: Mutex<ImageProcessor>,
    pub llm: LLMService,

    // 无竞争
    pub data_dir: PathBuf,
}
```

**锁获取顺序（防死锁）**：
```
metadata → bm25 → vector_index → embedding → 其他
```

---

## 9. 安全模型

| 层面 | 机制 |
|------|------|
| API Key 存储 | 系统凭据存储（keyring），不入文件 |
| LLM API Key | 前端内存 → 后端 Mutex，不持久化到磁盘 |
| 文件沙箱 | `run-skill-script` 在独立沙箱目录执行 |
| ZIP 路径防护 | zip-slip 校验（拒绝 ParentDir/RootDir/Prefix） |
| CSP | Tauri 默认 CSP + `style-src 'unsafe-inline'` |
| SQL 注入 | 全部使用参数化查询（rusqlite params!） |
| XSS | React 默认转义 + markmap HTML entity 转义 |

---

## 10. 项目上下文模型

```
Project（逻辑概念，无独立表）
  ├── project TEXT 字段贯穿 documents / chunks / wiki_pages / raw_sources
  ├── research_sessions
  ├── products
  └── 特殊前缀 chat-attachments:{session_id}（临时隔离）

前端 ProjectContext（src/contexts/ProjectContext.tsx）
  ├── projectId: string | undefined
  ├── setProjectId: (id) => void
  └── 所有页面通过 useProject() 统一读取
```

---

## 11. 构建与部署

```bash
# 开发
npm run tauri:dev        # Vite dev server + Tauri

# 构建
npm run tauri:build       # 生产构建（Windows MSI/NSIS）

# 类型检查
npx tsc --noEmit          # TypeScript
cargo check               # Rust

# 测试
npm run test:e2e          # Playwright E2E
cargo test                # Rust 单元测试
```

**Tauri 配置要点**（`src-tauri/tauri.conf.json`）：
- 窗口：1000x700，最小 800x600
- 权限：`core:default`、`opener:default`、`dialog:default`、`fs:*`
- 安装模式：WebView2 downloadBootstrapper
- 多语言：简体中文 NSIS 安装包

---

## 12. 相关文档索引

| 文档 | 位置 |
|------|------|
| llm_wiki 调研报告 | `2026-06-01-llm-wiki-research-report.md` |
| KB 重构 + 大纲设计决策 | `2026-06-01-kb-refactor-design-decisions.md` |
| 规格概要（快速导航） | `2026-06-01-kb-refactor-research-outliner-spec.md` |
| 项目规则 | `AGENTS.md` |

---

## 13. 最近变更

**v1.1 (2026-06-03)**

- 新增：ContextMenu 通用组件（Portal 渲染、视口边界检测、Esc 关闭）
- 新增：ImportModal 轻量导入弹窗（文本/文件/文件夹三 Tab）
- 新增：useImport hook（自动读取知识编译配置，封装 ingest 函数）
- 新增：kb_compilation 命令（get/set_kb_compilation_enabled）
- 修改：OutlineNode 添加 onContextMenu 事件
- 修改：OutlineTree 空状态添加"导入文档"按钮、面板空白区域右键支持
- 修复：Markdown 代码块样式冲突（pre>code 白底白字问题）
