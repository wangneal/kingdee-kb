use tauri::State;

use crate::app_state::AppState;
use crate::services::research_session::{QARecord, ResearchSession, SessionDetail};

/// 创建调研会话
#[tauri::command]
pub fn create_research_session(
    state: State<'_, AppState>,
    title: String,
    edition: String,
    module_code: String,
    interviewee: String,
    session_date: String,
    project_id: Option<i64>,
) -> Result<i64, String> {
    let project_id = match project_id {
        Some(id) => id,
        None => {
            let store = state.project_store.lock().map_err(|e| e.to_string())?;
            store.ensure_default_project()?
        }
    };
    state.research_session_store.create_session(
        &title,
        &edition,
        &module_code,
        &interviewee,
        &session_date,
        project_id,
    )
}

/// 列出调研会话，可按项目筛选
#[tauri::command]
pub fn list_research_sessions(
    state: State<'_, AppState>,
    project_id: Option<i64>,
) -> Result<Vec<ResearchSession>, String> {
    state.research_session_store.list_sessions(project_id)
}

/// 获取调研会话详情
#[tauri::command]
pub fn get_research_session(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<Option<SessionDetail>, String> {
    state.research_session_store.get_session_detail(session_id)
}

/// 更新调研会话
#[tauri::command]
pub fn update_research_session(
    state: State<'_, AppState>,
    session_id: i64,
    title: String,
    interviewee: String,
    session_date: String,
    status: String,
) -> Result<(), String> {
    state.research_session_store.update_session(
        session_id,
        &title,
        &interviewee,
        &session_date,
        &status,
    )
}

/// 删除调研会话
#[tauri::command]
pub fn delete_research_session(state: State<'_, AppState>, session_id: i64) -> Result<(), String> {
    state.research_session_store.delete_session(session_id)
}

/// 添加问答记录
#[tauri::command]
pub fn add_qa_record(
    state: State<'_, AppState>,
    session_id: i64,
    question_id: Option<i64>,
    question_text: String,
    answer_text: String,
    notes: String,
    sort_order: i32,
) -> Result<i64, String> {
    state.research_session_store.add_record(
        session_id,
        question_id,
        &question_text,
        &answer_text,
        &notes,
        sort_order,
    )
}

/// 更新问答记录
#[tauri::command]
pub fn update_qa_record(
    state: State<'_, AppState>,
    record_id: i64,
    answer_text: String,
    notes: String,
) -> Result<(), String> {
    state
        .research_session_store
        .update_record(record_id, &answer_text, &notes)
}

/// 删除问答记录
#[tauri::command]
pub fn delete_qa_record(state: State<'_, AppState>, record_id: i64) -> Result<(), String> {
    state.research_session_store.delete_record(record_id)
}

/// 获取会话所有问答记录
#[tauri::command]
pub fn get_session_records(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<Vec<QARecord>, String> {
    state.research_session_store.get_records(session_id)
}

/// 导出会话为 CSV
#[tauri::command]
pub fn export_session_csv(state: State<'_, AppState>, session_id: i64) -> Result<String, String> {
    state.research_session_store.export_csv(session_id)
}

/// 导出会话为 Markdown
#[tauri::command]
pub fn export_session_markdown(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<String, String> {
    state.research_session_store.export_markdown(session_id)
}

/// 重排问答记录
#[tauri::command]
pub fn reorder_qa_records(
    state: State<'_, AppState>,
    session_id: i64,
    record_ids: Vec<i64>,
) -> Result<(), String> {
    state
        .research_session_store
        .reorder_records(session_id, &record_ids)
}
