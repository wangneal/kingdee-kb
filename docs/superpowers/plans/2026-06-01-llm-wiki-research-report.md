# llm_wiki 调研报告

> **调研对象**: https://github.com/nashsu/llm_wiki  
> **克隆版本**: `b86d81b` (release: v0.4.16, 2026-05)  
> **定位**: 不是传统 RAG—把文档增量编译为持久化 Wiki  
> **日期**: 2026-06-01  

---

## 1. 项目概况

llm_wiki 是一个基于 **Karpathy 的 LLM Wiki 设计模式** 的桌面应用，用 Tauri v2 (Rust) + React 19 + TypeScript 实现。

**核心论文**: `llm-wiki.md` (12KB) — Karpathy 原始设计模式文档

**核心理念**: "知识编译一次，持续更新，而非每次查询重新推导"。  
它不是一个传统 RAG 系统（检索文档 chunk → LLM 生成回答），而是一个 **知识编译系统**（读取原始文档 → LLM 生成结构化 Wiki 页面 → 持久化存储 → 查询时直接读 Wiki 页面）。

---

## 2. 克隆证据

| 证据 | 值 |
|------|-----|
| 远程仓库 | `https://github.com/nashsu/llm_wiki` |
| 克隆命令 | `git clone https://github.com/nashsu/llm_wiki.git && cd llm_wiki && git log --oneline -1` |
| HEAD commit | `b86d81b` |
| 最新标签 | `v0.4.16` |
| 根 README | `README.md` (26KB) |
| 设计文档 | `llm-wiki.md` (12KB) |

---

## 3. 目录结构

```
llm_wiki/
├── src/ (React 前端)
│   ├── App.tsx                    # 入口，项目加载+初始化
│   ├── main.tsx
│   ├── index.css
│   │
│   ├── lib/ (核心逻辑，约 50 个 TS 文件)
│   │   ├── ingest.ts              # ★ 核心：两步 CoT 摄入管道 (2,511 行)
│   │   ├── ingest-queue.ts        # ★ 持久化摄入队列
│   │   ├── ingest-cache.ts        # ★ SHA256 增量缓存
│   │   ├── source-lifecycle.ts    # ★ 源文件生命周期管理
│   │   ├── page-merge.ts          # ★ 三层页面合并保护
│   │   ├── context-budget.ts      # ★ 上下文预算分配 (60/20/5/15)
│   │   ├── graph-relevance.ts     # ★ 4 信号相关性模型 (9.8KB)
│   │   ├── graph-insights.ts      # ★ 知识缺口检测
│   │   ├── wiki-graph.ts          # ★ 知识图谱构建 + Louvain 社区检测
│   │   ├── enrich-wikilinks.ts    # Wikilink 自动补全
│   │   ├── frontmatter.ts         # YAML frontmatter 解析容错
│   │   ├── search.ts              # 前端搜索接口
│   │   ├── embedding.ts           # 向量嵌入管理
│   │   ├── llm-client.ts          # LLM 流式客户端 (8+ 提供商)
│   │   ├── deep-research.ts       # 深度研究
│   │   ├── lint.ts                # Wiki 健康检查
│   │   ├── dedup.ts               # 去重检测
│   │   ├── web-search.ts          # Web 搜索 (Tavily/SerpApi/SearXNG)
│   │   └── ...
│   │
│   ├── components/ (UI 组件)
│   │   ├── chat/                  # 聊天面板
│   │   ├── editor/                # Wiki 编辑器 (Milkdown/ProseMirror)
│   │   ├── graph/                 # 知识图谱可视化 (sigma.js)
│   │   ├── layout/                # 三栏布局
│   │   ├── lint/                  # 健康检查 UI
│   │   ├── review/                # 审查队列 UI
│   │   ├── search/                # 搜索面板
│   │   ├── sources/               # 来源文件管理
│   │   ├── settings/              # 设置面板 (13 个子面板)
│   │   └── ui/                    # 通用组件 (shadcn)
│   │
│   ├── stores/ (Zustand 状态管理)
│   │   ├── wiki-store.ts          # 全局配置
│   │   ├── chat-store.ts          # 多会话聊天
│   │   ├── ingest-queue.ts        # 摄入队列状态
│   │   └── ...
│   │
│   ├── types/
│   │   └── wiki.ts                # 核心类型 (WikiProject, FileNode, WikiPage)
│   │
│   └── i18n/                      # 国际化 (en/zh)
│
├── src-tauri/ (Rust 后端)
│   ├── src/
│   │   ├── lib.rs                 # Tauri 插件注册
│   │   ├── commands/
│   │   │   ├── search.rs          # ★ RRF 混合搜索引擎 (1,187 行)
│   │   │   ├── vectorstore.rs     # ★ LanceDB 向量存储 (1,018 行)
│   │   │   ├── fs.rs              # 文件系统操作
│   │   │   ├── extract_images.rs  # PDF 图片提取 (pdfium)
│   │   │   ├── file_sync.rs       # 文件同步 (notify)
│   │   │   └── ...
│   │   └── api_server.rs          # 本地 HTTP API
│   │
│   └── capabilities/
│
├── extension/ (Chrome 扩展剪藏)
│   └── manifest.json              # Manifest V3
│
└── assets/ (截图)
```

---

## 4. 核心机制拆解

### 4.1 三步数据架构

```
Raw Sources (不可变原始文档)  →  Wiki (LLM 生成的 Markdown)  →  Schema (规则与配置)
```

**数据目录样例：**
```
my-wiki/
├── purpose.md              # Wiki 的"灵魂"——目标、研究范围
├── schema.md               # Wiki 结构规则、页面类型
├── raw/sources/            # 上传的原始文档（不可变）
├── wiki/
│   ├── index.md            # 内容目录（LLM 导航入口）
│   ├── log.md              # 操作日志（时间线）
│   ├── overview.md         # 全局摘要
│   ├── entities/           # 实体页（人物、组织、产品）
│   ├── concepts/           # 概念页（理论、方法、技术）
│   ├── sources/            # 来源摘要
│   ├── queries/            # 保存的聊天回答
│   ├── synthesis/          # 跨来源分析
│   └── comparisons/        # 对比分析
└── .llm-wiki/              # 应用配置、聊天历史、审查项
```

### 4.2 两步链路式思考摄入

**文件**: `src/lib/ingest.ts` (2,511 行)

```
Step 1 (Analysis): LLM 读源文件 → 结构化分析
  输出: Key Entities, Key Concepts, Main Findings, Connections, Contradictions, Recommendations

Step 2 (Generation): LLM 基于分析 → 生成 Wiki 页面 (FILE 块)
  输出: FILE 块 (wiki/sources/xxx.md, wiki/entities/xxx.md 等) + REVIEW 块

Step 2.5 (Review Suggestion): 可选第三次 LLM 调用
  条件: 生成内容 ≥ 10K 字符 或 ≥ 4 个 FILE 块
```

**关键设计参数：**
- LLM temperature: 0.1 (低随机性，确保一致性)
- max_tokens: 4K (Step 1) / 8K-32K (Step 2)
- 长文档 (≥ sourceBudget): 语义分块 → 逐块分析 → checkpoint 持久化 → 合并

### 4.3 持久化摄入队列

**文件**: `src/lib/ingest-queue.ts`

```
持久化: .llm-wiki/ingest-queue.json
状态机: pending → processing → [success/failed]
重试: 最多 3 次，第 3 次后标记 failed（用户手动重试）
崩溃恢复: 启动时加载队列，processing 任务重置为 pending
```

### 4.4 SHA256 增量缓存

**文件**: `src/lib/ingest-cache.ts`

```
缓存 key: 源文件路径
缓存 value: { hash, timestamp, filesWritten[] }
命中条件: ① SHA256 匹配 ② 所有 filesWritten 文件存在
失效: 内容变更 / 用户删除生成页 / 应用版本升级
```

### 4.5 4 信号知识图谱

**文件**: `src/lib/graph-relevance.ts` (9.8KB) / `src/lib/wiki-graph.ts`

```
信号                       权重    含义
────────────────────────────────────────────
Direct Link ([[wikilink]])   ×3.0   页面间直接引用
Source Overlap               ×4.0   共享同一原始文档
Adamic-Adar                  ×1.5   共同邻居加权
Type Affinity                ×1.0   页面类型间亲和度

社区检测: Louvain 算法 (graphology-communities-louvain)
知识缺口: 孤立节点 (deg≤1) / 稀疏社区 (cohesion<0.15) / 桥接节点
```

### 4.6 4 阶段检索管道

```
Phase 1:    关键词搜索（Rust 分词 + CJK bigram + 文件名/标题/内容评分）
Phase 1.5:  向量搜索（LanceDB ANN → chunk → page 聚合，可选）
Phase 2:    图扩展（以 Top 结果为种子，4 信号遍历全图）
Phase 3:    预算控制（5% index + 50% pages + ~30% history + 15% response）
Phase 4:    上下文组装（编号页面 + 系统提示 + 引用格式 [1][2]）
```

**Rust 后端**: `src-tauri/src/commands/search.rs` (1,187 行)

### 4.7 Wiki 页面结构

```yaml
---
title: "页面标题"
type: entity          # entity | concept | source | synthesis | query | other
sources:              # 原始来源追踪
  - "paper-abc.pdf"
wikilinks:            # 交叉引用
  - "related-page"
tags: [...]
created: 2026-01-01
---
```

---

## 5. 关键设计理念

1. **Wiki 是编译产物，不是检索中间结果** — 与传统 RAG 的根本区别
2. **Markdown + Frontmatter** — 兼容 Obsidian，用户可直接编辑
3. **`index.md` 作为 LLM 导航入口** — 中等规模下比嵌入搜索更可靠
4. **可选向量搜索** — 默认关闭，启用后召回率从 58.2% → 71.4%
5. **异步审查系统** — LLM 标记需人工判断的项，不阻塞摄入
6. **`[[wikilink]]` 自动补全** — LLM 只返回替换列表，不直接重写页面
7. **本地 HTTP API** — 端口 19828，允许外部 AI Agent 查询

---

## 6. 对 KingdeeKB 的可借鉴点分级

### 可直接复制
- 持久化摄入队列（JSON 文件 + 状态机 + 崩溃恢复）
- SHA256 三重验证缓存（内容哈希 + 文件存在性检查）
- 项目级互斥锁（串行 ingest 防止 index 竞争）
- Frontmatter YAML 元数据（类型 + 来源追踪 + 标签）

### 需改造后采纳
- 两步摄入（Step 1 纯 Rust 快速分析，Step 2 LLM 可选）
- Wiki 页面层（在 chunk 之上叠加可读知识页）
- `[[wikilink]]` 交叉引用（用于知识页之间）
- 4 信号知识图谱（基于共享 project/tags/section_path 推导）
- 图扩展检索（作为 Phase 2 可选增强）
- 上下文预算分配（适用于 Agent 检索）

### 暂不实现
- Chrome 扩展剪藏（超出知识库范围）
- 本地 HTTP API 服务器（无外部工具集成需求）
- REVIEW 审查系统（需产品验证）
- Louvain 社区可视化（数据量不足以支撑）
- Deep Research / Web 搜索（金蝶实施场景不需要）
- VLM 图片 caption（增加包体积 >50MB）
- LLM 自动 wikilink 补全（质量不稳定）
