//! 验证命令 — 前端在 Chat 完成后调用，获取验证报告

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::app_state::AppState;
use crate::services::verification::pipeline::VerificationPipeline;
use crate::services::verification::types::{
    ScenarioType, VerificationInput, VerificationReport,
};

/// 前端提交的验证请求
#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub generated_text: String,
    pub scenario: String, // "chat" | "search" | "doc_gen" | "research"
}

/// 前端收到的验证结果
#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub report: VerificationReport,
}

/// 对已生成的文本执行验证（前端在 done 事件后调用）
#[tauri::command]
pub async fn run_verification(
    _state: State<'_, AppState>,
    request: VerifyRequest,
) -> Result<VerifyResponse, String> {
    let scenario = match request.scenario.as_str() {
        "search" => ScenarioType::SearchQA,
        "doc_gen" => ScenarioType::DocGen,
        "research" => ScenarioType::Research,
        _ => ScenarioType::Chat,
    };

    let pipeline = VerificationPipeline::default_with_all();
    let input = VerificationInput {
        generated_text: request.generated_text,
        retrieved_chunks: vec![],
        chunk_titles: vec![],
        available_chunk_ids: vec![],
        query: String::new(),
        scenario,
    };

    let report = pipeline.verify(&input).await;
    Ok(VerifyResponse { report })
}
