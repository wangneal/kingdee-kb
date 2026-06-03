# 统一项目管理系统实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 将 KingdeeKB 的字符串项目作用域统一升级为单数据库内的 `projects.id` 外键，并完成项目切换、阶段感知、跨项目搜索和风险跟踪统一归属。

**架构：** 后端先在 `metadata.db` 中建立项目核心表和统一 `project_id` schema，再逐步改造各 Store、命令和搜索索引。前端随后把 `ProjectContext` 从字符串上下文升级为数字项目上下文，最后接入全局 ProjectSwitcher、概览页和各业务页面。

**技术栈：** Tauri v2、Rust、rusqlite、React 19、TypeScript、Tailwind CSS v4、lucide-react、Tantivy、usearch。

---

## 实施边界

本计划依据 `docs/superpowers/plans/2026-06-03-project-management-design.md` 执行。

**硬约束：**
- 只允许使用一个 SQLite 数据库文件：`metadata.db`
- 不创建新的业务数据库文件
- 不保留旧 `project TEXT` 作为长期兼容字段
- 不做历史 `project TEXT` 到 `project_id` 的旧值迁移
- 聊天附件不得继续使用 `chat-attachments:{session_id}` 写入项目字段
- 检索页跨项目搜索，Agent/RAG 保持当前项目隔离

**验证命令：**
- 前端类型检查：`pnpm typecheck`
- 前端构建：`pnpm build`
- Rust 检查：`cargo check`（工作目录：`src-tauri`）
- Rust 测试：`cargo test`（工作目录：`src-tauri`）

---

## 文件结构与职责

### 后端新增文件

- `src-tauri/src/services/project_store.rs`：项目核心表、阶段表、产品关联、公共资料、归档状态、项目活跃校验。
- `src-tauri/src/commands/project.rs`：Tauri 项目管理命令，供前端项目切换器、概览页、设置页调用。

### 后端重点修改文件

- `src-tauri/src/services/metadata.rs`：`documents`、`deletion_outbox`、聊天附件字段、项目字段重建。
- `src-tauri/src/services/product_store.rs`：从 `products.db` 迁入 `metadata.db`，`project TEXT` 改为 `project_id`。
- `src-tauri/src/app_state.rs`：移除 `products.db` 路径，注入 `ProjectStore`，所有 Store 指向 `metadata.db`。
- `src-tauri/src/services/wiki_page.rs`、`knowledge_graph.rs`、`raw_source.rs`、`analysis_cache.rs`、`ingest_cache.rs`、`research_session.rs`、`risk_control.rs`：统一 `project_id` 字段和查询参数。
- `src-tauri/src/services/ingestion.rs`、`ingestion_pipeline.rs`、`ingestion_queue.rs`：摄入入口、队列和管道统一传递 `project_id` 与 `document_scope`。
- `src-tauri/src/services/hybrid_search.rs`、`bm25_service.rs`、`rerank.rs`：搜索结果返回项目 ID/名称，并排除聊天附件。
- `src-tauri/src/services/rig_tool.rs`、`rig_agent.rs`、`memory.rs`：Agent 检索、阶段提示词和聊天记忆接入当前项目。
- `src-tauri/src/services/verification_cache.rs`、`verification_log.rs`：新增 `project_id` 隔离。
- `src-tauri/src/commands/*.rs`：所有项目相关命令参数从 `project: String` 改为 `project_id: i64` 或 `Option<i64>`。
- `src-tauri/src/lib.rs`、`src-tauri/src/commands/mod.rs`、`src-tauri/src/services/mod.rs`：注册新增模块。

### 前端新增文件

- `src/components/ProjectSwitcher.tsx`：侧边栏项目切换、新建项目入口、归档项目展示。

### 前端重点修改文件

- `src/lib/tauri-commands.ts`、`src/lib/wiki-commands.ts`：命令封装类型从字符串项目改为数字项目 ID。
- `src/contexts/ProjectContext.tsx`：项目详情、阶段、产品、切换、推进阶段状态管理。
- `src/contexts/AgentContext.tsx`：发送 Agent 请求时传入数字项目 ID。
- `src/components/Layout.tsx`：插入 ProjectSwitcher，检索导航标注全部项目。
- `src/components/ImportModal.tsx`、`src/hooks/useImport.ts`、`src/pages/Import.tsx`：移除硬编码项目字符串，读当前项目。
- `src/pages/Home.tsx`、`Settings.tsx`、`Search.tsx`、`Chat.tsx`、`Browse.tsx`、`ResearchAssistant.tsx`、`RiskControl.tsx`、`Products.tsx`、`KnowledgeGraph.tsx`、`Wizard.tsx`、`Spotlight.tsx`：接入新项目上下文和命令类型。

---

## 任务 1：建立项目核心 Store 和 schema

**文件：**
- 创建：`src-tauri/src/services/project_store.rs`
- 修改：`src-tauri/src/services/mod.rs`
- 修改：`src-tauri/src/app_state.rs`

- [ ] **步骤 1：编写 ProjectStore 数据结构和建表 SQL**

在 `src-tauri/src/services/project_store.rs` 中创建 Store。注释必须使用中文。

```rust
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const PHASE_KEYS: [&str; 7] = [
    "survey",
    "blueprint",
    "development",
    "testing",
    "go_live",
    "acceptance",
    "closed",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub client_name: String,
    pub current_phase: String,
    pub status: String,
    pub document_count: i64,
    pub wiki_count: i64,
    pub product_count: i64,
    pub risk_count: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseInfo {
    pub phase_key: String,
    pub phase_name: String,
    pub phase_index: i32,
    pub status: String,
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    pub actual_start: Option<String>,
    pub actual_end: Option<String>,
    pub overdue_status: String,
    pub overdue_days: i64,
}

pub struct ProjectStore {
    db: Connection,
}

impl ProjectStore {
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let db = Connection::open(db_path).map_err(|e| format!("打开项目数据库失败: {e}"))?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("设置项目数据库忙超时失败: {e}"))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("初始化项目数据库 PRAGMA 失败: {e}"))?;
        let store = Self { db };
        store.ensure_schema()?;
        Ok(store)
    }

    pub fn ensure_schema(&self) -> Result<(), String> {
        self.db.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                client_name TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                current_phase TEXT NOT NULL DEFAULT 'survey',
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS project_phases (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                phase_key TEXT NOT NULL,
                phase_index INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                planned_start TEXT,
                planned_end TEXT,
                actual_start TEXT,
                actual_end TEXT,
                UNIQUE(project_id, phase_key),
                UNIQUE(project_id, phase_index)
            );

            CREATE UNIQUE INDEX IF NOT EXISTS idx_project_phases_current
            ON project_phases(project_id) WHERE status = 'current';

            CREATE TABLE IF NOT EXISTS project_products (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                product_name TEXT NOT NULL,
                product_version TEXT DEFAULT '',
                UNIQUE(project_id, product_name)
            );

            CREATE TABLE IF NOT EXISTS project_materials (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                description TEXT DEFAULT '',
                file_path TEXT NOT NULL,
                file_type TEXT DEFAULT '',
                text_content TEXT DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_project_materials_project_id
            ON project_materials(project_id);
            "
        ).map_err(|e| format!("初始化项目 schema 失败: {e}"))?;
        Ok(())
    }
}
```

- [ ] **步骤 2：添加默认项目初始化方法**

同文件追加默认项目方法。由于不做历史旧值迁移，默认项目只用于新 schema 下的初始可用状态。

```rust
impl ProjectStore {
    pub fn ensure_default_project(&self) -> Result<i64, String> {
        if let Some(id) = self.db.query_row(
            "SELECT id FROM projects WHERE status = 'active' ORDER BY id LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        ).optional().map_err(|e| format!("查询默认项目失败: {e}"))? {
            return Ok(id);
        }

        let tx = self.db.transaction().map_err(|e| format!("开启项目事务失败: {e}"))?;
        tx.execute(
            "INSERT INTO projects (name, client_name, description) VALUES (?1, ?2, ?3)",
            params!["默认项目", "", "系统初始化项目"],
        ).map_err(|e| format!("创建默认项目失败: {e}"))?;
        let project_id = tx.last_insert_rowid();

        for (index, phase_key) in PHASE_KEYS.iter().enumerate() {
            let status = if *phase_key == "survey" { "current" } else { "pending" };
            tx.execute(
                "INSERT INTO project_phases (project_id, phase_key, phase_index, status) VALUES (?1, ?2, ?3, ?4)",
                params![project_id, phase_key, index as i64, status],
            ).map_err(|e| format!("初始化项目阶段失败: {e}"))?;
        }

        tx.commit().map_err(|e| format!("提交默认项目失败: {e}"))?;
        Ok(project_id)
    }
}
```

- [ ] **步骤 3：注册服务模块**

在 `src-tauri/src/services/mod.rs` 增加：

```rust
pub mod project_store;
```

- [ ] **步骤 4：接入 AppState**

在 `src-tauri/src/app_state.rs` 增加字段和初始化：

```rust
use crate::services::project_store::ProjectStore;

pub struct AppState {
    pub project_store: Arc<Mutex<ProjectStore>>,
}
```

将该字段插入现有 `AppState` 结构体字段列表中，位置放在 `metadata` 字段之后，便于项目状态校验优先访问。

在 `AppState::new` 中创建 `ProjectStore`，使用同一个 `db_path = data_dir.join("metadata.db")`：

```rust
let project_store = {
    let store = ProjectStore::new(&db_path)?;
    store.ensure_default_project()?;
    Arc::new(Mutex::new(store))
};
```

在 `Ok(Self {` 初始化字段列表中写入：

```rust
project_store,
```

- [ ] **步骤 5：运行 Rust 检查**

运行：`cargo check`，工作目录：`src-tauri`

预期：如果只新增 Store 且未注册命令，允许出现未使用警告；不允许出现编译错误。

---

## 任务 2：重建 metadata schema 为 `project_id` 体系

**文件：**
- 修改：`src-tauri/src/services/metadata.rs`
- 修改：`src-tauri/src/services/verification_cache.rs`
- 修改：`src-tauri/src/services/verification_log.rs`

- [ ] **步骤 1：修改 DocumentMeta 类型**

在 `metadata.rs` 中将：

```rust
pub project: String,
```

改为：

```rust
pub project_id: i64,
pub project_name: Option<String>,
pub document_scope: String,
pub chat_session_id: Option<String>,
```

- [ ] **步骤 2：替换 documents 建表 SQL**

在 `init_schema` 中把 `documents` 表目标 schema 改为：

```sql
CREATE TABLE IF NOT EXISTS documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    source_path TEXT,
    sha256 TEXT UNIQUE,
    created_at TEXT DEFAULT (datetime('now')),
    project_id INTEGER NOT NULL REFERENCES projects(id),
    document_scope TEXT NOT NULL DEFAULT 'knowledge',
    chat_session_id TEXT DEFAULT NULL,
    raw_source_identity TEXT DEFAULT NULL
);

CREATE INDEX IF NOT EXISTS idx_documents_project_id ON documents(project_id);
CREATE INDEX IF NOT EXISTS idx_documents_scope_session ON documents(document_scope, chat_session_id);
```

- [ ] **步骤 3：替换 deletion_outbox schema**

目标 SQL：

```sql
CREATE TABLE IF NOT EXISTS deletion_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    status TEXT NOT NULL DEFAULT 'pending',
    error TEXT,
    vector_keys TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_deletion_outbox_project_id ON deletion_outbox(project_id);
```

- [ ] **步骤 4：新增 schema 版本重建入口**

在 `MetadataStore::init_schema` 中加入检测逻辑：如果 `documents` 仍存在 `project` 列，则重建 `documents` / `deletion_outbox`。

```rust
fn column_exists(&self, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = self.db
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| format!("读取表结构失败: {e}"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("读取表字段失败: {e}"))?;
    for name in rows {
        if name.map_err(|e| format!("解析字段名失败: {e}"))? == column {
            return Ok(true);
        }
    }
    Ok(false)
}
```

重建策略：创建新表，保留可直接保留的文档字段，并统一写入 `ensure_default_project()` 返回的默认项目 ID。

- [ ] **步骤 5：修改 insert_document 签名**

将：

```rust
project: Option<&str>,
```

改为：

```rust
project_id: i64,
document_scope: &str,
chat_session_id: Option<&str>,
```

SQL 改为：

```sql
INSERT OR IGNORE INTO documents
(title, source_path, sha256, project_id, document_scope, chat_session_id, raw_source_identity)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
```

- [ ] **步骤 6：为 verification 表增加项目字段**

在 `verification_cache.rs` 目标 schema 中加入：

```sql
project_id INTEGER NOT NULL REFERENCES projects(id)
```

并创建索引：

```sql
CREATE INDEX IF NOT EXISTS idx_verification_cache_project_id
ON verification_cache(project_id);
```

在 `verification_log.rs` 做同样处理。

- [ ] **步骤 7：运行 Rust 检查**

运行：`cargo check`，工作目录：`src-tauri`

预期：由于调用方尚未全部迁移，可能出现参数不匹配错误；记录所有错误，作为任务 3 和任务 4 的输入。

---

## 任务 3：迁移所有 Store 的项目字段与单库边界

**文件：**
- 修改：`src-tauri/src/services/product_store.rs`
- 修改：`src-tauri/src/services/wiki_page.rs`
- 修改：`src-tauri/src/services/knowledge_graph.rs`
- 修改：`src-tauri/src/services/raw_source.rs`
- 修改：`src-tauri/src/services/analysis_cache.rs`
- 修改：`src-tauri/src/services/ingest_cache.rs`
- 修改：`src-tauri/src/services/ingestion_queue.rs`
- 修改：`src-tauri/src/services/research_session.rs`
- 修改：`src-tauri/src/services/risk_control.rs`
- 修改：`src-tauri/src/app_state.rs`

- [ ] **步骤 1：ProductStore 接入 metadata.db**

将 `ProductStore::new(db_path: PathBuf)` 改为接收 `&Path` 并打开 `metadata.db`，不再使用 `products.db`。

目标构造函数：

```rust
impl ProductStore {
    pub fn new(db_path: &std::path::Path) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("打开产物数据库失败: {e}"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("设置产物数据库忙超时失败: {e}"))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("启用产物外键失败: {e}"))?;
        let store = Self { conn };
        store.ensure_schema()?;
        Ok(store)
    }
}
```

在 `app_state.rs` 删除：

```rust
let products_db_path = data_dir.join("products.db");
let products = ProductStore::new(products_db_path)?;
```

替换为：

```rust
let products = ProductStore::new(&db_path)?;
```

- [ ] **步骤 2：ProductMeta 字段迁移**

将 `ProductMeta.project: String` 改为：

```rust
pub project_id: i64,
pub project_name: Option<String>,
```

目标 `products` 表字段：

```sql
project_id INTEGER NOT NULL REFERENCES projects(id)
```

- [ ] **步骤 3：迁移 Wiki、图谱、原始文件、缓存 Store**

所有 `project: String` / `project: &str` 改为 `project_id: i64`。

查询条件示例：

```sql
WHERE project_id = ?1
```

禁止保留：

```sql
WHERE project = ?1
```

- [ ] **步骤 4：迁移 ingestion_queue**

将 `QueueItem.project: String` 改为：

```rust
pub project_id: i64,
```

队列持久化 JSON 字段同步改为 `project_id`。

- [ ] **步骤 5：迁移 research_session 和间接表校验**

`research_sessions.project TEXT` 改为 `project_id INTEGER NOT NULL REFERENCES projects(id)`。

`session_qa_records` 和 `outline_nodes` 不新增 `project_id`，但所有命令入口必须先查询 `research_sessions.project_id`，确认属于当前项目。

- [ ] **步骤 6：迁移 risk_control**

`risk_projects` 目标字段：

```sql
project_id INTEGER NOT NULL REFERENCES projects(id),
name TEXT NOT NULL,
focus_area TEXT DEFAULT '',
contract_doc_id INTEGER DEFAULT NULL,
sow_doc_id INTEGER DEFAULT NULL,
UNIQUE(project_id, name)
```

`contract_scope_items.project_id` 和 `project_health_metrics.project_id` 重命名为 `risk_project_id`。

命令和 Store 方法中统一语义：
- `project_id` 表示统一项目 ID
- `risk_project_id` 表示风险跟踪条目 ID

- [ ] **步骤 7：运行 Rust 检查**

运行：`cargo check`，工作目录：`src-tauri`

预期：Store 层字段不一致错误减少；剩余错误集中在 commands 和 services 调用方。

---

## 任务 4：改造摄入、搜索、附件和 Agent/RAG

**文件：**
- 修改：`src-tauri/src/services/ingestion.rs`
- 修改：`src-tauri/src/services/ingestion_pipeline.rs`
- 修改：`src-tauri/src/services/hybrid_search.rs`
- 修改：`src-tauri/src/services/bm25_service.rs`
- 修改：`src-tauri/src/services/rerank.rs`
- 修改：`src-tauri/src/services/rig_tool.rs`
- 修改：`src-tauri/src/services/rig_agent.rs`
- 修改：`src-tauri/src/services/memory.rs`

- [ ] **步骤 1：定义文档作用域类型**

在摄入服务附近定义：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentScope {
    Knowledge,
    ChatAttachment,
}

impl DocumentScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge",
            Self::ChatAttachment => "chat_attachment",
        }
    }
}
```

- [ ] **步骤 2：摄入函数接收 project_id 和 document_scope**

`ingest_text`、`ingest_file`、`ingest_directory` 目标参数：

```rust
project_id: i64,
document_scope: DocumentScope,
chat_session_id: Option<String>,
```

普通导入调用传：

```rust
DocumentScope::Knowledge,
None,
```

聊天附件调用传：

```rust
DocumentScope::ChatAttachment,
Some(session_id),
```

- [ ] **步骤 3：搜索服务保留 Option 项目过滤**

`hybrid_search` 和 `bm25_search` 目标签名：

```rust
project_id: Option<i64>,
chat_session_id: Option<String>,
```

过滤规则：
- `project_id = Some(id)`：只查当前项目 `document_scope = 'knowledge'`
- `project_id = None`：跨项目查全部 `document_scope = 'knowledge'`
- `chat_session_id = Some(sid)`：额外允许当前会话 `document_scope = 'chat_attachment'`

- [ ] **步骤 4：搜索结果返回项目来源**

将搜索结果结构从：

```rust
pub project: String,
```

改为：

```rust
pub project_id: i64,
pub project_name: String,
```

SQL 查询 JOIN：

```sql
JOIN projects p ON p.id = documents.project_id
```

- [ ] **步骤 5：Agent 阶段上下文注入**

`rig_agent.rs` 构建系统提示词时查询：
- 当前项目名称
- 当前阶段
- 项目产品
- 阶段计划结束
- 超期状态

提示词片段格式：

```text
当前项目：{project_name}
当前阶段：{phase_name}
关联产品：{product_names}
阶段状态：{overdue_status_text}
```

- [ ] **步骤 6：运行 Rust 检查和搜索相关测试**

运行：`cargo check`，工作目录：`src-tauri`

运行：`cargo test hybrid_search bm25`，工作目录：`src-tauri`

预期：搜索服务编译通过，相关测试通过或没有匹配测试时输出 0 tests。

---

## 任务 5：新增项目命令并统一命令层参数

**文件：**
- 创建：`src-tauri/src/commands/project.rs`
- 修改：`src-tauri/src/commands/mod.rs`
- 修改：`src-tauri/src/lib.rs`
- 修改：`src-tauri/src/commands/ingestion.rs`
- 修改：`src-tauri/src/commands/ingestion_queue.rs`
- 修改：`src-tauri/src/commands/document.rs`
- 修改：`src-tauri/src/commands/search_llm.rs`
- 修改：`src-tauri/src/commands/product.rs`
- 修改：`src-tauri/src/commands/research.rs`
- 修改：`src-tauri/src/commands/wiki_page.rs`
- 修改：`src-tauri/src/commands/knowledge_graph.rs`
- 修改：`src-tauri/src/commands/risk_blueprint.rs`
- 修改：`src-tauri/src/commands/template_doc.rs`
- 修改：`src-tauri/src/commands/media.rs`

- [ ] **步骤 1：创建 project 命令模块**

`src-tauri/src/commands/project.rs`：

```rust
use crate::app_state::AppState;
use crate::services::project_store::{PhaseInfo, ProjectSummary};
use tauri::State;

#[tauri::command]
pub async fn list_projects(
    state: State<'_, AppState>,
    include_archived: bool,
) -> Result<Vec<ProjectSummary>, String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.list_projects(include_archived)
}

#[tauri::command]
pub async fn advance_project_phase(
    state: State<'_, AppState>,
    id: i64,
) -> Result<PhaseInfo, String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.ensure_project_active(id)?;
    store.advance_project_phase(id)
}
```

同文件继续实现以下命令函数：`get_project`、`create_project`、`update_project`、`archive_project`、`unarchive_project`、`set_project_phase`、`get_project_phases`、`update_project_phase_plan`、`set_project_products`、`get_project_products`、`add_project_material`、`remove_project_material`、`list_project_materials`、`search_project_materials`。

- [ ] **步骤 2：注册 command 模块**

在 `src-tauri/src/commands/mod.rs` 增加：

```rust
pub mod project;
```

在 `src-tauri/src/lib.rs` 的 invoke handler 中注册：

```rust
commands::project::list_projects,
commands::project::get_project,
commands::project::create_project,
commands::project::update_project,
commands::project::archive_project,
commands::project::unarchive_project,
commands::project::advance_project_phase,
commands::project::set_project_phase,
commands::project::get_project_phases,
commands::project::update_project_phase_plan,
commands::project::set_project_products,
commands::project::get_project_products,
commands::project::add_project_material,
commands::project::remove_project_material,
commands::project::list_project_materials,
commands::project::search_project_materials,
```

- [ ] **步骤 3：统一命令参数类型**

所有项目数据页命令使用：

```rust
project_id: i64
```

全局搜索命令使用：

```rust
project_id: Option<i64>
```

风险子项命令使用：

```rust
risk_project_id: i64
```

禁止新增 `project: String`、`project_id: Option<String>`、`kb_project` 参数。

- [ ] **步骤 4：写保护检查**

在所有写命令入口调用：

```rust
let project_store = state.project_store.lock().map_err(|e| e.to_string())?;
project_store.ensure_project_active(project_id)?;
```

对只拿到 `document_id`、`session_id`、`risk_project_id` 的命令，先查询所属 `project_id`，再调用 `ensure_project_active`。

- [ ] **步骤 5：运行 Rust 检查**

运行：`cargo check`，工作目录：`src-tauri`

预期：命令层参数迁移后无编译错误。

---

## 任务 6：改造前端命令封装和 ProjectContext

**文件：**
- 修改：`src/lib/tauri-commands.ts`
- 修改：`src/lib/wiki-commands.ts`
- 修改：`src/contexts/ProjectContext.tsx`
- 修改：`src/contexts/AgentContext.tsx`

- [ ] **步骤 1：新增前端类型**

在 `tauri-commands.ts` 中新增：

```typescript
export interface PhaseInfo {
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
  overdueStatusText: string
}

export interface ProjectSummary {
  id: number
  name: string
  clientName: string
  currentPhase: string
  status: "active" | "archived"
  documentCount: number
  wikiCount: number
  productCount: number
  riskCount: number
  createdAt: string
}
```

- [ ] **步骤 2：统一 snake_case 到 camelCase 映射**

新增映射函数：

```typescript
function mapPhaseInfo(raw: RawPhaseInfo): PhaseInfo {
  return {
    phaseKey: raw.phase_key,
    phaseName: raw.phase_name,
    phaseIndex: raw.phase_index,
    status: raw.status,
    plannedStart: raw.planned_start,
    plannedEnd: raw.planned_end,
    actualStart: raw.actual_start,
    actualEnd: raw.actual_end,
    overdueStatus: raw.overdue_status,
    overdueDays: raw.overdue_days,
    overdueStatusText: formatOverdueStatus(raw.overdue_status, raw.overdue_days),
  }
}
```

- [ ] **步骤 3：重写 ProjectContext**

目标接口：

```typescript
interface ProjectContextValue {
  projectId: number | null
  setProjectId: (id: number | null) => void
  projectName: string | null
  currentPhase: PhaseInfo | null
  phases: PhaseInfo[]
  projectProducts: string[]
  loading: boolean
  error: string | null
  switchProject: (id: number) => Promise<void>
  advancePhase: () => Promise<void>
  refreshProject: () => Promise<void>
}
```

localStorage 中只保存数字字符串，读取时用 `Number.parseInt`，无效值清空。

- [ ] **步骤 4：AgentContext 适配 number 项目 ID**

将所有发送 Agent 请求的地方从字符串 project 改为数字 `projectId`。

无当前项目时，阻止发送项目型 Agent 请求，并返回前端错误：

```typescript
throw new Error("请先创建或选择项目")
```

- [ ] **步骤 5：运行前端类型检查**

运行：`pnpm typecheck`

预期：命令封装和上下文相关类型无错误；页面调用方错误进入任务 7 处理。

---

## 任务 7：接入前端页面和项目切换 UI

**文件：**
- 创建：`src/components/ProjectSwitcher.tsx`
- 修改：`src/components/Layout.tsx`
- 修改：`src/pages/Home.tsx`
- 修改：`src/pages/Settings.tsx`
- 修改：`src/pages/Import.tsx`
- 修改：`src/components/ImportModal.tsx`
- 修改：`src/hooks/useImport.ts`
- 修改：`src/pages/Search.tsx`
- 修改：`src/pages/Chat.tsx`
- 修改：`src/pages/Browse.tsx`
- 修改：`src/pages/ResearchAssistant.tsx`
- 修改：`src/pages/RiskControl.tsx`
- 修改：`src/pages/Products.tsx`
- 修改：`src/pages/KnowledgeGraph.tsx`
- 修改：`src/pages/Wizard.tsx`
- 修改：`src/components/Spotlight.tsx`

- [ ] **步骤 1：创建 ProjectSwitcher**

组件行为：
- 显示当前项目名和当前阶段
- 下拉列出活跃项目和归档项目
- 切换项目调用 `switchProject(id)`
- 无项目时显示创建入口
- 点击当前项目区域跳转 `/`

核心结构：

```tsx
export default function ProjectSwitcher() {
  const { projectId, projectName, currentPhase, switchProject } = useProject()
  const navigate = useNavigate()
  const [projects, setProjects] = useState<ProjectSummary[]>([])
  const [open, setOpen] = useState(false)

  useEffect(() => {
    listProjects(true).then(setProjects).catch(() => setProjects([]))
  }, [projectId])

  return (
    <div className="border-b border-neutral-200 p-3">
      <button type="button" onClick={() => navigate("/")} className="w-full rounded-md px-2 py-2 text-left hover:bg-neutral-50">
        <div className="truncate text-sm font-medium text-neutral-800">{projectName ?? "未选择项目"}</div>
        <div className="text-xs text-neutral-500">{currentPhase?.phaseName ?? "未设置阶段"}</div>
      </button>
      {open && (
        <div className="mt-2 rounded-md border border-neutral-200 bg-white shadow-sm">
          {projects.map((project) => (
            <button key={project.id} type="button" onClick={() => switchProject(project.id)} className="block w-full px-3 py-2 text-left text-sm hover:bg-neutral-50">
              {project.name}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
```

- [ ] **步骤 2：Layout 插入 ProjectSwitcher**

在 Logo 和 nav 之间插入：

```tsx
<ProjectSwitcher />
```

`/search` 标签改为：

```tsx
{ to: "/search", icon: Search, label: "检索（全部项目）" }
```

- [ ] **步骤 3：Import 和 ImportModal 移除项目选择器**

删除页面内 `default/enterprise/flagship` 选择器状态，所有导入调用使用：

```typescript
const { projectId } = useProject()
if (projectId === null) throw new Error("请先选择项目")
await ingestFile(path, projectId)
```

- [ ] **步骤 4：Search 页面全局检索**

`Search.tsx` 调用：

```typescript
const res = await hybridSearch(query.trim(), null, 30)
```

结果列表显示：

```tsx
<span>项目：{result.projectName}</span>
```

- [ ] **步骤 5：项目数据页统一阻止空项目**

`Browse/Chat/Research/Products/Graph/Risk/Wizard/Spotlight` 在需要当前项目时使用：

```tsx
if (projectId === null) {
  return <div className="p-6 text-sm text-neutral-500">请先创建或选择项目</div>
}
```

- [ ] **步骤 6：Home 改造为项目仪表盘**

显示：
- 当前阶段
- 阶段进度条
- 文档数
- 风险项数
- 最近活动
- 推进阶段按钮

推进阶段按钮调用：

```typescript
await advancePhase()
```

- [ ] **步骤 7：Settings 增加项目管理 section**

新增区域支持：
- 创建项目
- 编辑项目名称、客户名称、描述
- 设置项目产品
- 归档/取消归档
- 管理公共资料

- [ ] **步骤 8：运行前端检查**

运行：`pnpm typecheck`

运行：`pnpm build`

预期：类型检查和构建均通过。

---

## 任务 8：补齐测试和全量验证

**文件：**
- 修改或创建：`src-tauri/src/services/project_store.rs` 内联单元测试模块
- 修改或创建：相关 Rust service 测试模块
- 修改或创建：`tests` 或现有 Playwright 测试文件（如果项目已有 E2E 测试目录）

- [ ] **步骤 1：ProjectStore 单元测试**

在 `project_store.rs` 添加测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_default_project_initializes_survey_phase() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("metadata.db");
        let store = ProjectStore::new(&db_path).expect("创建项目存储失败");

        let project_id = store.ensure_default_project().expect("创建默认项目失败");
        let phases = store.get_project_phases(project_id).expect("读取阶段失败");

        assert_eq!(phases.len(), 7);
        assert_eq!(phases[0].phase_key, "survey");
        assert_eq!(phases[0].status, "current");
    }
}
```

- [ ] **步骤 2：归档写保护测试**

测试 `ensure_project_active`：归档项目返回错误，活跃项目通过。

- [ ] **步骤 3：搜索隔离测试**

构造：
- 项目 A 知识文档
- 项目 B 知识文档
- 项目 A 聊天附件文档

验证：
- 检索页 `project_id = None` 返回 A/B 知识文档，不返回聊天附件
- Agent `project_id = Some(A)` 返回 A 知识文档，不返回 B
- Agent 带 `chat_session_id` 时可返回当前会话附件

- [ ] **步骤 4：前端类型验证**

运行：`pnpm typecheck`

预期：无 TypeScript 错误。

- [ ] **步骤 5：前端构建验证**

运行：`pnpm build`

预期：Vite 构建成功。

- [ ] **步骤 6：Rust 验证**

运行：`cargo check`，工作目录：`src-tauri`

运行：`cargo test`，工作目录：`src-tauri`

预期：检查和测试均通过。

- [ ] **步骤 7：人工验收路径**

启动：`pnpm tauri:dev`

验收：
- 首次启动自动有默认项目
- 侧边栏显示 ProjectSwitcher
- 新建项目后自动切换
- 导入文档进入当前项目
- 检索页显示跨项目结果和项目来源
- Chat 只检索当前项目知识库
- 聊天附件只在当前会话可检索
- 风险跟踪归属当前统一项目
- 归档项目后写入命令返回“项目已归档，不可修改”

---

## 自检清单

- 设计目标覆盖：项目实体、阶段、产品、公共资料、归档、跨项目搜索、风险统一归属、聊天附件作用域均有任务覆盖。
- 单数据库约束覆盖：`ProductStore` 和所有 Store 均指向 `metadata.db`，计划中未新增独立 DB。
- 前端类型覆盖：`ProjectContext`、命令封装、页面调用均纳入计划。
- 搜索边界覆盖：检索页传 `None`，Agent/RAG 传 `Some(project_id)`，聊天附件使用 `document_scope + chat_session_id`。
- 占位符检查：计划中的任务都包含具体文件、代码形状、命令和预期结果。
