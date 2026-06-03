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
pub fn archive_project(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.archive_project(project_id)
}

#[tauri::command]
pub fn ensure_project_active(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.ensure_project_active(project_id)
}
