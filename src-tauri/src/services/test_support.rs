//! 测试基础设施：提供共享的 DB setup 函数，消除各 Store 测试里复制粘贴的建表逻辑。
//!
//! 用真实的 ProjectStore 初始化 projects 表（schema 始终与生产一致），
//! 后续 Store 测试用同一 db 文件打开即可，无需手工复制 projects 的 DDL。

#![cfg(test)]

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::services::project_store::ProjectStore;

/// 建临时 DB，用真实 ProjectStore 初始化 projects 表 + 插入一个默认项目。
///
/// 返回 (TempDir, db_path, default_project_id)。
/// 调用方必须持有 TempDir（不能提前 drop），否则临时文件被删除。
pub fn setup_db_with_project() -> (TempDir, PathBuf, i64) {
    let dir = tempfile::tempdir().expect("创建临时目录失败");
    let db_path = dir.path().join("test.db");

    let project_id = init_projects(&db_path);

    (dir, db_path, project_id)
}

/// 在指定 db_path 上用 ProjectStore 建 projects 表并插入默认项目。
/// 返回默认项目的 id。可单独调用（已有自己的 TempDir 管理时）。
fn init_projects(db_path: &Path) -> i64 {
    let store = ProjectStore::new(db_path).expect("创建 ProjectStore 失败");
    let project_id = store.ensure_default_project().expect("创建默认项目失败");
    project_id
}

/// 在已有 db_path 上插入一个指定状态的项目，返回 project_id。
///
/// 用于测试 archived 项目等场景（create_project 默认建 active，建后改 status）。
pub fn insert_project(db_path: &Path, name: &str, status: &str) -> i64 {
    let store = ProjectStore::new(db_path).expect("打开 ProjectStore 失败");
    let project_id = store
        .create_project(name, "", "")
        .expect("创建项目失败");
    if status == "archived" {
        store
            .archive_project(project_id)
            .expect("归档项目失败");
    }
    project_id
}
