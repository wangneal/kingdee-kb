use crate::app_state::AppState;
use crate::services::project_store::{Project, ProjectPhase, ProjectProduct, ProjectStore, ProjectSummary};
use tauri::State;

async fn with_project_store<T, F>(state: State<'_, AppState>, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&mut ProjectStore) -> Result<T, String> + Send + 'static,
{
    let store = state.project_store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = store.lock().map_err(|e| e.to_string())?;
        task(&mut store)
    })
    .await
    .map_err(|e| format!("项目命令执行失败: {}", e))?
}

#[tauri::command]
pub async fn ensure_default_project(state: State<'_, AppState>) -> Result<i64, String> {
    with_project_store(state, |store| store.ensure_default_project()).await
}

#[tauri::command]
pub async fn create_project(
    state: State<'_, AppState>,
    name: String,
    client_name: Option<String>,
    description: Option<String>,
) -> Result<i64, String> {
    with_project_store(state, move |store| {
        store.create_project(
            &name,
            client_name.as_deref().unwrap_or_default(),
            description.as_deref().unwrap_or_default(),
        )
    })
    .await
}

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectSummary>, String> {
    with_project_store(state, |store| store.list_projects()).await
}

#[tauri::command]
pub async fn get_project(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Option<Project>, String> {
    with_project_store(state, move |store| store.get_project(project_id)).await
}

#[tauri::command]
pub async fn get_project_phases(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<ProjectPhase>, String> {
    with_project_store(state, move |store| store.get_project_phases(project_id)).await
}

#[tauri::command]
pub async fn update_project(
    state: State<'_, AppState>,
    project_id: i64,
    name: String,
    client_name: String,
    description: String,
) -> Result<(), String> {
    with_project_store(state, move |store| {
        store.update_project(project_id, &name, &client_name, &description)
    })
    .await
}

#[tauri::command]
pub async fn update_project_phase_plan(
    state: State<'_, AppState>,
    project_id: i64,
    phase_key: String,
    planned_start: Option<String>,
    planned_end: Option<String>,
) -> Result<(), String> {
    with_project_store(state, move |store| {
        store.update_phase_plan(
            project_id,
            &phase_key,
            planned_start.as_deref(),
            planned_end.as_deref(),
        )
    })
    .await
}

#[tauri::command]
pub async fn archive_project(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    with_project_store(state, move |store| store.archive_project(project_id)).await
}

#[tauri::command]
pub async fn restore_project(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    with_project_store(state, move |store| store.restore_project(project_id)).await
}

#[tauri::command]
pub async fn set_current_project_phase(
    state: State<'_, AppState>,
    project_id: i64,
    phase_key: String,
) -> Result<(), String> {
    with_project_store(state, move |store| {
        store.set_current_phase(project_id, &phase_key)
    })
    .await
}

#[tauri::command]
pub async fn ensure_project_active(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<(), String> {
    with_project_store(state, move |store| store.ensure_project_active(project_id)).await
}

#[tauri::command]
pub async fn list_project_products(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<ProjectProduct>, String> {
    with_project_store(state, move |store| store.list_project_products(project_id)).await
}

#[tauri::command]
pub async fn add_project_product(
    state: State<'_, AppState>,
    project_id: i64,
    product_name: String,
    product_version: String,
) -> Result<i64, String> {
    with_project_store(state, move |store| {
        store.add_project_product(project_id, &product_name, &product_version)
    })
    .await
}

#[tauri::command]
pub async fn delete_project_product(
    state: State<'_, AppState>,
    project_id: i64,
    product_id: i64,
) -> Result<(), String> {
    with_project_store(state, move |store| {
        store.delete_project_product(project_id, product_id)
    })
    .await
}
