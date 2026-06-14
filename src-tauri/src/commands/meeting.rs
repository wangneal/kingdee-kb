// 会议管理 Tauri 命令
// 提供本地会议存储、同步、转写拉取、纪要生成等业务闭环。

use serde_json::{json, Value};
use tauri::State;

use crate::app_state::AppState;
use crate::services::meeting_store::{
    build_transcript_raw, extract_meeting_list, mcp_json_to_upsert, parse_official_minutes,
    Meeting, MeetingDataSource, MeetingFilter, MeetingWithAssets, SaveTranscript,
};
use crate::services::tencent_meeting_mcp::TencentMeetingMcpClient;

const KEYRING_SERVICE: &str = "com.neal.kingdee.kb";
const TOKEN_ACCOUNT: &str = "tencent_meeting_token";

// ── 辅助函数 ──────────────────────────────────────────────────────────────

fn read_token() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, TOKEN_ACCOUNT)
        .map_err(|e| format!("无法访问系统凭据存储: {}", e))?;
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取腾讯会议 Token 失败: {}", e)),
    }
}

fn mcp_client() -> Result<TencentMeetingMcpClient, String> {
    let token = read_token()?.ok_or_else(|| "请先在设置中配置腾讯会议 Token".to_string())?;
    Ok(TencentMeetingMcpClient::new(token))
}

// ── 命令 ──────────────────────────────────────────────────────────────────

/// 从腾讯会议 MCP 同步会议到本地存储
/// 同步只缓存为未关联状态（unlinked），避免整批归入当前项目。
/// 项目关联需通过 link_meeting_to_project 显式操作。
#[tauri::command]
pub async fn sync_tencent_meetings(
    state: State<'_, AppState>,
    days: Option<u32>,
) -> Result<usize, String> {
    let client = mcp_client()?;
    let days = days.unwrap_or(30);

    // 拉取未开始/进行中的会议
    let upcoming_result = client
        .get_user_meetings(json!({}))
        .await
        .unwrap_or_else(|_| json!({}));

    // 拉取已结束的历史会议
    let ended_result = client
        .get_user_ended_meetings(json!({"days": days}))
        .await
        .unwrap_or_else(|_| json!({}));

    let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
    let mut count = 0;

    // 分别处理两个接口：upcoming 缺状态字段时兜底 scheduled，
    // ended 缺状态字段时兜底 ended（腾讯已结束接口不返回 status 字段）
    for m in &extract_meeting_list(&upcoming_result) {
        if let Some(upsert) = mcp_json_to_upsert(m, MeetingDataSource::Upcoming) {
            match store.upsert_from_tencent(&upsert, None) {  // 始终 unlinked，防止整批归入当前项目
                Ok(_) => count += 1,
                Err(e) => {
                    tracing::warn!("upsert 会议失败(meeting_id={}): {}", upsert.meeting_id, e);
                }
            }
        }
    }
    for m in &extract_meeting_list(&ended_result) {
        if let Some(upsert) = mcp_json_to_upsert(m, MeetingDataSource::Ended) {
            match store.upsert_from_tencent(&upsert, None) {
                Ok(_) => count += 1,
                Err(e) => {
                    tracing::warn!("upsert 会议失败(meeting_id={}): {}", upsert.meeting_id, e);
                }
            }
        }
    }

    Ok(count)
}

/// 按条件查询本地会议列表
#[tauri::command]
pub fn list_meetings(
    state: State<'_, AppState>,
    project_id: Option<i64>,
    status: Option<String>,
    link_status: Option<String>,
    query: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<Meeting>, String> {
    let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
    let filter = MeetingFilter {
        project_id,
        status,
        link_status,
        query,
        limit: limit.or(Some(100)),
        offset,
    };
    store.list(&filter)
}

/// 获取会议及其转写、纪要、项目归属
#[tauri::command]
pub fn get_meeting_with_assets(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Option<MeetingWithAssets>, String> {
    let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
    store.get_with_assets(id)
}

/// 将会议关联到项目
#[tauri::command]
pub fn link_meeting_to_project(
    state: State<'_, AppState>,
    meeting_id: i64,
    project_id: i64,
) -> Result<(), String> {
    let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
    store.link_project(meeting_id, project_id)
}

/// 取消会议的项目关联
#[tauri::command]
pub fn unlink_meeting_from_project(
    state: State<'_, AppState>,
    meeting_id: i64,
) -> Result<(), String> {
    let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
    store.unlink_project(meeting_id)
}

/// 标记未归属会议为忽略
#[tauri::command]
pub fn ignore_unlinked_meeting(
    state: State<'_, AppState>,
    meeting_id: i64,
) -> Result<(), String> {
    let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
    store.ignore(meeting_id)
}

/// 拉取会议转写并保存到本地
#[tauri::command]
pub async fn fetch_meeting_transcript(
    state: State<'_, AppState>,
    meeting_id: i64,
    project_id: Option<i64>,
) -> Result<i64, String> {
    // 获取会议信息
    let meeting = {
        let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
        store
            .get(meeting_id)?
            .ok_or_else(|| format!("会议 id={} 不存在", meeting_id))?
    };

    let effective_project_id = project_id
        .or(meeting.project_id)
        .ok_or_else(|| "会议未关联项目，请先关联项目再拉取转写".to_string())?;

    // 调用 MCP 拉取转写 + 官方纪要（include_minutes=true）
    let client = mcp_client()?;
    let transcript_result = client
        .fetch_transcript(
            Some(meeting.meeting_id.clone()),
            meeting.meeting_code.clone(),
            None,
            true,
        )
        .await?;

    // 保存到本地：官方纪要存入 transcript_raw（结构化 JSON），供后续纪要生成读取
    let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
    let record_file_id = {
        let r = transcript_result.record_file_id.trim();
        if r.is_empty() { None } else { Some(r.to_string()) }
    };
    let input = SaveTranscript {
        meeting_id,
        project_id: effective_project_id,
        record_file_id,
        transcript_text: transcript_result.transcript,
        transcript_raw: build_transcript_raw(transcript_result.minutes.as_deref()),
        raw_source_id: None,
    };
    store.save_transcript(&input)
}

/// 生成会议纪要（使用统一纪要服务）
///
/// project_id 由会议关联的项目派生，不接受前端传入，避免跨项目资产错配。
#[tauri::command]
pub async fn generate_meeting_minutes(
    state: State<'_, AppState>,
    meeting_id: i64,
) -> Result<Value, String> {
    use crate::services::meeting_minutes_service::{
        GenerateMeetingMinutesInput, MeetingMinutesService, MeetingMinutesSource,
    };

    // 获取会议信息
    let meeting = {
        let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
        store
            .get(meeting_id)?
            .ok_or_else(|| format!("会议 id={} 不存在", meeting_id))?
    };

    // 从会议关联的项目派生 project_id，避免跨项目资产分离
    let effective_project_id = meeting
        .project_id
        .ok_or_else(|| "该会议尚未关联项目，请先关联项目再生成纪要".to_string())?;

    // 获取转写
    let transcript = {
        let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
        store
            .get_transcript(meeting_id)?
            .ok_or_else(|| "请先拉取转写再生成纪要".to_string())?
    };

    // 构建统一纪要服务输入：官方纪要从 transcript_raw 读出（fetch_meeting_transcript 拉取时存入）
    let official_minutes = parse_official_minutes(&transcript.transcript_raw);
    let input = GenerateMeetingMinutesInput {
        project_id: effective_project_id,
        meeting_id: Some(meeting_id),
        title: meeting.subject.clone(),
        start_time: Some(meeting.start_time.clone()),
        end_time: meeting.end_time.clone(),
        meeting_code: meeting.meeting_code.clone(),
        transcript: transcript.transcript_text.clone(),
        official_minutes,
        source: MeetingMinutesSource::TencentMeeting,
    };

    // 调用统一纪要服务
    let output = MeetingMinutesService::generate(
        &input,
        &state.data_dir,
        &state.project_store,
        &state.meeting_store,
        &state.raw_sources,
        &state.products,
        &state.llm,
    )?;

    // 从待办数量推断"未解决问题积压"健康指标（尽力而为）
    if output.todo_count > 0 {
        let value = (output.todo_count as f64 * 10.0).min(100.0);
        if let Ok(store) = state.risk_control_store.try_lock() {
            let _ = store.record_health_metric(
                effective_project_id,
                "issue_count",
                value,
                "会议纪要自动推断",
            );
        }
    }

    Ok(json!({
        "minutes_id": output.minutes_id,
        "file_path": output.file_path,
        "product_id": output.product_id,
        "content_length": output.content_md.len(),
    }))
}

/// 重新生成会议纪要
///
/// 复用 generate_meeting_minutes：project_id 由会议关联项目派生，
/// 此处仅校验会议存在并转发。
#[tauri::command]
pub async fn regenerate_meeting_minutes(
    state: State<'_, AppState>,
    meeting_id: i64,
) -> Result<Value, String> {
    {
        let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
        let meeting = store
            .get(meeting_id)?
            .ok_or_else(|| format!("会议 id={} 不存在", meeting_id))?;
        if meeting.project_id.is_none() {
            return Err("会议未关联项目，无法重新生成纪要".to_string());
        }
    }
    generate_meeting_minutes(state, meeting_id).await
}

/// 查询最近生成的会议纪要
#[tauri::command]
pub fn list_recent_meeting_minutes(
    state: State<'_, AppState>,
    project_id: Option<i64>,
    limit: Option<u32>,
) -> Result<Vec<crate::services::meeting_store::MeetingMinutes>, String> {
    let store = state.meeting_store.lock().map_err(|e| e.to_string())?;
    store.list_recent_minutes(project_id, limit.unwrap_or(10))
}

/// 读取项目活动日志内容（00_项目管理/活动日志.md）
///
/// 会议纪要生成时会把纪要元信息和待办追加到此文件。此命令让前端可以查看。
/// 文件不存在时返回空字符串。
#[tauri::command]
pub fn read_project_activity_log(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<String, String> {
    let log_path = state
        .data_dir
        .join("projects")
        .join(project_id.to_string())
        .join("00_项目管理")
        .join("活动日志.md");
    match std::fs::read_to_string(&log_path) {
        Ok(content) => Ok(content),
        Err(_) => Ok(String::new()),
    }
}
