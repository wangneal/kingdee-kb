use tauri::State;

use crate::app_state::AppState;
use crate::services::metadata::AgentSessionSnapshot;
use crate::services::rig_tool::{
    list_skill_permission_rules as list_skill_permission_rules_service, load_rig_tool_config,
    read_recent_tool_audit_records,
    revoke_skill_permission_rule as revoke_skill_permission_rule_service, rig_tool_profiles,
    save_rig_tool_config, summarize_recent_tool_audit_records, RigToolAuditRecord,
    RigToolAuditSummary, RigToolConfig, RigToolOutputContent, RigToolProfileInfo,
    SkillPermissionRuleInfo,
};

#[tauri::command]
pub async fn list_agent_tool_profiles() -> Result<Vec<RigToolProfileInfo>, String> {
    Ok(rig_tool_profiles())
}

#[tauri::command]
pub async fn list_agent_tool_audit(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<RigToolAuditRecord>, String> {
    read_recent_tool_audit_records(&state.data_dir, limit.unwrap_or(50))
}

#[tauri::command]
pub async fn list_agent_tool_audit_summary(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<RigToolAuditSummary, String> {
    summarize_recent_tool_audit_records(&state.data_dir, limit.unwrap_or(200))
}

#[tauri::command]
pub async fn get_agent_tool_config(state: State<'_, AppState>) -> Result<RigToolConfig, String> {
    load_rig_tool_config(&state.data_dir)
}

#[tauri::command]
pub async fn set_agent_tool_config(
    state: State<'_, AppState>,
    config: RigToolConfig,
) -> Result<RigToolConfig, String> {
    save_rig_tool_config(&state.data_dir, config)
}

#[tauri::command]
pub async fn read_agent_tool_output(
    state: State<'_, AppState>,
    output_path: String,
    max_bytes: Option<usize>,
    offset_bytes: Option<u64>,
) -> Result<RigToolOutputContent, String> {
    crate::services::rig_tool::read_saved_tool_output(
        &state.data_dir,
        &output_path,
        max_bytes.unwrap_or(512 * 1024),
        offset_bytes.unwrap_or(0),
    )
}

#[tauri::command]
pub async fn list_skill_permission_rules(
    state: State<'_, AppState>,
) -> Result<Vec<SkillPermissionRuleInfo>, String> {
    list_skill_permission_rules_service(&state.data_dir)
}

#[tauri::command]
pub async fn revoke_skill_permission_rule(
    state: State<'_, AppState>,
    rule: String,
) -> Result<Vec<SkillPermissionRuleInfo>, String> {
    revoke_skill_permission_rule_service(&state.data_dir, &rule)
}

#[tauri::command]
pub async fn get_latest_agent_session(
    state: State<'_, AppState>,
    project_id: i64,
    slot: String,
) -> Result<Option<AgentSessionSnapshot>, String> {
    let metadata = state.metadata.lock().map_err(|e| e.to_string())?;
    metadata.get_latest_agent_session_snapshot(project_id, &slot)
}

#[tauri::command]
pub async fn get_agent_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<AgentSessionSnapshot>, String> {
    let metadata = state.metadata.lock().map_err(|e| e.to_string())?;
    metadata.get_agent_session_snapshot(&session_id)
}
