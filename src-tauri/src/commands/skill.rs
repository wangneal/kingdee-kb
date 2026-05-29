//! 技能系统 Tauri 命令
//!
//! 提供前端调用的技能管理接口：
//!   - list_skills / get_skill / search_skills

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

use crate::app_state::AppState;
use crate::services::skill_types::Skill;

/// 列出所有技能
#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<Skill>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.list_all())
}

/// 按名称获取技能详情
#[tauri::command]
pub async fn get_skill(state: State<'_, AppState>, name: String) -> Result<Option<Skill>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.get(&name))
}

/// 搜索技能
#[tauri::command]
pub async fn search_skills(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<Skill>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.search(&query))
}

/// 获取技能统计
#[tauri::command]
pub async fn get_skill_stats(state: State<'_, AppState>) -> Result<SkillStatsResponse, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    let by_category = manager.stats().into_iter().collect();
    Ok(SkillStatsResponse {
        total: manager.count(),
        by_category,
    })
}

/// 重新扫描技能目录
#[tauri::command]
pub async fn rescan_skills(state: State<'_, AppState>) -> Result<SkillScanResult, String> {
    let mut manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    manager.scan();
    Ok(SkillScanResult {
        total: manager.count(),
        by_category: manager.stats().into_iter().collect(),
    })
}

/// 匹配最佳技能
#[tauri::command]
pub async fn match_skill(
    state: State<'_, AppState>,
    input: String,
) -> Result<Option<Skill>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.match_best(&input))
}

/// 从一个 SKILL.md 文件导入新技能，复制到 skills/ 目录
#[tauri::command]
pub async fn import_skill(state: State<'_, AppState>, file_path: String) -> Result<String, String> {
    let mut manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    let src = PathBuf::from(&file_path);

    if !src.exists() {
        return Err(format!("文件不存在: {}", file_path));
    }
    if !src.is_file() {
        return Err("路径不是文件".to_string());
    }

    let content = std::fs::read_to_string(&src).map_err(|e| format!("读取文件失败: {}", e))?;

    // 解析技能名
    let (meta, _) = crate::services::skill_manager::SkillManager::parse_skill_md_public(&content);
    let name = import_skill_name(&src, meta.name.as_deref());

    // 复制到 skills/<name>/SKILL.md
    manager.import_skill(&name, &content)
}

fn import_skill_name(src: &PathBuf, metadata_name: Option<&str>) -> String {
    let metadata_name = metadata_name.unwrap_or("").trim();
    if is_safe_import_name(metadata_name) && metadata_name != "kingdee-implementation-suite" {
        return metadata_name.to_string();
    }

    src.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|name| is_safe_import_name(name))
        .or_else(|| {
            src.file_stem()
                .and_then(|n| n.to_str())
                .filter(|name| is_safe_import_name(name))
        })
        .unwrap_or("unknown")
        .to_string()
}

fn is_safe_import_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ─── 响应类型 ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStatsResponse {
    pub total: usize,
    pub by_category: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillScanResult {
    pub total: usize,
    pub by_category: Vec<(String, usize)>,
}
