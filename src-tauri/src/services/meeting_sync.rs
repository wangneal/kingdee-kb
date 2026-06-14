// 腾讯会议定时同步服务
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{info, warn};
use crate::app_state::AppState;
use crate::services::meeting_minutes_service::{GenerateMeetingMinutesInput, MeetingMinutesService, MeetingMinutesSource};
use crate::services::meeting_store::{
    build_transcript_raw, extract_meeting_list, mcp_json_to_upsert, parse_official_minutes,
    MeetingDataSource, MeetingFilter, SaveTranscript,
};
use crate::services::tencent_meeting_mcp::TencentMeetingMcpClient;

const KEYRING_SERVICE: &str = "com.neal.kingdee.kb";
const TOKEN_ACCOUNT: &str = "tencent_meeting_token";

#[derive(Clone, serde::Serialize)]
pub struct MeetingSyncEvent { pub kind: String, pub message: String }

fn read_token() -> Result<String, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, TOKEN_ACCOUNT).map_err(|e| format!("凭据访问失败: {}", e))?;
    entry.get_password().map_err(|_| "腾讯会议 Token 未配置".to_string())
}
fn mcp_client() -> Result<TencentMeetingMcpClient, String> { Ok(TencentMeetingMcpClient::new(read_token()?)) }

fn emit(app: &AppHandle, kind: &str, message: &str) {
    let _ = app.emit("meeting-sync", MeetingSyncEvent { kind: kind.to_string(), message: message.to_string() });
}

pub async fn run_sync_cycle(app: AppHandle) -> Result<(), String> {
    info!("开始同步腾讯会议");
    emit(&app, "info", "开始同步腾讯会议...");
    let client = mcp_client().map_err(|e| { emit(&app, "error", &e); e })?;
    let upcoming = client.get_user_meetings(json!({})).await.unwrap_or_else(|_| json!({}));
    let ended = client.get_user_ended_meetings(json!({"days": 7})).await.unwrap_or_else(|_| json!({}));
    let state = app.try_state::<AppState>().ok_or_else(|| "AppState 不可用".to_string())?;
    let (upserted_count, meetings_to_process);
    {
        let store = state.meeting_store.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        let mut count = 0usize;
        // 分别处理两个接口：upcoming 列表状态字段缺失时兜底为 scheduled，
        // ended 列表状态字段缺失时兜底为 ended（腾讯已结束接口不返回状态字段）
        for m in extract_meeting_list(&upcoming) {
            if let Some(upsert) = mcp_json_to_upsert(&m, MeetingDataSource::Upcoming) {
                if store.upsert_from_tencent(&upsert, None).is_ok() { count += 1; }
            }
        }
        for m in extract_meeting_list(&ended) {
            if let Some(upsert) = mcp_json_to_upsert(&m, MeetingDataSource::Ended) {
                if store.upsert_from_tencent(&upsert, None).is_ok() { count += 1; }
            }
        }
        upserted_count = count;
        meetings_to_process = store.list(&MeetingFilter { project_id: None, status: Some("ended".into()), link_status: Some("linked".into()), query: None, limit: Some(50), offset: None })?;
    }
    emit(&app, "info", &format!("upsert {} 场会议", upserted_count));
    let mut processed = 0usize;
    for meeting in &meetings_to_process {
        let Some(pid) = meeting.project_id else { continue; };
        { let store = state.meeting_store.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
          if let Some(a) = store.get_with_assets(meeting.id)? { if a.minutes.is_some() { continue; } } }
        let (transcript, official_minutes) = {
            // Step 1: 检查已有转写（加锁后立即释放）
            let existing = {
                let store = state.meeting_store.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
                store.get_transcript(meeting.id)?.map(|t| (t.transcript_text, parse_official_minutes(&t.transcript_raw)))
            }; // store guard 在此释放

            if let Some((t, m)) = existing {
                (t, m)
            } else {
                // Step 2: 从 MCP 拉取转写 + 官方纪要（不持锁）
                if meeting.meeting_id.is_empty() { continue; }
                let r = match client.fetch_transcript(Some(meeting.meeting_id.clone()), meeting.meeting_code.clone(), None, true).await {
                    Ok(r) => r, Err(e) => { warn!("转写失败: {}", e); continue; }
                };
                if r.transcript.is_empty() { continue; }
                // Step 3: 保存转写，并把官方纪要存入 transcript_raw（结构化 JSON）
                let transcript_raw = build_transcript_raw(r.minutes.as_deref());
                let inp = SaveTranscript { meeting_id: meeting.id, project_id: pid, record_file_id: empty_to_none(&r.record_file_id), transcript_text: r.transcript.clone(), transcript_raw, raw_source_id: None };
                let store = state.meeting_store.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
                let _ = store.save_transcript(&inp);
                (r.transcript, r.minutes)
            }
        };
        if transcript.is_empty() { continue; }
        let inp = GenerateMeetingMinutesInput { project_id: pid, meeting_id: Some(meeting.id), title: meeting.subject.clone(), start_time: Some(meeting.start_time.clone()), end_time: meeting.end_time.clone(), meeting_code: meeting.meeting_code.clone(), transcript, official_minutes, source: MeetingMinutesSource::TencentMeeting };
        match MeetingMinutesService::generate(&inp, &state.data_dir, &state.project_store, &state.meeting_store, &state.raw_sources, &state.products, &state.llm) {
            Ok(o) => { processed += 1; emit(&app, "success", &format!("「{}」纪要: {}", meeting.subject, o.file_path)); }
            Err(e) => { warn!("纪要失败: {}", e); emit(&app, "error", &format!("「{}」纪要失败: {}", meeting.subject, e)); }
        }
    }
    emit(&app, "done", &format!("完成: upsert={}, 纪要={}", upserted_count, processed));
    Ok(())
}

pub fn start_sync_loop(app: AppHandle) {
    let app_clone = app.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("sync runtime");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(30 * 60));
            let a = app_clone.clone();
            rt.block_on(async { if let Err(e) = run_sync_cycle(a).await { warn!("同步失败: {}", e); } });
        }
    });
}

/// 空字符串转为 None，避免存 Some("") 脏数据。
fn empty_to_none(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
