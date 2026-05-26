# 双轨风险把控舱 — 多项目改造 + 合同范围自动提取 + KB 轻量关联 + 整库备份

## 1. 目标

将现有双轨风险把控舱从"单项目全局"改造为"多项目隔离"；支持从合同/SOW 文档经 LLM 自动提取合同范围定义；风险项目轻量关联知识库项目标签以过滤文档列表；提供 SQLite 整库备份导出与导入恢复。

## 2. 数据模型

### 2.1 risk_projects 表

```sql
CREATE TABLE IF NOT EXISTS risk_projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    client_name TEXT DEFAULT '',
    kb_project TEXT DEFAULT '',        -- 关联知识库 project 标签，用于过滤文档列表
    contract_doc_id INTEGER DEFAULT NULL,
    sow_doc_id INTEGER DEFAULT NULL,
    created_at TEXT DEFAULT (datetime('now'))
);
```

- 新建于 `metadata.db` 中，与 `contract_scope_items` / `project_health_metrics` 在同一数据库
- `kb_project` — 轻量关联：用户在创建风险项目时选择对应的知识库 project 标签（KB 中 `documents.project` 字段），提取合同时按此过滤文档列表。无需新建 projects 表，不需改造 KB schema
- `contract_doc_id` / `sow_doc_id` 关联知识库文档（`documents.id`），用于后续自动提取

### 2.2 现有表改造

**contract_scope_items**: 新增 `project_id INTEGER NOT NULL REFERENCES risk_projects(id)`，对历史数据默认值=-1（无项目）

**project_health_metrics**: 新增 `project_id INTEGER NOT NULL REFERENCES risk_projects(id)`，同上

**索引**: `idx_scope_project` / `idx_health_project` 分别建立在 `(project_id, ...)` 上

## 3. 后端改动

### 3.1 RiskControlStore 新增方法

```rust
// 项目 CRUD
create_risk_project(name, client_name) -> Result<i64>
list_risk_projects() -> Result<Vec<RiskProject>>
get_risk_project(id) -> Result<Option<RiskProject>>
delete_risk_project(id) -> Result<()>

// 改造: 加 project_id 参数
list_scope_items(project_id, limit, offset) -> 改为按 project_id 筛选
add_scope_item(project_id, category, description, is_in_scope, detail) -> 加 project_id
calculate_health_score(project_id) -> 按 project_id 筛选指标

// 新增: 合同提取
extract_scope_from_document(llm, doc_chunks, project_id) -> Result<Vec<CandidateScopeItem>>
confirm_scope_items(project_id, Vec<CandidateScopeItem>) -> 批量写入
```

### 3.2 CandidateScopeItem 类型

```rust
struct CandidateScopeItem {
    category: String,
    description: String,
    is_in_scope: bool,
    detail: String,       // 依据/原文引用
    confidence: f64,      // LLM 置信度 0-1
}
```

### 3.3 Tauri 命令

```rust
// 现有命令加 project_id 参数
add_scope_item(state, project_id, ...)
list_scope_items(state, project_id)
check_scope_creep(state, project_id, requirement)
get_project_health(state, project_id)
record_health_metric(state, project_id, ...)

// 新增命令
create_risk_project(name, client_name, kb_project?) -> i64
list_risk_projects() -> Vec<RiskProject>
extract_scope_from_document(
    state, project_id, doc_id
) -> Vec<CandidateScopeItem>
confirm_scope_items(
    state, project_id, items: Vec<CandidateScopeItem>
) -> usize  // 返回写入条数

// 整库备份
export_database(state, target_path: String) -> Result<()>
import_database(state, backup_path: String) -> Result<ImportDbResult>
```

### 3.4 文档提取流程（LLM）

```
1. 从 MetadataStore 按 doc_id 获取所有文档分块 (get_chunks_by_document)
2. 拼接分块内容（若过长则截取前 8000 tokens）
3. 调用 LLM，system prompt 指示提取 in-scope / out-of-scope 条目
4. 返回结构化 JSON → 前端展示候选列表
```

System prompt 关键指令：
> 你是 ERP 实施项目的合同审计员。分析以下合同/SOW 文档内容，提取所有明确属于"实施范围内"和"明确排除"的功能模块。对每项给出原文依据引用。返回 JSON 数组。

## 4. 前端改动

### 4.1 tauri-commands.ts 新增

类型定义：
- `RiskProject { id, name, client_name, kb_project, contract_doc_id, sow_doc_id, created_at }`
- `CandidateScopeItem { category, description, is_in_scope, detail, confidence }`
- +之前缺失的所有类型：`ContractScopeItem`, `ScopeCreepResult`, `ProjectHealthScore`, `HealthDimension`, `DefenseScriptRequest`, `DefenseScriptResult`, `ScriptItem`

API 函数：
- `listScopeItems()`, `addScopeItem(...)`, `deleteScopeItem(...)`
- `checkScopeCreep(requirement)` → `ScopeCreepResult`
- `getProjectHealth()` → `ProjectHealthScore`（注意参数不匹配需修正）
- `createRiskProject(name, clientName)` → `number`
- `listRiskProjects()` → `RiskProject[]`
- `extractScopeFromDocument(projectId, docId)` → `CandidateScopeItem[]`
- `confirmScopeItems(projectId, items)` → `number`
- 其他已缺失的函数...

### 4.2 RiskControl.tsx 改造

**项目选择器（顶部）**:
```
[▼ 项目选择下拉] [+ 新建项目]
```

- 加载时调用 `listRiskProjects()`
- 切换项目 → 重新加载对应 Tab 的数据
- "新建项目" → 弹出对话框输入项目名+客户名
- 当前选中项目存入 `useState`

**提取范围按钮（ScopeTab）**:
```
[从合同/SOW 提取范围] (按钮)
```
- 点击 → 弹出文档选择对话框
- 文档列表默认按当前项目的 `kb_project` 过滤（调用 `listDocuments(kbProject)`）
- 若无 `kb_project` 则展示全部文档
- 选中文档 → 调用 `extractScopeFromDocument(projectId, docId)`
- 展示候选列表（绿色=范围内，红色=排除，灰色=低置信度）
- 每项可编辑，可删除
- 底部 [确认导入 N 项] 按钮 → 调用 `confirmScopeItems()`

**健康度 Tab**：
- 当前项目无指标时展示"暂无数据，录入指标后自动计算"
- 数据录入时关联当前 project_id

## 5. 实施步骤

### 阶段 1: 数据层（risk_control.rs）

1. 新增 `RiskProject` 结构体
2. `init_tables()` 新增 `risk_projects` 表
3. `contract_scope_items` / `project_health_metrics` 加 `project_id` 列（ALTER TABLE）
4. 现有方法加 `project_id` 参数，SQL 加 `WHERE project_id = ?`
5. 新增项目 CRUD 方法
6. 新增 `extract_scope_from_document()` / `confirm_scope_items()`
7. 单元测试

### 阶段 2: Tauri 命令（lib.rs）

1. 新增 `create_risk_project` / `list_risk_projects` 命令
2. 改造 `add_scope_item` / `list_scope_items` 加 `project_id`
3. 改造 `get_project_health` 加 `project_id`
4. 改造 `check_scope_creep` 加 `project_id`
5. 新增 `extract_scope_from_document` / `confirm_scope_items` 命令
6. 注册到 `generate_handler![]`

### 阶段 3: 前端 API（tauri-commands.ts）

1. 添加所有缺失的类型定义
2. 添加所有缺失的 invoke 函数
3. 修正参数签名

### 阶段 4: 前端 UI（RiskControl.tsx）

1. 项目选择器组件
2. 提取范围弹窗 + 预览确认
3. 各 Tab 按 `projectId` 传递参数

## 6. 数据备份导出导入（整库级）

### 6.1 方案

使用 SQLite 的 `VACUUM INTO` 命令创建一致性整库备份，而非模块级 JSON 导出。`metadata.db` 包含：文档、分块、知识库配置、风险控制数据、研究会话等全部核心数据。

### 6.2 后端

```rust
// 直接操作 metadata.db 的整库备份
export_database(target_path: &str) -> Result<()>   // VACUUM INTO 'path'
import_database(backup_path: &str) -> Result<ImportDbResult>

struct ImportDbResult {
    db_size_bytes: u64,
    document_count: i64,
    chunk_count: i64,
    risk_project_count: i64,
}
```

导出细节：
1. 从 AppState 获取 `metadata.db` 路径
2. 执行 `conn.execute_batch("VACUUM INTO 'target_path'")`
3. VACUUM INTO 在 SQLite 3.27+ 支持，生成独立紧凑的数据库文件，对源库无影响

导入细节：
1. 校验备份文件是合法 SQLite 文件（读取前 16 字节验证 SQLite header）
2. 关闭当前所有服务连接
3. 用备份文件替换当前 `metadata.db`
4. 重新初始化 AppState 中的各服务（MetadataStore, RiskControlStore, ResearchSessionStore 等均自动从新 DB 重建）

### 6.3 Tauri 命令

```rust
export_database(target_path: String) -> Result<()>
import_database(backup_path: String) -> Result<ImportDbResult>
```

### 6.4 前端

- 放置在双轨风险把控舱页面的页脚，也可考虑放在设置页面
- **导出** → 调用 `save` 对话框（默认名 `kingdee-kb-backup-2026-05-26.db`）→ 调用 `exportDatabase(path)`
- **导入** → 确认提示（"导入将替换全部数据，不可撤销，是否继续？"）→ 调用 `open` 对话框选 `.db` 文件 → 调用 `importDatabase(path)` → 刷新页面或提示重启

### 4.3 新增导入安全提示

导入数据库前弹出二次确认：
> "此操作将用备份文件替换当前全部数据（包括知识库文档、风险数据、研究会话等），且不可撤销。建议先导出当前数据作为备份。是否继续？"

确认后执行导入，成功后刷新界面或弹出提示重启应用。

## 7. 规格自检

- [x] 无占位符/TODO？ — 有明确实施步骤
- [x] 内部一致？ — 数据模型/后端/前端改动对齐
- [x] 范围聚焦？ — 聚焦在一个风险控制模块内
- [x] 无歧义？ — 字段类型、API 签名、流程明确

## 8. 实施步骤（更新版）

### 阶段 1: 数据层（risk_control.rs）

1. 新增 `RiskProject` 结构体
2. `init_tables()` 新增 `risk_projects` 表
3. `contract_scope_items` / `project_health_metrics` 加 `project_id` 列（ALTER TABLE）
4. 现有方法加 `project_id` 参数，SQL 加 `WHERE project_id = ?`
5. 新增项目 CRUD 方法
6. 新增 `extract_scope_from_document()` / `confirm_scope_items()`
7. 新增 `export_risk_data()` / `import_risk_data()`
8. 单元测试

### 阶段 2: Tauri 命令（lib.rs）

1. 新增 `create_risk_project` / `list_risk_projects` 命令
2. 改造 `add_scope_item` / `list_scope_items` 加 `project_id`
3. 改造 `get_project_health` 加 `project_id`
4. 改造 `check_scope_creep` 加 `project_id`
5. 新增 `extract_scope_from_document` / `confirm_scope_items` 命令
6. 新增 `export_database` / `import_database` 命令（整库备份）
7. 注册到 `generate_handler![]`

### 阶段 3: 前端 API（tauri-commands.ts）

1. 添加所有缺失的类型定义
2. 添加所有缺失的 invoke 函数
3. 修正参数签名

### 阶段 4: 前端 UI（RiskControl.tsx）

1. 项目选择器组件（含 `kb_project` 字段，关联 KB 项目标签）
2. 提取范围弹窗（按 kb_project 过滤文档）+ 预览确认
3. 各 Tab 按 `projectId` 传递参数
4. 整库导出/导入按钮（文件对话框 + 导入确认提示）

## 9. 备注

- 旧数据（无 project_id）设 project_id=-1 兼容，新建数据强制关联项目
- 文档提取功能依赖 LLM 已配置、文档已入库
- 删除 project 时级联删除关联数据（scope_items + health_metrics）
- 整库导入不可部分回滚，导入前必须二次确认
- 导出文件后缀 `.db`（实际是 SQLite 数据库文件），用户也可用 DB Browser 等工具直接打开查看
- `kb_project` 仅做字符串匹配过滤，不建立外键约束
