# 统一项目管理系统设计方案（修订版）

> **版本**: v0.3（待审核）
> **日期**: 2026-06-03
> **状态**: 设计稿

---

## 1. 问题定义

### 1.1 现状

项目（project）在整个系统中是一个**分散的字符串作用域参数**，缺乏统一管理：

| 问题 | 表现 |
|------|------|
| 无项目实体 | 没有 `projects` 表，project 是各表的 `TEXT` 字段 |
| 无项目切换 UI | `ProjectContext.setProjectId` 零调用 |
| 两套项目系统割裂 | 知识库项目（字符串）与 RiskProject（独立实体）互不关联 |
| 缺少项目结构 | 没有阶段/进度/产品的概念 |
| 导入无项目归属 | `ImportModal` 默认 `"default"` |

### 1.2 目标

1. **统一项目模型** — 所有项目作用域字段统一重构为 `project_id` 外键，不保留旧 `project TEXT` 字段
2. **丰富的项目结构** — 阶段/轻量进度/产品/公共资料
3. **AI 阶段感知** — 每个阶段注入提示词、推荐当前阶段适合使用的功能
4. **全局项目切换** — 侧边栏切换，所有页面数据自动跟随
5. **跨项目搜索** — 检索页不区分项目
6. **归档替代删除** — 项目不可删除，只能归档
7. **风险跟踪** — 一个项目下多个风险跟踪条目

### 1.3 项目管理边界

本方案要做的是**统一项目管理**，不是通用重型 PM 系统。

**要做**：
- 项目实体统一：知识库、调研、风险、产物、AI 上下文都归属同一项目
- 轻量阶段管理：固定实施阶段、计划/实际时间、当前阶段、超期提醒
- 项目上下文：产品、公共资料、阶段状态注入 AI
- 项目概览：阶段时间轴、统计、风险、最近活动

**不做**：
- 任务拆解、负责人分配、任务依赖
- 拖拽甘特图、资源排期、工时统计
- 审批流、多角色权限管理

一句话：KingdeeKB 的项目管理用于组织实施上下文和阶段节奏，帮助 AI 给出更贴近当前项目状态的建议。

---

## 2. 数据模型

### 2.0 数据库边界（硬约束）

所有项目管理相关数据必须位于**同一个 SQLite 数据库**中，只允许通过不同表表达不同业务域，不允许为项目、产物、风险或其他子系统新建独立数据库文件。

**约束**：
- `projects`、`project_phases`、`project_products`、`project_materials` 与所有带 `project_id` 的业务表必须共用同一个数据库连接
- 所有 `project_id REFERENCES projects(id)` 外键只在同一数据库内建立，不做跨数据库引用
- `ProductStore`、`RiskControlStore` 等现有独立存储访问层必须改为接入统一数据库，而不是继续维护独立 DB 文件
- 新增表只能新增到统一数据库中；后续子系统扩展也必须遵守该边界

### 2.1 `projects` 表（核心）

```sql
CREATE TABLE IF NOT EXISTS projects (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,         -- 项目名称
    client_name     TEXT NOT NULL DEFAULT '',      -- 客户名称
    description     TEXT NOT NULL DEFAULT '',      -- 项目描述
    current_phase   TEXT NOT NULL DEFAULT 'survey', -- 当前阶段标识
    status          TEXT NOT NULL DEFAULT 'active', -- active | archived
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### 2.2 `project_phases` 表（阶段定义）

参考金蝶实施交付物的标准阶段：

```sql
CREATE TABLE IF NOT EXISTS project_phases (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    phase_key       TEXT NOT NULL,                -- survey | blueprint | development | testing | go_live | acceptance | closed
    phase_index     INTEGER NOT NULL,             -- 阶段排序
    status          TEXT NOT NULL DEFAULT 'pending', -- pending | current | completed
    planned_start   TEXT,                         -- 计划开始日期
    planned_end     TEXT,                         -- 计划结束日期
    actual_start    TEXT,                         -- 实际开始日期
    actual_end      TEXT,                         -- 实际完成日期
    UNIQUE(project_id, phase_key),
    UNIQUE(project_id, phase_index)
);
```

**轻量时间轴规则**：
- 创建项目时初始化 7 条标准阶段，`survey` 为 `current`，其余为 `pending`
- 阶段推进时，当前阶段写入 `actual_end` 并置为 `completed`，下一阶段写入 `actual_start` 并置为 `current`
- `planned_start/planned_end` 由用户在项目概览或设置页维护
- 超期状态不入库，运行时根据 `planned_end` 和当前日期计算

**标准阶段定义**（固定顺序）：

| phase_key | 阶段名称 | 推荐功能 | AI 提示词注入 |
|-----------|---------|---------|-------------|
| `survey` | 调研 | 调研助手、AI对话、文档导入 | "当前在调研阶段，重点收集客户业务需求" |
| `blueprint` | 蓝图 | 文档生成（蓝图模板）、AI对话 | "当前在蓝图阶段，重点设计系统方案" |
| `development` | 开发 | AI对话、技能体系 | "当前在开发阶段，重点技术实现" |
| `testing` | 测试 | AI对话（测试相关） | "当前在测试阶段，重点测试用例" |
| `go_live` | 上线 | AI对话（上线支持） | "当前在上线阶段，重点切换策略" |
| `acceptance` | 验收 | AI对话（验收支持） | "当前在验收阶段，重点验收标准" |
| `closed` | 结项 | 项目概览、AI对话（结项复盘） | "项目已结项" |

结项阶段只表示业务实施流程结束，不自动等同于归档。真正的后端只读由 `projects.status = 'archived'` 控制，用户可在项目管理中手动归档结项项目。

### 2.3 `project_products` 表（关联金蝶产品）

```sql
CREATE TABLE IF NOT EXISTS project_products (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    product_name    TEXT NOT NULL,                -- 产品名称（如：金蝶云星空、EAS）
    product_version TEXT DEFAULT '',              -- 产品版本
    UNIQUE(project_id, product_name)
);
```

**用途**：AI 对话时根据关联的产品，调用对应的金蝶 API 技能查询资料。

### 2.4 `project_materials` 表（公共资料）

```sql
CREATE TABLE IF NOT EXISTS project_materials (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    description     TEXT DEFAULT '',
    file_path       TEXT NOT NULL,                -- 资料文件路径（独立于文档库存储）
    file_type       TEXT DEFAULT '',              -- pdf | docx | xlsx | ...
    text_content    TEXT DEFAULT '',              -- 上传时自动抽取的纯文本，供 AI 按需读取
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
```

**区别于文档库**：公共资料是项目级的参考文件（如客户组织架构、接口清单、项目约束说明），AI 在对话时可检索，但不进入知识库搜索索引。合同和 SOW 仍走文档库导入，供风险跟踪和搜索使用。

### 2.5 索引

所有 `project_id` 外键字段显式建索引，按 project_id 列表/筛选是大流量查询：

```sql
CREATE INDEX IF NOT EXISTS idx_documents_project_id ON documents(project_id);
CREATE INDEX IF NOT EXISTS idx_wiki_pages_project_id ON wiki_pages(project_id);
CREATE INDEX IF NOT EXISTS idx_products_project_id ON products(project_id);
CREATE INDEX IF NOT EXISTS idx_research_sessions_project_id ON research_sessions(project_id);
CREATE INDEX IF NOT EXISTS idx_risk_projects_project_id ON risk_projects(project_id);
CREATE INDEX IF NOT EXISTS idx_raw_sources_project_id ON raw_sources(project_id);
CREATE INDEX IF NOT EXISTS idx_ingest_cache_project_id ON ingest_cache(project_id);
CREATE INDEX IF NOT EXISTS idx_analysis_cache_project_id ON analysis_cache(project_id);
CREATE INDEX IF NOT EXISTS idx_deletion_outbox_project_id ON deletion_outbox(project_id);
CREATE INDEX IF NOT EXISTS idx_knowledge_graph_project_id ON knowledge_graph(project_id);
CREATE INDEX IF NOT EXISTS idx_verification_cache_project_id ON verification_cache(project_id);
CREATE INDEX IF NOT EXISTS idx_verification_logs_project_id ON verification_logs(project_id);
CREATE INDEX IF NOT EXISTS idx_project_materials_project_id ON project_materials(project_id);
CREATE INDEX IF NOT EXISTS idx_project_phases_project_id ON project_phases(project_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_project_phases_current ON project_phases(project_id) WHERE status = 'current';
```

### 2.6 旧 `project TEXT` 字段移除 + 新 `project_id` 字段改造

所有相关表将旧 `project TEXT` 字段替换为 `project_id INTEGER REFERENCES projects(id)`。以下为目标字段方向，实际实现通过 SQLite 重建表生成目标 schema：

```sql
documents.project_id          INTEGER NOT NULL REFERENCES projects(id)
wiki_pages.project_id         INTEGER NOT NULL REFERENCES projects(id)
raw_sources.project_id        INTEGER NOT NULL REFERENCES projects(id)
products.project_id           INTEGER NOT NULL REFERENCES projects(id)
ingest_queue.project_id       INTEGER NOT NULL REFERENCES projects(id)
research_sessions.project_id  INTEGER NOT NULL REFERENCES projects(id)
ingest_cache.project_id       INTEGER NOT NULL REFERENCES projects(id)
analysis_cache.project_id     INTEGER NOT NULL REFERENCES projects(id)
deletion_outbox.project_id    INTEGER NOT NULL REFERENCES projects(id)
knowledge_graph.project_id    INTEGER NOT NULL REFERENCES projects(id)
verification_cache.project_id INTEGER NOT NULL REFERENCES projects(id)
verification_logs.project_id  INTEGER NOT NULL REFERENCES projects(id)
```

**处理原则**：
- 目标 schema 不保留旧 `project TEXT` 字段
- 不做旧值迁移，不尝试把历史 `project TEXT` 转换为 `project_id`
- 所有数据读写统一以 `project_id` 为唯一项目作用域字段
- SQLite 如需移除旧列，采用重建表方式生成目标 schema
- `verification_cache` / `verification_logs` 属于新增 `project_id` 隔离，不是旧 `project TEXT` 字段替换；重建表时直接生成目标 schema

### 2.7 RiskProject 改造

```sql
CREATE TABLE risk_projects (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    focus_area      TEXT DEFAULT '',
    contract_doc_id INTEGER DEFAULT NULL,
    sow_doc_id      INTEGER DEFAULT NULL,
    created_at      TEXT DEFAULT (datetime('now')),
    UNIQUE(project_id, name)
);
```

**变更要点**：
- `kb_project` 字段移除，不保留旧风险项目字符串作用域
- `project_id` 强制 NOT NULL，新建风险项目时必选项目
- `contract_scope_items` / `project_health_metrics` 中原本指向风险项目的 `project_id` 字段需要重命名为 `risk_project_id`，避免与统一项目 `projects.id` 混淆
- `contract_doc_id` / `sow_doc_id`：继续指向 `documents` 表（文档库中的导入文档），**不** 改走 `project_materials`
  - 理由：合同和 SOW 是知识库的一部分，应可搜索、可检索
  - `project_materials` 仅用于项目级的参考文件（如需求说明书、接口文档），不进入文档库索引

风险子表目标字段：

```sql
contract_scope_items.risk_project_id  INTEGER NOT NULL REFERENCES risk_projects(id)
project_health_metrics.risk_project_id INTEGER NOT NULL REFERENCES risk_projects(id)
```

### 2.8 聊天附件作用域

旧实现中 `chat-attachments:{session_id}` 使用字符串项目作用域隔离聊天附件。统一 `project_id INTEGER` 后，不再允许把 `chat-attachments:*` 写入任何项目字段。

**目标机制**：
- 聊天附件仍写入 `documents/chunks/raw_sources` 等同一套知识库表，并使用当前项目的 `project_id`
- `documents` 增加 `document_scope TEXT NOT NULL DEFAULT 'knowledge'`，取值：`knowledge | chat_attachment`
- `documents` 增加 `chat_session_id TEXT DEFAULT NULL`，仅 `document_scope = 'chat_attachment'` 时写入当前会话 ID
- 普通知识库浏览、跨项目检索、统计默认只包含 `document_scope = 'knowledge'`
- Agent/RAG 检索当前会话附件时，额外限定 `document_scope = 'chat_attachment' AND chat_session_id = 当前会话 ID`
- 禁止使用负数 `project_id`、特殊字符串或独立数据库模拟附件作用域

这样既保留会话附件隔离，又保证 `project_id` 始终只表达真实项目归属。

### 2.9 数据关系总览

```
projects
  ├── project_phases (0..N 阶段)
  ├── project_products (0..N 关联产品)
  ├── project_materials (0..N 公共资料)
  ├── documents (0..N 文档, project_id FK)
  ├── wiki_pages (0..N, project_id FK)
  ├── knowledge_graph (0..N 图谱边, project_id FK)
  ├── raw_sources (0..N, project_id FK)
  ├── products (0..N 产物, project_id FK)
  ├── research_sessions (0..N, project_id FK)
  │   ├── session_qa_records (0..N, 通过 session_id 间接归属项目)
  │   └── outline_nodes (0..N, 通过 session_id 间接归属项目)
  ├── verification_cache / verification_logs (0..N 校验记录, project_id FK)
  ├── deletion_outbox (0..N 删除补偿记录, project_id FK)
  └── risk_projects (0..N 风险跟踪, project_id FK)
```

---

## 3. 后端命令

### 3.1 新增 `commands/project.rs`

```rust
// ── 查询 ──
async fn list_projects(state, include_archived: bool) -> Result<Vec<ProjectSummary>, String>
async fn get_project(state, id: i64) -> Result<ProjectDetail, String>
async fn get_project_activity(state, id: i64, limit: i64) -> Result<Vec<ProjectActivity>, String>

// ── 管理 ──
async fn create_project(state, name, client_name?, description?, product_names?) -> Result<i64, String>
async fn update_project(state, id, name?, client_name?, description?) -> Result<(), String>
async fn archive_project(state, id) -> Result<(), String>    // 归档，不可删除
async fn unarchive_project(state, id) -> Result<(), String>

// ── 阶段 ──
async fn advance_project_phase(state, id) -> Result<PhaseInfo, String>  // 推进到下一阶段
async fn set_project_phase(state, id, phase_key) -> Result<PhaseInfo, String> // 手动设置阶段，仅允许向后跳转
async fn get_project_phases(state, id) -> Result<Vec<PhaseInfo>, String>
async fn update_project_phase_plan(state, project_id, phase_key, planned_start?, planned_end?) -> Result<PhaseInfo, String>

// ── 产品 ──
async fn set_project_products(state, id, product_names: Vec<String>) -> Result<(), String>
async fn get_project_products(state, id) -> Result<Vec<String>, String>

// ── 公共资料 ──
async fn add_project_material(state, id, title, description?, file_path) -> Result<i64, String>
async fn remove_project_material(state, material_id) -> Result<(), String>
async fn list_project_materials(state, id) -> Result<Vec<MaterialInfo>, String>
async fn search_project_materials(state, id, query) -> Result<Vec<MaterialMatch>, String> // 供 AI 工具按需取项目资料上下文
```

### 3.2 其他命令改造

| 命令 | 变更 |
|------|------|
| `ingest_text/file/directory` | 参数改为 `project_id: i64`，替代 `project: String` |
| `enqueue_ingestion` | 参数改为 `project_id: i64`，导入队列归属项目 |
| `transcribe_and_ingest_video` | 参数改为 `project_id: i64`，视频转写导入归属项目 |
| `list_documents / delete_document / delete_documents_batch / get_stats` | 参数改为 `project_id: i64` |
| `hybrid_search` / `bm25_search` | 命令签名保留 `project_id: Option<i64>`；检索页传 `None` 搜索全部项目，Agent/RAG 传 `Some(project_id)` 限定当前项目 |
| `save_chat_memory` | 参数改为 `project_id: i64`，聊天记忆归属项目 |
| `list_products / delete_product / export_product / regenerate_product` | 参数改为 `project_id: i64` |
| `create_research_session / list_research_sessions` | 参数改为 `project_id: i64` |
| `smart_fill / generate_doc / generate_recipe_doc` | 参数改为 `project_id: i64`，文档生成上下文和生成产物归属当前项目 |
| `list_wiki_pages / get_wiki_page_by_slug / search_wikilink_candidates / get_wikilink_targets / get_backlinks` | 参数改为 `project_id: i64` |
| `build_knowledge_graph / traverse_graph / get_graph_neighbors / get_graph_stats / graph_expand_search` | 参数改为 `project_id: i64` |
| `run_verification / list_verification_logs / get_verification_cache` | 参数改为 `project_id: i64`，验证结果按项目隔离 |
| `create_risk_project` | 参数改为统一项目 `project_id: i64`，移除 `kb_project` |
| `add_scope_item / list_scope_items / check_scope_creep / record_health_metric / get_project_health` | 参数中的风险项目 ID 改名为 `risk_project_id: i64`；命令内通过 `risk_projects.project_id` 校验统一项目归属 |

### 3.3 归档写保护（后端强制约束）

项目归档后，所有写操作**必须**在命令层校验 `projects.status != 'archived'`。

**受保护的写命令清单**：

| 子系统 | 命令 | 校验方式 |
|--------|------|----------|
| 项目管理 | `update_project / set_project_products` | 检查传入 `project_id` 的项目状态 |
| 导入 | `ingest_text / ingest_file / ingest_directory / enqueue_ingestion / transcribe_and_ingest_video` | 检查传入 `project_id` 的项目状态 |
| 文档 | `delete_document / delete_documents_batch` | 检查文档归属项目状态 |
| Wiki | `approve_wiki_page / update_wiki_page` | 检查 Wiki 页归属项目的状态 |
| 图谱 | `build_knowledge_graph` | 检查传入 `project_id` 的项目状态 |
| 产物 | `regenerate_product / delete_product` | 检查产品归属项目的状态 |
| 调研 | `create_research_session / save_qa_record / ...` | 检查调研会话归属项目 |
| 风险 | `create_risk_project` | 检查传入统一项目 `project_id` |
| 风险子项 | `add_scope_item / check_scope_creep / record_health_metric / ...` | 先用 `risk_project_id` 查询 `risk_projects.project_id`，再检查统一项目状态 |
| 验证 | `run_verification / write_verification_log / update_verification_cache` | 检查传入 `project_id` 的项目状态 |
| AI 记忆 | `save_chat_memory` | 检查传入 `project_id` 的项目状态 |
| 公共资料 | `add_project_material / remove_project_material` | 检查 `project_id` |
| 阶段 | `advance_project_phase / set_project_phase / update_project_phase_plan` | 直接检查项目状态 |

**实现方式**——新增通用辅助函数：

```rust
// 在 project_store.rs 或命令层
fn ensure_project_active(store: &MetadataStore, project_id: i64) -> Result<(), String> {
    let project = store.get_project(project_id)?;
    if project.status == "archived" {
        return Err("项目已归档，不可修改".to_string());
    }
    Ok(())
}
```

各命令入口调用：`ensure_project_active(&metadata, project_id)?;`

### 3.4 数据结构

```rust
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub client_name: String,
    pub current_phase: String,
    pub status: String,           // active | archived
    pub document_count: i64,
    pub wiki_count: i64,
    pub product_count: i64,
    pub risk_count: i64,
    pub created_at: String,
}

pub struct ProjectDetail {
    pub id: i64,
    pub name: String,
    pub client_name: String,
    pub description: String,
    pub current_phase: PhaseInfo,
    pub phases: Vec<PhaseInfo>,
    pub products: Vec<String>,
    pub document_count: i64,
    pub wiki_count: i64,
    pub risk_count: i64,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

pub struct PhaseInfo {
    pub phase_key: String,
    pub phase_name: String,
    pub phase_index: i32,
    pub status: String,           // pending | current | completed
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    pub actual_start: Option<String>,
    pub actual_end: Option<String>,
    pub overdue_status: String,   // on_track | due_soon | overdue | none
    pub overdue_days: i64,
}

pub struct MaterialInfo {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub file_path: String,
    pub file_type: String,
    pub created_at: String,
}

pub struct MaterialMatch {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub text_content: String,
    pub score: f32,
}

pub struct ProjectActivity {
    pub activity_type: String,      // document | wiki | product | research | risk | phase
    pub title: String,
    pub description: String,
    pub occurred_at: String,
}
```

### 3.5 约束

| 操作 | 约束 |
|------|------|
| 归档项目 | 设置 `status = 'archived'`，所有功能只读 |
| 归档后写入 | 后端强制校验，拒绝所有写操作 |
| 创建项目 | 事务写入 `projects`、初始化 7 条 `project_phases`、写入 `project_products`，任一失败整体回滚 |
| 重命名项目 | 新名称唯一 |
| 项目同名产品 | `UNIQUE(project_id, product_name)` |
| 阶段推进 | 原子操作：当前阶段 `status='completed'`、写入 `actual_end`；下一阶段 `status='current'`、写入 `actual_start`；同步更新 `projects.current_phase` |
| 手动阶段跳转 | 仅允许跳到更靠后的阶段；目标阶段之前全部置为 `completed`，目标阶段置为 `current`，之后置为 `pending`；同步更新 `projects.current_phase` |
| 阶段计划时间 | `planned_start` 和 `planned_end` 同时存在时，必须满足 `planned_start <= planned_end` |

---

## 4. 前端设计

### 4.1 全局项目切换器 — `ProjectSwitcher`

**位置**：Layout 侧边栏，Logo 与导航之间

```
┌─────────────────────┐
│ 实施顾问AI助手       │
│                     │
│ ┌─────────────────┐ │
│ │ 📁 金蝶ERP项目  │ │  ← 当前项目（单击切到概览）
│ │  调研阶段       │ │  ← 当前阶段标识
│ │                 ▼│ │
│ │ ┌──────────────┐ │ │
│ │ │ ✅ 金蝶ERP    │ │ │
│ │ │ 企业版        │ │ │
│ │ │ 旗舰版        │ │ │
│ │ │ 📦 旗舰版(归档)│ │ │
│ │ │────────────── │ │ │
│ │ │ + 新建项目    │ │ │
│ │ │ 管理项目      │ │ │
│ │ └──────────────┘ │ │
│ └─────────────────┘ │
├─────────────────────┤
│  概览               │
│  知识库             │
│  检索（全部项目）    │
│  AI 对话            │
│  调研助手           │
│  风险把控           │
│  ...                │
└─────────────────────┘
```

**交互**：
- 切换项目后所有页面（知识库/对话/调研/风险等）数据自动切换
- 检索页标注"全部项目"表示不区分项目
- 项目数量 0 时显示引导创建

**概览页（Home.tsx）改造**：显示项目仪表盘——当前阶段、阶段进度条、文档数、风险项数、最近活动。

### 4.2 新建项目流程

创建项目时的表单：

```
┌──────────────────────────────┐
│ 新建项目                     │
│                              │
│ 项目名称 *    _____________  │
│ 客户名称      _____________  │
│ 项目描述      _____________  │
│                              │
│ 关联产品（AI 据此查资料）    │
│ ☑ 金蝶云星空                 │
│ ☐ 金蝶EAS                    │
│ ☐ 金蝶K/3 WISE               │
│ ☐ 金蝶KIS                    │
│ ☐ 自定义...  _____________   │
│                              │
│ 初始阶段：调研（固定）       │
│                              │
│ [取消]  [创建项目]           │
└──────────────────────────────┘
```

创建成功后自动切换到新项目。

### 4.3 AI 阶段感知

**当前阶段信息注入 Agent**：

```typescript
// Chat.tsx
const { currentPhase, projectProducts } = useProject()

// 构建系统提示词时注入阶段信息
const phasePrompt = `当前项目处于"${currentPhaseName}"阶段。
可用功能：${enabledFeatures.join("、")}
关联产品：${projectProducts.join("、")}
阶段计划结束：${currentPhase?.plannedEnd ?? "未设置"}
阶段状态：${currentPhase?.overdueStatusText ?? "正常"}
${phaseSpecificPrompt}`

// 阶段切换时主动提醒
useEffect(() => {
  if (phaseJustAdvanced) {
    toast(`项目已进入"${newPhaseName}"阶段`)
  }
}, [phaseJustAdvanced])
```

**AI 主动提示**：当阶段推进时，AI 对用户消息响应时根据阶段附加上下文建议。例如：

> 当前在**调研阶段**，您可以使用调研助手来记录客户访谈内容。[打开调研助手 →]

当阶段超期时，AI 优先提醒实施节奏风险。例如：

> 当前调研阶段已超期 2 天，建议先补齐调研纪要、未确认需求和蓝图准备材料。

### 4.4 阶段管理 UI

概览页新增轻量阶段时间轴：

```
┌──────────────────────────────────────────┐
│ 项目进度                                  │
│                                          │
│ ●────○────○────○────○────○────○          │
│ 调研   蓝图   开发   测试   上线   验收   结项 │
│ 06/01-06/10                              │
│ 当前阶段：调研 · 已超期 2 天              │
│                                          │
│ 计划结束：2026-06-10                     │
│ 实际开始：2026-06-01                     │
│                                          │
│ [推进到下一阶段]                          │
│                                          │
│ 注意：推进后无法回退                     │
└──────────────────────────────────────────┘
```

即将超期时显示黄色标记：

```
┌──────────────────────────────────────────┐
│ 项目进度                                  │
│                                          │
│ ●────○────○────○────○────○────○          │
│ 调研   蓝图   开发   测试   上线   验收   结项 │
│ ⚠ 即将超期 · 距计划结束还有 2 天          │
│                                          │
│ 计划结束：2026-06-10                     │
│ 实际开始：2026-05-28                     │
│                                          │
│ [推进到下一阶段]                          │
└──────────────────────────────────────────┘
```

**推进逻辑**：
- 只能单向推进（调研→蓝图→...→结项）
- 手动设置阶段也只能向后跳转，用于补录或纠错，不允许回退
- 推进前检查前置条件（如：调研阶段有调研记录才允许进入蓝图）
- 推进时自动写入 `actual_end` / 下一阶段 `actual_start`
- 推进后 AI 提示词自动更新

**超期提醒规则**：
- `planned_end` 为空：不显示超期状态
- 当前日期 > `planned_end` 且阶段未完成：显示“已超期 N 天”
- 距离 `planned_end` <= 3 天且阶段未完成：显示“即将超期”
- 项目已归档或阶段已完成：不触发超期提醒

### 4.5 侧边栏导航适配

Layout 导航根据当前项目阶段动态显示：

```tsx
const navItems = [
  { to: "/", icon: LayoutDashboard, label: "概览" },
  { to: "/browse", icon: BookOpen, label: "知识库" },
  { to: "/search", icon: Search, label: "检索（全部项目）" },
  { to: "/chat", icon: MessageSquare, label: "AI 对话" },
  { to: "/research", icon: ClipboardList, label: "调研助手", phase: "survey" },
  { to: "/risk", icon: ShieldAlert, label: "风险把控" },
  { to: "/templates", icon: FileEdit, label: "文档生成" },
  { to: "/products", icon: Package, label: "产物管理" },
  { to: "/graph", icon: Network, label: "知识图谱" },
  { to: "/settings", icon: Settings, label: "设置" },
]
```

调研助手在非调研阶段可访问但 AI 提示调整。

### 4.6 ProjectContext 改造

```tsx
interface ProjectContextValue {
  projectId: number | null
  setProjectId: (id: number | null) => void
  projectName: string | null
  currentPhase: PhaseInfo | null
  phases: PhaseInfo[]
  projectProducts: string[]
  loading: boolean                     // 切换/加载项目时的加载状态
  error: string | null                 // 切换/加载失败时的错误信息
  switchProject: (id: number) => Promise<void>
  advancePhase: () => Promise<void>
}
```

前端 `PhaseInfo` 使用 camelCase 字段，由 `src/lib/tauri-commands.ts` 在命令封装层从后端 snake_case 映射得到：

```tsx
interface PhaseInfo {
  phaseKey: string
  phaseName: string
  phaseIndex: number
  status: "pending" | "current" | "completed"
  plannedStart: string | null
  plannedEnd: string | null
  actualStart: string | null
  actualEnd: string | null
  overdueStatus: "none" | "on_track" | "due_soon" | "overdue"
  overdueDays: number
  overdueStatusText: string          // 前端根据 overdueStatus/overdueDays 派生，用于 UI 和 AI 提示词
}
```

### 4.7 设置页 — 项目管理

```
┌──────────────────────────────────────┐
│ 项目管理                             │
│                                      │
│ ┌──── 活跃项目 ────────────────────┐ │
│ │ 项目名   客户   阶段   文档  风险 │ │
│ │ 金蝶ERP  金蝶   调研   12    2   │ │
│ │ 企业版   腾讯   蓝图   8     1   │ │
│ │ [+ 新建项目]    [管理产品]        │ │
│ └──────────────────────────────────┘ │
│                                      │
│ ┌──── 已归档 ──────────────────────┐ │
│ │ 旗舰版   阿里   验收   3     0   │ │
│ └──────────────────────────────────┘ │
│                                      │
│ 归档项目可取消归档                   │
└──────────────────────────────────────┘
```

### 4.8 ImportModal 和公共资料

**ImportModal** 负责导入**文档库文档**（进入 ingestion 管道，建向量索引，可搜索）。

**公共资料**（客户组织架构、接口清单、项目约束说明等）走**独立的上传入口**，在项目设置或项目详情页中上传，存入 `project_materials` 表。合同和 SOW 不走公共资料入口，仍通过 ImportModal 进入文档库。

**AI 对公共资料的检索机制**：公共资料不进知识库向量索引，采用**按需读取**策略：

| 场景 | 机制 |
|------|------|
| AI 对话中引用 | Agent 工具 `search_project_materials(query, project_id)` → 扫描 `project_materials` 表的 title/description 做关键词匹配 → 命中的文件实时读取全文作为上下文注入 |
| 文件预读 | 上传时自动抽取文本存入 `project_materials.text_content TEXT` 字段，避免每次读取都解析原始文件 |
| 语义搜索 | 可选：为 `project_materials.text_content` 建独立的轻量向量索引（与文档库索引分离） |
| 前端查看 | 项目详情页列出所有公共资料，支持下载 |

区别于文档库：
- 文档库：用户导入，走 ingestion 管道，建向量+BM25 索引，可搜索
- 公共资料：项目级配置，仅 AI 对话时按需检索，不进用户搜索结果

### 4.9 跨项目搜索

检索页调用的 `hybrid_search` / `bm25_search` **移除** `project_id` 参数，检索时搜索所有项目的文档。

注意：这只适用于用户主动进入检索页的全局检索。AI 对话、调研助手、文档生成等 Agent/RAG 场景仍必须使用当前 `project_id` 限定知识库范围，避免不同项目资料混入当前项目上下文。

**搜索结果结构变更**——返回结果必须带项目信息，否则前端无法标注来源：

```typescript
// 搜索结果条目
interface SearchResult {
  document_id: number
  chunk_id: number
  content: string
  section_path: string
  score: number
  // 新增字段
  project_id: number
  project_name: string
}
```

后端在搜索时 JOIN `documents.project_id` 获取项目名称，前端在结果列表中标注来源：

```
┌─────────────────────────────────────┐
│ 检索结果（跨项目）                    │
│                                     │
│ 企⋮ 金蝶云星空客户需求分析           │
│   项目：金蝶ERP  |  调研阶段         │
│                                     │
│ 企⋮ 合同条款解读                    │
│   项目：企业版  |  蓝图阶段          │
└─────────────────────────────────────┘
```

### 4.10 项目贯穿边界

| 功能区 | 项目策略 | 说明 |
|--------|----------|------|
| `Import.tsx` 独立导入页 | 必须接入 `ProjectContext` | 移除页面内硬编码项目选项，不再使用 `default/enterprise/flagship` 字符串 |
| `ImportModal.tsx` 导入弹窗 | 必须接入当前 `projectId` | 调用方不再传字符串 project，统一读取数字 `project_id` |
| `Browse/Chat/Research/Products/Graph/Risk` | 必须接入 `project_id` | 项目数据页跟随 ProjectSwitcher 当前项目 |
| `Search.tsx` 检索页 | 全局跨项目检索 | 不传当前 `project_id`，调用搜索命令时传 `None`；结果必须标注来源项目 |
| `Templates.tsx` 模板库 | 保持全局 | 模板目录是全局资源；生成产物时才写入当前项目 |
| `Skills.tsx` 技能系统 | 保持全局 | 技能跨项目复用；AI 使用技能时注入当前项目上下文 |
| `Settings.tsx` 基础设置 | 全局 + 项目管理分区 | LLM/ASR/Embedding 仍是全局；新增项目管理 section 管理项目 CRUD、归档、产品和公共资料 |
| `outline_nodes / session_qa_records` | 通过调研会话间接归属项目 | 命令层必须校验 `session_id` 所属 `research_sessions.project_id` 与当前项目一致 |
| 聊天附件 | 使用 `documents.document_scope = 'chat_attachment'` + `chat_session_id` | 不进入普通跨项目搜索；仅当前会话 Agent 可检索，禁止继续使用 `chat-attachments:{session_id}` 作为项目字段 |

---

## 5. 文件变更清单

### 后端

| 文件 | 变更 |
|------|------|
| `src-tauri/src/commands/project.rs` | **新建**：项目管理命令 |
| `src-tauri/src/commands/mod.rs` | 新增 `pub mod project` |
| `src-tauri/src/lib.rs` | 注册项目管理命令 |
| `src-tauri/src/app_state.rs` | 注入 `ProjectStore` 或项目存储访问能力 |
| `src-tauri/src/services/project_store.rs` | **新建**：项目存储层 |
| `src-tauri/src/commands/ingestion.rs` | 参数 `project: String` → `project_id: i64` |
| `src-tauri/src/commands/ingestion_queue.rs` | 参数 `project: String` → `project_id: i64` |
| `src-tauri/src/commands/document.rs` | 同左 |
| `src-tauri/src/commands/search_llm.rs` | 搜索命令保留 `project_id: Option<i64>`；检索页传 `None`，Agent/RAG 传当前项目 |
| `src-tauri/src/commands/wiki_page.rs` | 参数 `project: String` → `project_id: i64` |
| `src-tauri/src/commands/knowledge_graph.rs` | 参数 `project: String` → `project_id: i64` |
| `src-tauri/src/commands/product.rs` | 参数改为 `project_id` |
| `src-tauri/src/commands/template_doc.rs` | 文档生成/智能填充命令参数改为 `project_id` |
| `src-tauri/src/commands/risk_blueprint.rs` | 移除 `kb_project`，改用 `project_id` |
| `src-tauri/src/commands/research.rs` | 参数改为 `project_id` |
| `src-tauri/src/commands/media.rs` | 视频转写导入参数改为 `project_id` |
| `src-tauri/src/services/wiki_page.rs` | 表结构和 Store 方法改为 `project_id` |
| `src-tauri/src/services/knowledge_graph.rs` | 表结构和 Store 方法改为 `project_id` |
| `src-tauri/src/services/ingestion.rs` | 摄入服务内部参数 `project` → `project_id`，写入 `documents.document_scope` |
| `src-tauri/src/services/ingestion_pipeline.rs` | 摄入管道传递 `project_id` 和文档作用域 |
| `src-tauri/src/services/raw_source.rs` | 表结构和 Store 方法改为 `project_id` |
| `src-tauri/src/services/analysis_cache.rs` | 表结构和 Store 方法改为 `project_id` |
| `src-tauri/src/services/ingest_cache.rs` | 表结构和 Store 方法改为 `project_id` |
| `src-tauri/src/services/product_store.rs` | 接入统一数据库；表结构和 Store 方法改为 `project_id`，不再使用独立 DB 文件 |
| `src-tauri/src/services/research_session.rs` | 表结构和 Store 方法改为 `project_id` |
| `src-tauri/src/services/outline.rs` | 大纲节点通过 `session_id` 间接归属项目，命令层校验会话项目一致性 |
| `src-tauri/src/services/research_indexer.rs` | 调研索引逻辑校验会话项目一致性 |
| `src-tauri/src/services/risk_control.rs` | 接入统一数据库；移除 `kb_project`，风险跟踪归属统一项目，不再使用独立 DB 文件 |
| `src-tauri/src/services/rig_tool.rs` | Agent 工具检索改为接收 `project_id`，并支持当前会话附件过滤 |
| `src-tauri/src/services/doc_generator.rs` | 文档生成上下文改为读取当前 `project_id` |
| `src-tauri/src/services/smart_completion.rs` | 智能填充上下文改为读取当前 `project_id` |
| `src-tauri/src/services/question_recommend.rs` | 问题推荐上下文改为读取当前 `project_id` |
| `src-tauri/src/services/hybrid_search.rs` | 支持检索页全局搜索；Agent/RAG 保留当前项目过滤；结果返回项目信息 |
| `src-tauri/src/services/bm25_service.rs` | 支持检索页全局搜索；Agent/RAG 保留当前项目过滤；结果返回项目信息 |
| `src-tauri/src/services/rig_agent.rs` | 注入项目阶段、产品、计划时间、超期状态 |
| `src-tauri/src/services/memory.rs` | 聊天记忆改为 `project_id` 归属 |
| `src-tauri/src/services/verification_cache.rs` | 增加 `project_id` 隔离 |
| `src-tauri/src/services/verification_log.rs` | 增加 `project_id` 隔离 |
| `src-tauri/src/services/metadata.rs` | 建表 + 重构项目作用域字段为 `project_id`，不保留旧 `project TEXT` |

### 前端

| 文件 | 变更 |
|------|------|
| `src/contexts/ProjectContext.tsx` | **重构**：`projectId: number`，新增阶段/产品字段 |
| `src/components/ProjectSwitcher.tsx` | **新建**：侧边栏项目切换下拉 |
| `src/components/Layout.tsx` | 插入 ProjectSwitcher，导航阶段感知 |
| `src/App.tsx` | 确认 Provider 层级仍满足 ProjectContext → AgentContext 依赖 |
| `src/contexts/AgentContext.tsx` | 适配 `projectId: number | null`，发送 Agent 请求时传 `project_id` |
| `src/pages/Home.tsx` | **改造**：项目仪表盘（阶段进度/统计） |
| `src/pages/Import.tsx` | 移除内部项目选择器，改读 ProjectContext |
| `src/pages/Chat.tsx` | 注入阶段提示词 + 主动建议 |
| `src/pages/Search.tsx` | 明确"全部项目" |
| `src/pages/Browse.tsx` | 适配 `project_id: number` |
| `src/pages/Settings.tsx` | 项目管理 section |
| `src/pages/RiskControl.tsx` | 适配新模型 |
| `src/pages/ResearchAssistant.tsx` | 适配 `project_id: number`，调研会话归属当前项目 |
| `src/pages/KnowledgeGraph.tsx` | 适配 `project_id: number` |
| `src/pages/Templates.tsx` | 保持模板全局，生成产物时使用当前项目 |
| `src/pages/Wizard.tsx` | 文档生成向导使用当前 `project_id`，生成结果归属当前项目 |
| `src/pages/Skills.tsx` | 保持技能全局，执行/AI 使用时注入当前项目上下文 |
| `src/components/Spotlight.tsx` | 全局入口调用 Agent 时注入当前 `project_id` |
| `src/components/ImportModal.tsx` | 读上下文 project_id |
| `src/lib/tauri-commands.ts` | 新增/修改命令封装 |
| `src/lib/wiki-commands.ts` | Wiki/图谱命令封装改为 `project_id: number` |

---

## 6. 风险与注意事项

- **单数据库边界**：禁止新增独立 DB 文件；`ProductStore` 等历史独立存储必须迁入统一数据库
- **聊天附件作用域**：`chat-attachments:{session_id}` 字符串作用域必须迁移为 `document_scope + chat_session_id`，否则 `project_id INTEGER` 改造后附件隔离会失效
- **索引重建**：`project TEXT` 改为 `project_id INTEGER` 后，BM25/Tantivy 索引和相关缓存索引需要按目标 schema 重建
- **SQLite 重建表**：删除旧列、修改 UNIQUE 约束、新增 NOT NULL 字段均通过重建表完成，不使用临时兼容字段长期保留旧 schema
- **推进阶段不可回退**：明确告知用户，可考虑加二次确认
- **归档项目只读**：AI 对话可查看历史但不可导入新内容
- **搜索不分项目**：检索结果可能来自不同项目，需标注来源项目名
- **`project_materials` 与文档库的区别**：公共资料（客户组织架构、接口清单等）不走 ingestion 管道，不进入向量索引，仅在 AI 对话时按需查询；合同和 SOW 仍进入文档库
- **轻量项目管理边界**：只做阶段时间轴和超期提醒，不做任务级甘特、负责人、依赖、工时和资源排期
