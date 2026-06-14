use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PHASE_KEYS: [&str; 7] = [
    "survey",
    "blueprint",
    "development",
    "testing",
    "go_live",
    "acceptance",
    "closed",
];

const PHASE_NAMES: [&str; 7] = ["调研", "蓝图", "开发", "测试", "上线", "验收", "关闭"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub client_name: String,
    pub description: String,
    pub status: String,
    pub current_phase: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPhase {
    pub id: i64,
    pub project_id: i64,
    pub phase_key: String,
    pub phase_name: String,
    pub phase_index: i64,
    pub status: String,
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    pub actual_start: Option<String>,
    pub actual_end: Option<String>,
}

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

/// 项目产品条目（金蝶产品版本）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectProduct {
    pub id: i64,
    pub project_id: i64,
    pub product_name: String,
    pub product_version: String,
}

pub struct ProjectStore {
    db: Connection,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl ProjectStore {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, String> {
        let db_path = db_path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建数据库目录失败: {}", e))?;
        }

        let db = Connection::open(&db_path).map_err(|e| format!("打开项目数据库失败: {}", e))?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("设置数据库忙超时失败: {}", e))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("初始化项目数据库配置失败: {}", e))?;

        let store = Self { db, db_path };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), String> {
        self.db
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS projects (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    client_name TEXT NOT NULL DEFAULT '',
                    description TEXT NOT NULL DEFAULT '',
                    status TEXT NOT NULL DEFAULT 'active',
                    current_phase TEXT NOT NULL DEFAULT 'survey',
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                    CHECK (status IN ('active', 'archived'))
                );

                CREATE INDEX IF NOT EXISTS idx_projects_status ON projects(status);

                CREATE TABLE IF NOT EXISTS project_phases (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                    phase_key TEXT NOT NULL,
                    phase_name TEXT NOT NULL,
                    phase_index INTEGER NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    planned_start TEXT,
                    planned_end TEXT,
                    actual_start TEXT,
                    actual_end TEXT,
                    UNIQUE(project_id, phase_key),
                    UNIQUE(project_id, phase_index),
                    CHECK (status IN ('pending', 'current', 'completed'))
                );

                CREATE UNIQUE INDEX IF NOT EXISTS idx_project_phases_current
                    ON project_phases(project_id)
                    WHERE status = 'current';

                CREATE INDEX IF NOT EXISTS idx_project_phases_project_id
                    ON project_phases(project_id, phase_index);

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
                ",
            )
            .map_err(|e| format!("初始化项目表失败: {}", e))
    }

    pub fn ensure_default_project(&self) -> Result<i64, String> {
        self.db
            .execute(
                "INSERT INTO projects (name, client_name, description, status, current_phase)
                 VALUES ('默认项目', '', '系统初始化项目', 'active', 'survey')
                 ON CONFLICT(name) DO UPDATE SET status = 'active'",
                [],
            )
            .map_err(|e| format!("创建默认项目失败: {}", e))?;

        let project_id = self
            .db
            .query_row(
                "SELECT id FROM projects WHERE name = '默认项目' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("读取默认项目失败: {}", e))?;

        let project_ids = {
            let mut stmt = self
                .db
                .prepare("SELECT id FROM projects WHERE status = 'active'")
                .map_err(|e| format!("读取活动项目失败: {}", e))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, i64>(0))
                .map_err(|e| format!("查询活动项目失败: {}", e))?;
            let mut ids = Vec::new();
            for id in rows {
                ids.push(id.map_err(|e| format!("读取活动项目 ID 失败: {}", e))?);
            }
            ids
        };
        for id in project_ids {
            self.ensure_project_phases(id)?;
        }
        Ok(project_id)
    }

    pub fn create_project(
        &self,
        name: &str,
        client_name: &str,
        description: &str,
    ) -> Result<i64, String> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err("项目名称不能为空".to_string());
        }

        self.db
            .execute(
                "INSERT INTO projects (name, client_name, description, status, current_phase)
                 VALUES (?1, ?2, ?3, 'active', 'survey')",
                params![trimmed_name, client_name.trim(), description.trim()],
            )
            .map_err(|e| format!("创建项目失败: {}", e))?;

        let project_id = self.db.last_insert_rowid();
        self.ensure_project_phases(project_id)?;
        Ok(project_id)
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>, String> {
        let document_count = "(SELECT COUNT(*) FROM documents d WHERE d.project_id = p.id)";
        let wiki_count = "(SELECT COUNT(*) FROM wiki_pages w WHERE w.project_id = p.id)";
        let product_count = "(SELECT COUNT(*) FROM products pr WHERE pr.project_id = p.id)";
        let risk_scope_count =
            "(SELECT COUNT(*) FROM contract_scope_items s WHERE s.project_id = p.id)";
        let risk_metric_count =
            "(SELECT COUNT(*) FROM project_health_metrics h WHERE h.project_id = p.id)";
        let sql = format!(
            "SELECT
                p.id,
                p.name,
                p.client_name,
                p.current_phase,
                p.status,
                {},
                {},
                {},
                ({} + {}),
                p.created_at
             FROM projects p
             ORDER BY p.status ASC, p.updated_at DESC, p.id DESC",
            document_count, wiki_count, product_count, risk_scope_count, risk_metric_count
        );

        let mut stmt = self
            .db
            .prepare(&sql)
            .map_err(|e| format!("准备读取项目列表失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ProjectSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    client_name: row.get(2)?,
                    current_phase: row.get(3)?,
                    status: row.get(4)?,
                    document_count: row.get(5)?,
                    wiki_count: row.get(6)?,
                    product_count: row.get(7)?,
                    risk_count: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })
            .map_err(|e| format!("读取项目列表失败: {}", e))?;

        let mut projects = Vec::new();
        for project in rows {
            projects.push(project.map_err(|e| format!("转换项目列表失败: {}", e))?);
        }
        Ok(projects)
    }

    pub fn get_project(&self, project_id: i64) -> Result<Option<Project>, String> {
        self.db
            .query_row(
                "SELECT id, name, client_name, description, status, current_phase, created_at, updated_at
                 FROM projects WHERE id = ?1",
                params![project_id],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        client_name: row.get(2)?,
                        description: row.get(3)?,
                        status: row.get(4)?,
                        current_phase: row.get(5)?,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(|e| format!("读取项目失败: {}", e))
    }

    // ─── 项目产品（金蝶产品版本）CRUD ───

    pub fn list_project_products(&self, project_id: i64) -> Result<Vec<ProjectProduct>, String> {
        self.db
            .prepare(
                "SELECT id, project_id, product_name, product_version FROM project_products WHERE project_id = ?1 ORDER BY id",
            )
            .map_err(|e| format!("查询项目产品失败: {}", e))?
            .query_map(params![project_id], |row| {
                Ok(ProjectProduct {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    product_name: row.get(2)?,
                    product_version: row.get(3)?,
                })
            })
            .map_err(|e| format!("读取项目产品失败: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("解析项目产品失败: {}", e))
    }

    pub fn add_project_product(
        &self,
        project_id: i64,
        product_name: &str,
        product_version: &str,
    ) -> Result<i64, String> {
        self.ensure_project_active(project_id)?;
        let name = product_name.trim();
        let version = product_version.trim();
        if name.is_empty() {
            return Err("产品名称不能为空".to_string());
        }
        self.db
            .execute(
                "INSERT INTO project_products (project_id, product_name, product_version) VALUES (?1, ?2, ?3)
                 ON CONFLICT(project_id, product_name) DO UPDATE SET product_version = ?3",
                params![project_id, name, version],
            )
            .map_err(|e| format!("添加项目产品失败: {}", e))?;
        // upsert 后按唯一键查询真实 id，避免 last_insert_rowid() 在 ON CONFLICT 时返回错误值
        self.db
            .query_row(
                "SELECT id FROM project_products WHERE project_id = ?1 AND product_name = ?2",
                params![project_id, name],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| format!("获取产品 ID 失败: {}", e))
    }

    pub fn delete_project_product(&self, project_id: i64, product_id: i64) -> Result<(), String> {
        self.ensure_project_active(project_id)?;
        let affected = self
            .db
            .execute(
                "DELETE FROM project_products WHERE id = ?1 AND project_id = ?2",
                params![product_id, project_id],
            )
            .map_err(|e| format!("删除项目产品失败: {}", e))?;
        if affected == 0 {
            return Err("产品记录不存在或不属于当前项目".to_string());
        }
        Ok(())
    }

    pub fn get_project_phases(&self, project_id: i64) -> Result<Vec<ProjectPhase>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, project_id, phase_key, phase_name, phase_index, status, planned_start, planned_end, actual_start, actual_end
                 FROM project_phases WHERE project_id = ?1 ORDER BY phase_index ASC",
            )
            .map_err(|e| format!("准备读取项目阶段失败: {}", e))?;

        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(ProjectPhase {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    phase_key: row.get(2)?,
                    phase_name: row.get(3)?,
                    phase_index: row.get(4)?,
                    status: row.get(5)?,
                    planned_start: row.get(6)?,
                    planned_end: row.get(7)?,
                    actual_start: row.get(8)?,
                    actual_end: row.get(9)?,
                })
            })
            .map_err(|e| format!("读取项目阶段失败: {}", e))?;

        let mut phases = Vec::new();
        for phase in rows {
            phases.push(phase.map_err(|e| format!("转换项目阶段失败: {}", e))?);
        }
        Ok(phases)
    }

    pub fn update_project(
        &self,
        project_id: i64,
        name: &str,
        client_name: &str,
        description: &str,
    ) -> Result<(), String> {
        self.ensure_project_active(project_id)?;
        let name = name.trim();
        if name.is_empty() {
            return Err("项目名称不能为空".to_string());
        }
        if self
            .get_project(project_id)?
            .is_some_and(|project| project.name == "默认项目" && name != "默认项目")
        {
            return Err("默认项目不能改名".to_string());
        }
        self.db
            .execute(
                "UPDATE projects SET name = ?2, client_name = ?3, description = ?4,
                 updated_at = datetime('now') WHERE id = ?1",
                params![project_id, name, client_name.trim(), description.trim()],
            )
            .map_err(|e| format!("更新项目详情失败: {}", e))?;
        Ok(())
    }

    pub fn update_phase_plan(
        &self,
        project_id: i64,
        phase_key: &str,
        planned_start: Option<&str>,
        planned_end: Option<&str>,
    ) -> Result<(), String> {
        self.ensure_project_active(project_id)?;
        if !PHASE_KEYS.contains(&phase_key) {
            return Err(format!("无效项目阶段: {}", phase_key));
        }
        if matches!((planned_start, planned_end), (Some(start), Some(end)) if end < start) {
            return Err("计划结束日期不能早于计划开始日期".to_string());
        }
        self.db
            .execute(
                "UPDATE project_phases SET planned_start = ?3, planned_end = ?4
                 WHERE project_id = ?1 AND phase_key = ?2",
                params![project_id, phase_key, planned_start, planned_end],
            )
            .map_err(|e| format!("更新阶段计划失败: {}", e))?;
        Ok(())
    }

    pub fn archive_project(&self, project_id: i64) -> Result<(), String> {
        if self
            .get_project(project_id)?
            .is_some_and(|project| project.name == "默认项目")
        {
            return Err("默认项目不能归档".to_string());
        }
        let rows = self
            .db
            .execute(
                "UPDATE projects SET status = 'archived', updated_at = datetime('now') WHERE id = ?1",
                params![project_id],
            )
            .map_err(|e| format!("归档项目失败: {}", e))?;
        if rows == 0 {
            return Err(format!("项目不存在: {}", project_id));
        }
        Ok(())
    }

    pub fn restore_project(&self, project_id: i64) -> Result<(), String> {
        let rows = self
            .db
            .execute(
                "UPDATE projects SET status = 'active', updated_at = datetime('now') WHERE id = ?1",
                params![project_id],
            )
            .map_err(|e| format!("恢复项目失败: {}", e))?;
        if rows == 0 {
            return Err(format!("项目不存在: {}", project_id));
        }
        Ok(())
    }

    /// 硬删除项目：级联清空所有子表数据并删除 `projects` 行。
    ///
    /// 依赖子表 `ON DELETE CASCADE` 外键约束自动级联：
    /// documents / document_chunks / raw_sources / project_products / project_phases /
    /// wiki_pages / meeting_records / ingest_cache / analysis_cache /
    /// project_default_documents 等。
    ///
    /// 事务：用 `unchecked_transaction()` 包裹 `DELETE FROM projects WHERE id=?`，
    /// 失败显式 rollback（drop guard 不会自动 rollback）。
    ///
    /// 校验：默认项目拒绝（与 `archive_project` 一致）；调用方应先校验"无 pending 队列"。
    /// 物理文件（`data_dir/raw/<project_id>/`）由调用方在事务成功后清理。
    pub fn delete_project(&self, project_id: i64) -> Result<(), String> {
        if self
            .get_project(project_id)?
            .is_some_and(|p| p.name == "默认项目")
        {
            return Err("默认项目不能删除".to_string());
        }
        let tx = self
            .db
            .unchecked_transaction()
            .map_err(|e| format!("启动项目删除事务失败: {}", e))?;
        let body: Result<(), String> = (|| {
            let rows = tx
                .execute("DELETE FROM projects WHERE id = ?1", params![project_id])
                .map_err(|e| format!("删除项目行失败: {}", e))?;
            if rows == 0 {
                return Err(format!("项目不存在: {}", project_id));
            }
            Ok(())
        })();
        match body {
            Ok(()) => tx
                .commit()
                .map_err(|e| format!("提交项目删除事务失败: {}", e))?,
            Err(e) => {
                // rollback 失败不回写 — 事务会随 drop 隐式回滚，但显式回滚更稳妥
                let _ = tx.rollback();
                return Err(e);
            }
        }
        Ok(())
    }

    /// 设置项目的当前阶段。
    ///
    /// 用 `unchecked_transaction()` 包裹 3 步写入，**使用 `&self`**（其他方法都 `&self`），
    /// 允许通过共享引用（`Arc<Mutex<ProjectStore>>`）并发调用
    pub fn set_current_phase(&self, project_id: i64, phase_key: &str) -> Result<(), String> {
        let phase_index = PHASE_KEYS
            .iter()
            .position(|key| *key == phase_key)
            .ok_or_else(|| format!("无效项目阶段: {}", phase_key))?
            as i64;

        self.ensure_project_active(project_id)?;
        let transaction = self
            .db
            .unchecked_transaction()
            .map_err(|e| format!("启动项目阶段事务失败: {}", e))?;
        // 把事务体收集为 Result，失败时显式 rollback
        // （unchecked_transaction 的 guard 在 drop 时不会自动回滚）
        let body: Result<(), String> = (|| {
            transaction
                .execute(
                    "UPDATE project_phases
                     SET status = CASE
                         WHEN phase_index < ?2 THEN 'completed'
                         ELSE 'pending'
                     END,
                     actual_start = CASE
                         WHEN phase_index <= ?2 THEN COALESCE(actual_start, datetime('now'))
                         ELSE NULL
                     END,
                     actual_end = CASE
                         WHEN phase_index < ?2 THEN COALESCE(actual_end, datetime('now'))
                         ELSE NULL
                     END
                     WHERE project_id = ?1",
                    params![project_id, phase_index],
                )
                .map_err(|e| format!("更新项目阶段状态失败: {}", e))?;
            transaction
                .execute(
                    "UPDATE project_phases
                     SET status = 'current', actual_start = COALESCE(actual_start, datetime('now'))
                     WHERE project_id = ?1 AND phase_index = ?2",
                    params![project_id, phase_index],
                )
                .map_err(|e| format!("设置项目当前阶段失败: {}", e))?;
            transaction
                .execute(
                    "UPDATE projects
                     SET current_phase = ?2, updated_at = datetime('now')
                     WHERE id = ?1",
                    params![project_id, phase_key],
                )
                .map_err(|e| format!("更新项目当前阶段失败: {}", e))?;
            Ok(())
        })();
        if let Err(e) = body {
            let _ = transaction.rollback();
            return Err(e);
        }
        transaction
            .commit()
            .map_err(|e| format!("提交项目阶段事务失败: {}", e))
    }

    pub fn ensure_project_active(&self, project_id: i64) -> Result<(), String> {
        match self.get_project(project_id)? {
            Some(project) if project.status == "active" => Ok(()),
            Some(_) => Err("项目已归档，不可修改".to_string()),
            None => Err(format!("项目不存在: {}", project_id)),
        }
    }

    fn ensure_project_phases(&self, project_id: i64) -> Result<(), String> {
        for (index, phase_key) in PHASE_KEYS.iter().enumerate() {
            let status = if index == 0 { "current" } else { "pending" };
            self.db
                .execute(
                    "INSERT OR IGNORE INTO project_phases (project_id, phase_key, phase_name, phase_index, status, actual_start)
                     VALUES (?1, ?2, ?3, ?4, ?5, CASE WHEN ?5 = 'current' THEN datetime('now') ELSE NULL END)",
                    params![project_id, phase_key, PHASE_NAMES[index], index as i64, status],
                )
                .map_err(|e| format!("初始化项目阶段失败: {}", e))?;
        }
        Ok(())
    }
}

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

    #[test]
    fn ensure_default_project_returns_named_default_project() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("metadata.db");
        let store = ProjectStore::new(&db_path).expect("创建项目存储失败");
        let other_project_id = store
            .create_project("已有项目", "", "")
            .expect("创建已有项目失败");

        let default_project_id = store.ensure_default_project().expect("创建默认项目失败");
        let default_project = store
            .get_project(default_project_id)
            .expect("读取默认项目失败")
            .expect("默认项目应存在");

        assert_ne!(default_project_id, other_project_id);
        assert_eq!(default_project.name, "默认项目");
    }

    #[test]
    fn archived_project_is_rejected_by_active_check() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("metadata.db");
        let store = ProjectStore::new(&db_path).expect("创建项目存储失败");

        let project_id = store
            .create_project("待归档项目", "", "")
            .expect("创建项目失败");
        store.archive_project(project_id).expect("归档项目失败");

        let err = store
            .ensure_project_active(project_id)
            .expect_err("已归档项目应拒绝写入");
        assert_eq!(err, "项目已归档，不可修改");
    }

    #[test]
    fn archived_project_can_be_restored() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("metadata.db");
        let store = ProjectStore::new(&db_path).expect("创建项目存储失败");
        let project_id = store
            .create_project("可归档项目", "", "")
            .expect("创建项目失败");

        store.archive_project(project_id).expect("归档项目失败");
        store.restore_project(project_id).expect("恢复项目失败");

        let project = store
            .get_project(project_id)
            .expect("读取项目失败")
            .expect("项目应存在");
        assert_eq!(project.status, "active");
    }

    #[test]
    fn create_project_adds_project_and_phases() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("metadata.db");
        let store = ProjectStore::new(&db_path).expect("创建项目存储失败");

        let project_id = store
            .create_project("星空实施", "深圳客户", "一期上线")
            .expect("创建项目失败");
        let project = store
            .get_project(project_id)
            .expect("读取项目失败")
            .expect("项目应存在");
        let phases = store.get_project_phases(project_id).expect("读取阶段失败");

        assert_eq!(project.name, "星空实施");
        assert_eq!(project.client_name, "深圳客户");
        assert_eq!(project.current_phase, "survey");
        assert_eq!(phases.len(), 7);
        assert_eq!(phases[0].phase_index, 0);
    }

    #[test]
    fn list_projects_returns_real_project_counts() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("metadata.db");
        let store = ProjectStore::new(&db_path).expect("创建项目存储失败");
        let project_id = store.ensure_default_project().expect("创建默认项目失败");
        store
            .db
            .execute_batch(
                "
                CREATE TABLE documents (id INTEGER PRIMARY KEY, project_id INTEGER);
                CREATE TABLE wiki_pages (id INTEGER PRIMARY KEY, project_id INTEGER);
                CREATE TABLE products (id INTEGER PRIMARY KEY, project_id INTEGER);
                CREATE TABLE contract_scope_items (id INTEGER PRIMARY KEY, project_id INTEGER);
                CREATE TABLE project_health_metrics (id INTEGER PRIMARY KEY, project_id INTEGER);
                ",
            )
            .expect("创建统计表失败");
        for table in ["documents", "wiki_pages", "products"] {
            store
                .db
                .execute(
                    &format!("INSERT INTO {} (project_id) VALUES (?1)", table),
                    params![project_id],
                )
                .expect("插入项目统计数据失败");
        }
        store
            .db
            .execute(
                "INSERT INTO contract_scope_items (project_id) VALUES (?1)",
                params![project_id],
            )
            .expect("插入范围统计数据失败");
        store
            .db
            .execute(
                "INSERT INTO project_health_metrics (project_id) VALUES (?1)",
                params![project_id],
            )
            .expect("插入健康统计数据失败");

        let projects = store.list_projects().expect("读取项目列表失败");
        assert_eq!(projects[0].document_count, 1);
        assert_eq!(projects[0].wiki_count, 1);
        assert_eq!(projects[0].product_count, 1);
        assert_eq!(projects[0].risk_count, 2);
    }

    #[test]
    fn set_current_phase_updates_project_and_phase_statuses() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("metadata.db");
        let store = ProjectStore::new(&db_path).expect("创建项目存储失败");
        let project_id = store.ensure_default_project().expect("创建默认项目失败");

        store
            .set_current_phase(project_id, "testing")
            .expect("更新当前阶段失败");

        let project = store
            .get_project(project_id)
            .expect("读取项目失败")
            .expect("项目应存在");
        let phases = store.get_project_phases(project_id).expect("读取阶段失败");
        assert_eq!(project.current_phase, "testing");
        assert_eq!(phases[0].status, "completed");
        assert_eq!(phases[3].status, "current");
        assert_eq!(phases[4].status, "pending");
    }

    #[test]
    fn updates_project_details_and_phase_plan() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("metadata.db");
        let store = ProjectStore::new(&db_path).expect("创建项目存储失败");
        let project_id = store
            .create_project("旧名称", "", "")
            .expect("创建项目失败");
        store
            .update_project(project_id, "新名称", "客户甲", "项目描述")
            .expect("更新项目详情失败");
        store
            .update_phase_plan(
                project_id,
                "blueprint",
                Some("2026-06-01"),
                Some("2026-06-30"),
            )
            .expect("更新阶段计划失败");
        let project = store.get_project(project_id).unwrap().unwrap();
        let phases = store.get_project_phases(project_id).unwrap();
        assert_eq!(project.name, "新名称");
        assert_eq!(project.client_name, "客户甲");
        assert_eq!(phases[1].planned_start.as_deref(), Some("2026-06-01"));
        assert_eq!(phases[1].planned_end.as_deref(), Some("2026-06-30"));
    }
}
