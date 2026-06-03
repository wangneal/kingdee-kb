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

const PHASE_NAMES: [&str; 7] = [
    "调研",
    "蓝图",
    "开发",
    "测试",
    "上线",
    "验收",
    "关闭",
];

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
                "INSERT OR IGNORE INTO projects (name, client_name, description, status, current_phase)
                 VALUES ('默认项目', '', '系统初始化项目', 'active', 'survey')",
                [],
            )
            .map_err(|e| format!("创建默认项目失败: {}", e))?;

        let project_id = self
            .db
            .query_row(
                "SELECT id FROM projects WHERE status = 'active' ORDER BY id ASC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("读取默认项目失败: {}", e))?;
        self.ensure_project_phases(project_id)?;
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
        let sql = if self.table_exists("products")? {
            "SELECT
                p.id,
                p.name,
                p.client_name,
                p.current_phase,
                p.status,
                COALESCE(COUNT(pr.id), 0) AS product_count,
                p.created_at
             FROM projects p
             LEFT JOIN products pr ON pr.project_id = p.id
             GROUP BY p.id
             ORDER BY p.status ASC, p.updated_at DESC, p.id DESC"
        } else {
            "SELECT
                p.id,
                p.name,
                p.client_name,
                p.current_phase,
                p.status,
                0 AS product_count,
                p.created_at
             FROM projects p
             ORDER BY p.status ASC, p.updated_at DESC, p.id DESC"
        };

        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("准备读取项目列表失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ProjectSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    client_name: row.get(2)?,
                    current_phase: row.get(3)?,
                    status: row.get(4)?,
                    document_count: 0,
                    wiki_count: 0,
                    product_count: row.get(5)?,
                    risk_count: 0,
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| format!("读取项目列表失败: {}", e))?;

        let mut projects = Vec::new();
        for project in rows {
            projects.push(project.map_err(|e| format!("转换项目列表失败: {}", e))?);
        }
        Ok(projects)
    }

    fn table_exists(&self, table_name: &str) -> Result<bool, String> {
        self.db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                params![table_name],
                |row| row.get::<_, i64>(0),
            )
            .map(|exists| exists == 1)
            .map_err(|e| format!("检查数据表失败: {}", e))
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

    pub fn archive_project(&self, project_id: i64) -> Result<(), String> {
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
    fn archived_project_is_rejected_by_active_check() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("metadata.db");
        let store = ProjectStore::new(&db_path).expect("创建项目存储失败");

        let project_id = store.ensure_default_project().expect("创建默认项目失败");
        store.archive_project(project_id).expect("归档项目失败");

        let err = store
            .ensure_project_active(project_id)
            .expect_err("已归档项目应拒绝写入");
        assert_eq!(err, "项目已归档，不可修改");
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
    fn list_projects_works_before_products_table_exists() {
        let dir = tempdir().expect("创建临时目录失败");
        let db_path = dir.path().join("metadata.db");
        let store = ProjectStore::new(&db_path).expect("创建项目存储失败");

        let project_id = store.ensure_default_project().expect("创建默认项目失败");
        let projects = store.list_projects().expect("读取项目列表失败");

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, project_id);
        assert_eq!(projects[0].product_count, 0);
    }
}
