use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::services::question_tool;
use crate::services::react_agent::ReActEvent;
use crate::services::risk_control::{ContractScopeItem, ScopeCreepResult, ProjectHealthScore, DefenseScriptRequest, DefenseScriptResult, RiskProject, ImportDbResult, CandidateScopeItem};

// ─── P1: 双轨风险把控舱 ───

#[tauri::command]
pub fn add_scope_item(
    state: State<'_, AppState>,
    project_id: i64,
    category: String,
    description: String,
    is_in_scope: bool,
    detail: String,
) -> Result<i64, String> {
    state.risk_control_store.add_scope_item(project_id, &category, &description, is_in_scope, &detail)
}

#[tauri::command]
pub fn list_scope_items(state: State<'_, AppState>, project_id: i64) -> Result<Vec<ContractScopeItem>, String> {
    state.risk_control_store.list_scope_items(project_id, None, None)
}

#[tauri::command]
pub fn delete_scope_item(state: State<'_, AppState>, item_id: i64) -> Result<(), String> {
    state.risk_control_store.delete_scope_item(item_id)
}

#[tauri::command]
pub async fn check_scope_creep(
    state: State<'_, AppState>,
    project_id: i64,
    requirement: String,
) -> Result<ScopeCreepResult, String> {
    state.risk_control_store.check_scope_creep(project_id, &state.llm, &requirement).await
}

#[tauri::command]
pub fn record_health_metric(
    state: State<'_, AppState>,
    project_id: i64,
    indicator_type: String,
    value: f64,
    notes: String,
) -> Result<i64, String> {
    state.risk_control_store.record_health_metric(project_id, &indicator_type, value, &notes)
}

#[tauri::command]
pub fn get_project_health(state: State<'_, AppState>, project_id: i64) -> Result<ProjectHealthScore, String> {
    state.risk_control_store.calculate_health_score(project_id)
}

#[tauri::command]
pub async fn generate_risk_report(
    state: State<'_, AppState>,
    context: String,
) -> Result<String, String> {
    state.risk_control_store.generate_risk_report(&state.llm, &context).await
}

#[tauri::command]
pub async fn generate_defense_script(
    state: State<'_, AppState>,
    request: DefenseScriptRequest,
) -> Result<DefenseScriptResult, String> {
    state.risk_control_store.generate_defense_script(&state.llm, &request).await
}

// --- P1.4: 风险项目管理 ---

#[tauri::command]
pub fn create_risk_project(
    state: State<'_, AppState>,
    name: String,
    client_name: Option<String>,
    kb_project: Option<String>,
) -> Result<i64, String> {
    state.risk_control_store.create_risk_project(
        &name,
        &client_name.unwrap_or_default(),
        &kb_project.unwrap_or_default(),
    )
}

#[tauri::command]
pub fn list_risk_projects(state: State<'_, AppState>) -> Result<Vec<RiskProject>, String> {
    state.risk_control_store.list_risk_projects()
}

#[tauri::command]
pub fn delete_risk_project(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    state.risk_control_store.delete_risk_project(project_id)
}

// --- P1.5: 合同范围提取 ---

#[tauri::command]
pub async fn extract_scope_from_document(
    state: State<'_, AppState>,
    _project_id: i64,
    doc_id: i64,
) -> Result<Vec<CandidateScopeItem>, String> {
    let chunks = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.get_chunks_by_document(doc_id)?
    };
    if chunks.is_empty() {
        return Err("文档中未找到任何内容分块".to_string());
    }
    state.risk_control_store.extract_scope_from_document(&state.llm, &chunks).await
}

#[tauri::command]
pub fn confirm_scope_items(
    state: State<'_, AppState>,
    project_id: i64,
    items: Vec<CandidateScopeItem>,
) -> Result<usize, String> {
    state.risk_control_store.confirm_scope_items(project_id, &items)
}

// --- P1.6: 整库备份 ---

#[tauri::command]
pub fn export_database(state: State<'_, AppState>, target_path: String) -> Result<(), String> {
    state.risk_control_store.export_database(&target_path)
}

#[tauri::command]
pub fn import_database(
    state: State<'_, AppState>,
    backup_path: String,
) -> Result<ImportDbResult, String> {
    state.risk_control_store.import_database(&backup_path)
}

// --- P2.1: 本地脱敏 ---

#[tauri::command]
pub fn desensitize_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<crate::services::desensitize::DesensitizeResult, String> {
    let result = state.desensitizer.desensitize(&text);
    Ok(crate::services::desensitize::DesensitizeResult {
        safe_text: result.safe_text,
        mapping: result.mapping,
    })
}

#[tauri::command]
pub fn add_sensitive_keyword(state: State<'_, AppState>, keyword: String) -> Result<(), String> {
    state.desensitizer.add_keyword(&keyword);
    Ok(())
}

#[tauri::command]
pub fn list_sensitive_keywords(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.desensitizer.get_keywords())
}

#[tauri::command]
pub fn remove_sensitive_keyword(state: State<'_, AppState>, keyword: String) -> Result<bool, String> {
    Ok(state.desensitizer.remove_keyword(&keyword))
}

// --- P2.2: 蓝图提炼 ---

const BLUEPRINT_SYSTEM_PROMPT: &str = "\
你是一个金蝶ERP业务架构师。根据调研记录提炼业务蓝图。\n\
按四段结构：\n\
1.【现有流程 As-Is】具体流程步骤和角色\n\
2.【系统标准流程 To-Be】含系统路径\n\
3.【差异配置点】配置路径: 配置值\n\
4.【对应单据类型】单据名称（编码规则）\n\
禁止空话，不确定写[待确认]";

#[tauri::command]
pub async fn extract_blueprint(
    state: State<'_, AppState>,
    research_context: String,
) -> Result<String, String> {
    use crate::services::llm_service::ChatMessage;
    let messages = vec![
        ChatMessage { role: "system".to_string(), content: BLUEPRINT_SYSTEM_PROMPT.to_string() },
        ChatMessage { role: "user".to_string(), content: research_context },
    ];
    let config = state.llm.get_config()?;
    state.llm.chat_completion(&messages, &config).await
}

// --- P2.3: Fit-Gap 分析 ---

const FITGAP_SYSTEM_PROMPT: &str = "\
你是一个ERP差异分析专家。分析以下需求，每项判断Fit/Gap。\n\
严格Markdown表格：|序号|需求|分类|Fit/Gap|理由|建议方案|\n\
理由必须具体到模块功能，建议必须可执行。";

#[tauri::command]
pub async fn analyze_fit_gap(
    state: State<'_, AppState>,
    requirements: String,
) -> Result<String, String> {
    use crate::services::llm_service::ChatMessage;
    let messages = vec![
        ChatMessage { role: "system".to_string(), content: FITGAP_SYSTEM_PROMPT.to_string() },
        ChatMessage { role: "user".to_string(), content: requirements },
    ];
    let config = state.llm.get_config()?;
    state.llm.chat_completion(&messages, &config).await
}

// --- ReAct 对话 ---

#[tauri::command]
pub async fn react_chat(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    message: String,
    system_extra: String,
    session_id: String,
) -> Result<(), String> {
    use tokio::sync::mpsc;

    let (tx, mut rx) = mpsc::unbounded_channel::<ReActEvent>();

    let sid = session_id;

    let pending = state.pending_questions.clone();
    let ah = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let state = ah.state::<AppState>();
        crate::services::rig_agent::RigAgent::run(
            &state.llm,
            &message,
            &system_extra,
            &[],
            tx,
            &sid,
            pending,
        )
        .await;
    });

    while let Some(event) = rx.recv().await {
        let payload = serde_json::to_value(&event).unwrap_or_default();
        if app_handle.emit("react-event", payload).is_err() {
            break;
        }
        match &event {
            ReActEvent::Done { .. } | ReActEvent::Error { .. } => break,
            _ => {}
        }
    }

    Ok(())
}

/// 回答问题工具的待处理问题
#[tauri::command]
pub async fn answer_question(
    state: State<'_, AppState>,
    question_id: String,
    answer: String,
) -> Result<(), String> {
    question_tool::answer_question(&state.pending_questions, &question_id, &answer).await
}
