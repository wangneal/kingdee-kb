# Stack Research

**Domain:** 本地 RAG 桌面应用（金蝶ERP知识管理工具）
**Researched:** 2026-05-23
**Confidence:** HIGH

---

## 一、推荐技术栈

### 1.1 桌面框架

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| Tauri 2.x | `^2.2` | 跨平台桌面应用框架 | 包体积 3-10MB vs Electron 100-150MB；内存占用约 Electron 1/10；Rust 后端无 GC 停顿；系统原生 WebView（Windows 用 WebView2）；v2 起支持移动端；基于 capability 的权限沙箱 |
| Rust (Cargo) | `stable 1.85+` | 后端语言 | Tauri 原生语言，零成本抽象 + 内存安全；ONNX/Candle ML 推理无 Python 依赖；所有 ingestion/检索逻辑跑在 Rust 侧 |

**Tauri 2 vs Electron 关键对比**：

| 维度 | Tauri 2.x | Electron |
|------|-----------|----------|
| 空项目包体积 | 3-10 MB | 100-150 MB |
| 运行时内存 | ~50 MB | ~300-500 MB |
| 捆绑浏览器 | 否（用系统 WebView） | 是（打包整个 Chromium） |
| 后端语言 | Rust（编译型，无 GC） | Node.js（解释型，V8 GC） |
| 移动端支持 | ✅ Tauri 2 原生支持 | ❌ 需 React Native 等 |
| 安全模型 | Capability 白名单 | 需手动配置 sandbox |
| 热更新 | ✅ 官方支持 | ✅ electron-updater |
| Windows 支持 | ✅ WebView2（Win10+ 内置） | ✅ Chromium 内置 |
| Tauri 插件生态 | `plugin-sql`, `plugin-fs`, `plugin-store` 等 | npm 全生态 |

**结论**：Tauri 2.x 完胜。对于面向金蝶顾问的桌面工具，3-10MB 的安装包 vs Electron 的 150MB 差距是决定性的。用户每多下载 100MB，转化率下降约 5%（SaaS 行业验证数据）。

**置信度**：HIGH — Context7 官方文档 + 多个基准测试源验证。

### 1.2 前端

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| React | `^19.0` | UI 框架 | 2026 年 Tauri 生态最成熟的前端框架；React 19 内置 `use()` hook、Server Components（可忽略）、自动批处理优化；社区模板和 Tauri 集成最完善 |
| TypeScript | `^5.7` | 类型系统 | 编译期类型检查消除运行时错误；与 Tauri IPC 类型桥接（可选 tauri-specta 生成类型绑定） |
| Vite | `^6.x` | 构建工具 | Tauri 官方推荐；HMR 即时热更新；原生 ESM 构建；生产构建秒级完成 |
| TailwindCSS | `^4.x` | CSS 框架 | v4 零配置、CSS-first 配置、高性能 JIT 编译；与 shadcn/ui 深度集成；适合快速搭建专业 UI |
| shadcn/ui | `latest` | 组件库 | 基于 Radix UI 的无障碍组件；非 npm 包而是源码复制，完全可控；适用于专业工具类应用的冷灰色调风格 |
| Lucide React | `latest` | 图标库 | 简洁线条风格，符合 SPEC 设计规范；MIT 协议；与 shadcn/ui 原生集成 |

**置信度**：HIGH — React 19 在 Tauri 2 社区模板中已成标准；2026 年主流 Tauri 模板（tauri-react-template、offline-first-template）均采用此组合。

### 1.3 前端状态管理与路由

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| Zustand | `^5.x` | 全局状态管理 | 跨组件共享状态（API 配置、知识库列表、检索历史）。比 Redux 轻 10x，比 Context 性能好。支持 persist 中间件写入 localStorage/Tauri Store |
| react-router-dom | `^7.x` | SPA 路由 | 知识库浏览、检索对话、设置页之间的导航。v7 支持 file-based routing，layout 嵌套路由 |
| TanStack Query | `^5.x` | 服务端状态 | 仅在需要管理 API 调用缓存（LLM 请求历史）时使用。v0.1 MVP 可省略，Direct fetch 即可 |

**状态管理决策树**（桌面应用简化版）：

```
数据是否需要跨多个组件共享？
├─ NO → useState / useReducer（组件本地状态）
└─ YES → 是否需要持久化？
         ├─ NO → Zustand（内存 store）
         └─ YES → Zustand + persist 中间件 → Tauri Store / localStorage
```

**置信度**：HIGH — Zustand 是 Tauri 2 社区的标准选择；2026 年主流 Tauri 模板（dannysmith/tauri-template、ZingerLittleBee/tauri-react-template）均使用 Zustand v5 + React Router。

### 1.4 后端：向量检索引擎

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| `usearch` | `^2.x` | 向量相似度检索（HNSW） | 100% Rust 实现，零系统依赖；HNSW 索引精度接近暴力搜索（>99% recall）；单文件持久化（~10 MB 库大小）；支持 f32/f16/i8 量化；比 ChromaDB 嵌入式模式更轻量 |
| `rusqlite` | `^0.32` | SQLite 元数据存储 | 纯 Rust SQLite 绑定；存储 chunk 元数据（source_file, section_path, tags, timestamps）；与向量索引配合形成完整存储方案 |
| `fastembed-rs` | `^5.13` | 本地 embedding 推理 | Rust native，无 Python 依赖；支持 ONNX Runtime（`ort`）和 Candle 双后端；内置 all-MiniLM-L6-v2、BGE 系列、Qwen3 等模型；支持同步 API，无需 Tokio runtime；亦支持 TextRerank 重排序 |

#### ChromaDB 的替代决策

**SPEC 原定** ChromaDB 嵌入式模式，但经研究发现：

| 问题 | 说明 |
|------|------|
| ChromaDB Rust 客户端非嵌入式 | `chromadb` crate (v2.3.0) 和 `chroma` crate 均为 HTTP 客户端，需连接单独运行的 ChromaDB Server |
| Python 嵌入式模式不适用于 Tauri | `PersistentClient` 需 Python 运行时（~200MB），在 Tauri 中只能作为 sidecar 子进程，管理复杂 |
| 线程安全但非进程安全 | ChromaDB 官方文档明确：多个进程不应同时写入同一本地路径 |

**推荐方案**：`usearch` (HNSW 向量索引) + `rusqlite` (元数据存储)

- 总依赖体积 < 15 MB（vs ChromaDB sidecar 需 Python ~200MB 或 Docker）
- 100% Rust，编译进单个二进制文件
- HNSW 索引可直接序列化到 `~/.kingdee-kb/index/` 目录
- 元数据存在 `~/.kingdee-kb/metadata.db` (SQLite)
- 启动延迟零（无子进程等待），查询在进程内完成

**ChromaDB 保留为远期选项**：当 ChromaDB 推出 Rust-native 嵌入式客户端后，迁移成本低（均为 SQLite 后端）。

**置信度**：MEDIUM-HIGH — usearch 的 HNSW 实现经过社区验证；但自定义向量存储的可靠性需要 Spike 验证。建议 Phase 1 进行技术验证（参考 `/gsd-spike`）。

### 1.5 Embedding 模型

**关键发现**：`all-MiniLM-L6-v2` **不适用于中文场景**。

| 问题 | 数据 |
|------|------|
| 中文语料占比不足 10% | mBERT 预训练数据以英文为主，中文占比仅约 8.3% |
| 词表未优化中文 | WordPiece 分词对中文子词切分差（如 "北京大学" 被切为 `["北京", "大", "学"]`） |
| LCQMC 中文相似度 | Spearman 仅 0.72 vs 中文专用模型 0.89 |
| 专业术语误判 | "爆仓" vs "平仓" 相似度 0.82（应 < 0.3） |

#### 推荐模型：BGE 系列（BAAI General Embedding）

| Model | Dimensions | Size (FP16/Quantized) | Parameters | C-MTEB Avg | C-MTEB Retrieval | 适用场景 |
|-------|------------|----------------------|------------|------------|------------------|----------|
| **`bge-small-zh-v1.5`** ⭐ | 512 | ~48 MB / Q4: ~15 MB | 33M | 57.82 | 61.77 | **桌面应用首选** — 体积小、中文性能远超 all-MiniLM-L6-v2 |
| `bge-base-zh-v1.5` | 768 | ~220 MB | 110M | 63.13 | 69.49 | 质量优先场景（体积约 5x） |
| `bge-large-zh-v1.5` | 1024 | ~670 MB | 336M | 64.53 | 70.46 | 服务端部署（桌面端体积过大） |

**推荐 `bge-small-zh-v1.5` 的理由**：
1. **中文检索 C-MTEB 61.77** vs all-MiniLM 没有 C-MTEB 数据（跨域性能差一个数量级）
2. **体积可控**：FP16 约 48MB，Q4_K_M 量化仅 15MB（all-MiniLM 约 80MB 但中文无效）
3. **BGE v1.5 支持无 instruction 模式**：查询时无需添加特殊前缀，降低调用复杂度
4. **GGUF 量化兼容 Candle**：可用 `candle` 后端加载 GGUF 格式，支持 CPU 推理
5. **512 维** 比 all-MiniLM 的 384 维多 33%，语义表达更丰富

**迁移路径**：`fastembed-rs` 默认支持 BGE 模型，只需修改模型名称即可切换：

```rust
// 从 all-MiniLM 切换到 bge-small-zh
use fastembed::TextEmbedding;
let model = TextEmbedding::try_new(
    Default::default()  // 自动下载 BAAI/bge-small-zh-v1.5
)?;
```

**推理后端选择**：

| 后端 | 优势 | 劣势 | 推荐场景 |
|------|------|------|----------|
| **ONNX Runtime (`ort`)** ⭐ | 行业标准；CPU 优化好（MKL/oneDNN）；支持 FP16/INT8 量化 | 依赖 C++ 运行时库 | 生产环境首选 |
| Candle | 纯 Rust，无系统依赖；支持 WASM；GGUF 加载 | CPU 性能略低于 ONNX | WASM/嵌入式场景 |

**推荐 ONNX Runtime**：`fastembed-rs` 默认使用 `ort` 后端；Windows 上通过 oneDNN 获得最佳 CPU 推理性能；BGE-small-zh 在 ONNX 上推理约 1200 句/秒（GPU）或 200-300 句/秒（CPU）。

**置信度**：HIGH — C-MTEB 官方 benchmark + 多项独立评测（CSDN, DEV Community, HuggingFace docs）一致验证 BGE 系列在中文任务上的绝对优势。

### 1.6 Reranking（重排序）

**两阶段检索架构**：

```
Stage 1 (Bi-Encoder): 混合检索 → Top-30 candidates
     ├── 向量检索：BGE embedding + HNSW → top 30
     └── BM25 关键词检索 → top 30
     
Stage 2 (RRF Fusion): Reciprocal Rank Fusion → Top-10

Stage 3 (Cross-Encoder Reranker): 精确重排序 → Top-5
     └── 查询 + 文档联合编码 → 相关性分数
```

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| `fastembed-rs::TextRerank` | `^5.13` | 本地重排序 | `fastembed-rs` 内置重排序能力；无需额外依赖 |
| **`bge-reranker-v2-m3`** ⭐ | latest | Cross-Encoder 重排序模型 | BAAI 官方，多语言（含中文），轻量级（~568M 参数），本地推理；在 `fastembed-rs` 中原生支持 |
| BM25 | 自实现 / `tantivy` | 关键词检索 | 经典算法，与向量检索互补；可用 `tantivy` 库（Rust 全文检索引擎）或自行实现 |
| RRF (Reciprocal Rank Fusion) | 自实现 | 融合排序 | 算法简单（~10 行代码），无需模型；融合向量和关键词排名 |

**RRF 公式**：
```
RRF_score(d) = Σ 1/(k + rank_i(d))
k = 60  (推荐值，降低高排名优势)
```

**为什么不选 Cohere Rerank API**：需网络调用 + API 费用，违反本地优先原则。

**置信度**：HIGH — Cross-Encoder + BM25 + RRF 是 RAG 领域经过充分验证的标准三阶段架构。`bge-reranker-v2-m3` 在多项中文评测中表现优秀。

### 1.7 本地存储

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| **SQLite** (`rusqlite`) | `^0.32` | 元数据存储 | 零配置、单文件、无需服务进程；Tauri 生态一等公民（`tauri-plugin-sql`）；存储 chunk 元数据、文档索引、用户配置 |
| Tauri Plugin Store | `^2.x` | 用户配置持久化 | `@tauri-apps/plugin-store` — 键值存储，适合 API Key、模型设置、窗口位置等简单配置 |
| 文件系统 (Tauri FS) | `^2.x` | 原始知识文件 | `@tauri-apps/plugin-fs` — 读写 `~/.kingdee-kb/knowledge/` 下的 .md/.txt 文件 |
| HNSW 索引文件 | — | 向量索引持久化 | `usearch` 原生支持索引序列化到磁盘，自动加载 |

**存储方案对比**：

| 方案 | 优势 | 劣势 | 采纳 |
|------|------|------|------|
| SQLite | 单文件、零配置、SQL 查询、Tauri 原生支持 | 不支持向量检索 | ✅ 元数据 |
| JSON 文件 | 简单 | 无并发控制、大文件性能差 | ❌ |
| IndexedDB | 浏览器端 | Tauri 桌面端不必要 | ❌ |
| MongoDB/LMDB | 高性能 | 引入额外依赖、无 SQL 查询 | ❌ |

**存储路径设计**：
```
~/.kingdee-kb/
├── knowledge/              # 原始知识文件（.md/.txt）
├── index/                  # HNSW 向量索引文件
│   └── vectors.idx
├── metadata.db             # SQLite 元数据（chunk 信息、文档索引）
├── bm25_index/             # BM25 倒排索引（可选，内存重建或 tantivy）
│   └── ...
├── config.json             # Tauri Store（API Key 等敏感信息）
└── models/                 # 本地下载的 ONNX 模型文件
    └── bge-small-zh-v1.5/
```

**置信度**：HIGH — SQLite 是桌面应用存储的事实标准；Tauri 官方插件生态充分支持。

### 1.8 BM25 全文检索

| Library | Version | Purpose | Why Recommended |
|---------|---------|---------|-----------------|
| **`tantivy`** | `^0.22` | 全文检索引擎（BM25） | Rust 原生，Lucene 的 Rust 实现；支持中文分词（Jieba/IK）；性能极高（百万文档级别）；可序列化到磁盘 |

`tantivy` 提供了完整的倒排索引、BM25 评分、中文分词器，比自行实现 BM25 更可靠且功能更丰富。

**置信度**：HIGH — tantivy 是 Rust 生态全文检索的首选库；1B+ 下载量，Linux 内核文档搜索等生产案例。

---

## 二、安装命令

```bash
# 前端
npm install react@^19 react-dom@^19
npm install react-router-dom@^7
npm install zustand@^5
npm install lucide-react

# 开发依赖
npm install -D typescript@^5.7
npm install -D vite@^6
npm install -D tailwindcss@^4
npm install -D @tailwindcss/vite
npm install -D @types/react @types/react-dom

# Tauri CLI
cargo install tauri-cli --version "^2.2"

# Rust 依赖（添加到 src-tauri/Cargo.toml）
cargo add tauri --features "tray-icon"
cargo add tauri-plugin-sql --features sqlite
cargo add tauri-plugin-fs
cargo add tauri-plugin-store
cargo add tauri-plugin-dialog
cargo add tauri-plugin-opener
cargo add tauri-plugin-shell

# 向量检索 + Embedding
cargo add fastembed --features "ort-download-binaries-native-tls"
cargo add usearch
cargo add rusqlite --features bundled
cargo add tantivy

# 基础工具
cargo add serde --features derive
cargo add serde_json
cargo add tokio --features full
cargo add uuid --features v4
cargo add anyhow
cargo add thiserror

# Dev
cargo install cargo-audit
```

---

## 三、备选方案

| 推荐 | 替代方案 | 何时选择替代 |
|------|---------|-------------|
| Tauri 2.x | Electron | 需要完整的 Chrome DevTools；团队只有 Node.js 经验且不愿学 Rust |
| `usearch` + `rusqlite` | ChromaDB (sidecar) | ChromaDB 推出 Rust-native 嵌入式模式后优先切换 |
| `bge-small-zh-v1.5` | `bge-base-zh-v1.5` | 对检索精度有更高要求且可接受 ~220MB 模型体积 |
| `bge-small-zh-v1.5` | `all-MiniLM-L6-v2` | 仅处理英文文档的纯英文场景（本项目不适用） |
| `bge-reranker-v2-m3` | Cohere Rerank API | 可接受网络依赖 + API 费用（违反本地优先原则） |
| `fastembed-rs` (ONNX) | Python `sentence-transformers` | 可接受捆绑 Python 运行时的 ~200MB 体积增加 |
| `tantivy` (BM25) | 自实现 BM25 | 原型阶段快速验证，但正式版必须用 tantivy |
| SQLite | PostgreSQL / MySQL | 需要多客户端并发写入（桌面应用不存在此需求） |

---

## 四、不要使用的技术及原因

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| **Electron** | 包体积 100-150MB（Tauri 的 10-50x）；内存 300-500MB；打包冗余 Chromium | Tauri 2.x |
| **`all-MiniLM-L6-v2`** | 中文性能严重不足（语料占比 <10%）；LCQMC 相似度仅 0.72 vs 专用模型 0.89；专业术语误判 | `bge-small-zh-v1.5` |
| **ChromaDB (Python 嵌入式)** | 需要 Python 运行时（~200MB）；子进程管理复杂；Rust 无原生嵌入式客户端 | `usearch` + `rusqlite` |
| **OpenAI Embedding API** | 需要网络 + API 费用；违反本地优先原则；每次 embedding 调用产生延迟 | 本地 `bge-small-zh-v1.5` |
| **Pinecone / Milvus / Weaviate** | 需要服务端部署；需 Docker/云服务；违反零服务器成本约束 | 本地 HNSW 索引 |
| **Redux / MobX** | 在桌面应用中过度设计；模板代码多；学习曲线陡 | Zustand |
| **Vue / Svelte / Leptos** | Tauri 社区 React 生态最完善；模板和教程最多；团队 React 经验 | React 19 |
| **Next.js (SSR)** | 桌面应用不需要 SSR；增加构建复杂度 | Vite SPA 模式 |
| **Python (sentence-transformers)** | 需要 Python 运行时；增加 200MB+ 依赖 | `fastembed-rs` (Rust) |
| **Docker** | 桌面应用不需要容器化；增加用户安装门槛 | 编译为原生二进制 |
| **MongoDB / PostgreSQL** | 需要独立服务进程；桌面应用过度设计 | SQLite |
| **Cohere / VoyageAI Embedding API** | 需要网络 + API 费用；数据外传风险 | 本地 embedding |
| **LangChain / LlamaIndex** | Python 生态，不适用于 Tauri Rust 后端；引入不必要抽象层 | 直接调用，精简架构 |
| **React 18（而非 19）** | React 19 已稳定（2024.12）；v18 缺少 `use()`、Server Components 预留接口、自动批处理等 | React 19 |
| **Rust Python bindings (PyO3)** | 引入 GIL 限制 + Python 运行时；破坏 Tauri 的纯 Rust 优势 | `fastembed-rs` (ONNX) |
| **Wails** | Tauri 2 移动端支持、更大社区、更多官方插件 | Tauri 2.x |

---

## 五、版本兼容性矩阵

| Package A | Compatible With | Notes |
|-----------|-----------------|-------|
| Tauri 2.x | React 18+ / 19 | Tauri 不限制前端框架版本 |
| Tauri 2.x | Vite 5 / 6 / 7 | 通过 `@tauri-apps/cli` 集成 |
| `fastembed` v5.13 | `ort` ^2.0.0-rc.11 | ONNX Runtime 后端 |
| `fastembed` v5.13 | `candle-core` ^0.10.2 | Candle 后端（可选） |
| `bge-small-zh-v1.5` (512-dim) | `usearch` 任意版本 | 向量维度需与模型一致，否则检索失败 |
| `tauri-plugin-sql` | SQLite 3.x | 自动通过 `bundled` feature 链接 |
| `rusqlite` ^0.32 | SQLite 3.45+ | `bundled` feature 自动编译 SQLite |
| `tantivy` ^0.22 | tokenizers ^0.22 | 中文需 xiaoxigua/jieba 分词器 |
| TailwindCSS v4 | `@tailwindcss/vite` | v4 集成方式，非 `postcss` 插件 |
| React 19 | TypeScript ^5.7 | TypeScript 5.7+ 提供 React 19 JSX 类型 |
| Node.js | ^20 LTS / ^22 | Tauri 前端构建需要 |

---

## 六、快速启动检查清单

- [ ] Windows 已安装 WebView2 Runtime（Win10 1809+/Win11 内置）
- [ ] Rust 工具链 `rustup` + `stable-x86_64-pc-windows-msvc`
- [ ] Microsoft Visual C++ Build Tools（用于编译 Rust 原生依赖）
- [ ] Node.js 22 LTS + npm
- [ ] 首次运行自动下载 `bge-small-zh-v1.5`（~48MB）
- [ ] `.gitignore` 添加 `src-tauri/target/` 和 `~/.kingdee-kb/`

---

## 七、数据来源

| Source | Topic | Confidence |
|--------|-------|------------|
| Context7 `/websites/v2_tauri_app` (2736 snippets) | Tauri 2.x 架构、IPC、插件生态 | HIGH |
| Context7 `/huggingface/sentence-transformers` (1784 snippets) | MiniLM 模型参数、BGE 系列对比 | HIGH |
| Context7 `/websites/cookbook_chromadb_dev` (400 snippets) | ChromaDB 嵌入式模式部署模式 | HIGH |
| Context7 `/reactjs/react.dev` (3032 snippets) | React 19 架构、Hooks 模式 | HIGH |
| HuggingFace `BAAI/bge-small-zh-v1.5` README | BGE 模型参数、C-MTEB benchmark | HIGH |
| 知乎/CSDN — "Tauri vs Electron 2026对比" | 性能基准、社区反馈 | MEDIUM |
| Exa — "Embedding Models Compared: What Actually Matters for RAG" (2026.05) | 嵌入模型跨域评测 | MEDIUM |
| crates.io `fastembed` v5.13.1 | Rust embedding 库版本、特性 | HIGH |
| crates.io `chromadb` v2.3.0 | ChromaDB Rust client 现状 | HIGH |
| GitHub `FlagOpen/FlagEmbedding` | BGE 系列中文 benchmark | HIGH |
| dev.to — "Building Embedding API with Rust, Axum, ONNX" | Rust ONNX 推理实践 | MEDIUM |
| GitHub `Anush008/fastembed-rs` | Rust 本地 embedding/reranking | HIGH |

---

*Stack research for: KingdeeKB — 本地 RAG 桌面知识管理工具*
*Researched: 2026-05-23*
*Next: 建议进行 Spike (技术验证) 验证 `usearch` + `bge-small-zh-v1.5` ONNX 推理的本地可行性*
