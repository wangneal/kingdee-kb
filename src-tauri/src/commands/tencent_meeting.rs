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
