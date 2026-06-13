use tauri::{Emitter, State};

use std::collections::HashSet;

use crate::app_state::AppState;
use crate::error::AppError;
use crate::services::hybrid_search;
use crate::services::question_tool;
use crate::services::react_agent::ReActEvent;
use crate::services::risk_control::{
    CandidateScopeItem, ContractScopeItem, DefenseScriptRequest, DefenseScriptResult,
    ImportDbResult, ProjectHealthScore, ScopeCreepResult,
};

// ─── P1: 双轨风险把控舱 ───

/// 行业典型阶段排期窗口：(min_day, max_day)。不在表里的阶段不调整。
const PHASE_WINDOWS: &[(&str, u32, u32)] = &[
    ("上线", 1, 1),   // 月初或年初
    ("验收", 8, 22),  // 月中
    ("测试", 15, 31), // 月底前两周
];

/// 阶段级联：把线性顺延结果调整到行业窗口。
/// - 早于当前月窗口起点 → 推到当前月窗口起点
/// - 落在窗口内 → 不动
/// - 晚于当前月窗口终点 → 推到下月窗口起点
fn adjust_to_phase_window(phase_name: &str, linear: chrono::NaiveDate) -> chrono::NaiveDate {
    use chrono::{Datelike, NaiveDate};
    let Some(&(_, min_d, max_d)) = PHASE_WINDOWS.iter().find(|(n, _, _)| *n == phase_name) else {
        return linear;
    };
    let day = linear.day();
    if day < min_d {
        return NaiveDate::from_ymd_opt(linear.year(), linear.month(), min_d).unwrap_or(linear);
    }
    if day > max_d {
        let (y, m) = if linear.month() == 12 { (linear.year() + 1, 1) } else { (linear.year(), linear.month() + 1) };
        return NaiveDate::from_ymd_opt(y, m, min_d).unwrap_or(linear);
    }
    linear
}

#[tauri::command]
pub async fn add_scope_item(
    state: State<'_, AppState>,
    project_id: i64,
    category: String,
    description: String,
    is_in_scope: bool,
    detail: String,
) -> Result<i64, String> {
    state.risk_control_store.lock().await.add_scope_item(
        project_id,
        &category,
        &description,
        is_in_scope,
        &detail,
    )
}

#[tauri::command]
pub async fn list_scope_items(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<ContractScopeItem>, String> {
    state
        .risk_control_store
        .lock()
        .await
        .list_scope_items(project_id, None, None)
}

#[tauri::command]
pub async fn delete_scope_item(
    state: State<'_, AppState>,
    project_id: i64,
    item_id: i64,
) -> Result<(), String> {
    state
        .risk_control_store
        .lock()
        .await
        .delete_scope_item(project_id, item_id)
}

#[tauri::command]
pub async fn check_scope_creep(
    state: State<'_, AppState>,
    project_id: i64,
    requirement: String,
) -> Result<ScopeCreepResult, String> {
    state
        .risk_control_store
        .lock()
        .await
        .check_scope_creep(project_id, &state.llm, &requirement)
        .await
}

#[tauri::command]
pub async fn record_health_metric(
    state: State<'_, AppState>,
    project_id: i64,
    indicator_type: String,
    value: f64,
    notes: String,
) -> Result<i64, String> {
    state.risk_control_store.lock().await.record_health_metric(
        project_id,
        &indicator_type,
        value,
        &notes,
    )
}

#[tauri::command]
pub async fn get_project_health(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<ProjectHealthScore, String> {
    state
        .risk_control_store
        .lock()
        .await
        .calculate_health_score(project_id)
}

fn truncate_risk_evidence(content: &str, max_chars: usize) -> String {
    let truncated: String = content.chars().take(max_chars).collect();
    if content.chars().count() > max_chars {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

/// 解析日期字符串：`%Y-%m-%d` 或 `%Y-%m-%d %H:%M:%S`。
/// 两种是数据库里实际存的格式（planned_* 存日期，actual_* 存 datetime）。
fn parse_flexible_date(input: &str) -> Option<chrono::NaiveDate> {
    use chrono::NaiveDate;
    // 长串截前 10 字符即可剥掉 " 03:13:48" 形式的时分秒
    let s = input.trim();
    let date_part = if s.len() >= 10 { &s[..10] } else { s };
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

/// 单阶段进度判定（无级联）
///
/// 仅基于本阶段的 planned/actual 判定自身超期情况，不考虑前置阶段的顺延。
/// 级联延迟由 `assess_phase_schedule_with_cascade` 统一处理。
fn assess_phase_schedule(
    planned_end: Option<&str>,
    actual_end: Option<&str>,
    today: chrono::NaiveDate,
) -> String {
    let planned_end = planned_end.and_then(parse_flexible_date);
    let actual_end = actual_end.and_then(parse_flexible_date);
    match (planned_end, actual_end) {
        (Some(planned_end), Some(actual_end)) if actual_end > planned_end => {
            format!(
                "实际完成晚于计划 {} 天",
                (actual_end - planned_end).num_days()
            )
        }
        (Some(_), Some(_)) => "已按计划日期完成（或提前完成）".to_string(),
        (Some(planned_end), None) if planned_end < today => {
            format!(
                "计划完成日期已过 {} 天且未记录实际完成日期，判定为超期",
                (today - planned_end).num_days()
            )
        }
        (Some(_), None) => "计划完成日期未到，无实际完成记录".to_string(),
        (None, Some(_)) => "无计划完成日期可比较，但已记录实际完成时间".to_string(),
        (None, None) => "无计划完成日期且无实际完成记录".to_string(),
    }
}

/// 单阶段进度判定 + 级联延迟
///
/// 返回 (judgement, projected_end)：
/// - judgement 含本阶段自身判定 + 可选"预计超期 N 天"段
/// - projected_end 用于下一个阶段的 prev_projected_end
fn assess_phase_schedule_with_cascade(
    phase_name: Option<&str>,
    planned_start: Option<&str>,
    planned_end: Option<&str>,
    actual_start: Option<&str>,
    actual_end: Option<&str>,
    prev_projected_end: Option<chrono::NaiveDate>,
    today: chrono::NaiveDate,
) -> (String, Option<chrono::NaiveDate>) {
    let ps = planned_start.and_then(parse_flexible_date);
    let pe = planned_end.and_then(parse_flexible_date);
    let ae = actual_end.and_then(parse_flexible_date);
    let mut judgement = assess_phase_schedule(planned_end, actual_end, today);

    // 线性投影：本阶段"应该完成"的日期
    let linear = if let Some(end) = ae {
        Some(end) // 已完成
    } else if actual_start.is_some() {
        Some(today.max(pe.unwrap_or(today))) // 进行中
    } else if let Some(prev) = prev_projected_end {
        // 未开始 + 前置有延迟：最早开始 = max(prev, planned_start)；结束 = +计划工期
        let dur = match (ps, pe) { (Some(s), Some(e)) => (e - s).num_days().max(0), _ => 0 };
        Some(prev.max(ps.unwrap_or(prev)) + chrono::Duration::days(dur))
    } else {
        pe // 未开始 + 无前置延迟
    };
    // 行业窗口顺延（已完成不动；未完成 + 在典型窗口表内才调整）
    let projected = linear.map(|d| match (ae, phase_name) {
        (Some(_), _) => d,
        (None, Some(name)) => adjust_to_phase_window(name, d),
        _ => d,
    });

    // 级联延迟提示：本阶段未完成 + 投影晚于计划 → 追加"预计超期 N 天"
    if let (Some(prev), Some(proj), Some(planned)) = (prev_projected_end, projected, pe) {
        if ae.is_none() && proj > planned {
            judgement.push_str(&format!(
                "；前置阶段预计 {} 完成，本阶段预计超期 {} 天（计划完成 {}）",
                prev,
                (proj - planned).num_days(),
                planned
            ));
        }
    }
    (judgement, projected)
}

fn collect_project_risk_evidence(
    state: &AppState,
    project_id: i64,
    user_context: &str,
) -> Result<String, String> {
    let mut seen_chunk_ids = HashSet::new();
    let mut evidence = Vec::new();
    let today = chrono::Local::now().date_naive();

    let (project_summary, phase_summary) = {
        let project_store = state.project_store.lock().map_err(|e| e.to_string())?;
        let project = project_store
            .get_project(project_id)?
            .ok_or_else(|| format!("项目 {} 不存在", project_id))?;
        let phases = project_store.get_project_phases(project_id)?;
        let project_summary = format!(
            "项目名称：{}\n项目状态：{}\n当前阶段：{}\n项目描述：{}",
            project.name,
            project.status,
            project.current_phase,
            if project.description.trim().is_empty() {
                "未填写"
            } else {
                &project.description
            }
        );
        let phase_summary = if phases.is_empty() {
            "暂无项目阶段计划数据".to_string()
        } else {
            // 按 phase_index 排序，保证级联从前向后计算
            let mut sorted_phases: Vec<&_> = phases.iter().collect();
            sorted_phases.sort_by_key(|p| p.phase_index);

            let mut prev_projected_end: Option<chrono::NaiveDate> = None;
            sorted_phases
                .iter()
                .enumerate()
                .map(|(index, phase)| {
                    let (schedule_judgement, projected) = assess_phase_schedule_with_cascade(
                        Some(&phase.phase_name),
                        phase.planned_start.as_deref(),
                        phase.planned_end.as_deref(),
                        phase.actual_start.as_deref(),
                        phase.actual_end.as_deref(),
                        prev_projected_end,
                        today,
                    );
                    prev_projected_end = projected;
                    format!(
                        "【阶段计划{}】{}：状态={}；计划={} 至 {}；实际={} 至 {}；进度判断={}",
                        index + 1,
                        phase.phase_name,
                        phase.status,
                        phase.planned_start.as_deref().unwrap_or("未设置"),
                        phase.planned_end.as_deref().unwrap_or("未设置"),
                        phase.actual_start.as_deref().unwrap_or("未记录"),
                        phase.actual_end.as_deref().unwrap_or("未记录"),
                        schedule_judgement
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        (project_summary, phase_summary)
    };

    let documents = {
        let metadata = state.metadata.lock().map_err(|e| e.to_string())?;
        metadata.list_documents(Some(project_id))?
    };

    // 优先读取标题明确属于风控分析范围的文档，避免检索遗漏关键 SOW 或计划。
    let title_keywords = [
        "sow",
        "合同",
        "范围",
        "计划",
        "进度",
        "周报",
        "会议纪要",
        "里程碑",
        "风险",
        "问题",
        "验收",
        "交付",
    ];
    {
        let metadata = state.metadata.lock().map_err(|e| e.to_string())?;
        for document in documents.iter().filter(|document| {
            let title = document.title.to_lowercase();
            title_keywords.iter().any(|keyword| title.contains(keyword))
        }) {
            for chunk in metadata
                .get_chunks_by_document(document.id)?
                .into_iter()
                .take(2)
            {
                if seen_chunk_ids.insert(chunk.id) {
                    evidence.push((
                        document.title.clone(),
                        chunk
                            .section_path
                            .unwrap_or_else(|| "未标注章节".to_string()),
                        truncate_risk_evidence(&chunk.content, 900),
                    ));
                }
                if evidence.len() >= 12 {
                    break;
                }
            }
            if evidence.len() >= 12 {
                break;
            }
        }
    }

    // 分主题检索，覆盖合同范围、计划进度、延期阻塞和交付验收。
    let mut queries = Vec::new();
    if !user_context.trim().is_empty() {
        queries.push(user_context.trim());
    }
    queries.extend([
        "SOW 合同 项目范围 排除项 变更 里程碑 交付物 验收标准",
        "项目计划 当前进度 计划完成日期 实际完成日期 延期 超期 里程碑",
        "周报 会议纪要 未解决问题 阻塞 风险 待办 客户配合 决策",
        "交付 验收 测试 上线 数据准备 质量问题",
    ]);
    let project_filter = project_id.to_string();
    let mut search_errors = Vec::new();
    for query in queries {
        match hybrid_search::hybrid_search(
            query,
            Some(&project_filter),
            &[],
            5,
            &state.embedding,
            &state.vector_index,
            &state.bm25,
            &state.metadata,
            None,
            Some(&state.wiki_pages),
        ) {
            Ok(results) => {
                for result in results {
                    if seen_chunk_ids.insert(result.chunk_id) {
                        evidence.push((
                            result.title,
                            result
                                .section_path
                                .unwrap_or_else(|| "未标注章节".to_string()),
                            truncate_risk_evidence(&result.content, 900),
                        ));
                    }
                    if evidence.len() >= 20 {
                        break;
                    }
                }
            }
            Err(error) => search_errors.push(format!("检索“{}”失败：{}", query, error)),
        }
        if evidence.len() >= 20 {
            break;
        }
    }

    let document_titles = if documents.is_empty() {
        "当前项目知识库暂无文档".to_string()
    } else {
        documents
            .iter()
            .map(|document| format!("- {}", document.title))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let evidence_text = if evidence.is_empty() {
        "未检索到可用于风险判断的项目文档证据".to_string()
    } else {
        evidence
            .iter()
            .enumerate()
            .map(|(index, (title, section, content))| {
                format!(
                    "【证据{}】文档：{}；章节：{}\n{}",
                    index + 1,
                    title,
                    section,
                    content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let search_error_text = if search_errors.is_empty() {
        "无".to_string()
    } else {
        search_errors.join("\n")
    };

    Ok(format!(
        "分析基准日期：{}\n\n项目主数据：\n{}\n\n项目阶段计划与确定性超期判断：\n{}\n\n当前项目文档清单（共 {} 份）：\n{}\n\n检索到的项目证据：\n{}\n\n检索异常：\n{}\n\n前端补充上下文：\n{}",
        today,
        project_summary,
        phase_summary,
        documents.len(),
        document_titles,
        evidence_text,
        search_error_text,
        if user_context.trim().is_empty() {
            "无"
        } else {
            user_context
        }
    ))
}

#[cfg(test)]
mod risk_evidence_tests {
    use super::{adjust_to_phase_window, assess_phase_schedule, assess_phase_schedule_with_cascade, parse_flexible_date};

    /// 项目 19 蓝图回归：actual_end 存 "2026-06-04 03:13:48" 形式。
    /// 修复前误判为"已超期 149 天且未记录实际完成日期"，修复后必须返回"实际完成晚于计划 140 天"。
    #[test]
    fn blueprint_late_completion_with_datetime_actual() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 6, 13).unwrap();
        let result = assess_phase_schedule(
            Some("2026-01-15"),
            Some("2026-06-04 03:13:48"),
            today,
        );
        assert_eq!(result, "实际完成晚于计划 140 天");
    }

    /// 单阶段判定：计划未到 + 无实际完成
    #[test]
    fn pending_before_deadline() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 6, 13).unwrap();
        assert_eq!(
            assess_phase_schedule(Some("2026-06-20"), None, today),
            "计划完成日期未到，无实际完成记录"
        );
    }

    /// 日期解析：纯日期 vs datetime
    #[test]
    fn parse_flexible_date_both_formats() {
        assert_eq!(parse_flexible_date("2026-01-15"), chrono::NaiveDate::from_ymd_opt(2026, 1, 15));
        assert_eq!(parse_flexible_date("2026-06-04 03:13:48"), chrono::NaiveDate::from_ymd_opt(2026, 6, 4));
    }

    /// 行业窗口调整关键场景：上线 5/15 → 6/1
    #[test]
    fn adjust_to_phase_window_pushes_to_next_window() {
        assert_eq!(
            adjust_to_phase_window("上线", chrono::NaiveDate::from_ymd_opt(2026, 5, 15).unwrap()),
            chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap()
        );
        assert_eq!(
            adjust_to_phase_window("验收", chrono::NaiveDate::from_ymd_opt(2026, 6, 25).unwrap()),
            chrono::NaiveDate::from_ymd_opt(2026, 7, 8).unwrap()
        );
        // 无窗口的阶段不动
        assert_eq!(
            adjust_to_phase_window("调研", chrono::NaiveDate::from_ymd_opt(2026, 5, 7).unwrap()),
            chrono::NaiveDate::from_ymd_opt(2026, 5, 7).unwrap()
        );
    }

    /// 级联延迟：前置上线已超期，验收应输出"预计超期 N 天"
    #[test]
    fn cascade_acceptance_phase_should_report_expected_overdue() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 6, 13).unwrap();
        let prev = chrono::NaiveDate::from_ymd_opt(2026, 7, 29).unwrap();
        let (judgement, projected) = assess_phase_schedule_with_cascade(
            Some("验收"),
            Some("2026-06-15"),
            Some("2026-06-20"),
            None,
            None,
            Some(prev),
            today,
        );
        assert!(judgement.contains("预计超期"), "实际: {}", judgement);
        assert!(projected.is_some());
    }
}

#[tauri::command]
pub async fn generate_risk_report(
    state: State<'_, AppState>,
    project_id: i64,
    context: String,
) -> Result<String, String> {
    let evidence_context = collect_project_risk_evidence(&state, project_id, &context)?;
    state
        .risk_control_store
        .lock()
        .await
        .generate_risk_report(project_id, &state.llm, &evidence_context)
        .await
}

#[tauri::command]
pub async fn generate_defense_script(
    state: State<'_, AppState>,
    project_id: i64,
    request: DefenseScriptRequest,
) -> Result<DefenseScriptResult, String> {
    let evidence_context = collect_project_risk_evidence(&state, project_id, &request.context)?;
    let request = DefenseScriptRequest {
        context: evidence_context,
        ..request
    };
    state
        .risk_control_store
        .lock()
        .await
        .generate_defense_script(&state.llm, &request)
        .await
}

// --- P1.5: 合同范围提取 ---

#[tauri::command]
pub async fn extract_scope_from_document(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    project_id: i64,
    doc_id: i64,
) -> Result<Vec<CandidateScopeItem>, String> {
    let chunks = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        let document = meta
            .get_document(doc_id)?
            .ok_or_else(|| format!("文档 {} 不存在", doc_id))?;
        if document.project_id != project_id || document.document_scope != "knowledge" {
            return Err(format!(
                "文档 {} 不属于当前项目 {} 的知识库",
                doc_id, project_id
            ));
        }
        meta.get_chunks_by_document(doc_id)?
    };
    if chunks.is_empty() {
        return Err("文档中未找到任何内容分块".to_string());
    }
    state
        .risk_control_store
        .lock()
        .await
        .extract_scope_from_document(&state.llm, &chunks, Some(&app_handle), project_id, doc_id)
        .await
}

#[tauri::command]
pub async fn confirm_scope_items(
    state: State<'_, AppState>,
    project_id: i64,
    items: Vec<CandidateScopeItem>,
) -> Result<usize, String> {
    state
        .risk_control_store
        .lock()
        .await
        .confirm_scope_items(project_id, &items)
}

// --- P1.6: 整库备份 ---

#[tauri::command]
pub async fn export_database(
    state: State<'_, AppState>,
    target_path: String,
) -> Result<(), String> {
    state
        .risk_control_store
        .lock()
        .await
        .export_database(&target_path)
}

#[tauri::command]
pub async fn import_database(
    state: State<'_, AppState>,
    backup_path: String,
) -> Result<ImportDbResult, String> {
    state
        .risk_control_store
        .lock()
        .await
        .import_database(&backup_path)
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
pub fn remove_sensitive_keyword(
    state: State<'_, AppState>,
    keyword: String,
) -> Result<bool, String> {
    Ok(state.desensitizer.remove_keyword(&keyword))
}

// --- P2.3: Fit-Gap 分析 ---

const FITGAP_SYSTEM_PROMPT: &str = "\
你是一个ERP差异分析专家。分析以下需求，每项判断Fit/Gap。\n\
严格Markdown表格：|序号|需求|分类|Fit/Gap|理由|建议方案|\n\
理由必须具体到模块功能，建议必须可执行。";

#[tauri::command]
pub async fn analyze_fit_gap(
    state: State<'_, AppState>,
    project_id: i64,
    requirements: String,
) -> Result<String, AppError> {
    use crate::services::llm_service::ChatMessage;
    let evidence_context = collect_project_risk_evidence(&state, project_id, &requirements)
        .map_err(|e| AppError::Internal(e))?;
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: FITGAP_SYSTEM_PROMPT.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "待分析需求：\n{}\n\n当前项目证据：\n{}\n\n必须优先依据当前项目合同范围与知识库证据判断；证据不足时标记“待确认”，禁止仅凭通用经验断言。",
                requirements, evidence_context
            ),
        },
    ];
    let config = state.llm.get_active_config().map_err(AppError::Internal)?;
    let provider_id = config.id.clone();
    state
        .llm
        .chat_completion(&messages, &config)
        .await
        .map_err(|e| AppError::classify_llm_error(provider_id, &e))
}

// --- Agent 对话 ---

/// Agent 对话入口：使用 rig_agent 处理用户消息，通过 SSE 事件流返回结果。
#[tauri::command]
pub async fn agent_chat(
    app_handle: tauri::AppHandle,
    message: String,
    session_id: String,
    project_id: Option<i64>,
    history: Option<Vec<crate::services::llm_service::ChatMessage>>,
    provider_id: Option<String>,
    model_id: Option<String>,
    attachments: Option<Vec<crate::services::types::AttachmentInfo>>,
) -> Result<(), String> {
    use tauri::Manager;
    use tokio::sync::mpsc;

    // 手动获取 AppState，避免框架级别的 State 注入竞争
    let state = app_handle
        .try_state::<AppState>()
        .ok_or("后端尚未初始化完成，请稍后重试")?;

    let (tx, mut rx) = mpsc::unbounded_channel::<ReActEvent>();

    let sid = session_id;
    let ledger_project_id = match project_id {
        Some(pid) => pid,
        None => {
            let store = state.project_store.lock().map_err(|e| e.to_string())?;
            store.ensure_default_project()?
        }
    };
    let pid = Some(ledger_project_id);
    let history = history.unwrap_or_default();

    initialize_agent_ledger(
        &state,
        &sid,
        ledger_project_id,
        "chat",
        &message,
        provider_id.as_deref(),
        model_id.as_deref(),
    )?;

    let pending = state.pending_questions.clone();
    let llm = state.llm.clone();
    let embedding = state.embedding.clone();
    let vector_index = state.vector_index.clone();
    let bm25 = state.bm25.clone();
    let metadata = state.metadata.clone();
    let data_dir = state.data_dir.clone();
    let products = state.products.clone();
    let project_store = state.project_store.clone();
    let risk_store = state.risk_control_store.clone();
    let skill_manager = state.skill_manager.clone();
    let image_processor = state.image_processor.clone();
    let llm_providers = state.llm_providers.clone();
    let ledger_metadata = state.metadata.clone();

    // 注册取消标志
    let cancel_flag = state.register_cancel_flag(&sid);
    let cleanup_sid = sid.clone();

    // 由主模型在同一次流式请求中决定是否调用技能，避免发送前额外路由造成首包延迟。
    let system_extra = "需要专业实施流程、交付物或外部技能时，先调用 use-skill(action=\"search\", name_or_query=...) 查找并加载技能；普通问答直接回答。\n\n".to_string();

    // spawn_monitored 替代裸 spawn：panic 时自动 emit task:failed 到前端
    crate::services::spawn_safe::spawn_monitored("agent_chat_run", Some(&app_handle), async move {
        crate::services::rig_agent::RigAgent::run(
            &llm,
            &message,
            &system_extra,
            &history,
            tx,
            &sid,
            pending,
            pid,
            None,
            embedding,
            vector_index,
            bm25,
            metadata,
            data_dir,
            products,
            project_store,
            risk_store,
            skill_manager,
            Some(cancel_flag),
            provider_id.as_deref(),
            model_id.as_deref(),
            attachments,
            image_processor,
            llm_providers,
            None, // wiki_pages
        )
        .await;
    });

    let event_app = app_handle.clone();
    let app_handle_for_emit = event_app.clone(); // 短期 borrow 给 spawn_monitored
    crate::services::spawn_safe::spawn_monitored("agent_event_writer", Some(&app_handle_for_emit), async move {
        let mut assistant_message_id: Option<String> = None;
        let mut assistant_content = String::new();
        let mut active_tool_call_id: Option<String> = None;

        while let Some(event) = rx.recv().await {
            let payload = serde_json::to_value(&event).unwrap_or_default();
            persist_agent_event(
                &ledger_metadata,
                &cleanup_sid,
                event_type_name(&event),
                &payload.to_string(),
            );
            update_agent_ledger_from_event(
                &ledger_metadata,
                &cleanup_sid,
                &event,
                &mut assistant_message_id,
                &mut assistant_content,
                &mut active_tool_call_id,
            );
            if event_app.emit("react-event", payload).is_err() {
                break;
            }
            match &event {
                ReActEvent::Done { .. } | ReActEvent::Error { .. } => break,
                _ => {}
            }
        }

        if let Some(state) = event_app.try_state::<AppState>() {
            state.remove_cancel_flag(&cleanup_sid);
        }
    });

    Ok(())
}

/// 回答问题工具的待处理问题
#[tauri::command]
pub async fn answer_question(
    app_handle: tauri::AppHandle,
    question_id: String,
    answer: String,
    session_id: Option<String>,
    _project_id: Option<i64>,
) -> Result<(), String> {
    use tauri::Manager;
    let state = app_handle
        .try_state::<AppState>()
        .ok_or("后端尚未初始化完成")?;
    if let Some(session_id) = session_id.as_deref() {
        persist_agent_user_reply(&state, session_id, "clarification_answered", &answer);
    }
    question_tool::answer_question(&state.pending_questions, &question_id, &answer).await
}

/// 取消问题工具的待处理问题
#[tauri::command]
pub async fn reject_question(
    app_handle: tauri::AppHandle,
    question_id: String,
    session_id: Option<String>,
    _project_id: Option<i64>,
) -> Result<(), String> {
    use tauri::Manager;
    let state = app_handle
        .try_state::<AppState>()
        .ok_or("后端尚未初始化完成")?;
    if let Some(session_id) = session_id.as_deref() {
        persist_agent_user_reply(
            state.inner(),
            session_id,
            "clarification_rejected",
            "已取消回答该澄清问题。",
        );
    }
    question_tool::reject_question(&state.pending_questions, &question_id).await
}

/// 取消正在运行的 agent 流式会话
#[tauri::command]
pub async fn cancel_agent_stream(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    state.cancel_agent_session(&session_id);
    if let Ok(metadata) = state.metadata.lock() {
        let _ = metadata.insert_agent_event(
            &uuid::Uuid::new_v4().to_string(),
            &session_id,
            "cancel_requested",
            &serde_json::json!({ "session_id": session_id }).to_string(),
        );
        let _ = metadata.update_agent_session_status(&session_id, "cancelled", true);
    }
    Ok(())
}

fn initialize_agent_ledger(
    state: &AppState,
    session_id: &str,
    project_id: i64,
    slot: &str,
    message: &str,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> Result<(), String> {
    let metadata = state.metadata.lock().map_err(|e| e.to_string())?;
    metadata.create_agent_session(session_id, project_id, slot, provider_id, model_id)?;
    metadata.insert_agent_message(
        &uuid::Uuid::new_v4().to_string(),
        session_id,
        "user",
        message,
        "complete",
        None,
    )?;
    metadata.insert_agent_event(
        &uuid::Uuid::new_v4().to_string(),
        session_id,
        "session_started",
        &serde_json::json!({
            "session_id": session_id,
            "project_id": project_id,
            "slot": slot,
            "provider_id": provider_id,
            "model_id": model_id
        })
        .to_string(),
    )?;
    Ok(())
}

fn persist_agent_user_reply(state: &AppState, session_id: &str, event_type: &str, content: &str) {
    if let Ok(metadata) = state.metadata.lock() {
        let _ = metadata.insert_agent_message(
            &uuid::Uuid::new_v4().to_string(),
            session_id,
            "user",
            content,
            "complete",
            None,
        );
        let _ = metadata.insert_agent_event(
            &uuid::Uuid::new_v4().to_string(),
            session_id,
            event_type,
            &serde_json::json!({ "session_id": session_id, "content": content }).to_string(),
        );
        let _ = metadata.update_agent_session_status(session_id, "running", false);
    }
}

fn persist_agent_event(
    metadata: &std::sync::Arc<std::sync::Mutex<crate::services::metadata::MetadataStore>>,
    session_id: &str,
    event_type: &str,
    payload_json: &str,
) {
    if let Ok(metadata) = metadata.lock() {
        if let Err(e) = metadata.insert_agent_event(
            &uuid::Uuid::new_v4().to_string(),
            session_id,
            event_type,
            payload_json,
        ) {
            tracing::warn!("写入 Agent 事件失败: {}", e);
        }
    }
}

fn update_agent_ledger_from_event(
    metadata: &std::sync::Arc<std::sync::Mutex<crate::services::metadata::MetadataStore>>,
    session_id: &str,
    event: &ReActEvent,
    assistant_message_id: &mut Option<String>,
    assistant_content: &mut String,
    active_tool_call_id: &mut Option<String>,
) {
    let Ok(metadata) = metadata.lock() else {
        return;
    };
    match event {
        ReActEvent::TextDelta { content, .. } => {
            let message_id = ensure_assistant_message(
                &metadata,
                session_id,
                assistant_message_id,
                assistant_content,
            );
            assistant_content.push_str(content);
            let _ = metadata.update_agent_message(&message_id, assistant_content, "streaming");
        }
        ReActEvent::ToolCall { name, args, .. } => {
            let message_id = ensure_assistant_message(
                &metadata,
                session_id,
                assistant_message_id,
                assistant_content,
            );
            let tool_call_id = uuid::Uuid::new_v4().to_string();
            let _ = metadata.insert_agent_tool_call(
                &tool_call_id,
                session_id,
                Some(&message_id),
                name,
                "rig-tool-profile-v1",
                "unknown",
                args,
            );
            *active_tool_call_id = Some(tool_call_id);
        }
        ReActEvent::ToolResult { name, result, .. } => {
            if let Some(tool_call_id) = active_tool_call_id.as_deref() {
                let preview = truncate_agent_ledger_text(result, 2000);
                let result_json = serde_json::json!({ "tool": name, "result": result }).to_string();
                let _ = metadata.insert_agent_tool_result(
                    &uuid::Uuid::new_v4().to_string(),
                    tool_call_id,
                    &result_json,
                    &preview,
                    None,
                    "ok",
                );
                let _ = metadata.finish_agent_tool_call(tool_call_id, "ok");
            }
        }
        ReActEvent::Clarification { payload, .. } => {
            let message_id = ensure_assistant_message(
                &metadata,
                session_id,
                assistant_message_id,
                assistant_content,
            );
            *assistant_content = payload.prompt.clone();
            let _ = metadata.update_agent_message(&message_id, assistant_content, "waiting_user");
            let _ = metadata.update_agent_session_status(session_id, "waiting_user", false);
            *assistant_message_id = None;
            assistant_content.clear();
        }
        ReActEvent::Done { .. } => {
            if let Some(message_id) = assistant_message_id.as_deref() {
                let _ = metadata.update_agent_message(message_id, assistant_content, "complete");
            }
            let _ = metadata.update_agent_session_status(session_id, "complete", true);
        }
        ReActEvent::Error { message, .. } => {
            let message_id = ensure_assistant_message(
                &metadata,
                session_id,
                assistant_message_id,
                assistant_content,
            );
            if assistant_content.is_empty() {
                *assistant_content = format!("请求失败：{}", message);
            }
            let _ = metadata.update_agent_message(&message_id, assistant_content, "error");
            let status = if message.contains("取消") {
                "cancelled"
            } else {
                "error"
            };
            let _ = metadata.update_agent_session_status(session_id, status, true);
        }
        ReActEvent::Thinking { .. }
        | ReActEvent::PlanGenerated { .. }
        | ReActEvent::StepStart { .. }
        | ReActEvent::StepResult { .. }
        | ReActEvent::Replan { .. }
        | ReActEvent::PlannerTimeout { .. } => {}
    }
}

fn ensure_assistant_message(
    metadata: &crate::services::metadata::MetadataStore,
    session_id: &str,
    assistant_message_id: &mut Option<String>,
    assistant_content: &mut String,
) -> String {
    if let Some(id) = assistant_message_id.as_ref() {
        return id.clone();
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _ = metadata.insert_agent_message(
        &id,
        session_id,
        "assistant",
        assistant_content,
        "streaming",
        None,
    );
    *assistant_message_id = Some(id.clone());
    id
}

fn event_type_name(event: &ReActEvent) -> &'static str {
    match event {
        ReActEvent::Thinking { .. } => "thinking",
        ReActEvent::ToolCall { .. } => "tool_call",
        ReActEvent::ToolResult { .. } => "tool_result",
        ReActEvent::TextDelta { .. } => "text_delta",
        ReActEvent::Error { .. } => "error",
        ReActEvent::Done { .. } => "done",
        ReActEvent::Clarification { .. } => "clarification",
        ReActEvent::PlanGenerated { .. } => "plan_generated",
        ReActEvent::StepStart { .. } => "step_start",
        ReActEvent::StepResult { .. } => "step_result",
        ReActEvent::Replan { .. } => "replan",
        ReActEvent::PlannerTimeout { .. } => "planner_timeout",
    }
}

fn truncate_agent_ledger_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        value.chars().take(max_chars).collect()
    }
}
