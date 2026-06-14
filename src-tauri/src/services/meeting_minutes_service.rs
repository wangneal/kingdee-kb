// 统一会议纪要生成服务
// 腾讯会议转写、视频导入、手动粘贴都调用此服务，避免多处维护 prompt。

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::services::llm_service::LLMService;
use crate::services::meeting_store::{MeetingStore, SaveMinutes};
use crate::services::product_store::ProductStore;
use crate::services::project_store::ProjectStore;
use crate::services::raw_source::{InsertRawSource, RawSourceStore};

// ── 输入/输出结构 ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeetingMinutesSource {
    #[serde(rename = "tencent_meeting")]
    TencentMeeting,
    #[serde(rename = "video_import")]
    VideoImport,
    #[serde(rename = "manual")]
    Manual,
}

impl std::fmt::Display for MeetingMinutesSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TencentMeeting => write!(f, "腾讯会议"),
            Self::VideoImport => write!(f, "视频导入"),
            Self::Manual => write!(f, "手动转写"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GenerateMeetingMinutesInput {
    pub project_id: i64,
    pub meeting_id: Option<i64>,
    pub title: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub meeting_code: Option<String>,
    pub transcript: String,
    pub official_minutes: Option<String>,
    pub source: MeetingMinutesSource,
}

#[derive(Debug, Clone)]
pub struct GenerateMeetingMinutesOutput {
    pub content_md: String,
    pub decisions_json: String,
    pub todos_json: String,
    pub file_path: String,
    pub product_id: Option<i64>,
    pub raw_source_id: Option<i64>,
    pub minutes_id: Option<i64>,
}

/// LLM 从纪要正文中提取的决策和待办（尽力而为，解析失败时为空）
#[derive(Debug, Clone, Default, Deserialize)]
struct MinutesExtras {
    #[serde(default)]
    decisions: Vec<String>,
    #[serde(default)]
    todos: Vec<String>,
}

// ── 系统提示词 ────────────────────────────────────────────────────────────

const MEETING_MINUTES_SYSTEM_PROMPT: &str = "\
你是一位专业的会议纪要撰写助手。请根据以下会议/访谈的语音转写文本，生成结构化的会议纪要。

【输出格式要求】
## 会议纪要

### 基本信息
- **会议主题**：（从内容推断）
- **会议类型**：（需求调研/项目评审/方案讨论/其他）
- **关键参与者**：（从上下文推断）

### 核心议题
（列出 3-7 个主要讨论议题）

### 关键决策
（列出会议中做出的明确决定，用编号列表）

### 待办事项
（列出后续行动项，包含负责人和截止时间如果提及）

### 风险与关注点
（列出识别到的风险或需要关注的问题）

### 详细讨论记录
（按时间顺序整理关键对话要点）

---
【注意事项】
1. 语音转写可能有错误，请根据上下文合理推断正确内容
2. 不要编造转写文本中没有的信息
3. 保持客观中立，准确反映讨论内容
4. 使用中文输出";

/// 决策/待办提取 prompt：要求 LLM 从已生成的纪要正文中提取结构化 JSON。
///
/// 独立于纪要生成调用，避免纪要正文被 JSON 格式污染；
/// 解析失败时降级为空数组，不阻断纪要主流程。
const MINUTES_EXTRAS_PROMPT: &str = "\
请从下面的会议纪要正文中提取结构化信息。只输出一个 JSON 对象，不要任何额外文字，不要 markdown 代码块标记。

JSON 结构：
{
  \"decisions\": [\"会议中做出的明确决定 1\", \"决定 2\"],
  \"todos\": [\"后续行动项 1（含负责人/截止时间，如提及）\", \"行动项 2\"]
}

提取规则：
1. decisions 只收录明确的决策或结论，不要收录讨论过程或未定的想法
2. todos 只收录需要后续执行的行动项，不要收录已完成的事项
3. 若纪要中没有对应内容，返回空数组 []
4. 保持每条简洁，一整句表达完整意思";

// ── 服务实现 ──────────────────────────────────────────────────────────────

pub struct MeetingMinutesService;

impl MeetingMinutesService {
    /// 统一纪要生成入口
    ///
    /// 所有来源（腾讯会议、视频导入、手动粘贴）都走这个函数。
    ///
    /// 注意：此函数接受 Arc<Mutex<...>> 并在需要数据库操作时短暂持锁，
    /// LLM 调用期间释放所有锁，避免阻塞其他操作。
    pub fn generate(
        input: &GenerateMeetingMinutesInput,
        data_dir: &Path,
        project_store: &Arc<Mutex<ProjectStore>>,
        meeting_store: &Arc<Mutex<MeetingStore>>,
        raw_sources: &Arc<Mutex<RawSourceStore>>,
        products: &Arc<Mutex<ProductStore>>,
        llm: &LLMService,
    ) -> Result<GenerateMeetingMinutesOutput, String> {
        // 1. 校验项目存在且未归档（短暂持锁）
        let project = {
            let ps = project_store
                .lock()
                .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
            let project = ps
                .get_project(input.project_id)?
                .ok_or_else(|| format!("项目 id={} 不存在", input.project_id))?;
            if project.status == "archived" {
                return Err("不能为已归档的项目生成纪要".to_string());
            }
            project
        }; // 释放 project_store 锁

        // 1b. 如果关联了会议，提前校验项目一致性（短暂持锁）
        if let Some(meeting_id) = input.meeting_id {
            let ms = meeting_store
                .lock()
                .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
            let meeting = ms
                .get(meeting_id)?
                .ok_or_else(|| format!("会议 id={} 不存在", meeting_id))?;
            if let Some(meeting_pid) = meeting.project_id {
                if meeting_pid != input.project_id {
                    return Err(format!(
                        "纪要的 project_id({}) 与会议的 project_id({}) 不一致",
                        input.project_id, meeting_pid
                    ));
                }
            }
        } // 释放 meeting_store 锁

        // 2. 调 LLM 生成纪要正文（无锁）
        let minutes_text = Self::call_llm(input, llm)?;

        // 2b. 从纪要正文提取决策和待办（无锁，尽力而为，失败降级为空）
        let extras = Self::extract_decisions_and_todos(&minutes_text, llm);
        let decisions_json = serde_json::to_string(&extras.decisions)
            .unwrap_or_else(|_| "[]".to_string());
        let todos_json = serde_json::to_string(&extras.todos)
            .unwrap_or_else(|_| "[]".to_string());

        // 3. 生成规范文件名
        let date = input
            .start_time
            .as_deref()
            .and_then(|s| s.split('T').next())
            .unwrap_or("unknown-date");
        let safe_title = input
            .title
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
            .take(30)
            .collect::<String>();
        let filename = format!("{}_{}.md", date, safe_title);

        // 4. 写入项目资产目录
        let minutes_dir = data_dir
            .join("projects")
            .join(input.project_id.to_string())
            .join("00_项目管理")
            .join("会议纪要");
        std::fs::create_dir_all(&minutes_dir)
            .map_err(|e| format!("创建纪要目录失败: {}", e))?;
        let file_path = minutes_dir.join(&filename);

        // 5. 组装完整 markdown
        let content_md = Self::assemble_markdown(input, &project.name, &minutes_text);

        std::fs::write(&file_path, &content_md)
            .map_err(|e| format!("写入纪要文件失败: {}", e))?;

        // 6. 将转写登记为 raw_sources
        let raw_source_id = Self::register_raw_source(input, &minutes_dir, &filename, raw_sources)?;

        // 7. 将纪要登记为 products
        let product_id = Self::register_product(
            input,
            &file_path,
            products,
        )?;

        // 8. 保存到 meeting_minutes 表（短暂持锁）
        let minutes_id = if let Some(meeting_id) = input.meeting_id {
            let ms = meeting_store
                .lock()
                .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
            // 查找转写 id
            let transcript_id = ms
                .get_transcript(meeting_id)?
                .map(|t| t.id);

            let save_input = SaveMinutes {
                meeting_id,
                project_id: input.project_id,
                transcript_id,
                content_md: content_md.clone(),
                official_minutes: input.official_minutes.clone(),
                decisions_json: decisions_json.clone(),
                todos_json: todos_json.clone(),
                file_path: file_path.to_string_lossy().to_string(),
                product_id,
                generator: "stakeholder-comms".to_string(),
                model_used: None,
            };
            Some(ms.save_minutes(&save_input)?)
        } else {
            None
        }; // 释放 meeting_store 锁

        // 9. 将待办追加到活动日志（尽力而为，不阻塞主流程）
        Self::append_activity_log(input, &project.name, &extras.todos, data_dir);

        Ok(GenerateMeetingMinutesOutput {
            content_md,
            decisions_json,
            todos_json,
            file_path: file_path.to_string_lossy().to_string(),
            product_id,
            raw_source_id,
            minutes_id,
        })
    }

    /// 调 LLM 生成纪要
    ///
    /// 短转写（≤ 60,000 字符）：直接 Stuff（单次 LLM 调用）。
    /// 长转写（> 60,000 字符）：Map-Reduce，先分段提取要点，再汇总为完整纪要。
    fn call_llm(
        input: &GenerateMeetingMinutesInput,
        llm: &LLMService,
    ) -> Result<String, String> {
        let total_chars = input.transcript.chars().count();

        if total_chars <= MEETING_MINUTES_MAX_PROMPT_CHARS {
            // Stuff 模式：转写文本在上下文窗口内，直接生成
            return Self::call_llm_stuff(input, llm);
        }

        // Map-Reduce 模式：转写文本超出上下文窗口
        tracing::info!(
            "[MeetingMinutes] 转写文本过长（{} 字符），启用 Map-Reduce 模式",
            total_chars
        );
        Self::call_llm_map_reduce(input, llm)
    }

    /// Stuff 模式：转写文本直接生成纪要
    fn call_llm_stuff(
        input: &GenerateMeetingMinutesInput,
        llm: &LLMService,
    ) -> Result<String, String> {
        let transcript = &input.transcript;

        let mut user_prompt = format!(
            "以下是会议/访谈的语音转写文本：\n\n---\n{}\n---\n\n请生成结构化的会议纪要。",
            transcript
        );

        if let Some(ref official) = input.official_minutes {
            user_prompt.push_str(&format!(
                "\n\n---\n以下是腾讯会议官方 AI 纪要（仅供参考）：\n{}",
                official
            ));
        }

        llm.generate_text_sync(MEETING_MINUTES_SYSTEM_PROMPT, &user_prompt)
    }

    /// Map-Reduce 模式：分段提取要点 → 汇总为完整纪要
    fn call_llm_map_reduce(
        input: &GenerateMeetingMinutesInput,
        llm: &LLMService,
    ) -> Result<String, String> {
        // Map 步骤：将转写文本按字符数分块（带 10% 重叠）
        let chunks: Vec<String> = {
            let chars: Vec<char> = input.transcript.chars().collect();
            let mut chunks = Vec::new();
            let mut start = 0usize;

            while start < chars.len() {
                let end = (start + MAP_CHUNK_CHARS).min(chars.len());
                let chunk: String = chars[start..end].iter().collect();
                chunks.push(chunk);

                if end >= chars.len() {
                    break;
                }
                // 向前移动，保留重叠
                start = end.saturating_sub(MAP_CHUNK_OVERLAP_CHARS);
            }
            chunks
        };

        tracing::info!(
            "[MeetingMinutes] Map-Reduce: {} 个分块，每块约 {} 字符",
            chunks.len(),
            MAP_CHUNK_CHARS
        );

        // Map: 对每个分块提取要点摘要
        let map_prompt = "\
你是一位会议纪要助手。请从以下会议转写片段中提取关键要点（3-7 条），只输出要点列表。

【要点格式】
- 议题：xxx
- 关键讨论：xxx
- 决策/结论：xxx（如果有）

只输出要点，不要输出完整纪要。";

        let mut partial_summaries: Vec<String> = Vec::with_capacity(chunks.len());
        for (i, chunk) in chunks.iter().enumerate() {
            let user = format!(
                "会议转写片段 {}/{}：\n\n---\n{}\n---\n\n请提取关键要点。",
                i + 1,
                chunks.len(),
                chunk
            );
            match llm.generate_text_sync(map_prompt, &user) {
                Ok(summary) => {
                    if !summary.trim().is_empty() {
                        partial_summaries.push(format!("## 片段 {} 要点\n{}", i + 1, summary));
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "[MeetingMinutes] Map 步骤片段 {} 失败: {}，已跳过",
                        i + 1,
                        e
                    );
                }
            }
        }

        if partial_summaries.is_empty() {
            return Err("Map-Reduce: 所有分块的要点提取均失败".to_string());
        }

        // Reduce: 汇总所有片段要点生成最终纪要
        let combined = partial_summaries.join("\n\n");

        let mut reduce_prompt = format!(
            "以下是会议/访谈的语音转写文本的分段要点摘要（共 {} 段）：\n\n---\n{}\n---\n\n请基于以上分段要点生成一份完整、连贯的结构化会议纪要。",
            partial_summaries.len(),
            combined
        );

        if let Some(ref official) = input.official_minutes {
            reduce_prompt.push_str(&format!(
                "\n\n---\n以下是腾讯会议官方 AI 纪要（仅供参考）：\n{}",
                official
            ));
        }

        llm.generate_text_sync(MEETING_MINUTES_SYSTEM_PROMPT, &reduce_prompt)
    }

    /// 从纪要正文提取决策和待办（尽力而为，解析失败降级为空）。
    ///
    /// 独立于纪要生成调用：纪要正文保持自然语言可读，结构化数据由此处单独提取。
    /// LLM 调用或 JSON 解析失败时返回空 `MinutesExtras`，不阻断纪要主流程。
    fn extract_decisions_and_todos(
        minutes_text: &str,
        llm: &LLMService,
    ) -> MinutesExtras {
        let user_prompt = format!(
            "以下是会议纪要正文：\n\n---\n{}\n---\n\n请按规则提取 JSON。",
            minutes_text
        );

        let raw = match llm.generate_text_sync(MINUTES_EXTRAS_PROMPT, &user_prompt) {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!("[MeetingMinutes] 决策/待办提取 LLM 调用失败，降级为空: {}", e);
                return MinutesExtras::default();
            }
        };

        let cleaned = extract_json_from_text(&raw);
        match serde_json::from_str::<MinutesExtras>(&cleaned) {
            Ok(extras) => extras,
            Err(e) => {
                tracing::warn!(
                    "[MeetingMinutes] 决策/待办 JSON 解析失败，降级为空: {}（原始输出前 200 字: {:?}）",
                    e,
                    raw.chars().take(200).collect::<String>()
                );
                MinutesExtras::default()
            }
        }
    }

    /// 组装完整的 markdown 纪要文件
    fn assemble_markdown(
        input: &GenerateMeetingMinutesInput,
        project_name: &str,
        minutes_text: &str,
    ) -> String {
        format!(
            "# 会议纪要：{}\n\n\
             - 项目：{}\n\
             - 时间：{} - {}\n\
             - 来源：{}\n\
             - 会议号：{}\n\n\
             {}\n",
            input.title,
            project_name,
            input.start_time.as_deref().unwrap_or("—"),
            input.end_time.as_deref().unwrap_or("—"),
            input.source,
            input.meeting_code.as_deref().unwrap_or("—"),
            minutes_text,
        )
    }
}

/// 纪要生成最多发送的转写字数（与文档分析保持一致）
const MEETING_MINUTES_MAX_PROMPT_CHARS: usize = 60_000;
/// Map-Reduce 模式下每个分块的字符数（约 8K-12K tokens）
const MAP_CHUNK_CHARS: usize = 12_000;
/// Map-Reduce 模式分块重叠字符数（10% 重叠）
const MAP_CHUNK_OVERLAP_CHARS: usize = 1_200;


impl MeetingMinutesService {
    fn register_raw_source(
        input: &GenerateMeetingMinutesInput,
        minutes_dir: &Path,
        base_filename: &str,
        raw_sources: &Arc<Mutex<RawSourceStore>>,
    ) -> Result<Option<i64>, String> {
        let identity = if let Some(meeting_id) = input.meeting_id {
            format!("meeting:{}:transcript", meeting_id)
        } else {
            format!("meeting:manual:{}", input.title)
        };

        // 将转写文本写入独立文件（与纪要同目录，文件名加 _transcript 后缀）
        let transcript_filename = base_filename.replace(".md", "_transcript.txt");
        let transcript_path = minutes_dir.join(&transcript_filename);
        std::fs::write(&transcript_path, &input.transcript)
            .map_err(|e| format!("写入转写文件失败: {}", e))?;

        let insert = InsertRawSource {
            project_id: input.project_id,
            identity,
            original_path: input.title.clone(),
            storage_path: transcript_path.to_string_lossy().to_string(),
            sha256: {
                use sha2::{Sha256, Digest};
                let mut hasher = Sha256::new();
                hasher.update(input.transcript.as_bytes());
                format!("{:x}", hasher.finalize())
            },
            file_size: Some(input.transcript.len() as i64),
            mime_type: Some("text/plain".to_string()),
        };

        let rs = raw_sources
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| format!("无法锁定 raw_sources: {}", e))?;
        match rs.insert(&insert) {
            Ok(source) => Ok(Some(source.id)),
            Err(e) => {
                tracing::warn!("[MeetingMinutes] 登记 raw_source 失败: {}", e);
                Ok(None)
            }
        }
    }

    /// 将纪要登记到 products（短暂持锁）
    fn register_product(
        input: &GenerateMeetingMinutesInput,
        file_path: &Path,
        products: &Arc<Mutex<ProductStore>>,
    ) -> Result<Option<i64>, String> {
        let pr = products
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| format!("无法锁定 products: {}", e))?;
        match pr.create(
            "meeting_minutes",
            &format!("会议纪要：{}", input.title),
            input.project_id,
            &file_path.to_string_lossy(),
            0,
            0,
            "",
        ) {
            Ok(id) => Ok(Some(id)),
            Err(e) => {
                tracing::warn!("[MeetingMinutes] 登记 product 失败: {}", e);
                Ok(None)
            }
        }
    }

    /// 将纪要元信息和待办追加到项目活动日志（尽力而为）。
    ///
    /// 待办以 `- [ ]` 勾选项格式逐条记录；无待办时仅记录纪要生成事件。
    fn append_activity_log(
        input: &GenerateMeetingMinutesInput,
        project_name: &str,
        todos: &[String],
        data_dir: &Path,
    ) {
        let log_path = data_dir
            .join("projects")
            .join(input.project_id.to_string())
            .join("00_项目管理")
            .join("活动日志.md");

        let date = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let mut entry = format!(
            "\n## {}  会议纪要生成\n- 项目：{}\n- 会议：{}\n- 来源：{}\n- 时间：{}\n",
            date, project_name, input.title, input.source, date,
        );
        if !todos.is_empty() {
            entry.push_str(&format!("\n### 待办事项（{}）\n", todos.len()));
            for todo in todos {
                entry.push_str(&format!("- [ ] {}\n", todo));
            }
        }

        if let Ok(existing) = std::fs::read_to_string(&log_path) {
            let _ = std::fs::write(&log_path, format!("{}{}", existing, entry));
        } else {
            let header = "# 活动日志\n";
            let _ = std::fs::write(&log_path, format!("{}{}", header, entry));
        }
    }
}

/// 从 LLM 响应文本中提取 JSON 部分。
///
/// 复用 document_analysis 的清洗策略：先剥 markdown 代码块包裹，
/// 再退化为"首个 `{` 到末个 `}`"截取。保证 LLM 输出即便夹带说明文字也能解析。
fn extract_json_from_text(text: &str) -> String {
    let text = text.trim();

    // 尝试提取 ```json ... ``` 或 ``` ... ``` 代码块内容
    if text.starts_with("```") {
        // 去掉首行（可能是 ```json 或 ```）
        let after_first_line = text.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
        // 去掉结尾的 ```
        let without_fence = after_first_line
            .rsplit_once("```")
            .map(|(body, _)| body)
            .unwrap_or(after_first_line);
        let cleaned = without_fence.trim();
        if !cleaned.is_empty() {
            return cleaned.to_string();
        }
    }

    // 退化为从首个 `{` 到末个 `}` 截取
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }

    text.to_string()
}
