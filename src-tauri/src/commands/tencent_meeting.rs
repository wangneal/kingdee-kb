use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::app_state::AppState;
use crate::services::tencent_meeting_mcp::{
    TencentMeetingMcpClient, TencentMeetingToolResult, TencentMeetingTranscriptResult,
};

const KEYRING_SERVICE: &str = "com.neal.kingdee-kb";
const TOKEN_ACCOUNT: &str = "tencent_meeting_token";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TencentMeetingConfigStatus {
    pub configured: bool,
}

fn read_token() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, TOKEN_ACCOUNT)
        .map_err(|error| format!("无法访问系统凭据存储: {}", error))?;
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("读取腾讯会议 Token 失败: {}", error)),
    }
}

fn client_from_keyring() -> Result<TencentMeetingMcpClient, String> {
    let token = read_token()?.ok_or_else(|| "请先在设置中配置腾讯会议 Token".to_string())?;
    Ok(TencentMeetingMcpClient::new(token))
}

/// 保存腾讯会议 MCP Token。
#[tauri::command]
pub fn save_tencent_meeting_token(token: Option<String>) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, TOKEN_ACCOUNT)
        .map_err(|error| format!("无法访问系统凭据存储: {}", error))?;
    let token = token.unwrap_or_default();
    let token = token.trim();
    if token.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => return Ok(()),
            Err(error) => return Err(format!("删除腾讯会议 Token 失败: {}", error)),
        }
    }
    entry
        .set_password(token)
        .map_err(|error| format!("保存腾讯会议 Token 失败: {}", error))
}

/// 获取腾讯会议 MCP 配置状态。
#[tauri::command]
pub fn get_tencent_meeting_config_status() -> Result<TencentMeetingConfigStatus, String> {
    Ok(TencentMeetingConfigStatus {
        configured: read_token()?.is_some(),
    })
}

/// 查询腾讯会议 MCP 工具清单。
#[tauri::command]
pub async fn list_tencent_meeting_tools(_state: State<'_, AppState>) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.list_tools().await
}

/// 调用腾讯会议 MCP 任意工具。
#[tauri::command]
pub async fn call_tencent_meeting_tool(
    _state: State<'_, AppState>,
    name: String,
    arguments: Value,
) -> Result<TencentMeetingToolResult, String> {
    let client = client_from_keyring()?;
    client.call_tool(&name, arguments).await
}

/// 同步腾讯会议线上转写和智能纪要。
#[tauri::command]
pub async fn fetch_tencent_meeting_transcript(
    _state: State<'_, AppState>,
    meeting_id: Option<String>,
    meeting_code: Option<String>,
    record_file_id: Option<String>,
    include_minutes: bool,
) -> Result<TencentMeetingTranscriptResult, String> {
    let client = client_from_keyring()?;
    client
        .fetch_transcript(meeting_id, meeting_code, record_file_id, include_minutes)
        .await
}

/// 转换相对时间为 ISO 8601（用于自然语言预约）。
#[tauri::command]
pub async fn convert_tencent_meeting_timestamp(
    _state: State<'_, AppState>,
    arguments: Value,
) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.convert_timestamp(arguments).await
}

/// 创建/预约腾讯会议。
#[tauri::command]
pub async fn schedule_tencent_meeting(
    _state: State<'_, AppState>,
    arguments: Value,
) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.schedule_meeting(arguments).await
}

/// 修改腾讯会议。
#[tauri::command]
pub async fn update_tencent_meeting(
    _state: State<'_, AppState>,
    arguments: Value,
) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.update_meeting(arguments).await
}

/// 取消腾讯会议。
#[tauri::command]
pub async fn cancel_tencent_meeting(
    _state: State<'_, AppState>,
    arguments: Value,
) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.cancel_meeting(arguments).await
}

/// 查询会议详情（meeting_id 或会议号）。
#[tauri::command]
pub async fn get_tencent_meeting(
    _state: State<'_, AppState>,
    arguments: Value,
) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.get_meeting(arguments).await
}

/// 会议号转 meeting_id。
#[tauri::command]
pub async fn get_tencent_meeting_by_code(
    _state: State<'_, AppState>,
    arguments: Value,
) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.get_meeting_by_code(arguments).await
}

/// 查询未开始/进行中的会议列表。
#[tauri::command]
pub async fn list_tencent_user_meetings(
    _state: State<'_, AppState>,
    arguments: Value,
) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.get_user_meetings(arguments).await
}

/// 查询已结束的历史会议列表。
#[tauri::command]
pub async fn list_tencent_user_ended_meetings(
    _state: State<'_, AppState>,
    arguments: Value,
) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.get_user_ended_meetings(arguments).await
}

/// 查询会议录制列表。
#[tauri::command]
pub async fn list_tencent_meeting_records(
    _state: State<'_, AppState>,
    arguments: Value,
) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.get_records_list(arguments).await
}

/// 提交 Agent 反馈到腾讯会议 MCP 意见箱。
#[tauri::command]
pub async fn submit_tencent_meeting_feedback(
    _state: State<'_, AppState>,
    arguments: Value,
) -> Result<Value, String> {
    let client = client_from_keyring()?;
    client.submit_feedback(arguments).await
}
