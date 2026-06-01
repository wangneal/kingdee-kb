# 知识库重构 + 调研大纲编辑器及脑图视图 — 设计决策

> **前置阅读**: 先看 `2026-06-01-llm-wiki-research-report.md`  
> **状态**: 讨论稿，等待确认后进入开发  
> **日期**: 2026-06-01  

---

## 决策 1：知识库架构 — 从"chunk 检索"升级为"编译知识层"

**现状**：KingdeeKB 当前是传统 RAG 架构：文档 → chunk → 向量/BM25 索引 → 检索 chunk。

**目标**：在现有 chunk 层之上，叠加"编译知识层"（类似 llm_wiki 的 Wiki 页面）。

**方案选择**：

| 方案 | 描述 | 风险 | 推荐 |
|------|------|------|------|
| A. 推倒重来 | 废弃 chunk，全部改为 wiki 页面 | 存量数据全丢，AGENT 检索依赖 chunk | ❌ |
| **B. 叠加层** | 保留 chunk 作为底层检索，新增 wiki_pages 层 | 双轨维护成本 | ✅ |
| C. 增量迁移 | 逐步将 chunk 转化为 wiki 页面 | 过渡期长，产品体验割裂 | 可做阶段目标 |

**决策 B（叠加层）**：
- 现有 chunk 检索路径**不变**
- 新增 `wiki_pages` 表，存储 Markdown + Frontmatter 知识页
- Agent 检索时**优先**搜索 wiki_pages，回退到 chunks
- 两步摄入 Step 2 LLM 编译**默认关闭**，由配置控制

**取舍理由**：
- 不改现有检索路径 = 零回归风险
- LLM 编译默认关闭 = 不增加新用户的导入延迟
- 配置开启 = 高级用户可获得更好的知识组织

---

## 决策 2：两步摄入 — Step 1 纯 Rust，Step 2 LLM 可选

**现状**：当前是一步 ingest（extract → clean → chunk → embed → store）。

**方案选择**：

| 方案 | 描述 | 优缺点 |
|------|------|--------|
| A. 完整两步（llm_wiki 原样） | Step 1 LLM 分析 + Step 2 LLM 生成 | 延迟高、贵、不可控 |
| **B. Step 1 Rust + Step 2 LLM 可选** | 快速分析用 Rust，知识编译用 LLM 可选 | 平衡速度和质量 |
| C. 纯 Rust 分析 | 完全不用 LLM | 缺乏语义理解 |

**决策 B（修正后）**：
- **Step 1（分析）**：**双引擎**。LLM 语义分析为主（命名实体、关键概念、交叉引用、矛盾检测），Rust 本地提取为降级备用（TF-IDF 关键词、标题层级树、词频统计）。
- 当 `enable_kb_compilation = true` 且 LLM 可用时，默认使用 LLM 引擎；LLM 超时（30s，复杂文档如 50 页蓝图可能需要 20-40s）或无可用 LLM 时自动降级为 Rust 引擎。
- **Step 2（编译）**：调用 LLM 生成可选。生成 wiki_pages（蓝图/Fit-Gap/决策/配置/摘要）
- Step 2 由 `enable_kb_compilation` 配置开关控制，默认 `false`

**Step 1 输出结构（DocumentAnalysis）**——兼容两种引擎输出：
```rust
pub struct DocumentAnalysis {
    pub source_identity: String,         // 关联 raw_sources.identity
    pub sha256: String,                  // 源文件 SHA256
    pub title: String,                   // 提取的标题
    pub headings: Vec<Heading>,          // 标题层级树（两引擎均产出）
    pub keywords: Vec<KeywordScore>,     // TF-IDF 关键词（两引擎均产出）
    pub word_count: usize,
    pub char_count: usize,
    pub language: String,                // 检测到的语言
    // LLM 引擎专属字段（Rust 引擎输出为空数组）
    pub entities: Vec<String>,           // 命名实体（人名/组织/产品/系统）
    pub key_concepts: Vec<String>,       // 关键概念
    pub cross_references: Vec<CrossRef>, // 与已有 Wiki 的交叉引用
    pub contradictions: Vec<String>,     // 检测到的矛盾
}
pub struct Heading {
    pub level: u8,                       // 1-6
    pub text: String,
    pub children: Vec<Heading>,
}
pub struct KeywordScore {
    pub keyword: String,
    pub score: f32,
}
```

此结构写入 `analysis_cache` 表，被 Step 2 LLM 消费。LLM prompt 直接引用该结构的 JSON 序列化。

**analysis_cache 表定义**：
```sql
CREATE TABLE analysis_cache (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project         TEXT NOT NULL,
    source_identity TEXT NOT NULL,
    sha256          TEXT NOT NULL,              -- 分析时的源文件 SHA256，用于失效
    analysis_json   TEXT NOT NULL,              -- DocumentAnalysis 的 JSON 序列化
    analyzer_version TEXT NOT NULL DEFAULT '1', -- 分析器版本，升级后自动重分析
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project, source_identity, sha256)
);
```

**暂不做**：
- 长文档 checkpoint 恢复（llm_wiki 有，但 KingdeeKB 文档通常 ≤ 50 页）
- Step 2.5 Review Suggestion（复杂度高，收益不确定）

---

## 决策 3：知识页类型系统

**需要定义 KingdeeKB 特有的知识页类型**，不能照搬 llm_wiki 的 entity/concept/source。

| 类型 | 用途 | 生成时机 | 类比 llm_wiki |
|------|------|---------|---------------|
| `summary` | 文档摘要/实体提取 | 文档导入后 | entity + source |
| `blueprint` | 业务蓝图 | 调研完成后 | synthesis |
| `fitgap` | Fit-Gap 分析 | 需求对比标准功能 | comparison |
| `decision` | 实施决策记录 | 关键决策点 | — |
| `config` | 系统配置说明 | 配置完成后 | — |

**讨论点：是否需要增减类型？**

---

## 决策 4：原始资料区设计

**现状**：导入的文档直接处理为 chunk，不保留原始文件副本。

**方案选择**：

| 方案 | 描述 |
|------|------|
| A. 不保留原始文件 | 当前做法，节省磁盘 |
| **B. 保留原始文件** | 新增 raw_sources 表 + 文件副本 |
| C. 保留引用 | 仅记录 source_path，不复制文件 |

**决策 B**：
- 导入时复制文件到 `{data_dir}/raw/{project}/sources/`（不可变）
- `raw_sources` 表记录 identity、SHA256、存储路径
- 文件级 SHA256 追踪：文件变更 → 重新 ingest → 级联更新引用该 source 的 wiki_pages

**取舍理由**：
- B 比 A 多占磁盘，但提供源可追溯性（critical for 实施文档审计）
- B 比 C 可靠（用户删除源文件后仍可用）

---

## 决策 5：持久化摄入队列

**照搬 llm_wiki**，最小改动。

- JSON 文件持久化（`{data_dir}/ingest-queue.json`）
- 状态机：`pending → processing → done/failed`
- 崩溃恢复：启动时 processing → pending
- 最大重试：3 次
- 串行处理（一次一个），项目级互斥锁

**不做**：
- 暂停/恢复/取消单个任务（MVP 后迭代）
- 进度事件推送（前端轮询即可）

---

> **术语说明**：本模块实际交付的是「**大纲编辑器 + 脑图视图**」。编辑在大纲树中完成（键盘+拖拽），脑图使用 markmap 只读渲染。当前不做脑图内直接拖拽编辑。如果后续产品要求脑图内编辑，优先评估 `mind-elixir`（开源脑图编辑引擎，支持拖拽编辑节点/连线/布局）。

## 决策 6：调研大纲编辑器 + 脑图视图与 Q&A 的关系

**明确边界**：Q&A 是原始记录层，Outline/Mindmap 是结构化整理层，两者**并存**。

```
调研会话
  ├─ Q&A 记录层（不可变原始访谈记录）
  │   每条 Q&A 是独立的问答对
  │   由 Agent AI 辅助生成或手动输入
  │
  └─ 大纲/脑图层（结构化整理）
      用户在 Q&A 基础上人工创建
      大纲节点可关联 Q&A 记录（question_id）
      大纲可一键切换为脑图视图
```

**交互规则**：
- 创建大纲节点时**可选择**关联 Q&A 记录（可选，非必须）
- 删除大纲节点**不删除**关联的 Q&A 记录（仅解除关联）
- Q&A 列表视图中可看到所属的大纲路径

**不做**：
- 从脑图反向生成 Q&A（会增加数据不一致风险）
- LLM 自动从 Q&A 生成大纲（质量不可控，后续可迭代）

---

## 决策 7：脑图渲染方案

| 方案 | 库 | 体积 | 优缺点 |
|------|-----|------|--------|
| **markmap** | `markmap-lib` + `markmap-view` | ~50KB | 轻量，Markdown→SVG，一键切换; **只读** |
| React Flow | `@xyflow/react` | ~300KB | 可自定义交互，但重；不是专门脑图库 |
| D3.js | `d3-hierarchy` | ~100KB | 灵活但需大量手写代码 |
| AntV X6 | `@antv/x6` | ~500KB | 太重 |
| mind-elixir | `mind-elixir` | ~100KB | 开源脑图编辑引擎，支持拖拽编辑节点/连线/布局、快捷键、折叠、导出图片 |
| jsMind | `jsmind` | ~50KB | 轻量脑图展示+编辑，但社区活跃度较低 |

**决策**: **markmap**，原因：
- MVP 不做脑图内编辑，编辑在大纲树中完成，脑图只做只读展示。
- markmap 体积最小（~50KB），Markdown 层级结构天然映射到脑图，零额外数据转换。
- 若后续要求脑图内直接拖拽编辑，切换到 `mind-elixir`（开源脑图编辑引擎，支持拖拽编辑/快捷键/导出），需重新评估集成工作。

**集成风险评估**：

| 风险 | 评估 | 缓解方案 |
|------|------|---------|
| **CSP 限制** | Tauri 默认 CSP 允许 `style-src 'unsafe-inline'`；markmap 的 SVG 使用 inline style，在 Tauri webview 中正常工作 | 无需额外 CSP 配置 |
| **SVG 注入** | markmap 渲染的 SVG 包含用户文本内容；Tauri WebView 中仍存在 XSS 风险 | 输入渲染前做 HTML entity 转义 |
| **缩放/平移** | markmap-view 内置 zoom + pan，交互完整 | 开箱即用 |
| **导出** | SVG 可直接导出为 PNG（`new Image()` → canvas → blob），无需额外库 | 导出按钮加 1 天 |
| **大型脑图** | markmap 基于 d3-hierarchy，1000+ 节点时 SVG DOM 量大会卡顿 | 大纲阶段通常 ≤ 200 节点，无需优化 |
| **与编辑态的同步** | markmap 是只读展示，编辑态在大纲树组件中完成。数据源相同（OutlineContext），切换时重新渲染 | 无需状态同步逻辑 |
| **移动端性能** | 桌面端 Tauri webview 性能充裕，无移动端需求 | 不适用 |

---

## 决策 8：实现顺序

```
阶段一: 基础设施加固（2.5 天）
  内容: 分级锁、安全向量 key、删除补偿机制（outbox/校验清理）、去重代码
  原因: 修复当前稳定性问题，不阻塞后续任何阶段
  验证: cargo check + 并发导入不冲突 + 删除中途崩溃后三索引一致

阶段二: 原始资料 + 持久化队列（2 天）
  内容: raw_sources 表、文件复制、ingest_queue
  原因: 为编译知识层提供数据基础
  验证: 导入文件后保留原始副本 + 队列崩溃恢复

阶段三: 编译知识层（4 天）
  内容: wiki_pages 表、Step 1 Rust 分析、Step 2 LLM(可选)
  原因: 核心新能力，与阶段四可并行
  验证: 配置开启后导入文件能生成 wiki_pages

阶段四: 调研大纲编辑器 + 脑图视图（6 天，可与阶段三并行）
   内容: outline_nodes CRUD + 大纲树 UI + markmap 脑图视图 + Q&A 关联
  原因: 独立模块，不依赖阶段二/三
  验证: 键盘快捷键 + 拖拽 + 脑图切换 + Q&A 可关联

阶段五: 知识图谱 + 图检索（4 天，远期）
  前提: wiki_pages 有足够数据
  内容: wikilink 编辑器、4 信号图构建、图扩展检索
```

> **关于「删除补偿机制」的说明**：  
> KingdeeKB 的文档涉及三处索引（SQLite + usearch + BM25），SQLite 事务不能跨索引保证原子性。  
> 不采用「事务包装三索引」的假方案，改用：
> 1. **删除前**：在 SQLite 记录待删 chunk_id 列表（outbox）
> 2. **逐个删除**：先删 usearch → 再删 BM25 → 最后删 SQLite
> 3. **删除后**：校验三索引一致性，残留则触发补偿清理
> 4. **启动时**：扫描 outbox 中的未完成任务，执行补偿

## 补充：数据实体关系定义

### raw_sources ↔ documents ↔ wiki_pages 关系

```
┌──────────────────────────────────────────────────┐
│                   raw_sources                      │
│  id, project, identity(†), original_path, sha256, │
│  storage_path, status, created_at, deleted_at     │
│  文件存储: {data_dir}/raw/{project}/sources/      │
│  (†) UNIQUE(project, identity)                    │
└──────────┬───────────────────────────┬───────────┘
           │ 1:1 (raw_source_identity) │ 1:N (sources[] 引用)
           ▼                           ▼
┌──────────────────────┐   ┌─────────────────────────┐
│     documents        │   │      wiki_pages          │
│  (现有)              │   │  (新增)                  │
│  raw_source_identity   │   │  sources TEXT → JSON     │
│  → raw_sources.identity│   │  按 project+identity 解析  │
│  source_path (保留)    │   │  每条知识页可引用多源     │
│  sha256 = raw_sources │   │                          │
│    的 sha256（一致）   │   │  wikilinks TEXT → JSON   │
│                      │   │  数组引用其他 wiki_pages   │
│  弱关联，无外键硬约束  │   │  无外键约束（允许孤立页） │
└──────────────────────┘   └─────────────────────────┘
```

**关键约束**：
- `documents` 新增 `raw_source_identity TEXT` 字段，指向 `raw_sources.identity`；原有 `source_path` 保留其展示语义（原始文件路径）
- `documents.sha256` 必须与 `raw_sources.sha256` 一致（对同一文件）
- `wiki_pages.sources` 是 JSON 数组，默认按 `wiki_pages.project + identity` 解析到 `raw_sources`。如需引用其他项目的源，使用对象格式 `{"project":"other","identity":"path"}`。两种格式可混用。
- 无外键硬约束（允许人工创建独立知识页）
- 删除 `raw_sources` 时为 **soft delete**（`status = 'deleted'`）。`wiki_pages.sources` 中的引用**保持不变**（identity 不改写），展示层根据 `raw_sources.status='deleted'` 渲染 `[源已删除]` 标记。如需持久化删除状态，使用对象格式 `{"identity":"path","deleted":true}` 替换原字符串。后台可运行清理任务删除已无引用的原始文件。

### raw_sources 表完整定义

```sql
CREATE TABLE raw_sources (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    project       TEXT NOT NULL,
    identity      TEXT NOT NULL,              -- "papers/energy/solar.pdf"
    original_path TEXT NOT NULL,              -- 导入时的原始路径（用户看到的路径）
    storage_path  TEXT NOT NULL,              -- 实际存储路径（raw/{project}/sources/...）
    sha256        TEXT NOT NULL,
    file_size     INTEGER,
    mime_type     TEXT,
    status        TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','deleted')),
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at    TEXT,                       -- soft delete 时间
    UNIQUE(project, identity)
);

CREATE INDEX idx_raw_sources_project   ON raw_sources(project);
CREATE INDEX idx_raw_sources_status    ON raw_sources(status);
```

### wiki_pages 表完整字段定义

```sql
CREATE TABLE wiki_pages (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    project       TEXT NOT NULL,
    slug          TEXT NOT NULL,              -- "blueprints/ar-module", "fitgap/ar-reconciliation"
    title         TEXT NOT NULL,
    page_type     TEXT NOT NULL CHECK(page_type IN (
                    'summary','blueprint','fitgap','decision','config'
                  )),
    content       TEXT NOT NULL,              -- Markdown 正文（展示和检索的主字段）
    content_candidate TEXT,                    -- LLM 新版本候选，用户确认后提升为 content
    candidate_status TEXT CHECK(candidate_status IN ('auto','conflict','pending')), -- 候选状态
    frontmatter   TEXT NOT NULL DEFAULT '{}', -- YAML frontmatter 的 JSON 备份
    sources       TEXT NOT NULL DEFAULT '[]', -- JSON 数组: ["identity", {"project":"other","identity":"path"}], identity 默认按 wiki_pages.project 解析
    wikilinks     TEXT NOT NULL DEFAULT '[]', -- JSON 数组: ["wiki_pages.slug", ...]
    tags          TEXT NOT NULL DEFAULT '[]', -- JSON 数组
    page_metadata TEXT NOT NULL DEFAULT '{}', -- JSON 对象，按 page_type 不同结构：
      -- summary:     { source_identity, extracted_entities[], key_concepts[] }
      -- blueprint:   { edition, modules[], status }
      -- fitgap:      { module, gaps[], decisions[] }
      -- decision:    { decision_date, decision_maker, alternatives[] }
      -- config:      { module, system_path, parameters{} }
    candidate_version INTEGER,                -- 候选版本号（批准后→version）
    page_status   TEXT NOT NULL DEFAULT 'draft' CHECK(page_status IN ('draft','published')), -- 发布状态
    version       INTEGER NOT NULL DEFAULT 1, -- 已批准版本号（批准 content_candidate→content 时递增）
    CHECK(
      (content_candidate IS NULL AND candidate_status IS NULL AND candidate_version IS NULL) OR
      (
        content_candidate IS NOT NULL
        AND candidate_status IS NOT NULL
        AND candidate_version IS NOT NULL
        AND candidate_version = version + 1
      )
    ), -- 候选内容/状态/版本号必须同时存在/为空，且版本号 = 正式版 + 1
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
    -- project 不设外键：项目是逻辑概念，无独立 projects 表；未来有统一 project 表后再补迁移
);

CREATE INDEX idx_wiki_pages_project ON wiki_pages(project);
CREATE INDEX idx_wiki_pages_type   ON wiki_pages(page_type);
CREATE UNIQUE INDEX idx_wiki_pages_slug ON wiki_pages(project, slug);
```

**Frontmatter 存储**：YAML frontmatter 写入 `content` 字段顶部，同时 JSON 化备份到 `frontmatter` 字段（方便 SQL 查询）。两者必须一致。

### Wiki 页面重新生成时的合并策略

当源文件变更触发重新 ingest 时，已有 wiki_pages 可能被用户手工编辑过。不能简单覆盖。

**策略**：分层保护（参考 llm_wiki page-merge.ts，简化适配）

| 层级 | 内容 | 策略 |
|------|------|------|
| **L1: 元数据字段** | `sources`、`tags`、`wikilinks` | **确定性合并**：数组 union 合并，LLM 不参与 |
| **L2: 正文内容** | `content`（Markdown） | **候选发布**：新 LLM 版本写入 `content_candidate`，标记 `candidate_status = 'pending'`、`candidate_version = version + 1`（覆盖候选仍 `version + 1`，批准后才写入正式 `version`）。用户确认后提升 `content_candidate → content`、`candidate_version → version`、清理候选字段。用户未确认前，展示和检索仍用旧 `content`。 |
| **L3: 锁定字段** | `title`、`page_type`、`created_at` | **永不覆盖**：始终保留首次写入值 |

**差异检测**：
- LLM 新版本与当前 `content` 的差异比例使用**确定性 diff**计算（按 UTF-8 字符 diff，`diff.length / max(a.length, b.length)`），不超过 LLM。
- 如果差异 ≤ 30%：`content_candidate` 标记为 `auto`（候选），用户可在页面详情中审批。
- 如果差异 > 30%：`content_candidate` 标记为 `conflict`（冲突），前端显示冲突标记，人工介入。
- 首次生成候选时 `candidate_version = version + 1`；已存在候选时覆盖写入，`candidate_version` 保持 `version + 1` 不变（批准后才推进 `version`）
- diff 比例仅用于分类，不阻断任何写入（防止漏误判导致数据丢失）。

**冲突处理**：
- `content_candidate` 始终可写，永远不阻塞 LLM 生成。
- 用户确认前，`content`（旧版本）是展示和检索的主字段。
- 前端展示 `version` 标签 + `candidate_status` 标签（auto/conflict/pending），以及 `page_status`（published/draft，人工控制）。
- 用户可一键批准（`content_candidate → content`、`candidate_version → version`、**同时置 NULL 三个候选字段**)或回退（删除 `content_candidate`，**同步置 NULL `candidate_status` 和 `candidate_version`**）。
- **MVP 只保留上一版候选**：`content_candidate` 保存最新一个候选版本。后续可迭代为 `wiki_page_versions` 表实现完整版本历史。

### 迁移路径

1. 新增 `raw_sources` + `wiki_pages` 表，不影响存量 `documents` + `chunks`
2. 现有文档导入流程改为：复制到 raw/ → 写入 raw_sources → 走现有 ingest 写入 documents+chunks
3. documents 新增 `raw_source_identity` 字段（粘贴导入的保持 NULL）
4. `wiki_pages` 初始为空，走两步摄入逐步填充
5. 每次迁移完成后设置 `PRAGMA user_version = <版本号>`，用于版本跟踪和降级判断。降级时检查当前 version，仅清理新增表（不破坏存量 documents/chunks）。

### outline_nodes 表完整定义

```sql
CREATE TABLE outline_nodes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  INTEGER NOT NULL REFERENCES research_sessions(id) ON DELETE CASCADE,
    parent_id   INTEGER REFERENCES outline_nodes(id),
    content     TEXT NOT NULL DEFAULT '',
    note        TEXT NOT NULL DEFAULT '',
    collapsed   INTEGER NOT NULL DEFAULT 0,
    completed   INTEGER NOT NULL DEFAULT 0,
    marker      TEXT DEFAULT '',
    priority    TEXT DEFAULT '',
    tags        TEXT DEFAULT '',
    question_id INTEGER REFERENCES session_qa_records(id) ON DELETE SET NULL,
    sort_order  REAL NOT NULL DEFAULT 0,    -- Fractional Indexing：浮点数，插入时取相邻节点平均值
    created_at  TEXT DEFAULT (datetime('now')),
    updated_at  TEXT DEFAULT (datetime('now'))
);

CREATE INDEX idx_outline_nodes_session ON outline_nodes(session_id);
CREATE INDEX idx_outline_nodes_parent  ON outline_nodes(parent_id);
CREATE INDEX idx_outline_nodes_sort    ON outline_nodes(session_id, parent_id, sort_order);
CREATE INDEX idx_outline_nodes_question ON outline_nodes(question_id);
```

**关键约束**：
- `parent_id` 引用自身（树形结构），`ON DELETE` 无级联；删除时后端递归删除整棵子树（`delete_node_subtree`）
- `question_id` 引用 `session_qa_records`，`ON DELETE SET NULL` — 删除 Q&A 记录时大纲节点保留但解除关联
- **排序仅由 `sort_order REAL` 维护**，使用 **Fractional Indexing**：插入节点时取前后相邻节点 `sort_order` 的算术平均值。`move_node` 时仅需更新 `sort_order` 为新位置的平均值（$O(1)$），无需维护链表指针。
- 同 `session_id` 校验：移动/创建操作必须验证新 `parent_id` 属于同一个 session
- **禁止移动到子孙节点**：移动前递归校验新父节点不是当前节点或其子树成员

**`move_node` 约束**（服务端校验）：
1. `new_parent_id` 和 `target_node` 必须在同一 session
2. `new_parent_id` 不等同于 `source_id`（不能是自己的父节点）
3. **`source_id` 不能是 `new_parent_id` 的祖先**（防成环）：递归遍历 `new_parent_id` 的父链，若 `source_id` 出现在链中则拒绝
4. `new_sort_order` 应为前后相邻节点的平均值，后端不做精确校验（允许前端计算）

**CRUD 命令**：

```rust
// 新增: 创建节点，自动计算 sort_order（取相邻节点平均值）
fn create_node(session_id, parent_id, content) -> OutlineNode

// 更新: 支持部分字段更新（content, note, collapsed, completed, marker, priority, tags）
fn update_node(id, fields) -> ()

// 删除子树: 递归删除当前节点及所有子孙节点
fn delete_node_subtree(id) -> ()

// 移动: 更新 parent_id + sort_order（Fractional Indexing 取平均值）；
//        禁止移动到子孙节点；验证 parent_id 属于同一 session
fn move_node(id, new_parent_id, new_sort_order) -> ()

// 批量查询: 按 session_id 返回所有节点（ORDER BY parent_id, sort_order）
fn get_tree(session_id) -> Vec<OutlineNode>

// 导出: 支持两种 Markdown 格式
//   - "markdown_list": 嵌套缩进无序列表，供用户复制阅读
//   - "markdown_headings": 多级标题层级，供 markmap 脑图渲染
fn export_outline(session_id, format: String) -> String
```

**Fractional Indexing 算法**（前端计算，后端存储）：

```
首个节点: create_node 时 parent 下无子节点 → sort_order = 1.0
默认追加: create_node 时 parent 下已有子节点 → 取最后一个子节点的 sort_order + 1.0
插入在 B(1.0) 和 C(2.0) 之间 → sort_order = (1.0 + 2.0) / 2 = 1.5
插入在 B(1.0) 之前 → sort_order = B.sort_order - 1.0（允许负数）
插入在 C(2.0) 之后 → sort_order = C.sort_order + 1.0
精度溢出时（REAL 类型分辨力耗尽）→ 触发当前 parent 下全量重编号（重新分配 1,2,3...）
重编号安全性: 在事务中执行，对当前 session_id 加应用层写锁（通过项目级互斥锁），重编号期间其他线程对该 session 的写入排队等待
```

**快捷键映射**（区分导航态与编辑态）：

| 按键 | 导航态（节点选中但未编辑） | 编辑态（textarea 焦点） |
|------|--------------------------|------------------------|
| Enter | 新建同级节点 | 换行（textarea 默认行为） |
| Tab | 缩进为子节点 | 插入制表符（preventDefault + 插入空格） |
| Shift+Tab | 反缩进（提升层级） | — |
| ↑/↓ | 在节点间上下移动焦点 | 光标在 textarea 内移动 |
| ←/→ | 折叠/展开当前节点 | 光标在 textarea 内移动 |
| Ctrl+Z | 大纲级 Undo | 文本级 Undo（textarea 默认） |
| Ctrl+Shift+Z | 大纲级 Redo | 文本级 Redo（textarea 默认） |
| Delete/Backspace | 删除选中节点（确认对话框） | 删除字符 |
| Escape | 退出编辑态 → 导航态 | 失去焦点，回到导航态 |
| Ctrl+. | 折叠/展开当前节点 | — |

- 焦点在 textarea 内时，`Ctrl+Z`/`Ctrl+Shift+Z` 由浏览器 textarea 接管（文本级 Undo/Redo）
- 焦点在导航态（无 textarea 焦点）时，`Ctrl+Z`/`Ctrl+Shift+Z` 由 OutlineContext 接管（大纲级 Undo/Redo）
- `Tab`/`Shift+Tab` 在编辑态和非编辑态的行为不同：编辑态下先 `preventDefault()` 阻止焦点切换，再执行缩进操作

---

## 补充：极限场景审查修正

以下修正项来自对多线程死锁模型、存储引擎物理机制、磁盘 I/O 阻塞、孤儿垃圾数据的第二轮深度审查。每项修正会更新或覆盖前述相关决策的细节。

### 修正 1：锁获取顺序等级（防止死锁闭环）

**风险**：分级锁（Mutex→RwLock）后，写入线程持有 `vector_index Mutex` 等待 `metadata RwLock`，同时检索线程持有 `metadata RwLock` 等待 `vector_index Mutex`，形成死锁闭环，进程永久卡死。

**修正**：强制执行全局唯一的锁获取顺序：

```
加锁顺序（严格递增）:
  1. metadata (RwLock)
  2. bm25 (RwLock)
  3. vector_index (Mutex)
  4. embedding (Mutex)
  5. 其他服务
```

- **检索线程**和**写入线程**必须遵守相同顺序，禁止逆向加锁。
- 任何需要同时获取两把以上锁的代码路径，必须在注释中标注锁等级顺序。
- 违反顺序的代码应在 Code Review 中被拒绝。

---

### 修正 2：BM25 批量删除补 commit

**风险**：`remove_chunks` 仅调用 `writer.delete_term()`，删除操作在未 `commit()` 前对检索线程不可见（仍能搜出已删数据），且删除标记滞留在 IndexWriter 内存队列中，导致内存泄露。

**修正**：
```rust
pub fn remove_chunks(&self, chunk_ids: &[i64]) -> Result<(), String> {
    let writer = self.writer.lock().map_err(|e| e.to_string())?;
    for cid in chunk_ids {
        let term = tantivy::Term::from_field_i64(self.field_chunk_id, *cid);
        writer.delete_term(term);
    }
    writer.commit().map_err(|e| format!("BM25 提交删除失败: {}", e))?; // ← 追加 commit
    drop(writer);
    self.reader.reload().map_err(|e| format!("BM25 重载失败: {}", e))?;
    Ok(())
}
```

---

### 修正 3：usearch 逻辑删除 → 碎片整理/重建机制（异步 double-buffering）

**风险**：usearch 的 `remove()` 仅标记墓碑（tombstone），不释放内存/磁盘，不重构 HNSW 图。频繁增删后检索延迟单调递增。

**修正**：
```rust
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

impl VectorIndex {
    const COMPACT_THRESHOLD: f64 = 0.2; // 20%
    const COMPACT_COOLDOWN_SECS: u64 = 300; // 5 分钟冷却，防止频繁触发

    /// 删除向量并递增计数
    pub fn remove(&self, key: u64) -> Result<(), String> {
        self.index.read().map_err(|e| e.to_string())?.remove(key);
        self.deleted_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// 后台重建：收集幸存向量 → 创建新索引 → 原子 swap
    /// 使用 &self（不可变引用），通过内部 RwLock 实现无阻塞 swap
    pub fn compact(&self) -> Result<(), String> {
        let surviving: Vec<(u64, Vec<f32>)> = {
            let idx = self.index.read().map_err(|e| e.to_string())?;
            let n = idx.len() as u64;
            let mut survivors = Vec::with_capacity(n as usize);
            for key in 1..=n {
                if let Ok(vec) = idx.get(key) {
                    survivors.push((key, vec.to_vec()));
                }
            }
            survivors
        };
        let mut new_index = Index::new(IndexOptions {
            dimensions: 512,
            metric: MetricKind::Cos,
            quantization: ScalarKind::BF16,
            connectivity: 16,
            expansion_add: 200,
            expansion_search: 64,
            multi: false,
        }).map_err(|e| format!("创建新索引失败: {}", e))?;
        new_index.reserve(surviving.len()).map_err(|e| format!("预留容量失败: {}", e))?;
        for (key, vec) in &surviving {
            new_index.add(key, vec).map_err(|e| format!("添加向量失败: {}", e))?;
        }
        // 原子替换
        *self.index.write().map_err(|e| e.to_string())? = new_index;
        self.deleted_count.store(0, Ordering::SeqCst);
        *self.last_compact.write().map_err(|e| e.to_string())? = Instant::now();
        Ok(())
    }

    /// 检查是否需要重建（阈值 + 冷却期）
    pub fn check_compact(&self) -> bool {
        let total = self.index.read().map(|i| i.len() as f64).unwrap_or(0.0);
        if total <= 0.0 { return false; }
        let ratio = self.deleted_count.load(Ordering::SeqCst) as f64 / total;
        ratio >= Self::COMPACT_THRESHOLD
            && self.last_compact.read().map(|t| t.elapsed()).unwrap_or(Duration::MAX)
                > Duration::from_secs(Self::COMPACT_COOLDOWN_SECS)
    }
}

pub struct VectorIndex {
    index: Arc<RwLock<Index>>,          // RwLock 允许 &self 下并发读 + 原子 swap 写
    options: IndexOptions,
    deleted_count: AtomicUsize,          // 逻辑删除计数
    last_compact: RwLock<Instant>,       // 上次重建时间，用于冷却期判断
}
```

- `index` 使用 `Arc<RwLock<Index>>` 而非 `Mutex<Index>`，使 `compact()` 可以用 `&self` 调用，不阻塞 Mutex 持有链。
- 删除操作只在读锁上执行，重建只在写锁上执行（瞬间 swap）。
- `check_compact()` 由外部编排层独立调用，不在删除链路中，避免性能影响。
- `collect_surviving()` 内联到 `compact()` 中，减少一次索引遍历。

- `compact()` 使用 `&self`（不可变引用）而非 `&mut self`，内部使用 RwLock 或原子 swap 实现无阻塞重建。
- 20% 阈值 + 5 分钟冷却期，防止频繁触发。
- 后台任务通过 `tokio::spawn_blocking` 执行，不阻塞主线程。向量检索和写入在新索引 swap 完成后受影响。

---

### 修正 4：大纲编辑器防抖 + Undo/Redo 栈

**风险**：大纲编辑器是高频编辑场景，每次按键失焦都 `invoke` 物理写入 SQLite，产生写锁超时与输入卡顿；缺乏 Undo/Redo 栈，误删无法恢复。

**修正**：

**防抖持久化**：
```typescript
// OutlineContext.tsx
const SAVE_DEBOUNCE_MS = 800;

const saveNode = useMemoizedFn(async (node: OutlineNode) => {
  if (saveTimer.current) clearTimeout(saveTimer.current);
  saveTimer.current = setTimeout(async () => {
    await invoke("update_outline_node", { id: node.id, content: node.content });
    saveTimer.current = null;
  }, SAVE_DEBOUNCE_MS);
});
```

**Undo/Redo 栈**（关键规则：Undo/Redo 绕过防抖，直接同步持久化）：
```typescript
const undo = useCallback(() => {
  // 规则：Undo 操作必须取消待发的防抖保存，并用恢复后的内容重新触发保存
  if (saveTimer.current) {
    clearTimeout(saveTimer.current);
    saveTimer.current = null;
  }
  const prev = undoStack[undoStack.length - 1];
  if (!prev) return;
  setRedoStack(prevRedo => [...prevRedo, { nodes: currentNodes }]);
  setUndoStack(prev => prev.slice(0, -1));
  restoreNodes(prev.nodes);          // 恢复内存状态
  for (const node of prev.nodes) {
    saveNode(node);                   // 重新触发保存（用恢复后的内容）
  }
}, [undoStack, currentNodes]);

const redo = useCallback(() => {
  if (saveTimer.current) {           // Redo 同上处理
    clearTimeout(saveTimer.current);
    saveTimer.current = null;
  }
  const next = redoStack[redoStack.length - 1];
  if (!next) return;
  setUndoStack(prev => [...prev, { nodes: currentNodes }]);
  setRedoStack(prev => prev.slice(0, -1));
  restoreNodes(next.nodes);
  for (const node of next.nodes) {
    saveNode(node);
  }
}, [redoStack, currentNodes]);
```

- 每次用户操作（创建/删除/移动/修改）前推入 Undo 栈。
- `Ctrl+Z` 触发 Undo，`Ctrl+Shift+Z` 触发 Redo。
- Undo/Redo 仅操作内存状态，异步持久化由防抖机制处理。

---

### 修正 5：大纲删除孤儿 QA 级联清理（含收藏保护）

**风险**：删除大纲节点时，关联的 QA 记录（`question_id`）`ON DELETE SET NULL` 后失去关联，若该 QA 未被其他节点引用且未被收藏，则成为 UI 不可见的孤儿数据。

**前置条件**：`session_qa_records` 表需增加两个字段：
```sql
ALTER TABLE session_qa_records ADD COLUMN source TEXT NOT NULL DEFAULT 'auto' CHECK(source IN ('auto','manual'));
ALTER TABLE session_qa_records ADD COLUMN is_bookmarked INTEGER NOT NULL DEFAULT 0;
```

**修正**：
```rust
// outline.rs delete_node_subtree
pub fn delete_node_subtree(&self, id: i64) -> Result<(), String> {
    // 1. 收集所有要删除的节点 ID（包含子树）
    let all_ids = self.collect_subtree_ids(id)?;

    // 2. 找出这些节点关联的 question_id（使用临时表规避 SQLite 参数数量限制 32766）
    self.db.execute("CREATE TEMP TABLE IF NOT EXISTS _del_ids (id INTEGER PRIMARY KEY)")?;
    for cid in &all_ids {
        self.db.execute("INSERT OR IGNORE INTO _del_ids VALUES (?1)", params![cid])?;
    }
    let orphan_qas: Vec<i64> = self.db.prepare(
        "SELECT qa.id FROM session_qa_records qa
         WHERE qa.id IN (
           SELECT question_id FROM outline_nodes o
           JOIN _del_ids d ON o.id = d.id
         )
         AND qa.source != 'manual'
         AND qa.is_bookmarked = 0
         AND qa.id NOT IN (
           SELECT question_id FROM outline_nodes
           WHERE question_id IS NOT NULL
           AND id NOT IN (SELECT id FROM _del_ids)
         )"
    )?.query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    self.db.execute("DROP TABLE IF EXISTS _del_ids")?;

    // 3. 递归删除子节点
    for child_id in all_ids {
        self.db.execute("DELETE FROM outline_nodes WHERE id = ?1", params![child_id])?;
    }

    // 4. 级联删除孤儿 QA（未被任何大纲节点引用）
    for qa_id in orphan_qas {
        self.db.execute("DELETE FROM session_qa_records WHERE id = ?1", params![qa_id])?;
    }

    Ok(())
}
```

- 仅在 QA 未被任何大纲节点引用时才级联删除（不删除被收藏或手工创建的 QA）。
- 在同一个 SQLite 事务中执行，保证原子性。

---

### 修正 6：向量 Key 生成 — 改用 INSERT 自增（修复 last_insert_rowid 失效）

**风险**：`UPDATE vector_key_seq SET next_key = next_key + 1` 后调用 `last_insert_rowid()` 返回的是上一行 INSERT 的值，`UPDATE` 不会刷新它，产生大面积 key 冲突。

**修正**：改用单行自增 INSERT 模式
```sql
CREATE TABLE vector_key_seq (
    id    INTEGER PRIMARY KEY AUTOINCREMENT
);
```
```rust
pub fn next_vector_key(&self) -> Result<i64, String> {
    self.db.execute(
        "INSERT INTO vector_key_seq DEFAULT VALUES",
        [],
    ).map_err(|e| format!("分配向量 key 失败: {}", e))?;
    Ok(self.db.last_insert_rowid())
}
```
- `last_insert_rowid()` 对 INSERT 生效，保证唯一递增。
- 定期清理：`DELETE FROM vector_key_seq WHERE id < (SELECT MAX(id) - 100000 FROM vector_key_seq)`（保留最近 10 万条）

---

### 修正 7：两步摄入 Step 1 — LLM 语义分析为主引擎 + Rust 本地提取为降级

**风险**：将 Step 1 退化为纯 Rust 词频/正则匹配，丢失 llm_wiki 核心的语义实体/概念提炼能力，导致 Step 2 LLM 编译质量大幅下降。

**修正**：双引擎配置

| 引擎 | 角色 | 启用条件 |
|------|------|---------|
| **LLM 语义分析**（主引擎） | 命名实体识别、关键概念提炼、交叉引用发现、矛盾检测 | `enable_kb_compilation = true` 且 LLM 可用 |
| **Rust 本地提取**（降级备用） | TF-IDF 关键词、标题层级树、词频统计 | LLM 不可用或超时 |

- 默认启用 LLM 引擎（当 `enable_kb_compilation = true` 时）。
- LLM 超时（30s，可配置项 `llm_analysis_timeout`）或无可用 LLM 时自动降级为 Rust 引擎。
- `DocumentAnalysis` 结构兼容两种引擎输出：LLM 版本包含 `entities/concepts/connections` 语义字段；Rust 版本仅包含 `headings/keywords` 统计字段。

---

### 修正 8：增补 ingest_cache 表定义 + 历史 QA 迁移

**风险**：缺少 `ingest_cache` 表定义导致 SHA256 衍生文件映射不可追踪；迁移脚本仅创建根节点，忽略已有 `session_qa_records` 历史数据。

**修正 A — ingest_cache 表**：
```sql
CREATE TABLE ingest_cache (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project         TEXT NOT NULL,
    source_identity TEXT NOT NULL,
    sha256          TEXT NOT NULL,
    files_written   TEXT NOT NULL DEFAULT '[]',  -- JSON 数组: ["wiki_pages.slug", ...] 记录本次 ingest 生成/更新的 wiki 页面 slug。缓存命中时逐文件检查磁盘存在性（文件被用户删除后缓存自动失效）
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project, source_identity, sha256)     -- 含 sha256：文件变更后新增条目，不覆盖旧记录
);
```

**修正 B — 历史 QA 迁移**（替换原有的单纯根节点创建，拆为两步确保原子性）：

```sql
-- Step 1: 为每个有 QA 的 session 创建大纲根节点（如果尚未存在）
INSERT INTO outline_nodes (session_id, parent_id, content, sort_order)
SELECT DISTINCT qa.session_id, NULL, '', 0
FROM session_qa_records qa
WHERE qa.session_id NOT IN (
    SELECT DISTINCT session_id FROM outline_nodes WHERE parent_id IS NULL
);

-- Step 2: 将已有的历史 QA 转为根节点下的第一层子节点
INSERT INTO outline_nodes (session_id, parent_id, content, sort_order, question_id)
SELECT
    qa.session_id,
    root.id AS parent_id,
    qa.question_text AS content,
    CAST(ROW_NUMBER() OVER (PARTITION BY qa.session_id ORDER BY qa.id) AS REAL),
    qa.id AS question_id
FROM session_qa_records qa
JOIN outline_nodes root ON root.session_id = qa.session_id AND root.parent_id IS NULL;
```
- Step 1 确保根节点存在，与 Step 2 在同一个事务中执行。
- 使用 `ROW_NUMBER() OVER (PARTITION BY qa.session_id ORDER BY qa.id)` 替代 `CAST(qa.sort_order AS REAL)`，避免因旧数据中 `sort_order` 重复导致排序混乱。

---

### 修正 9：多视图大纲变更全局事件广播（防编辑冲突）

**风险**：系统支持多窗口/多视图（大纲树、问答模式、脑图视图），一个视图中修改大纲后其他视图无刷新通知，导致缓存旧数据、覆盖冲突。

**修正**：后端变更 API 成功后通过 Tauri 全局事件总线广播：

```rust
// 在 update_outline_node / delete_node_subtree / move_node 成功后：
app_handle.emit("outline:changed", session_id)?;
```

```typescript
// OutlineContext.tsx — 初始化时注册监听
import { listen } from "@tauri-apps/api/event";

useEffect(() => {
  const unlisten = listen<number>("outline:changed", (event) => {
    if (event.payload === sessionId) {
      refreshTree(); // 自动重新拉取 get_outline_tree
    }
  });
  return () => { unlisten.then(fn => fn()); };
}, [sessionId]);
```

- 事件 payload 为 `session_id`（number），接收端判断是否匹配当前 session 再刷新。
- `app_handle.emit` 可从 `tauri::AppHandle` 或 `tauri::State` 获取。
- 命令函数签名需增加 `app: tauri::AppHandle` 参数（Tauri 自动注入）。

---

### 修正 10：大纲节点 ASR 语音输入支持

**风险**：大纲节点 `content`/`note` 仅声明为普通文本域，与系统现有的腾讯云 ASR 语音转录功能割裂。高频编辑时用户不能像调研助手问答界面那样直接用语音输入。

**前置条件**：需要在 `@tauri-apps/api` 或新建 ASR 封装中提供统一的语音录制 API（当前 `tauri-commands.ts` 中无 `startWhisperRecording`，实际使用腾讯云 ASR）

**ASR 封装建议**：
```typescript
// src/lib/audio.ts — 统一 ASR 录制接口（不直接暴露 Whisper 名称）
export async function startAudioRecording(): Promise<void> {
  return invoke("start_whisper_recording");
}

export async function stopAudioRecording(): Promise<{ text: string }> {
  return invoke("stop_whisper_recording");
}
```

**大纲编辑器接入**：
```typescript
// OutlineNode.tsx
import { startAudioRecording, stopAudioRecording } from "../lib/audio";

function NodeEditor({ node, onUpdate }) {
  const [recording, setRecording] = useState(false);

  const handleVoiceInput = async () => {
    if (!recording) {
      await startAudioRecording();
      setRecording(true);
    } else {
      const result = await stopAudioRecording();
      setRecording(false);
      if (result.text) {
        onUpdate(node.id, node.content + (node.content ? "\n" : "") + result.text);
      }
    }
  };
  // ...
}
```

**全局录音状态锁**：同一时间只允许一个录音会话。当大纲编辑器正在录音时，调研助手的录音按钮显示"录音中（其他页面）"并禁用。
```typescript
// src/contexts/AudioContext.tsx（新增）
interface AudioContextValue {
  recording: boolean;
  recordingSource: string | null; // "research" | "outline" | null
  startRecording: (source: string) => Promise<void>;
  stopRecording: () => Promise<{ text: string }>;
}
```
- ASR provider 继承当前调研会话的 `selectedAsrProvider`（当从大纲编辑器触发时，若 `selectedAsrProvider` 不存在则使用默认值 `"whisper"`）

| 功能 | 理由 |
|------|------|
| Chrome 扩展剪藏 | 超出知识库模块范围 |
| 本地 HTTP API 服务器 | 无外部工具集成需求 |
| REVIEW 审查系统 | 需产品验证，复杂度高 |
| Louvain 社区检测可视化 | 页面数量不足以支撑 |
| Deep Research (Web 搜索) | 金蝶实施场景不需要 |
| VLM 图片 caption | 额外 >50MB 模型包体积 |
| LLM 自动 wikilink 补全 | 质量不稳定 |
| 长文档 checkpoint | 当前文档通常 ≤ 50 页 |
| Step 2.5 Review Suggestion | 复杂度高，收益不确定 |
| 暂停/恢复单任务队列管理 | MVP 后迭代 |
| 从脑图反向生成 Q&A | 数据不一致风险 |
| LLM 自动从 Q&A 生成大纲 | 质量不可控 |
