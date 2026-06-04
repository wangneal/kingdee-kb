use crate::app_state::AppState;
use crate::services::project_store::{Project, ProjectPhase, ProjectSummary};
use tauri::State;

#[tauri::command]
pub fn ensure_default_project(state: State<'_, AppState>) -> Result<i64, String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.ensure_default_project()
}

#[tauri::command]
pub fn create_project(
    state: State<'_, AppState>,
    name: String,
    client_name: Option<String>,
    description: Option<String>,
) -> Result<i64, String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.create_project(
        &name,
        client_name.as_deref().unwrap_or_default(),
        description.as_deref().unwrap_or_default(),
    )
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectSummary>, String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.list_projects()
}

#[tauri::command]
pub fn get_project(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Option<Project>, String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.get_project(project_id)
}

#[tauri::command]
pub fn get_project_phases(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<ProjectPhase>, String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.get_project_phases(project_id)
}

#[tauri::command]
pub fn update_project(
    state: State<'_, AppState>,
    project_id: i64,
    name: String,
    client_name: String,
    description: String,
) -> Result<(), String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.update_project(project_id, &name, &client_name, &description)
}

#[tauri::command]
pub fn update_project_phase_plan(
    state: State<'_, AppState>,
    project_id: i64,
    phase_key: String,
    planned_start: Option<String>,
    planned_end: Option<String>,
) -> Result<(), String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.update_phase_plan(
        project_id,
        &phase_key,
        planned_start.as_deref(),
        planned_end.as_deref(),
    )
}

#[tauri::command]
pub fn archive_project(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.archive_project(project_id)
}

#[tauri::command]
pub fn restore_project(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.restore_project(project_id)
}

#[tauri::command]
pub fn set_current_project_phase(
    state: State<'_, AppState>,
    project_id: i64,
    phase_key: String,
) -> Result<(), String> {
    let mut store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.set_current_phase(project_id, &phase_key)
}

#[tauri::command]
pub fn ensure_project_active(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.ensure_project_active(project_id)
}
