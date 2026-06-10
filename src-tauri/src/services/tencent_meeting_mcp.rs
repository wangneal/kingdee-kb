use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const BASE_URL: &str = "https://mcp.meeting.tencent.com/mcp/wemeet-open/v1";
const SKILL_VERSION: &str = "v1.0.9";

#[derive(Debug, Clone)]
pub struct TencentMeetingMcpClient {
    token: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TencentMeetingToolResult {
    pub tool_name: String,
    pub content_text: String,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TencentMeetingTranscriptResult {
    pub record_file_id: String,
    pub transcript: String,
    pub minutes: Option<String>,
    pub records_raw: Option<Value>,
    pub transcript_raw: Value,
    pub minutes_raw: Option<Value>,
}

impl TencentMeetingMcpClient {
    pub fn new(token: String) -> Self {
        Self {
            token,
            client: reqwest::Client::new(),
        }
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        if self.token.trim().is_empty() {
            return Err("腾讯会议 Token 未配置".to_string());
        }

        let body = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let response = self
            .client
            .post(BASE_URL)
            .header("Content-Type", "application/json")
            .header("X-Tencent-Meeting-Token", self.token.trim())
            .header("X-Skill-Version", SKILL_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|error| format!("腾讯会议 MCP 请求失败: {}", error))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("读取腾讯会议 MCP 响应失败: {}", error))?;
        if !status.is_success() {
            return Err(format!("腾讯会议 MCP 返回错误 {}: {}", status, text));
        }

        let value: Value = serde_json::from_str(&text)
            .map_err(|error| format!("解析腾讯会议 MCP 响应失败: {}", error))?;
        if let Some(error) = value.get("error") {
            return Err(format!("腾讯会议 MCP 错误: {}", error));
        }
        if let Some(error) = value.pointer("/result/error") {
            return Err(format!("腾讯会议工具错误: {}", error));
        }
        Ok(value)
    }

    pub async fn call_tool(
        &self,
        name: &str,
        mut arguments: Value,
    ) -> Result<TencentMeetingToolResult, String> {
        ensure_client_info(&mut arguments);
        let raw = self
            .request(
                "tools/call",
                json!({
                    "name": name,
                    "arguments": arguments
                }),
            )
            .await?;
        let content_text = extract_content_text(&raw);
        Ok(TencentMeetingToolResult {
            tool_name: name.to_string(),
            content_text,
            raw,
        })
    }

    pub async fn list_tools(&self) -> Result<Value, String> {
        self.request("tools/list", json!({})).await
    }

    pub async fn fetch_transcript(
        &self,
        meeting_id: Option<String>,
        meeting_code: Option<String>,
        record_file_id: Option<String>,
        include_minutes: bool,
    ) -> Result<TencentMeetingTranscriptResult, String> {
        let (record_file_id, records_raw) = match record_file_id
            .filter(|value| !value.trim().is_empty())
        {
            Some(id) => (id, None),
            None => {
                let mut arguments = json!({
                    "page_size": 10
                });
                if let Some(id) = meeting_id.filter(|value| !value.trim().is_empty()) {
                    arguments["meeting_id"] = json!(id);
                }
                if let Some(code) = meeting_code.filter(|value| !value.trim().is_empty()) {
                    arguments["meeting_code"] = json!(code);
                }
                if arguments.get("meeting_id").is_none() && arguments.get("meeting_code").is_none()
                {
                    return Err("请填写会议 ID、会议号或录制文件 ID".to_string());
                }

                let records = self.call_tool("get_records_list", arguments).await?;
                let record_id = find_first_string_by_key(&records.raw, "record_file_id")
                    .ok_or_else(|| "未从腾讯会议录制列表中找到 record_file_id".to_string())?;
                (record_id, Some(records.raw))
            }
        };

        let transcript_result = self
            .call_tool(
                "get_transcripts_details",
                json!({
                    "record_file_id": record_file_id,
                    "pid": "0",
                    "limit": "1000"
                }),
            )
            .await?;
        let transcript = transcript_result.content_text.trim().to_string();

        let (minutes, minutes_raw) = if include_minutes {
            match self
                .call_tool(
                    "get_smart_minutes",
                    json!({
                        "record_file_id": record_file_id
                    }),
                )
                .await
            {
                Ok(result) => (
                    Some(result.content_text.trim().to_string()),
                    Some(result.raw),
                ),
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        };

        Ok(TencentMeetingTranscriptResult {
            record_file_id,
            transcript,
            minutes,
            records_raw,
            transcript_raw: transcript_result.raw,
            minutes_raw,
        })
    }
}

fn ensure_client_info(arguments: &mut Value) {
    if !arguments.is_object() {
        *arguments = json!({});
    }
    if arguments.get("_client_info").is_some() {
        return;
    }
    arguments["_client_info"] = json!({
        "os": std::env::consts::OS,
        "agent": "KingdeeKB",
        "model": "KingdeeKB"
    });
}

fn extract_content_text(raw: &Value) -> String {
    raw.pointer("/result/content")
        .and_then(|content| content.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(|value| value.as_str()) == Some("text") {
                        item.get("text").and_then(|value| value.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn find_first_string_by_key(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(|item| item.as_str()) {
                return Some(found.to_string());
            }
            for child in map.values() {
                if let Some(found) = find_first_string_by_key(child, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(found) = find_first_string_by_key(item, key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}
