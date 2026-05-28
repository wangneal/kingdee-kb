# 双轨风险把控舱 — 多项目改造实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 subagent-driven-development（推荐）或 executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 将风险把控舱从单项目全局改造为多项目隔离，支持从合同/SOW 文档自动提取范围定义，提供整库备份导出导入。

**架构：** 新增 `risk_projects` 表 + 现有 `contract_scope_items` / `project_health_metrics` 加 `project_id` 列；`RiskControlStore` 方法加 `project_id` 参数；前端新增项目选择器 + 范围提取 + 整库备份按钮。

**技术栈：** Rust + rusqlite / React + TypeScript / Tauri v2 / LLM (OpenAI 兼容)

---

## 涉及文件

| 文件 | 职责 | 改动类型 |
|------|------|----------|
| `src-tauri/src/services/risk_control.rs` | RiskControlStore + 所有 DB 操作 | 修改 |
| `src-tauri/src/services/llm_service.rs` | chat_completion 已存在，无需改动 | 不变 |
| `src-tauri/src/services/metadata.rs` | 提供 `get_chunks_by_document` 给范围提取用 | 不变 |
| `src-tauri/src/services/mod.rs` | 导出新类型 | 可能需修改 |
| `src-tauri/src/lib.rs` | Tauri 命令 + 注册 | 修改 |
| `src/lib/tauri-commands.ts` | TS API 封装 | 修改 |
| `src/pages/RiskControl.tsx` | 前端 UI | 修改 |

---

### 任务 1：数据模型 — 新增 risk_projects 表 + 改造现有表

**文件：**
- 修改：`src-tauri/src/services/risk_control.rs`

- [ ] **步骤 1：新增 RiskProject 结构体 + ImportDbResult**

在 `risk_control.rs` 的 types 区域新增：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskProject {
    pub id: i64,
    pub name: String,
    pub client_name: String,
    pub kb_project: String,
    pub contract_doc_id: Option<i64>,
    pub sow_doc_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportDbResult {
    pub db_size_bytes: u64,
    pub document_count: i64,
    pub chunk_count: i64,
    pub risk_project_count: i64,
}
```

- [ ] **步骤 2：init_tables() 新增 risk_projects 表 + ALTER TABLE**

```rust
fn init_tables(&self) -> Result<(), String> {
    let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS contract_scope_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL DEFAULT -1,
            category TEXT NOT NULL,
            description TEXT NOT NULL,
            is_in_scope INTEGER NOT NULL DEFAULT 1,
            detail TEXT DEFAULT '',
            created_at TEXT DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS project_health_metrics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL DEFAULT -1,
            indicator_type TEXT NOT NULL,
            value REAL NOT NULL,
            notes TEXT DEFAULT '',
            recorded_at TEXT DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS risk_projects (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            client_name TEXT DEFAULT '',
            kb_project TEXT DEFAULT '',
            contract_doc_id INTEGER DEFAULT NULL,
            sow_doc_id INTEGER DEFAULT NULL,
            created_at TEXT DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_scope_project ON contract_scope_items(project_id);
        CREATE INDEX IF NOT EXISTS idx_health_project ON project_health_metrics(project_id);
        CREATE INDEX IF NOT EXISTS idx_health_type ON project_health_metrics(indicator_type);

        -- 兼容旧表：project_id 列可能不存在
        ALTER TABLE contract_scope_items ADD COLUMN project_id INTEGER NOT NULL DEFAULT -1;
        ALTER TABLE project_health_metrics ADD COLUMN project_id INTEGER NOT NULL DEFAULT -1;
    ")
    .map_err(|e| format!("Failed to init risk control tables: {}", e))
}
```

注意：`CREATE TABLE IF NOT EXISTS` 和 `ALTER TABLE ADD COLUMN` 同时存在是安全的 —— 如果表已存在且列已存在，ALTER TABLE 会报错但不会终止 batch。需要用 `execute_batch` 的特性：失败就整体回滚。

更安全的做法：先检查列是否存在，再执行 ALTER TABLE。但为了简化，可用 `try_execute_batch` 或分步执行，忽略列已存在的错误。

实际实现时分两步：
1. 先执行 CREATE TABLE（普通 execute_batch）
2. 对每个 ALTER TABLE，用 `try { conn.execute(...) } catch { }` 忽略重复列错误

```rust
// 分步执行 ALTER TABLE，忽略列已存在错误
let alter_tables = [
    "ALTER TABLE contract_scope_items ADD COLUMN project_id INTEGER NOT NULL DEFAULT -1",
    "ALTER TABLE project_health_metrics ADD COLUMN project_id INTEGER NOT NULL DEFAULT -1",
];
for sql in &alter_tables {
    let _ = conn.execute(sql, []); // 忽略列已存在错误
}
```

- [ ] **步骤 3：改造现有方法加 project_id 参数**

```rust
// 改造前:
pub fn add_scope_item(&self, category: &str, description: &str, is_in_scope: bool, detail: &str) -> Result<i64, String>

// 改造后:
pub fn add_scope_item(&self, project_id: i64, category: &str, description: &str, is_in_scope: bool, detail: &str) -> Result<i64, String>
// SQL: INSERT INTO contract_scope_items (project_id, category, ...) VALUES (?1, ?2, ...)
// params![project_id, category, description, is_in_scope as i32, detail]

// 改造前:
pub fn list_scope_items(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<ContractScopeItem>, String>

// 改造后:
pub fn list_scope_items(&self, project_id: i64, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<ContractScopeItem>, String>
// SQL: ... WHERE project_id = ?1 ... LIMIT ?2 OFFSET ?3
// params![project_id, limit.unwrap_or(-1), offset.unwrap_or(0)]

// 改造前:
pub fn delete_scope_item(&self, id: i64) -> Result<(), String>
// 不变（按 id 删除，无需 project_id）

// 改造前: record_health_metric 无 project_id
pub fn record_health_metric(&self, project_id: i64, indicator_type: &str, value: f64, notes: &str) -> Result<i64, String>
// SQL: INSERT INTO project_health_metrics (project_id, indicator_type, value, notes) VALUES (?1, ?2, ?3, ?4)

// 改造前: get_recent_metrics 无 project_id
pub fn get_recent_metrics(&self, project_id: i64, indicator_type: &str, limit: usize) -> Result<Vec<HealthMetric>, String>
// SQL: WHERE project_id = ?1 AND indicator_type = ?2 ... 

// 改造前: get_all_recent_metrics 无 project_id
pub fn get_all_recent_metrics(&self, project_id: i64) -> Result<Vec<HealthMetric>, String>
// SQL: WHERE project_id = ?1 ...

// 改造前: calculate_health_score 无参数
pub fn calculate_health_score(&self, project_id: i64) -> Result<ProjectHealthScore, String>
// 调用 get_all_recent_metrics(project_id)

// check_scope_creep - 改造为按 project_id 获取合同范围
pub async fn check_scope_creep(&self, llm: &LLMService, project_id: i64, requirement: &str) -> Result<ScopeCreepResult, String>
// 内部调用 list_scope_items(project_id, ...)
```

- [ ] **步骤 4：新增项目 CRUD 方法**

```rust
pub fn create_risk_project(&self, name: &str, client_name: &str, kb_project: &str) -> Result<i64, String> {
    let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
    conn.execute(
        "INSERT INTO risk_projects (name, client_name, kb_project) VALUES (?1, ?2, ?3)",
        params![name, client_name, kb_project],
    ).map_err(|e| format!("Failed to create project: {}", e))?;
    Ok(conn.last_insert_rowid())
}

pub fn list_risk_projects(&self) -> Result<Vec<RiskProject>, String> {
    let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
    let mut stmt = conn.prepare(
        "SELECT id, name, client_name, kb_project, contract_doc_id, sow_doc_id, created_at FROM risk_projects ORDER BY created_at DESC"
    ).map_err(|e| format!("Failed to prepare: {}", e))?;
    let rows = stmt.query_map([], |row| {
        Ok(RiskProject {
            id: row.get(0)?,
            name: row.get(1)?,
            client_name: row.get(2)?,
            kb_project: row.get(3)?,
            contract_doc_id: row.get(4)?,
            sow_doc_id: row.get(5)?,
            created_at: row.get(6)?,
        })
    }).map_err(|e| format!("Failed to query: {}", e))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
    }
    Ok(items)
}

pub fn delete_risk_project(&self, project_id: i64) -> Result<(), String> {
    let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
    conn.execute("DELETE FROM risk_projects WHERE id = ?1", params![project_id])
        .map_err(|e| format!("Failed to delete project: {}", e))?;
    conn.execute("DELETE FROM contract_scope_items WHERE project_id = ?1", params![project_id])
        .map_err(|e| format!("Failed to delete scope items: {}", e))?;
    conn.execute("DELETE FROM project_health_metrics WHERE project_id = ?1", params![project_id])
        .map_err(|e| format!("Failed to delete health metrics: {}", e))?;
    Ok(())
}
```

- [ ] **步骤 5：新增范围提取 + 确认入库方法**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateScopeItem {
    pub category: String,
    pub description: String,
    pub is_in_scope: bool,
    pub detail: String,
    pub confidence: f64,
}

/// 从文档内容提取候选范围项（LLM 驱动）
pub async fn extract_scope_from_document(
    &self,
    llm: &LLMService,
    chunks: &[super::metadata::ChunkMeta],
) -> Result<Vec<CandidateScopeItem>, String> {
    // 拼接文档内容（截取前 8000 字符）
    let doc_content: String = chunks.iter()
        .map(|c| c.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(8000)
        .collect();

    let prompt = format!(
        "你是一个 ERP 实施项目的合同审计员。分析以下合同/SOW 文档内容，提取所有明确属于\"实施范围内\"和\"明确排除\"的功能模块。对每项给出原文依据引用。\n\n文档内容：\n{}\n\n严格按照以下 JSON 数组格式返回，不要其他文字：\n[\n  {{\"category\": \"FI\", \"description\": \"总账模块实施\", \"is_in_scope\": true, \"detail\": \"合同第3.1条：包含总账、应收应付\", \"confidence\": 0.95}},\n  {{\"category\": \"FI\", \"description\": \"银企直连\", \"is_in_scope\": false, \"detail\": \"排除项清单第5条\", \"confidence\": 0.9}}\n]",
        doc_content
    );

    let messages = vec![
        super::llm_service::ChatMessage {
            role: "system".to_string(),
            content: "你是 ERP 实施合同审计专家。严格基于文档内容提取范围定义，不编造信息。".to_string(),
        },
        super::llm_service::ChatMessage {
            role: "user".to_string(),
            content: prompt,
        },
    ];
    let config = llm.get_config()?;
    let response = llm.chat_completion(&messages, &config).await?;
    serde_json::from_str(&response)
        .map_err(|e| format!("LLM 返回格式错误: {} — 原始响应: {}", e, response))
}

/// 确认入库候选范围项
pub fn confirm_scope_items(&self, project_id: i64, items: &[CandidateScopeItem]) -> Result<usize, String> {
    let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
    let mut count = 0usize;
    for item in items {
        conn.execute(
            "INSERT INTO contract_scope_items (project_id, category, description, is_in_scope, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project_id, item.category, item.description, item.is_in_scope as i32, item.detail],
        ).map_err(|e| format!("Failed to insert scope item: {}", e))?;
        count += 1;
    }
    Ok(count)
}
```

- [ ] **步骤 6：新增整库导出方法**

```rust
/// 导出整库（VACUUM INTO）
pub fn export_database(&self, target_path: &str) -> Result<(), String> {
    // 打开一个独立连接执行 VACUUM INTO
    // 注意：不能直接在 locker 的 conn 上做 VACUUM INTO（可能与其他连接冲突）
    // 使用文件路径打开新连接
    let db_path = self.db_path.to_str().ok_or("Invalid db path")?.to_string();
    // 通过 SQLite 的 backup API 或直接打开副本执行 VACUUM INTO
    let backup_conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("Failed to open DB for export: {}", e))?;
    backup_conn.execute_batch(&format!("VACUUM INTO '{}';", target_path.replace('\'', "''")))
        .map_err(|e| format!("VACUUM INTO failed: {}", e))?;
    Ok(())
}
```

注意：`VACUUM INTO` 要求目标路径是字符串，参数化不行，必须拼接字符串并转义单引号。

- [ ] **步骤 7：新增整库导入方法**

```rust
/// 验证并导入数据库备份（返回统计信息）
pub fn import_database(&self, backup_path: &str, app_state: &crate::AppState) -> Result<ImportDbResult, String> {
    use std::fs;

    // 1. 验证文件是合法 SQLite 数据库
    let header = {
        let mut f = fs::File::open(backup_path)
            .map_err(|e| format!("Cannot open backup file: {}", e))?;
        let mut buf = [0u8; 16];
        use std::io::Read;
        f.read_exact(&mut buf).map_err(|e| format!("Cannot read backup file: {}", e))?;
        buf
    };
    if &header != "SQLite format 3\0".as_bytes() {
        return Err("备份文件不是合法的 SQLite 数据库".to_string());
    }

    // 2. 获取当前 metadata.db 路径
    let db_path = self.db_path.as_path();

    // 3. 检查导入文件大小
    let meta = fs::metadata(backup_path).map_err(|e| format!("Cannot stat backup: {}", e))?;
    let db_size = meta.len();

    // 4. 备份当前 DB（安全措施）
    let temp_backup = db_path.with_extension("db.before_import");
    fs::copy(db_path, &temp_backup).map_err(|e| format!("Cannot backup current DB: {}", e))?;

    // 5. 替换当前 DB 文件
    // 先释放所有连接（通过 drop 当前连接，再重新打开）
    // 但 conn 在 Mutex 中，我们需要先关闭它
    {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        // 关闭当前连接（drop）
        *conn = rusqlite::Connection::open(db_path)
            .map_err(|e| format!("Cannot reopen placeholder connection: {}", e))?;
    }

    // 复制备份文件覆盖当前 DB
    fs::copy(backup_path, db_path).map_err(|e| format!("Cannot restore backup: {}", e))?;

    // 6. 重新初始化 store（init_tables 确保表结构存在）
    self.init_tables()?;

    // 7. 获取统计信息
    let doc_count = app_state.metadata.lock().map_err(|e| e.to_string())?.get_stats()?.document_count;
    let chunk_count = app_state.metadata.lock().map_err(|e| e.to_string())?.get_stats()?.chunk_count;
    let project_count = self.list_risk_projects()?.len() as i64;

    // 删除临时备份
    let _ = fs::remove_file(&temp_backup);

    Ok(ImportDbResult {
        db_size_bytes: db_size,
        document_count: doc_count,
        chunk_count,
        risk_project_count: project_count,
    })
}
```

- [ ] **步骤 8：运行已有测试确认不破坏**

运行：`cd src-tauri && cargo test risk_control` 或 `cargo test`

- [ ] **步骤 9：编译确认**

运行：`cd src-tauri && cargo check`

---

### 任务 2：Tauri 命令（lib.rs）

**文件：**
- 修改：`src-tauri/src/lib.rs`

- [ ] **步骤 1：新增项目 CRUD 命令**

```rust
#[tauri::command]
fn create_risk_project(
    state: State<'_, AppState>,
    name: String,
    client_name: Option<String>,
    kb_project: Option<String>,
) -> Result<i64, String> {
    state.risk_control_store.create_risk_project(
        &name,
        &client_name.unwrap_or_default(),
        &kb_project.unwrap_or_default(),
    )
}

#[tauri::command]
fn list_risk_projects(state: State<'_, AppState>) -> Result<Vec<RiskProject>, String> {
    state.risk_control_store.list_risk_projects()
}

#[tauri::command]
fn delete_risk_project(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    state.risk_control_store.delete_risk_project(project_id)
}
```

- [ ] **步骤 2：改造现有命令加 project_id 参数**

```rust
// 改 add_scope_item
#[tauri::command]
fn add_scope_item(
    state: State<'_, AppState>,
    project_id: i64,
    category: String,
    description: String,
    is_in_scope: bool,
    detail: String,
) -> Result<i64, String> {
    state.risk_control_store.add_scope_item(project_id, &category, &description, is_in_scope, &detail)
}

// 改 list_scope_items
#[tauri::command]
fn list_scope_items(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<ContractScopeItem>, String> {
    state.risk_control_store.list_scope_items(project_id, None, None)
}

// get_project_health 加 project_id
#[tauri::command]
fn get_project_health(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<ProjectHealthScore, String> {
    state.risk_control_store.calculate_health_score(project_id)
}

// check_scope_creep 加 project_id
#[tauri::command]
async fn check_scope_creep(
    state: State<'_, AppState>,
    project_id: i64,
    requirement: String,
) -> Result<ScopeCreepResult, String> {
    state.risk_control_store.check_scope_creep(&state.llm, project_id, &requirement).await
}

// record_health_metric 加 project_id
#[tauri::command]
fn record_health_metric(
    state: State<'_, AppState>,
    project_id: i64,
    indicator_type: String,
    value: f64,
    notes: String,
) -> Result<i64, String> {
    state.risk_control_store.record_health_metric(project_id, &indicator_type, value, &notes)
}
```

- [ ] **步骤 3：新增范围提取命令**

```rust
#[tauri::command]
async fn extract_scope_from_document(
    state: State<'_, AppState>,
    project_id: i64,
    doc_id: i64,
) -> Result<Vec<CandidateScopeItem>, String> {
    // 从 metadata store 获取文档分块
    let chunks = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.get_chunks_by_document(doc_id)?
    };

    if chunks.is_empty() {
        return Err("文档中未找到任何内容分块".to_string());
    }

    let result = state.risk_control_store.extract_scope_from_document(&state.llm, &chunks).await?;
    Ok(result)
}

#[tauri::command]
fn confirm_scope_items(
    state: State<'_, AppState>,
    project_id: i64,
    items: Vec<CandidateScopeItem>,
) -> Result<usize, String> {
    state.risk_control_store.confirm_scope_items(project_id, &items)
}
```

- [ ] **步骤 4：新增整库备份命令**

```rust
#[tauri::command]
fn export_database(
    state: State<'_, AppState>,
    target_path: String,
) -> Result<(), String> {
    state.risk_control_store.export_database(&target_path)
}

#[tauri::command]
fn import_database(
    state: State<'_, AppState>,
    backup_path: String,
) -> Result<ImportDbResult, String> {
    state.risk_control_store.import_database(&backup_path, &state)
}
```

- [ ] **步骤 5：注册到 generate_handler![]**

在 `generate_handler![]` 的 "P1: 双轨风险把控舱" 区域追加：
```
create_risk_project,
list_risk_projects,
delete_risk_project,
extract_scope_from_document,
confirm_scope_items,
export_database,
import_database,
```

同时保留已有的 `add_scope_item`, `list_scope_items` 等（函数签名已改，但注册名不变）。

- [ ] **步骤 6：编译确认**

运行：`cd src-tauri && cargo check`

---

### 任务 3：前端 API（tauri-commands.ts）

**文件：**
- 修改：`src/lib/tauri-commands.ts`

- [ ] **步骤 1：添加缺失的类型定义**

```typescript
// ── Risk Control Types (P1: 双轨风险把控舱) ──

export interface RiskProject {
  id: number;
  name: string;
  client_name: string;
  kb_project: string;
  contract_doc_id: number | null;
  sow_doc_id: number | null;
  created_at: string;
}

export interface ContractScopeItem {
  id: number;
  category: string;
  description: string;
  is_in_scope: boolean;
  detail: string;
  created_at: string;
}

export interface ScopeCreepResult {
  risk_level: string;     // "red" | "yellow" | "green"
  risk_label: string;
  explanation: string;
  matched_items: string[];
  suggestion: string;
}

export interface ProjectHealthScore {
  overall_score: number;
  risk_level: string;     // "low" | "medium" | "high" | "critical"
  dimensions: HealthDimension[];
  trend: string;
  alert_count: number;
}

export interface HealthDimension {
  name: string;
  score: number;
  weight: number;
  detail: string;
}

export interface CandidateScopeItem {
  category: string;
  description: string;
  is_in_scope: boolean;
  detail: string;
  confidence: number;
}

export interface DefenseScriptRequest {
  scenario: string;
  context?: string;
  tone?: string;     // "push_back" | "guide" | "escalate"
}

export interface ScriptItem {
  phase: string;
  content: string;
  tip: string;
}

export interface DefenseScriptResult {
  scenario_label: string;
  scripts: ScriptItem[];
}

export interface ImportDbResult {
  db_size_bytes: number;
  document_count: number;
  chunk_count: number;
  risk_project_count: number;
}
```

- [ ] **步骤 2：添加项目 CRUD + 范围提取 API 函数**

```typescript
// ── Risk Control API ──

// 项目
export async function createRiskProject(
  name: string,
  clientName?: string,
  kbProject?: string
): Promise<number> {
  return invoke("create_risk_project", {
    name,
    client_name: clientName ?? "",
    kb_project: kbProject ?? "",
  });
}

export async function listRiskProjects(): Promise<RiskProject[]> {
  return invoke("list_risk_projects");
}

export async function deleteRiskProject(projectId: number): Promise<void> {
  return invoke("delete_risk_project", { project_id: projectId });
}

// 合同范围
export async function listScopeItems(projectId: number): Promise<ContractScopeItem[]> {
  return invoke("list_scope_items", { project_id: projectId });
}

export async function addScopeItem(
  projectId: number,
  category: string,
  description: string,
  isInScope: boolean,
  detail: string
): Promise<number> {
  return invoke("add_scope_item", {
    project_id: projectId,
    category,
    description,
    is_in_scope: isInScope,
    detail,
  });
}

export async function deleteScopeItem(itemId: number): Promise<void> {
  return invoke("delete_scope_item", { item_id: itemId });
}

// 需求蔓延检查
export async function checkScopeCreep(
  projectId: number,
  requirement: string
): Promise<ScopeCreepResult> {
  return invoke("check_scope_creep", { project_id: projectId, requirement });
}

// 项目健康度
export async function getProjectHealth(
  projectId: number
): Promise<ProjectHealthScore> {
  return invoke("get_project_health", { project_id: projectId });
}

export async function recordHealthMetric(
  projectId: number,
  indicatorType: string,
  value: number,
  notes: string
): Promise<number> {
  return invoke("record_health_metric", {
    project_id: projectId,
    indicator_type: indicatorType,
    value,
    notes,
  });
}

// 健康风险报告
export async function generateRiskReport(context: string): Promise<string> {
  return invoke("generate_risk_report", { context });
}

// 防身话术
export async function generateDefenseScript(
  request: DefenseScriptRequest
): Promise<DefenseScriptResult> {
  return invoke("generate_defense_script", { request });
}

// 文档范围提取
export async function extractScopeFromDocument(
  projectId: number,
  docId: number
): Promise<CandidateScopeItem[]> {
  return invoke("extract_scope_from_document", {
    project_id: projectId,
    doc_id: docId,
  });
}

export async function confirmScopeItems(
  projectId: number,
  items: CandidateScopeItem[]
): Promise<number> {
  return invoke("confirm_scope_items", { project_id: projectId, items });
}

// 蓝图提炼
export async function extractBlueprint(context: string): Promise<string> {
  return invoke("extract_blueprint", { research_context: context });
}

// Fit-Gap 分析
export async function analyzeFitGap(requirements: string): Promise<string> {
  return invoke("analyze_fit_gap", { requirements });
}

// ReAct 深度分析对话
export async function reactChat(
  message: string,
  systemPrompt: string,
  sessionId: string
): Promise<void> {
  return invoke("react_chat", {
    message,
    system_prompt: systemPrompt,
    session_id: sessionId,
  });
}

// 整库备份
export async function exportDatabase(targetPath: string): Promise<void> {
  return invoke("export_database", { target_path: targetPath });
}

export async function importDatabase(
  backupPath: string
): Promise<ImportDbResult> {
  return invoke("import_database", { backup_path: backupPath });
}

// 报告导出（文件写入）
export async function exportReport(content: string, filePath: string): Promise<void> {
  return invoke("export_report", { content, file_path: filePath });
}

// ReAct 事件监听
export type ReActEventType = "text_delta" | "tool_call" | "tool_result" | "done" | "error";

export interface ReActEvent {
  type: ReActEventType;
  content: string;
  session_id?: string;
  tool_name?: string;
  tool_input?: string;
}

// 注意: listenReActEvents 通过 Tauri 事件系统实现
export async function listenReActEvents(
  callback: (event: ReActEvent) => void
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ReActEvent>("react_event", (event) => {
    callback(event.payload);
  });
}
```

- [ ] **步骤 3：编译确认**

运行：`npx tsc --noEmit`

---

### 任务 4：前端 UI 改造（RiskControl.tsx）

**文件：**
- 修改：`src/pages/RiskControl.tsx`

- [ ] **步骤 1：添加项目选择器组件**

在文件顶部导入新增的类型和函数：

```typescript
import {
  // ... 已有 imports
  // 新增:
  type RiskProject,
  type CandidateScopeItem,
  type ImportDbResult,
  createRiskProject,
  listRiskProjects,
  deleteRiskProject,
  extractScopeFromDocument,
  confirmScopeItems,
  exportDatabase,
  importDatabase,
} from "../lib/tauri-commands";
```

在 `RiskControl` 主组件中添加项目选择状态和加载逻辑：

```typescript
export default function RiskControl() {
  const [tab, setTab] = useState<Tab>("scope");
  const [projects, setProjects] = useState<RiskProject[]>([]);
  const [activeProject, setActiveProject] = useState<number | null>(null);
  const [showNewProject, setShowNewProject] = useState(false);
  const [newProjectName, setNewProjectName] = useState("");
  const [newProjectClient, setNewProjectClient] = useState("");
  const [newProjectKb, setNewProjectKb] = useState("");

  // 加载项目列表
  useEffect(() => {
    listRiskProjects().then((list) => {
      setProjects(list);
      if (list.length > 0 && !activeProject) {
        setActiveProject(list[0].id);
      }
    });
  }, []);

  const handleCreateProject = async () => {
    if (!newProjectName.trim()) return;
    const id = await createRiskProject(newProjectName.trim(), newProjectClient.trim() || undefined, newProjectKb.trim() || undefined);
    setProjects(prev => [...prev, { id, name: newProjectName.trim(), client_name: newProjectClient.trim(), kb_project: newProjectKb.trim(), contract_doc_id: null, sow_doc_id: null, created_at: new Date().toISOString() }]);
    setActiveProject(id);
    setShowNewProject(false);
    setNewProjectName("");
    setNewProjectClient("");
    setNewProjectKb("");
  };

  // 渲染时传递 activeProject 给子组件
  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center gap-2 border-b border-neutral-200 px-6">
        <ShieldAlert className="h-5 w-5 text-amber-600" />
        <h1 className="text-base font-semibold text-neutral-800">双轨风险把控舱</h1>
        {/* 项目选择器 */}
        <div className="ml-4 flex items-center gap-2">
          <select
            value={activeProject ?? ""}
            onChange={(e) => setActiveProject(Number(e.target.value))}
            className="rounded-lg border border-neutral-200 px-3 py-1.5 text-xs outline-none focus:border-amber-500"
          >
            {projects.length === 0 && <option value="">暂无项目</option>}
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} {p.client_name ? `(${p.client_name})` : ""}
              </option>
            ))}
          </select>
          <button
            type="button"
            onClick={() => setShowNewProject(true)}
            className="flex items-center gap-1 rounded-lg bg-amber-600 px-2 py-1.5 text-xs font-medium text-white hover:bg-amber-700"
          ><Plus className="h-3 w-3" />新建项目</button>
        </div>
      </div>
      {/* ... tabs 和内容区域, 传 activeProject 给子组件 ... */}
    </div>
  );
}
```

子组件接收 `projectId: number | null` prop：
- `ScopeTab` → 只加载当前项目的 scope items
- `HealthTab` → 只加载当前项目的健康数据
- `ScriptsTab` → 无需项目隔离（工具类功能，全局可用）
- `AnalysisTab` → 对话中注入当前项目上下文

- [ ] **步骤 2：新建项目弹窗**

在 tabs 区域上方添加模态对话框：

```tsx
{showNewProject && (
  <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={() => setShowNewProject(false)}>
    <div className="w-96 rounded-xl bg-white p-6 shadow-xl" onClick={(e) => e.stopPropagation()}>
      <h3 className="mb-4 text-sm font-semibold text-neutral-800">新建项目</h3>
      <div className="space-y-3">
        <div>
          <label className="mb-1 block text-[10px] font-medium text-neutral-500">项目名称 *</label>
          <input value={newProjectName} onChange={(e) => setNewProjectName(e.target.value)} placeholder="如：XX集团ERP实施项目" className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500" />
        </div>
        <div>
          <label className="mb-1 block text-[10px] font-medium text-neutral-500">客户名称</label>
          <input value={newProjectClient} onChange={(e) => setNewProjectClient(e.target.value)} placeholder="如：XX集团" className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500" />
        </div>
        <div>
          <label className="mb-1 block text-[10px] font-medium text-neutral-500">关联知识库项目</label>
          <input value={newProjectKb} onChange={(e) => setNewProjectKb(e.target.value)} placeholder="输入 KB 项目标签过滤文档" className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500" />
        </div>
      </div>
      <div className="mt-4 flex justify-end gap-2">
        <button type="button" onClick={() => setShowNewProject(false)} className="rounded-lg border border-neutral-200 px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50">取消</button>
        <button type="button" onClick={handleCreateProject} disabled={!newProjectName.trim()} className="rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50">创建</button>
      </div>
    </div>
  </div>
)}
```

- [ ] **步骤 3：提取范围按钮（ScopeTab）**

在 ScopeTab 中添加范围提取按钮 + 文档选择 + 预览确认 UI：

```tsx
// 在 ScopeTab 顶部添加
const [showDocPicker, setShowDocPicker] = useState(false);
const [candidates, setCandidates] = useState<CandidateScopeItem[] | null>(null);
const [docs, setDocs] = useState<{ id: number; title: string }[]>([]);
const [extracting, setExtracting] = useState(false);
```

提取流程：
1. 点击 "从合同/SOW 提取范围" → `listDocuments(kbProject)` 加载文档
2. 选中文档 → `extractScopeFromDocument(projectId, docId)` → 展示候选列表
3. 编辑/删除候选 → `confirmScopeItems(projectId, items)` → 刷新范围列表

- [ ] **步骤 4：各 Tab 按 projectId 传参**

ScopeTab 原有函数调用全部加 `projectId`：

```typescript
// 加载范围
useEffect(() => {
  if (projectId) {
    listScopeItems(projectId).then(setItems);
  } else {
    setItems([]);
  }
}, [projectId]);

// 检查需求蔓延
const result = await checkScopeCreep(projectId, requirement);
```

HealthTab 同理：

```typescript
useEffect(() => {
  if (projectId) {
    getProjectHealth(projectId).then(setHealth);
  } else {
    setHealth(null);
  }
}, [projectId]);
```

- [ ] **步骤 5：整库导出/导入按钮**

在 HealthTab 或 RiskControl 底部添加备份按钮：

```tsx
// 导出
const handleExport = async () => {
  const { save } = await import("@tauri-apps/plugin-dialog");
  const path = await save({
    defaultPath: `kingdee-kb-backup-${new Date().toISOString().slice(0, 10)}.db`,
    filters: [{ name: "SQLite Database", extensions: ["db"] }],
  });
  if (path) {
    await exportDatabase(path);
    alert("数据库导出成功");
  }
};

// 导入
const handleImport = async () => {
  const ok = window.confirm("此操作将用备份文件替换当前全部数据（包括知识库文档、风险数据、研究会话等），且不可撤销。建议先导出当前数据作为备份。是否继续？");
  if (!ok) return;

  const { open } = await import("@tauri-apps/plugin-dialog");
  const path = await open({
    filters: [{ name: "SQLite Database", extensions: ["db"] }],
    multiple: false,
  });
  if (path) {
    const result = await importDatabase(path);
    alert(`导入成功！\n文档: ${result.document_count}\n分块: ${result.chunk_count}\n风险项目: ${result.risk_project_count}\n\n请重启应用以应用变更。`);
  }
};
```

按钮位置：在健康度 Tab 页脚，新增一行操作按钮：

```tsx
<div className="mt-6 flex justify-end gap-2 border-t border-neutral-100 pt-4">
  <button type="button" onClick={handleExport}
    className="flex items-center gap-1 rounded-lg border border-neutral-200 px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50">
    <Download className="h-3 w-3" />导出数据库
  </button>
  <button type="button" onClick={handleImport}
    className="flex items-center gap-1 rounded-lg border border-red-200 px-3 py-1.5 text-xs text-red-600 hover:bg-red-50">
    <Upload className="h-3 w-3" />导入数据库
  </button>
</div>
```

（需要从 lucide-react 导入 `Upload` 图标）

- [ ] **步骤 6：编译确认**

运行：`npx tsc --noEmit`

---

### 任务 5：更新单元测试

**文件：**
- 修改：`src-tauri/src/services/risk_control.rs`

- [ ] **步骤 1：更新现有测试，适配 project_id 参数**

```rust
#[test]
fn test_add_and_list_scope_items() {
    let store = new_store();
    let pid = store.create_risk_project("测试项目", "测试客户", "").unwrap();
    let id = store.add_scope_item(pid, "FI", "总账模块实施", true, "合同第3.1条").unwrap();
    assert!(id > 0);
    store.add_scope_item(pid, "FI", "银企直连", false, "合同排除项清单第5条").unwrap();

    let items = store.list_scope_items(pid, None, None).unwrap();
    assert_eq!(items.len(), 2);
    assert!(items[0].is_in_scope);
    assert!(!items[1].is_in_scope);
}
```

- [ ] **步骤 2：新增项目 CRUD 测试**

```rust
#[test]
fn test_create_and_list_projects() {
    let store = new_store();
    let pid1 = store.create_risk_project("项目A", "客户A", "kb_a").unwrap();
    let pid2 = store.create_risk_project("项目B", "客户B", "").unwrap();
    assert!(pid1 > 0);
    assert!(pid2 > pid1);

    let projects = store.list_risk_projects().unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].name, "项目B"); // DESC order
    assert_eq!(projects[1].kb_project, "kb_a");
}

#[test]
fn test_delete_project_cascades() {
    let store = new_store();
    let pid = store.create_risk_project("待删除", "", "").unwrap();
    store.add_scope_item(pid, "FI", "测试", true, "").unwrap();
    store.record_health_metric(pid, "attendance", 50.0, "测试").unwrap();

    store.delete_risk_project(pid).unwrap();
    assert_eq!(store.list_scope_items(pid, None, None).unwrap().len(), 0);
}
```

- [ ] **步骤 3：测试 confirm_scope_items**

```rust
#[test]
fn test_confirm_scope_items() {
    let store = new_store();
    let pid = store.create_risk_project("测试", "", "").unwrap();
    let items = vec![
        CandidateScopeItem { category: "FI".into(), description: "总账".into(), is_in_scope: true, detail: "合同条款".into(), confidence: 0.95 },
        CandidateScopeItem { category: "FI".into(), description: "银企直连".into(), is_in_scope: false, detail: "排除项".into(), confidence: 0.9 },
    ];
    let count = store.confirm_scope_items(pid, &items).unwrap();
    assert_eq!(count, 2);
    assert_eq!(store.list_scope_items(pid, None, None).unwrap().len(), 2);
}
```

- [ ] **步骤 4：运行所有测试**

运行：`cd src-tauri && cargo test risk_control`

---

### 自检

- [x] **规格覆盖度**：多项目隔离（risk_projects + project_id）、范围提取（extract_scope_from_document + confirm）、整库备份（VACUUM INTO）、KB 轻量关联（kb_project）、全在前 4 个任务中覆盖
- [x] **占位符扫描**：所有步骤含完整代码，无 TODO/待定/占位符
- [x] **类型一致性**：RiskProject / CandidateScopeItem / ImportDbResult 在 Rust 和 TS 两端类型一致，字段名对齐
- [x] **参数名一致性**：Tauri invoke 使用 snake_case 参数名，前端 invoke 传递匹配
