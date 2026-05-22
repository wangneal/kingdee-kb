# Architecture Research

**Domain:** 本地 RAG 桌面应用（知识管理）
**Researched:** 2026-05-23
**Confidence:** HIGH

## 一、系统总览

### 1.1 整体架构

```
┌──────────────────────────────────────────────────────────────────────────┐
│                            Tauri 2.x Desktop Shell                        │
│                                                                           │
│  ┌────────────────────────────────────┐  ┌────────────────────────────┐  │
│  │         React 前端 (WebView)        │  │       Rust 后端 (Native)   │  │
│  │                                    │  │                            │  │
│  │  ┌──────────────────────────────┐  │  │  ┌──────────────────────┐  │  │
│  │  │    UI 层（TailwindCSS）       │  │  │  │   Tauri Commands     │  │  │
│  │  │  ┌──────────┐ ┌───────────┐  │  │  │  │   (IPC 入口层)       │  │  │
│  │  │  │知识库浏览│ │ 检索问答  │  │  │  │  └──────────┬───────────┘  │  │
│  │  │  └──────────┘ └───────────┘  │  │  │             │              │  │
│  │  │  ┌──────────┐ ┌───────────┐  │  │  │  ┌──────────▼───────────┐  │  │
│  │  │  │ API 配置 │ │   设置    │  │  │  │  │   AppState           │  │  │
│  │  │  └──────────┘ └───────────┘  │  │  │  │   (Mutex/Arc 共享)   │  │  │
│  │  └──────────────────────────────┘  │  │  └──────────┬───────────┘  │  │
│  │                                    │  │             │              │  │
│  │  ┌──────────────────────────────┐  │  │  ┌──────────▼───────────┐  │  │
│  │  │  状态管理层                  │  │  │  │  服务层 (Services)   │  │  │
│  │  │  ┌────────┐ ┌─────────────┐  │  │  │  │  ┌─────────────────┐ │  │  │
│  │  │  │Zustand │ │TanStack     │  │  │  │  │  │ IngestionService │ │  │  │
│  │  │  │(UI状态)│ │Query(数据缓存)│  │  │  │  │  │(解析/分块/入库) │ │  │  │
│  │  │  └────────┘ └─────────────┘  │  │  │  │  └─────────────────┘ │  │  │
│  │  └──────────────────────────────┘  │  │  │  ┌─────────────────┐ │  │  │
│  │                                    │  │  │  │ SearchService    │ │  │  │
│  │  ┌──────────────────────────────┐  │  │  │  │(检索/融合/Rerank)│ │  │  │
│  │  │  IPC 桥接层                  │  │  │  │  └─────────────────┘ │  │  │
│  │  │  invoke() ←→ tauri::command  │  │  │  │  ┌─────────────────┐ │  │  │
│  │  └──────────────────────────────┘  │  │  │  │ EmbeddingService │ │  │  │
│  └────────────────────────────────────┘  │  │  │(ONNX模型/向量化)  │ │  │  │
│                                          │  │  └─────────────────┘ │  │  │
│         Tauri IPC (JSON-RPC)             │  │  ┌─────────────────┐ │  │  │
│                                          │  │  │ InferenceService │ │  │  │
│                                          │  │  │(LLM API 调用)    │ │  │  │
│                                          │  │  └─────────────────┘ │  │  │
│                                          │  └──────────────────────┘  │  │
│                                          │             │              │  │
│                                          │  ┌──────────▼───────────┐  │  │
│                                          │  │  数据层              │  │  │
│                                          │  │  ┌───────────────┐   │  │  │
│                                          │  │  │ ChromaDB       │   │  │  │
│                                          │  │  │(向量存储/HNSW) │   │  │  │
│                                          │  │  └───────────────┘   │  │  │
│                                          │  │  ┌───────────────┐   │  │  │
│                                          │  │  │ 文件系统       │   │  │  │
│                                          │  │  │(~/.kingdee-kb/)│   │  │  │
│                                          │  │  └───────────────┘   │  │  │
│                                          │  └──────────────────────┘  │  │
│                                          └────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────┘
```

### 1.2 层级职责

| 层级 | 位置 | 职责 | 技术 |
|------|------|------|------|
| **UI 层** | React (WebView) | 界面渲染、用户交互、乐观更新 | React 18 + TailwindCSS |
| **状态层** | React | UI 状态管理、API 缓存、错误状态 | Zustand + TanStack Query |
| **IPC 桥接** | Tauri | 前后端通信、命令路由、序列化 | `invoke()` + `tauri::command` |
| **服务层** | Rust | 核心业务逻辑：解析、分块、向量化、检索、LLM 调用 | Rust (Tokio async) |
| **数据层** | Rust | 向量存储、文件 I/O、本地持久化 | ChromaDB + 文件系统 |

---

## 二、Tauri 架构模式

### 2.1 Rust 后端与 React 前端分离

**核心原则：前端不直接操作数据，后端不处理 UI。**

```
┌──────────────────────────────────────┐
│          React 前端 (WebView)         │
│                                      │
│  UI 状态 (Zustand)                   │
│  → 侧边栏展开/折叠                    │
│  → 当前选中知识条目                   │
│  → 检索输入框文本                     │
│                                      │
│  数据缓存 (TanStack Query)            │
│  → 知识列表                           │
│  → 标签树                             │
│  → 检索结果                           │
│  → 配置信息                           │
│                                      │
│  不允许直接访问文件系统或数据库         │
└──────────────┬───────────────────────┘
               │ invoke("command_name", { args })
               ▼
┌──────────────────────────────────────┐
│          Rust 后端 (Native)           │
│                                      │
│  Tauri Commands (IPC 入口)           │
│  → add_knowledge()                   │
│  → search()                          │
│  → chat()                            │
│  → get_config() / set_config()       │
│                                      │
│  共享状态 (AppState)                  │
│  → EmbeddingModel (Arc<Mutex<>>)     │
│  → ChromaClient                      │
│  → AppConfig                         │
│                                      │
│  所有文件 I/O、数据库操作在此完成       │
└──────────────────────────────────────┘
```

### 2.2 IPC 通信模式

**Tauri 命令范式：**

```rust
// === Rust 后端：定义命令 ===

use tauri::State;
use std::sync::Arc;
use tokio::sync::Mutex;

struct AppState {
    embedding_service: Arc<EmbeddingService>,
    search_service: Arc<SearchService>,
    ingestion_service: Arc<IngestionService>,
    config: Arc<Mutex<AppConfig>>,
}

// 命令1：知识入库（异步，耗时操作）
#[tauri::command]
async fn add_knowledge(
    files: Vec<String>,
    state: State<'_, AppState>,
) -> Result<IngestionResult, AppError> {
    state.ingestion_service.ingest_files(&files).await
}

// 命令2：检索（异步）
#[tauri::command]
async fn search(
    query: String,
    top_k: usize,
    state: State<'_, AppState>,
) -> Result<Vec<SearchResult>, AppError> {
    state.search_service.hybrid_search(&query, top_k).await
}

// 命令3：AI 问答（异步，流式返回）
#[tauri::command]
async fn chat(
    query: String,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    // 流式返回需要借助 Tauri Event 系统
    state.inference_service.chat_stream(&query, window).await
}

// === 注册命令 ===
fn main() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            add_knowledge,
            search,
            chat,
            get_config,
            set_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

```typescript
// === React 前端：调用命令 ===

import { invoke } from '@tauri-apps/api/core';

// 调用入库命令
const result = await invoke<IngestionResult>('add_knowledge', {
    files: ['path/to/doc.md'],
});

// 调用检索命令
const results = await invoke<SearchResult[]>('search', {
    query: '期货点价怎么处理',
    topK: 5,
});
```

### 2.3 前端项目结构

```
src/                          # React 前端
├── components/               # UI 组件
│   ├── layout/               # 布局组件（侧边栏、主内容区、顶栏）
│   ├── knowledge/            # 知识管理组件（树形目录、内容预览、添加/删除）
│   ├── search/               # 检索组件（搜索框、结果列表、搜索高亮）
│   ├── chat/                 # 对话组件（消息流、输入框、来源标注）
│   └── settings/             # 设置组件（API 配置、数据路径）
├── hooks/                    # 自定义 Hooks
│   ├── useKnowledge.ts       # 知识库数据获取与缓存
│   ├── useSearch.ts          # 检索逻辑
│   ├── useChat.ts            # 对话逻辑（流式）
│   └── useFileWatcher.ts     # 文件监视 Hook
├── stores/                   # Zustand 状态
│   ├── uiStore.ts            # UI 状态（侧边栏、选中项、搜索文本）
│   └── appStore.ts           # 应用状态（配置、连接状态）
├── services/                 # IPC 封装
│   ├── tauriBridge.ts        # Tauri invoke 封装（类型安全）
│   └── api.ts                # 统一的 API 调用层
├── lib/                      # 工具函数
│   ├── queryKeys.ts          # TanStack Query 键管理
│   └── types.ts              # 共享类型定义
└── App.tsx                   # 应用入口
```

### 2.4 Rust 后端项目结构

```
src-tauri/src/
├── main.rs                   # Tauri 应用入口、插件注册
├── commands/                 # Tauri IPC 命令处理器
│   ├── mod.rs
│   ├── knowledge.rs          # 知识 CRUD 命令
│   ├── search.rs             # 检索命令
│   ├── chat.rs               # 对话命令（流式）
│   └── config.rs             # 配置命令
├── services/                 # 核心服务层
│   ├── mod.rs
│   ├── ingestion/            # 入库服务
│   │   ├── mod.rs
│   │   ├── parser.rs         # 文件解析（.md/.txt）
│   │   ├── cleaner.rs        # 文本清洗
│   │   ├── chunker.rs        # 递归分块
│   │   └── pipeline.rs       # 入库流水线
│   ├── embedding/            # 向量化服务
│   │   ├── mod.rs
│   │   ├── model.rs          # ONNX 模型加载与管理
│   │   ├── tokenizer.rs      # 分词封装
│   │   └── batch.rs          # 批量向量化
│   ├── search/               # 检索服务
│   │   ├── mod.rs
│   │   ├── vector.rs         # 向量检索
│   │   ├── bm25.rs           # BM25 关键词检索（含 jieba 分词）
│   │   ├── fusion.rs         # RRFR 融合
│   │   └── reranker.rs       # 重排序（可选）
│   └── inference/            # LLM 推理服务
│       ├── mod.rs
│       ├── openai.rs         # OpenAI API 客户端
│       ├── prompt.rs         # Prompt 构造
│       └── streaming.rs      # 流式响应处理
├── storage/                  # 存储层
│   ├── mod.rs
│   ├── chroma.rs             # ChromaDB 客户端封装
│   └── filesystem.rs         # 文件系统操作
├── config.rs                 # 配置管理
├── error.rs                  # 错误类型定义
└── state.rs                  # AppState 定义
```

---

## 三、RAG Pipeline 架构

### 3.1 两阶段流水线

RAG 系统有两条**严格分离**的流水线：

```
┌──────────────────────────────────────────────────────────────────────┐
│                        Ingestion Pipeline (入库)                      │
│                      （离线运行，触发于用户添加文件）                    │
│                                                                       │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐│
│  │  Parse   │→│  Clean   │→│  Chunk   │→│  Embed   │→│  Store  ││
│  │  文件解析│  │  文本清洗│  │  递归分块│  │  向量化  │  │ 持久存储││
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘  └─────────┘│
│       │              │              │              │            │     │
│  .md/.txt →    移除噪声→    H2→段落→句子    ONNX→384维   ChromaDB   │
│  提取文本      规范化空白    chunk+元数据   批量(batch=32)  + BM25索引│
└──────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────┐
│                         Search Pipeline (检索)                        │
│                      （在线运行，每次用户查询触发）                      │
│                                                                       │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐│
│  │  Embed   │→│ Retrieve │→│  RRF     │→│  Context │→│  LLM    ││
│  │  查询向量│  │  混合检索│  │  结果融合│  │  上下文  │  │  生成   ││
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘  └─────────┘│
│       │              │              │              │            │     │
│  同模型编码  向量top30+BM25top30  k=60融合  top5→Prompt  OpenAI API │
│                                      │              │    (流式返回)  │
│                                      └──→ 重排序(可选) ←──┘            │
└──────────────────────────────────────────────────────────────────────┘
```

### 3.2 Ingestion Pipeline 详解

#### Stage 1: Parse（解析）
- **输入**：用户添加的 .md / .txt 文件
- **处理**：
  - 识别文件类型，提取 UTF-8 文本
  - 提取文件名作为文档标题
  - 提取 Markdown 标题结构（H1/H2/H3）
- **输出**：`ParsedDocument { title, sections, metadata }`

#### Stage 2: Clean（清洗）
- **规则**（来自 SPEC.md）：
  1. 移除 Markdown 语法噪声（`###`、`` ` ``、` ``` ` 等）
  2. 规范化空白字符（多空格→1个，换行统一为 `\n`）
  3. 保留标题层级结构（H1/H2/H3 → 结构化标记）
  4. 保留代码块内容（不参与分块，但保留结构标记）
  5. 移除空行（连续3个以上空行压缩为2个）
- **输出**：`CleanedDocument { clean_text, structure_tree }`

#### Stage 3: Chunk（递归分块）
- **策略**（来自 SPEC.md）：
  1. 按 H2（`##`）标题分割为顶级块
  2. 每个顶级块内，按自然段落再分割
  3. 单段落超过 512 tokens 时，按句子递归拆分
  4. 保留每个 chunk 的父级标题路径
  5. 合并过小的块（<100 tokens 与相邻块合并）
  6. 标记 chunk 在原文档中的位置（行号范围）
- **参数**：
  - 目标 chunk size：256-512 tokens
  - 重叠大小：50 tokens
  - 最大 token 数：1024
- **输出**：`Vec<Chunk> { content, metadata }`
- **元数据**：
  ```rust
  struct ChunkMetadata {
      source_file: String,      // 原始文件名
      title: String,             // 文档标题
      section_path: String,     // 章节路径 "星达铜业/采购/期货点价"
      heading: String,          // 当前块所属标题
      line_start: u32,          // 起始行号
      line_end: u32,            // 结束行号
      tags: Vec<String>,        // 自动推断标签
      content_hash: String,     // SHA256 内容哈希（去重/增量更新）
      created_at: DateTime,
  }
  ```

#### Stage 4: Embed（向量化）
- **模型**：`all-MiniLM-L6-v2` (384维，~90MB，ONNX 格式)
- **处理**：批量向量化，每批 32 个 chunk
- **进度**：通过 Tauri Event 向前端发送进度事件
- **输出**：`Vec<f32>` (384维) 每个 chunk

#### Stage 5: Store（持久化存储）
- **ChromaDB**：
  - Collection: `kingdee_knowledge`
  - 维度: 384
  - 每条记录包含：id (UUID)、content、embedding (384维)、metadata
- **BM25 索引**：在内存中维护，重启时从 ChromaDB 重建

### 3.3 Search Pipeline 详解

#### Stage 1: Embed Query（查询向量化）
- 使用与入库相同的 embedding 模型
- 输入：用户查询文本
- 输出：384 维向量

#### Stage 2: Hybrid Retrieve（混合检索）
```
┌─────────────────────────────────────────────┐
│              Hybrid Search                   │
│                                              │
│  用户查询: "期货点价怎么处理"                   │
│       │                                      │
│       ├──→ Embedding → 向量检索(top30)        │
│       │       ↓                              │
│       │   ChromaDB 余弦相似度                  │
│       │                                      │
│       └──→ jieba 分词 → BM25 检索(top30)      │
│               ↓                              │
│          关键词频率匹配                        │
│                                              │
│       ─────────── RRF 融合 ───────────       │
│       score = Σ(1 / (rank + 60))            │
│                                              │
│              ↓ top5                          │
│         返回最终结果                           │
└─────────────────────────────────────────────┘
```

#### Stage 3: RRF Fusion（RRF 融合）
```rust
fn reciprocal_rank_fusion(
    vector_results: &[ScoredChunk],
    bm25_results: &[ScoredChunk],
    k: f64,  // k=60
) -> Vec<ScoredChunk> {
    // 为每个在任一结果集中出现的 chunk 计算 RRF 分数
    // RRF(chunk) = Σ(1 / (rank_in_list + k))
    // 按 RRF 分数降序排列
}
```

#### Stage 4: Context Assembly（上下文组装）
```rust
fn assemble_context(results: Vec<SearchResult>, max_tokens: u32) -> String {
    // 格式：[来源：文件名 | 章节路径]\n内容\n\n
    // 超出 max_tokens 则截断
    // 默认 max_tokens = 4096 (与 LLM 上下文窗口匹配)
}
```

#### Stage 5: LLM Generation（LLM 生成）
```rust
fn build_prompt(context: &str, query: &str) -> ChatRequest {
    ChatRequest {
        system: "你是金蝶ERP实施顾问知识助手。基于知识库内容回答，无相关内容时明确说明。标注来源。",
        messages: vec![
            Message {
                role: "user",
                content: format!(
                    "知识库相关内容：\n{}\n\n用户问题：{}\n\n请根据以上内容回答。",
                    context, query
                ),
            }
        ],
        model: config.model.clone(),
        stream: true,  // 流式返回
    }
}
```

### 3.4 增量更新（Incremental Update）

**关键设计：避免重复向量化。**

```rust
struct IngestionPipeline {
    // 哈希去重：只在内容变更时重新处理
    async fn ingest_file(&self, path: &Path) -> Result<IngestionResult> {
        let content_hash = sha256(&content);
        
        // 检查是否已存在且未变更
        if let Some(existing) = self.get_existing_chunks(path).await {
            if existing.content_hash == content_hash {
                return Ok(IngestionResult::Skipped);  // 跳过，内容未变
            }
            // 内容变更：删除旧 chunks，重新入库
            self.remove_chunks(path).await?;
        }
        
        // 处理新内容
        let chunks = self.parse_and_chunk(path).await?;
        self.embed_and_store(chunks).await?;
    }
}
```

---

## 四、ChromaDB 嵌入式集成

### 4.1 集成方式选择

| 方案 | 描述 | 优点 | 缺点 | 推荐度 |
|------|------|------|------|--------|
| **A: Rust HTTP Client + 本地 Server** | 使用 `chromadb` crate 连接本地 Chroma 服务 | 官方支持，稳定 | 需要额外启动服务进程 | ★★★★☆ |
| **B: 嵌入 Python ChromaDB** | Rust 通过子进程调 Python ChromaDB | 功能完整 | Python 运行时依赖，沉重 | ★★☆☆☆ |
| **C: 使用替代方案** | LanceDB (Rust 原生) 或 sqlite-vec | 真正的嵌入，无外部依赖 | 偏离 SPEC 要求 | ★★★☆☆ |

**推荐方案 A**：使用 `chromadb` crate（Rust 官方客户端）连接本地 ChromaDB 服务。

### 4.2 本地 ChromaDB 部署

```rust
// src-tauri/src/storage/chroma.rs

use chromadb::client::{ChromaClient, ChromaClientOptions};
use chromadb::collection::{ChromaCollection, CollectionEntries, GetOptions, GetResult};

pub struct ChromaStore {
    client: ChromaClient,
    collection: ChromaCollection,
}

impl ChromaStore {
    /// 初始化 ChromaDB 连接
    /// ChromaDB 服务需在应用启动时自动启动（通过 Tauri sidecar）
    pub async fn new(persist_path: &Path) -> Result<Self, AppError> {
        // 连接到本地 ChromaDB 服务 (默认 http://localhost:8000)
        let client = ChromaClient::new(ChromaClientOptions {
            url: Some("http://localhost:8000".into()),
            database: "kingdee_kb".into(),
            auth: None,  // 本地无需认证
        });

        // 获取或创建 collection
        let collection = client
            .get_or_create_collection("kingdee_knowledge", None)
            .await?;

        Ok(Self { client, collection })
    }

    /// 批量插入 chunks
    pub async fn upsert_chunks(&self, chunks: &[ChunkWithEmbedding]) -> Result<()> {
        let ids: Vec<String> = chunks.iter().map(|c| c.id.clone()).collect();
        let documents: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        let embeddings: Vec<Vec<f32>> = chunks.iter().map(|c| c.embedding.clone()).collect();
        let metadatas: Vec<serde_json::Value> = chunks.iter()
            .map(|c| serde_json::to_value(&c.metadata).unwrap())
            .collect();

        let entries = CollectionEntries {
            ids,
            documents: Some(documents),
            embeddings: Some(embeddings),
            metadatas: Some(metadatas),
        };

        self.collection.upsert(entries, None).await?;
        Ok(())
    }

    /// 向量检索
    pub async fn query(
        &self,
        embedding: &[f32],
        top_k: u32,
        filter: Option<serde_json::Value>,
    ) -> Result<Vec<SearchResult>> {
        let result = self.collection
            .query(
                vec![embedding.to_vec()],
                QueryOptions {
                    n_results: Some(top_k as usize),
                    where_filter: filter,
                    include: Some(vec!["documents", "metadatas", "distances"]),
                },
            )
            .await?;
        
        // 转换为 SearchResult
        // ...
    }

    /// 按文件删除 chunks
    pub async fn delete_by_source(&self, source_file: &str) -> Result<()> {
        self.collection
            .delete(DeleteOptions {
                where_filter: Some(serde_json::json!({
                    "source_file": source_file
                })),
                ..Default::default()
            })
            .await?;
        Ok(())
    }
}
```

### 4.3 ChromaDB Sidecar 管理

ChromaDB 服务需要作为 sidecar 进程由 Tauri 管理生命周期：

```rust
// src-tauri/src/storage/chroma_sidecar.rs

use std::process::{Child, Command};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ChromaSidecar {
    process: Arc<Mutex<Option<Child>>>,
    data_path: PathBuf,
}

impl ChromaSidecar {
    pub fn new(data_path: &Path) -> Self {
        Self {
            process: Arc::new(Mutex::new(None)),
            data_path: data_path.to_path_buf(),
        }
    }

    /// 启动 ChromaDB 服务
    pub async fn start(&self) -> Result<()> {
        let chroma_data = self.data_path.join("chroma");
        std::fs::create_dir_all(&chroma_data)?;

        let child = Command::new("chroma")
            .args(["run", "--path", chroma_data.to_str().unwrap()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        *self.process.lock().await = Some(child);
        Ok(())
    }

    /// 停止 ChromaDB 服务（应用退出时调用）
    pub async fn stop(&self) {
        if let Some(mut child) = self.process.lock().await.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
```

### 4.4 备选方案：sqlite-vec

如果 ChromaDB 部署复杂度成为问题，可考虑使用 `sqlite-vec`：
- 纯 SQLite 扩展，无需额外服务
- 零配置，与现有 SQLite 集成
- 缺点：无 HNSW 索引，仅暴力检索（适合 <100K 向量）

---

## 五、本地 Embedding 模型管理

### 5.1 模型加载架构

```
┌─────────────────────────────────────────────────────────────┐
│                   EmbeddingService                           │
│                                                              │
│  ┌───────────────────┐    ┌───────────────────┐             │
│  │   ModelManager     │    │   TokenizerPool    │            │
│  │                    │    │                    │            │
│  │ - 模型下载(HF Hub) │    │ - HuggingFace      │            │
│  │ - 版本管理         │    │   Tokenizers       │            │
│  │ - 缓存(~/.cache/)  │    │ - 批量编码         │            │
│  │ - 自动更新         │    │ - 截断/填充        │            │
│  └────────┬───────────┘    └────────┬───────────┘           │
│           │                         │                        │
│           └──────────┬──────────────┘                        │
│                      ▼                                       │
│           ┌───────────────────┐                              │
│           │   ONNX Runtime     │                              │
│           │   (ort crate)      │                              │
│           │                    │                              │
│           │ - CPU 执行提供者    │                             │
│           │ - GraphOpt Level3  │                             │
│           │ - 线程安全(Mutex)   │                             │
│           └──────────┬─────────┘                             │
│                      ▼                                       │
│           ┌───────────────────┐                              │
│           │   Mean Pooling    │                              │
│           │   + L2 Normalize  │                              │
│           └───────────────────┘                              │
└─────────────────────────────────────────────────────────────┘
```

### 5.2 Rust 实现

```rust
// src-tauri/src/services/embedding/model.rs

use ort::{Environment, Session, SessionBuilder, GraphOptimizationLevel};
use tokenizers::Tokenizer;
use std::sync::Arc;

pub struct EmbeddingModel {
    session: Session,
    tokenizer: Arc<Tokenizer>,
    dimension: usize,        // 384
    max_length: usize,       // 256
}

impl EmbeddingModel {
    /// 从本地目录加载模型（首次自动下载）
    pub fn new(model_dir: &Path) -> Result<Self, AppError> {
        let onnx_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        // 如果模型文件不存在，尝试从 HuggingFace 下载
        if !onnx_path.exists() {
            Self::download_model(model_dir)?;
        }

        // 初始化 ONNX Runtime
        let environment = Environment::builder()
            .with_name("kingdee-embedding")
            .build()?
            .into_arc();

        // 加载 ONNX 模型
        let session = SessionBuilder::new(&environment)?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?  // 与 CPU 核心数匹配
            .with_model_from_file(onnx_path)?;

        // 加载分词器
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| AppError::ModelLoad(format!("分词器加载失败: {}", e)))?;

        Ok(Self {
            session,
            tokenizer: Arc::new(tokenizer),
            dimension: 384,
            max_length: 256,
        })
    }

    /// 从 HuggingFace 下载模型
    fn download_model(target_dir: &Path) -> Result<(), AppError> {
        // 使用 hf-hub crate 或 reqwest 从 HuggingFace 下载
        // 模型: sentence-transformers/all-MiniLM-L6-v2
        // 文件: model.onnx, tokenizer.json
        // 缓存路径: ~/.cache/huggingface/hub/
        // 提供下载进度回调
        unimplemented!()
    }

    /// 批量向量化
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, AppError> {
        let mut embeddings = Vec::with_capacity(texts.len());

        for text in texts {
            // 1. 分词
            let encoding = self.tokenizer.encode(*text, true)
                .map_err(|e| AppError::Embedding(format!("分词失败: {}", e)))?;

            // 2. 构建输入张量
            let input_ids: Vec<i64> = encoding.get_ids().iter()
                .map(|&id| id as i64).collect();
            let attention_mask: Vec<i64> = encoding.get_attention_mask().iter()
                .map(|&m| m as i64).collect();

            // 3. ONNX 推理
            let outputs = self.session.run(vec![
                ort::Value::from_array(input_ids.as_slice())?,
                ort::Value::from_array(attention_mask.as_slice())?,
            ])?;

            // 4. Mean Pooling + L2 Normalize
            let embedding = Self::mean_pooling_and_normalize(outputs, &attention_mask)?;
            embeddings.push(embedding);
        }

        Ok(embeddings)
    }

    /// Mean Pooling + L2 归一化
    fn mean_pooling_and_normalize(
        outputs: Vec<ort::Value>,
        attention_mask: &[i64],
    ) -> Result<Vec<f32>, AppError> {
        // 从最后一层 hidden states 取 mean pooling
        // 对 attention_mask 做加权平均
        // 然后 L2 归一化
        unimplemented!()
    }
}
```

### 5.3 模型缓存策略

```
~/.cache/huggingface/hub/
└── models--sentence-transformers--all-MiniLM-L6-v2/
    ├── blobs/
    │   ├── <sha256-hash1>  # model.onnx  (~90MB)
    │   └── <sha256-hash2>  # tokenizer.json
    ├── refs/
    │   └── main
    └── snapshots/
        └── <commit-hash>/
            ├── model.onnx -> ../../blobs/<sha256-hash1>
            └── tokenizer.json -> ../../blobs/<sha256-hash2>
```

**下载流程**：
1. 应用首次启动，检查 `~/.cache/huggingface/hub/` 是否已有模型
2. 如无，显示下载进度弹窗（通过 Tauri Event → 前端进度条）
3. 下载完成后持久化到缓存目录
4. 后续启动直接加载缓存

**关键 crate**：
- `ort` — ONNX Runtime Rust 绑定
- `tokenizers` — HuggingFace Tokenizers Rust 实现
- `hf-hub` — HuggingFace Hub 下载工具
- `reqwest` — HTTP 下载（含进度回调）

### 5.4 中文本地 Embedding 模型对比

| 模型 | 维度 | 大小 | 中文表现 | 推荐 |
|------|------|------|----------|------|
| `all-MiniLM-L6-v2` | 384 | ~90MB | 一般（英文优化） | **SPEC 默认** |
| `bge-small-zh-v1.5` | 512 | ~100MB | 优秀 | ★★★★☆ (未来升级候选) |
| `bge-large-zh-v1.5` | 1024 | ~1.3GB | SOTA | ★★★☆☆ (太重) |
| `bce-embedding-base_v1` | 768 | ~1.1GB | SOTA (中英双语) | ★★★☆☆ (太重) |
| `multilingual-e5-small` | 384 | ~120MB | 良好 | ★★★☆☆ |

**阶段建议**：
- **v0.1 (MVP)**：继续使用 `all-MiniLM-L6-v2` (384维)，满足基本需求
- **v0.2+**：评估升级到 `bge-small-zh-v1.5` (512维)，大幅提升中文检索质量
  - 注意：维度变更需要重建整个向量库，需设计迁移方案

---

## 六、文件系统监视

### 6.1 Tauri Plugin-FS 监视 API

```typescript
// src/hooks/useFileWatcher.ts

import { watch, BaseDirectory } from '@tauri-apps/plugin-fs';
import { useEffect } from 'react';

export function useFileWatcher(
    watchPath: string,
    onFileChange: (event: WatchEvent) => void,
) {
    useEffect(() => {
        let unlisten: (() => void) | undefined;

        async function startWatching() {
            // watchImmediate：即时通知（无防抖），递归监视子目录
            unlisten = await watch(
                watchPath,
                (event) => {
                    // event: { type: 'create'|'update'|'remove', paths: string[] }
                    // 只处理 .md/.txt 文件
                    const relevant = event.paths.filter(p =>
                        p.endsWith('.md') || p.endsWith('.txt')
                    );
                    if (relevant.length > 0) {
                        onFileChange({
                            type: event.type,
                            paths: relevant,
                        });
                    }
                },
                {
                    baseDir: BaseDirectory.AppData,
                    delayMs: 1000,    // 1秒防抖（防止编辑器快速保存触发多次）
                    recursive: true,   // 递归监视
                }
            );
        }

        startWatching();

        return () => {
            if (unlisten) unlisten();
        };
    }, [watchPath]);
}
```

### 6.2 自动入库逻辑

```typescript
// 监视知识库目录变化，自动触发入库

import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useFileWatcher } from './useFileWatcher';
import { knowledgeKeys } from '../lib/queryKeys';

function useAutoIngestion(kbPath: string) {
    const queryClient = useQueryClient();

    const ingestMutation = useMutation({
        mutationFn: (files: string[]) => invoke('add_knowledge', { files }),
        onSuccess: () => {
            // 入库完成后刷新知识列表
            queryClient.invalidateQueries({ queryKey: knowledgeKeys.all });
        },
    });

    useFileWatcher(kbPath, (event) => {
        if (event.type === 'create') {
            // 新文件 → 自动入库
            ingestMutation.mutate(event.paths);
        } else if (event.type === 'remove') {
            // 删除文件 → 自动从向量库移除
            invoke('remove_knowledge', { files: event.paths });
        }
        // 'update' 类型 → 暂不自动处理（避免锁冲突，改为手动更新）
    });
}
```

### 6.3 注意事项

1. **防抖**：编辑器的 "保存" 操作可能触发多次文件变更事件，需要防抖（推荐 1-2 秒）
2. **递归监视**：设置为 `recursive: true` 以监视子目录
3. **文件锁**：Windows 上某些编辑器会锁文件，入库时需要处理文件被占用的情况
4. **大文件**：超过 10MB 的文件应考虑提示用户或跳过
5. **启动扫描**：应用启动时应扫描目录，对比已有记录，发现新增文件

---

## 七、React 状态管理

### 7.1 三层状态架构

```
┌──────────────────────────────────────────────────────────────────┐
│                       状态管理三层架构                             │
│                                                                   │
│  ┌─────────────────────┐                                         │
│  │  TanStack Query      │  ← 服务端数据（来自 Rust 后端）          │
│  │  - 知识列表           │    自动缓存、去重、后台刷新、乐观更新      │
│  │  - 标签树             │    queryKey: ['knowledge', 'list']     │
│  │  - 检索结果           │    staleTime: 5分钟                    │
│  │  - 配置信息           │                                         │
│  └─────────────────────┘                                         │
│           │                                                       │
│  ┌────────▼────────────┐                                         │
│  │  Zustand             │  ← 客户端状态（不与服务端同步）           │
│  │  - 侧边栏展开/折叠    │    persist 到 localStorage              │
│  │  - 当前选中条目       │    （应用重启后恢复状态）                 │
│  │  - 搜索框文本        │                                         │
│  │  - 深色/浅色主题     │                                         │
│  └─────────────────────┘                                         │
│           │                                                       │
│  ┌────────▼────────────┐                                         │
│  │  useState            │  ← 组件级临时状态                        │
│  │  - 模态框开关        │    仅在单个组件内使用                    │
│  │  - 输入框草稿       │    不跨组件共享                          │
│  │  - hover 状态        │                                         │
│  └─────────────────────┘                                         │
└──────────────────────────────────────────────────────────────────┘
```

### 7.2 Zustand Store 设计

```typescript
// src/stores/uiStore.ts

import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface UIState {
    // 侧边栏
    sidebarExpanded: boolean;
    toggleSidebar: () => void;

    // 浏览状态
    selectedKnowledgeId: string | null;
    selectKnowledge: (id: string | null) => void;

    // 搜索
    searchQuery: string;
    setSearchQuery: (query: string) => void;

    // 对话
    isChatOpen: boolean;
    openChat: () => void;
    closeChat: () => void;
}

export const useUIStore = create<UIState>()(
    persist(
        (set) => ({
            sidebarExpanded: true,
            toggleSidebar: () =>
                set((s) => ({ sidebarExpanded: !s.sidebarExpanded })),

            selectedKnowledgeId: null,
            selectKnowledge: (id) => set({ selectedKnowledgeId: id }),

            searchQuery: '',
            setSearchQuery: (query) => set({ searchQuery: query }),

            isChatOpen: false,
            openChat: () => set({ isChatOpen: true }),
            closeChat: () => set({ isChatOpen: false }),
        }),
        {
            name: 'kingdee-ui-store',  // localStorage key
            partialize: (state) => ({
                sidebarExpanded: state.sidebarExpanded,  // 仅持久化侧边栏
            }),
        }
    )
);
```

### 7.3 TanStack Query 设计

```typescript
// src/lib/queryKeys.ts

export const knowledgeKeys = {
    all: ['knowledge'] as const,
    lists: () => [...knowledgeKeys.all, 'list'] as const,
    list: (filters: KnowledgeFilters) => [...knowledgeKeys.lists(), filters] as const,
    details: () => [...knowledgeKeys.all, 'detail'] as const,
    detail: (id: string) => [...knowledgeKeys.details(), id] as const,
};

export const searchKeys = {
    all: ['search'] as const,
    results: (query: string, filters: SearchFilters) =>
        ['search', query, filters] as const,
};

// src/hooks/useKnowledge.ts

import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';

export function useKnowledgeList(filters: KnowledgeFilters) {
    return useQuery({
        queryKey: knowledgeKeys.list(filters),
        queryFn: () => invoke<KnowledgeItem[]>('list_knowledge', { filters }),
        staleTime: 5 * 60 * 1000,  // 5分钟不过期
    });
}

export function useDeleteKnowledge() {
    const queryClient = useQueryClient();

    return useMutation({
        mutationFn: (id: string) => invoke('delete_knowledge', { id }),
        onSuccess: () => {
            // 删除成功后刷新列表
            queryClient.invalidateQueries({ queryKey: knowledgeKeys.lists() });
        },
    });
}
```

### 7.4 状态管理黄金法则

1. **服务端数据 → TanStack Query**（绝不放在 Zustand）
2. **全局 UI 状态 → Zustand**（侧边栏、主题、选中项）
3. **组件局部状态 → useState**（模态框、输入框）
4. **派生状态 → useMemo**（从源状态派生，不重复存储）
5. **不重复状态**：TanStack Query 已有数据，不要再存一份到 Zustand

---

## 八、错误处理与恢复模式

### 8.1 Rust 错误类型设计

```rust
// src-tauri/src/error.rs

use thiserror::Error;
use serde::Serialize;

/// 应用统一错误类型
/// 使用 tagged enum 模式，前端可以匹配 kind 字段
#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    #[error("IO 错误: {0}")]
    Io(String),

    #[error("数据库错误: {0}")]
    Database(String),

    #[error("Embedding 模型错误: {0}")]
    Embedding(String),

    #[error("LLM API 错误: {0}")]
    LLMApi(String),

    #[error("配置错误: {0}")]
    Config(String),

    #[error("解析错误: {0}")]
    Parse(String),

    #[error("网络错误: {0}")]
    Network(String),

    #[error("未找到: {0}")]
    NotFound(String),
}

// From 转换实现（减少样板代码）
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            AppError::Network("请求超时".into())
        } else if e.is_connect() {
            AppError::Network("网络连接失败".into())
        } else {
            AppError::Network(e.to_string())
        }
    }
}
```

### 8.2 前端错误匹配

```typescript
// src/services/tauriBridge.ts

import { invoke } from '@tauri-apps/api/core';

interface AppError {
    kind: 'Io' | 'Database' | 'Embedding' | 'LLMApi' | 'Config' | 'Parse' | 'Network' | 'NotFound';
    message: string;
}

async function safeInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    try {
        return await invoke<T>(command, args);
    } catch (e: unknown) {
        const error = e as AppError;

        switch (error.kind) {
            case 'Network':
                // 网络错误 → 显示离线模式提示
                showToast('网络连接失败，请检查网络', 'error');
                break;
            case 'LLMApi':
                // API 错误 → 提示检查 API Key
                showToast('AI 服务调用失败，请检查 API 配置', 'error');
                break;
            case 'Embedding':
                // 模型错误 → 提示重新下载
                showToast('Embedding 模型异常，请尝试重新启动', 'error');
                break;
            case 'NotFound':
                // 资源不存在 → 优雅提示
                showToast(error.message, 'warning');
                break;
            default:
                // 通用错误
                showToast(`操作失败: ${error.message}`, 'error');
        }

        throw error;  // 继续向上抛出，让 TanStack Query 处理
    }
}
```

### 8.3 优雅降级（Graceful Degradation）

```typescript
// 示例：LLM 不可用时的降级

function useChatQuery(query: string, enabled: boolean) {
    return useQuery({
        queryKey: ['chat', query],
        queryFn: () => safeInvoke<ChatResponse>('chat', { query }),
        enabled,
        retry: 1,  // 仅重试1次
        // 降级处理
        placeholderData: (previousData) => previousData,  // 保持上次数据
        onError: (error: AppError) => {
            if (error.kind === 'LLMApi') {
                // 降级：仅显示检索结果，不生成 AI 回答
                return {
                    answer: null,
                    sources: [],  // 仍然显示检索到的来源
                    fallback: true,
                    message: 'AI 服务暂时不可用，仅显示检索结果',
                };
            }
        },
    });
}
```

### 8.4 重试策略

```rust
// src-tauri/src/services/inference/retry.rs

use std::time::Duration;
use tokio::time::sleep;

/// 带指数退避的重试逻辑
pub async fn with_retry<T, F, Fut>(
    max_attempts: u32,
    operation: F,
) -> Result<T, AppError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, AppError>>,
{
    let mut attempt = 0;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempt += 1;
                if attempt >= max_attempts {
                    log::error!("操作失败，已重试 {} 次: {:?}", max_attempts, e);
                    return Err(e);
                }
                log::warn!("操作失败，第 {} 次重试: {:?}", attempt, e);
                // 指数退避: 1s, 2s, 4s...
                sleep(Duration::from_millis(1000 * 2u64.pow(attempt - 1))).await;
            }
        }
    }
}
```

### 8.5 应用崩溃恢复

```
┌──────────────────────────────────────────────────────────────┐
│                    崩溃恢复策略                                │
│                                                               │
│  1. 启动时检查                                                │
│     - ChromaDB 数据完整性校验                                  │
│     - config.json 格式校验                                     │
│     - 知识库目录是否存在                                       │
│                                                               │
│  2. 操作前快照                                                │
│     - 入库前记录操作日志                                       │
│     - 记录处理的文件列表和 chunks 数量                          │
│                                                               │
│  3. 崩溃后恢复                                                │
│     - 根据操作日志回滚未完成的操作                              │
│     - 重建 BM25 索引（从 ChromaDB 重新读取）                    │
│     - 向用户显示恢复提示                                       │
│                                                               │
│  4. 优雅退出                                                  │
│     - Tauri 的 RunEvent::ExitRequested 钩子                   │
│     - 停止 ChromaDB sidecar                                   │
│     - 保存未提交的配置变更                                     │
│     - 写入操作日志                                             │
└──────────────────────────────────────────────────────────────┘
```

---

## 九、中文文本处理

### 9.1 中文分词对 BM25 的影响

**关键问题**：BM25 默认使用空格分词，中文文本无空格分隔，直接使用会导致 BM25 检索**完全失效**。

```
❌ 错误方案：直接空格分词
  "期货点价标准产品支持吗" → ["期货点价标准产品支持吗"] (整个是一个 token)
  → BM25 无法匹配任何有意义的关键词

✅ 正确方案：jieba 分词后再建索引
  "期货点价标准产品支持吗" → ["期货", "点价", "标准", "产品", "支持", "吗"]
  → BM25 可以匹配 "期货"、"点价" 等关键词
```

### 9.2 BM25 中文分词实现

```rust
// src-tauri/src/services/search/bm25.rs

use jieba_rs::Jieba;  // jieba Rust 绑定
use std::sync::OnceLock;

static JIEBA: OnceLock<Jieba> = OnceLock::new();

fn get_jieba() -> &'static Jieba {
    JIEBA.get_or_init(|| Jieba::new())
}

/// 中文 BM25 分词
fn tokenize_chinese(text: &str) -> Vec<String> {
    let jieba = get_jieba();
    // 使用搜索引擎模式（更细粒度，适合检索）
    jieba.cut_for_search(text, true)
        .iter()
        .filter(|w| w.len() > 1)           // 过滤单字
        .map(|w| w.to_string())
        .collect()
}

/// 构建 BM25 索引
pub struct BM25Index {
    // 使用 tantivy 或手写 BM25
    // 分词器 → jieba
    documents: Vec<Vec<String>>,  // 分词后的文档
    // BM25 参数
    k1: f64,  // 1.5
    b: f64,   // 0.75
    avg_dl: f64,
}

impl BM25Index {
    pub fn new(chunks: &[Chunk]) -> Self {
        let documents: Vec<Vec<String>> = chunks
            .iter()
            .map(|c| tokenize_chinese(&c.content))
            .collect();
        // 计算平均文档长度
        let avg_dl = documents.iter().map(|d| d.len() as f64).sum::<f64>()
            / documents.len() as f64;

        Self { documents, k1: 1.5, b: 0.75, avg_dl }
    }

    pub fn search(&self, query: &str, top_k: usize) -> Vec<(usize, f64)> {
        let query_terms = tokenize_chinese(query);
        // BM25 公式计算...
        // 返回 (doc_index, score) 排序
        unimplemented!()
    }
}
```

### 9.3 Jieba 分词模式选择

| 模式 | 说明 | 适用场景 | 示例 |
|------|------|----------|------|
| 精确模式 (`cut`) | 最精确切分，无冗余 | 文本分析 | "期货点价" → ["期货", "点价"] |
| 全模式 (`cut_all`) | 所有可能词都输出 | 不适用 | "期货点价" → ["期货", "点价", "货点"...] |
| 搜索引擎模式 (`cut_for_search`) | 精确模式 + 长词再切分 | **BM25 索引** | "期货点价" → ["期货", "点价", "期货点价"] |

**推荐**：BM25 检索使用 `cut_for_search` 搜索引擎模式，提高召回率。

### 9.4 Embedding 模型的中文局限性

`all-MiniLM-L6-v2` 是英文优化的模型，对中文有一定支持但**不是最优**：

- 中文语义理解较弱：同义词、成语、行业术语匹配精度下降
- 分词粒度不同：英文按空格，中文无空格，模型内部 tokenizer 可能切分不合理
- 中文长文本效果差：模型最大输入 256 tokens，对中文长段落可能截断

**缓解措施**（MVP 阶段不需完美解决）：
1. 混合检索（向量+BM25）可部分弥补：BM25 对中文专有名词匹配更好
2. 对中文 chunk 做前缀增强：`"标题: 期货点价\n内容: {content}"`
3. 未来版本切换到 `bge-small-zh-v1.5`

### 9.5 中文处理关键检查点

| 检查点 | 说明 | 优先级 |
|--------|------|--------|
| BM25 分词 | 必须使用 jieba 分词，否则 BM25 失效 | **P0 (阻塞)** |
| Chunk 边界 | 中文句子以 `。！？` 为边界，不是 `.` | **P1 (重要)** |
| Token 计数 | 中文 1 字符 ≈ 1-2 tokens，不要硬编码英文比例 | P2 |
| 向量检索 | 英文模型对中文语义检索精度下降，依赖混合检索弥补 | P2 |
| 文件名处理 | 支持中文文件名和路径 | P2 |

---

## 十、推荐构建顺序

### 10.1 组件依赖图

```
                    ┌─────────────────┐
                    │   错误类型定义   │ (贯穿所有)
                    │   error.rs      │
                    └────────┬────────┘
                             │
                    ┌────────▼────────┐
                    │  配置管理        │
                    │  config.rs      │
                    └────────┬────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
     ┌────────▼────────┐ ┌──▼──────┐ ┌─────▼──────┐
     │ 文件系统操作     │ │ Embedding│ │ ChromaDB   │
     │ filesystem.rs   │ │ Service  │ │ Sidecar    │
     └────────┬────────┘ └──┬──────┘ └─────┬──────┘
              │              │              │
     ┌────────▼────────┐    │              │
     │ 文件解析+清洗    │    │              │
     │ parser/cleaner  │    │              │
     └────────┬────────┘    │              │
              │              │              │
     ┌────────▼────────┐    │              │
     │ 递归分块        │    │              │
     │ chunker.rs      │    │              │
     └────────┬────────┘    │              │
              │              │              │
              └──────────────┼──────────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
     ┌────────▼────────┐    │              │
     │ IngestionService │    │              │
     │ (入库流水线)     │    │              │
     └────────┬────────┘    │              │
              │              │              │
              └──────────────┼──────────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
     ┌────────▼────────┐ ┌──▼──────────┐ ┌─▼──────────┐
     │ BM25 中文索引    │ │向量检索     │ │ LLM 客户端  │
     │ bm25.rs         │ │ vector.rs   │ │ openai.rs   │
     └────────┬────────┘ └──┬──────────┘ └─┬──────────┘
              │              │              │
     ┌────────▼────────┐    │              │
     │ 混合检索+RRF    │    │              │
     │ fusion.rs       │    │              │
     └────────┬────────┘    │              │
              │              │              │
              └──────────────┼──────────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
     ┌────────▼──────────┐   │  ┌──────────▼───────────┐
     │ SearchService     │   │  │ InferenceService     │
     │ (检索流水线)      │   │  │ (LLM 对话)           │
     └────────┬──────────┘   │  └──────────┬───────────┘
              │              │              │
              └──────────────┼──────────────┘
                             │
              ┌──────────────┴──────────────┐
              │       Tauri Commands        │
              │       commands/*.rs         │
              └──────────────┬─────────────┘
                             │
              ┌──────────────┴──────────────┐
              │       React Frontend        │
              └─────────────────────────────┘
```

### 10.2 分阶段构建顺序

| 阶段 | 内容 | 依赖 | 验证方式 |
|------|------|------|----------|
| **Phase 0: 基础设施** | Tauri 2 项目初始化、React 18 + TailwindCSS、Zustand + TanStack Query 设置、错误类型定义、配置管理 | 无 | `tauri dev` 启动成功、前端渲染 |
| **Phase 1: 数据层** | ChromaDB Sidecar 启动/停止、文件系统操作、`~/.kingdee-kb/` 目录结构 | Phase 0 | ChromaDB 服务启动、Collection 创建 |
| **Phase 2: Embedding 引擎** | ONNX 模型下载/加载、分词器、批量向量化 | Phase 0 | 单文本 → 384维向量，批量向量化正确 |
| **Phase 3: 入库流水线** | 文件解析(.md/.txt)、文本清洗、递归分块、入库 ChromaDB | Phase 1 + 2 | 拖入 .md 文件 → 分块 → 向量化 → ChromaDB 存储 |
| **Phase 4: BM25 中文检索** | jieba 分词集成、BM25 索引构建、关键词检索 | Phase 3 | 中文查询 → BM25 返回相关文档 |
| **Phase 5: 混合检索引擎** | 向量检索 + BM25 融合(RRF)、搜索结果组装 | Phase 1 + 2 + 4 | 查询 → 混合结果（语义+关键词） |
| **Phase 6: LLM 对话** | OpenAI API 客户端、流式响应(Tauri Event)、上下文组装 | Phase 5 | 问答 → 基于检索上下文生成回答 |
| **Phase 7: 前端知识管理 UI** | 知识添加(粘贴/拖拽)、树形目录、内容预览、删除 | Phase 3 | 完整的知识 CRUD 交互 |
| **Phase 8: 前端检索问答 UI** | 搜索框、检索结果展示、对话界面、流式渲染 | Phase 5 + 6 | 查询 → 结果展示 → AI 问答 |
| **Phase 9: 前端设置 UI** | API Key 配置、Embedding 下载进度、数据路径 | Phase 0 | 配置持久化、下载进度条 |
| **Phase 10: 文件监视+自动入库** | Tauri Plugin-FS watch、文件变更检测、自动入库 | Phase 3 + 7 | 拖入文件 → 自动入库 → UI 刷新 |
| **Phase 11: 错误处理+恢复** | 崩溃恢复、操作日志、优雅降级、重试逻辑 | Phase 4-8 | 模拟网络断开 → 降级提示 |

### 10.3 关键建议

1. **Phase 0-3 必须在任何 UI 工作之前完成**：核心数据流（文件 → 分块 → 向量化 → 存储）是应用的基础
2. **Phase 4 (BM25) 可在 Phase 5 之前并行开发**：BM25 是独立于向量的检索方式
3. **Phase 6 (LLM) 依赖 Phase 5**（检索结果作为上下文），但可在 Phase 5 完成前并行开发 OpenAI 客户端连通性
4. **Phase 7-9 (前端 UI) 可在 Rust 后端开发期间并行进行**：使用 Mock 数据 + Tauri WebView 独立开发

---

## 十一、架构反模式

### 反模式 1：前端直接操作 ChromaDB

**错误**：将 ChromaDB 客户端放在 React 端，通过 JavaScript 直接操作。

**为什么错**：
- ChromaDB 的 JS 客户端需要 Node.js 环境，Tauri WebView 中没有
- 破坏了安全边界（允许 WebView 操作数据库）

**正确做法**：所有 ChromaDB 操作通过 Rust 后端的 Tauri Commands 进行。

### 反模式 2：将服务器数据存储在 Zustand

**错误**：`useQuery` 获取数据后，手动 `setState` 到 Zustand store。

**为什么错**：
- 手动管理缓存、去重、失效，极易出错
- TanStack Query 已经帮你做了这一切

**正确做法**：服务端数据只存在于 TanStack Query 缓存中，前端通过 `useQuery` / `useMutation` 访问。

### 反模式 3：入库和检索共享状态导致阻塞

**错误**：入库时锁住整个 AppState，检索请求被阻塞。

**为什么错**：入库是耗时操作，用户期望检索即时响应。

**正确做法**：
- 入库使用后台任务队列（`tokio::spawn`），不阻塞主线程
- 共享状态使用 `RwLock` 读多写少场景，检索只需读锁
- 入库时仍可进行检索（使用当前已入库的数据）

### 反模式 4：忽略中文本地化

**错误**：BM25 直接用空格分词处理中文。

**为什么错**：中文无空格分隔，会导致 BM25 完全失效，混合检索退化到纯向量检索。

**正确做法**：BM25 索引阶段使用 jieba 分词（搜索引擎模式）。

---

## 十二、扩展性考量

| 规模 | 数据量 | 架构关注点 |
|------|--------|------------|
| 初期 (0-1K 文档) | <10K chunks | 单线程足够，无需优化 |
| 中期 (1K-10K 文档) | 10K-100K chunks | BM25 索引需考虑内存占用；ChromaDB 保持默认 HNSW |
| 后期 (10K+ 文档) | 100K+ chunks | 考虑 sqlite-vec 替代 ChromaDB（零依赖）；增加向量检索缓存；批量入库分批处理 |
| 超大 (>1M chunks) | >1M | 考虑升级到 LanceDB（磁盘优先架构）；Embedding 模型考虑 GPU 加速 |

### 扩展优先级

1. **首个瓶颈**：ChromaDB 内存占用（当 chunks > 50K 时 HNSW 索引增长）
   - 解决：调整 HNSW 参数（降低 M 值）或切换到 sqlite-vec
2. **第二个瓶颈**：Embedding 模型吞吐（大量文件同时入库时）
   - 解决：增加批处理大小、使用 Rayon 并行

---

## 十三、集成点

### 外部服务

| 服务 | 集成方式 | 注意事项 |
|------|----------|----------|
| OpenAI API | HTTPS 请求 (`reqwest` crate) | 用户自填 API Key；需处理网络超时；支持流式 SSE |
| ChromaDB | 本地 HTTP (`chromadb` crate) | 需管理 ChromaDB 进程生命周期；使用 `localhost:8000` |
| HuggingFace Hub | HTTPS 下载 | 仅首次下载模型；缓存到 `~/.cache/huggingface/`；需显示下载进度 |
| 文件系统 | Tauri Plugin-FS | 监视 `~/.kingdee-kb/knowledge/` 目录；递归监视 |

### 内部边界

| 边界 | 通信方式 | 注意事项 |
|------|----------|----------|
| React ↔ Rust | Tauri `invoke()` / Event | 命令同步返回，流式数据用 Event 推送 |
| IngestionService ↔ ChromaDB | 直接调用 (HTTP) | 入库时使用连接池避免创建过多连接 |
| SearchService ↔ BM25Index | 内存访问 (Arc) | BM25 索引存于内存，重启时从 ChromaDB 重建 |
| EmbeddingService ↔ ONNX | 直接调用 (ort) | 单线程推理（Mutex 保护），批量请求 |

---

## 十四、源码

### 官方文档
- [Tauri 2.x Documentation](https://v2.tauri.app/) — HIGH
- [ChromaDB Cookbook](https://cookbook.chromadb.dev/) — HIGH
- [ORT (ONNX Runtime) Rust Bindings](https://docs.rs/ort/latest/ort/) — HIGH
- [tokenizers crate](https://docs.rs/tokenizers/latest/tokenizers/) — HIGH

### 参考项目
- [shodhRAG](https://github.com/varun29ankuS/shodhRAG) — Rust + Tauri 本地 RAG，LanceDB + Tantivy — HIGH
- [Gloss](https://github.com/RecursiveIntell/Gloss) — Tauri 2 + Rust 知识管理，fastembed + usearch — HIGH
- [memory-prosthetic](https://github.com/GaoZimeng0425/memory-prosthetic) — Tauri 2 + React 记忆辅助工具，all-MiniLM-L6-v2 — HIGH
- [smart-locale-search](https://github.com/dan99nik/smart-locale-search) — Tauri 2 桌面搜索，ONNX embedding — HIGH

### 研究文章
- RAG Architecture Guide (2026) by Niraj Kumar — HIGH
- Local AI with Tauri + Postgres + pgvector (Electric, 2024) — MEDIUM
- Rust Error Handling in Tauri Commands — HIGH
- Chinese Tokenization Survey (2026) — HIGH
- RAG Series: Embedding Models for Chinese (2026) — HIGH

---

*Architecture research for: KingdeeKB (本地 RAG 知识管理桌面应用)*
*Researched: 2026-05-23*
*Confidence: HIGH — 基于多个已发布的开源项目验证 + 官方文档 + 最佳实践文章*
