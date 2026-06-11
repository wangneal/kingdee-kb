use tauri::State;

use crate::app_state::AppState;
use crate::services::prompts::RECIPE_INVESTIGATION;
use crate::services::question_recommend::{
    FollowUpRequest, FollowUpResult, RecommendRequest, RecommendedQuestion,
};
use crate::services::research_outline::Edition;
use crate::services::research_session::{QARecord, ResearchSession, SessionDetail};
use crate::services::smart_completion::SmartFillResult;

/// 获取调研报告配方（4 段硬结构）
///
/// 返回与后端 `prompts::RECIPE_INVESTIGATION` 同源的字符串。前端拼 prompt 时
/// 直接 `await` 拉取，避免硬编码导致配方漂移。
#[tauri::command]
pub fn get_investigation_recipe() -> String {
    RECIPE_INVESTIGATION.to_string()
}

/// 获取当前研究版本
#[tauri::command]
pub fn get_current_edition(state: State<'_, AppState>) -> Result<String, String> {
    let edition = state.edition_config.current();
    Ok(edition.as_str().to_string())
}

/// 切换研究版本
#[tauri::command]
pub fn set_edition(state: State<'_, AppState>, edition: String) -> Result<(), String> {
    let edition =
        Edition::from_str(&edition).ok_or_else(|| format!("Invalid edition: {}", edition))?;
    state.edition_config.set(&edition)
}

/// 列出当前版本的所有已导入研究模块
#[tauri::command]
pub fn list_research_modules(
    state: State<'_, AppState>,
) -> Result<Vec<(i64, String, String)>, String> {
    let edition = state.edition_config.current();
    state.research_indexer.list_outlines(&edition)
}

/// 从目录批量导入研究大纲
#[tauri::command]
pub fn import_research_outlines(state: State<'_, AppState>, dir: String) -> Result<String, String> {
    let edition = state.edition_config.current();
    let result = state
        .research_indexer
        .import_directory(std::path::Path::new(&dir), edition)?;
    let mut summary = format!(
        "导入成功: {} 个模块, {} 个问题\n跳过: {} 个文件",
        result.imported, result.total_questions, result.skipped
    );
    if !result.errors.is_empty() {
        let error_list: Vec<&str> = result.errors.iter().take(3).map(|s| s.as_str()).collect();
        summary.push_str(&format!(
            "\n错误 (前{}个): {}",
            error_list.len(),
            error_list.join("; ")
        ));
    }
    if result.imported == 0 && !result.errors.is_empty() {
        return Err(format!("导入失败: {}", result.errors.join("; ")));
    }
    Ok(summary)
}

/// 根据当前对话主题推荐相关研究问题。
#[tauri::command]
pub fn recommend_questions(
    state: State<'_, AppState>,
    request: RecommendRequest,
) -> Result<Vec<RecommendedQuestion>, String> {
    let edition = state.edition_config.current();
    crate::services::question_recommend::recommend_questions(
        &request,
        &state.research_indexer,
        &edition,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
}

/// 基于已回答的问答对生成后续问题。
#[tauri::command]
pub async fn generate_followup_questions(
    state: State<'_, AppState>,
    request: FollowUpRequest,
) -> Result<FollowUpResult, String> {
    crate::services::question_recommend::generate_followup_questions(
        &request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

/// 使用 KB 上下文 + LLM 智能填充研究问题答案。
#[tauri::command]
pub async fn smart_fill_for_question(
    state: State<'_, AppState>,
    question_text: String,
    project_name: Option<String>,
) -> Result<SmartFillResult, String> {
    let edition = state.edition_config.current();
    crate::services::question_recommend::smart_fill_for_question(
        &question_text,
        &edition,
        project_name.as_deref(),
        &state.research_indexer,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

// ─── 研究会话管理 ───

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

#[tauri::command]
pub fn list_research_sessions(
    state: State<'_, AppState>,
    project_id: Option<i64>,
) -> Result<Vec<ResearchSession>, String> {
    state.research_session_store.list_sessions(project_id)
}

#[tauri::command]
pub fn get_research_session(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<Option<SessionDetail>, String> {
    state.research_session_store.get_session_detail(session_id)
}

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

#[tauri::command]
pub fn delete_research_session(state: State<'_, AppState>, session_id: i64) -> Result<(), String> {
    state.research_session_store.delete_session(session_id)
}

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

#[tauri::command]
pub fn delete_qa_record(state: State<'_, AppState>, record_id: i64) -> Result<(), String> {
    state.research_session_store.delete_record(record_id)
}

#[tauri::command]
pub fn get_session_records(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<Vec<QARecord>, String> {
    state.research_session_store.get_records(session_id)
}

#[tauri::command]
pub fn export_session_csv(state: State<'_, AppState>, session_id: i64) -> Result<String, String> {
    state.research_session_store.export_csv(session_id)
}

#[tauri::command]
pub fn export_session_markdown(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<String, String> {
    state.research_session_store.export_markdown(session_id)
}

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
