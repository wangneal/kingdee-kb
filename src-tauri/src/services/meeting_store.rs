// 会议存储服务：meetings / meeting_transcripts / meeting_minutes
// 遵循项目隔离语义，会议缓存可未归属，转写和纪要必须归属项目。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── 数据结构 ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: i64,
    pub project_id: Option<i64>,
    pub meeting_id: String,
    pub meeting_code: Option<String>,
    pub subject: String,
    pub host_user_id: Option<String>,
    pub invitees_json: String,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_minutes: Option<i64>,
    pub status: String,
    pub link_status: String,
    pub source: String,
    pub raw_payload_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingTranscript {
    pub id: i64,
    pub meeting_id: i64,
    pub project_id: i64,
    pub record_file_id: Option<String>,
    pub transcript_text: String,
    pub transcript_raw: String,
    pub raw_source_id: Option<i64>,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMinutes {
    pub id: i64,
    pub meeting_id: i64,
    pub project_id: i64,
    pub transcript_id: Option<i64>,
    pub content_md: String,
    pub official_minutes: Option<String>,
    pub decisions_json: String,
    pub todos_json: String,
    pub file_path: String,
    pub product_id: Option<i64>,
    pub generator: String,
    pub model_used: Option<String>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MeetingFilter {
    pub project_id: Option<i64>,
    pub status: Option<String>,
    pub link_status: Option<String>,
    pub query: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TencentMeetingUpsert {
    pub meeting_id: String,
    pub meeting_code: Option<String>,
    pub subject: String,
    pub host_user_id: Option<String>,
    pub invitees_json: String,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_minutes: Option<i64>,
    pub status: String,
    pub raw_payload_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveTranscript {
    pub meeting_id: i64,
    pub project_id: i64,
    pub record_file_id: Option<String>,
    pub transcript_text: String,
    pub transcript_raw: String,
    pub raw_source_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveMinutes {
    pub meeting_id: i64,
    pub project_id: i64,
    pub transcript_id: Option<i64>,
    pub content_md: String,
    pub official_minutes: Option<String>,
    pub decisions_json: String,
    pub todos_json: String,
    pub file_path: String,
    pub product_id: Option<i64>,
    pub generator: String,
    pub model_used: Option<String>,
}

/// 会议 + 转写 + 纪要 复合视图
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingWithAssets {
    pub meeting: Meeting,
    pub transcript: Option<MeetingTranscript>,
    pub minutes: Option<MeetingMinutes>,
    pub project_name: Option<String>,
}

// ── 存储实现 ──────────────────────────────────────────────────────────────

pub struct MeetingStore {
    db: Connection,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl MeetingStore {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, String> {
        let db_path = db_path.as_ref().to_path_buf();
        let db = Connection::open(&db_path)
            .map_err(|e| format!("打开会议数据库失败: {}", e))?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("设置 busy_timeout 失败: {}", e))?;
        Ok(Self { db, db_path })
    }

    /// 创建所有会议相关表及索引（幂等）
    pub fn ensure_table(&self) -> Result<(), String> {
        self.db
            .execute_batch(
                "
CREATE TABLE IF NOT EXISTS meetings (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id         INTEGER REFERENCES projects(id) ON DELETE SET NULL,
  meeting_id         TEXT NOT NULL UNIQUE,
  meeting_code       TEXT,
  subject            TEXT NOT NULL,
  host_user_id       TEXT,
  invitees_json      TEXT NOT NULL DEFAULT '[]',
  start_time         TEXT NOT NULL,
  end_time           TEXT,
  duration_minutes   INTEGER,
  status             TEXT NOT NULL,
  link_status        TEXT NOT NULL DEFAULT 'unlinked',
  source             TEXT NOT NULL DEFAULT 'tencent_mcp',
  raw_payload_json   TEXT NOT NULL DEFAULT '{}',
  created_at         TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at         TEXT NOT NULL DEFAULT (datetime('now')),
  CHECK (status IN ('scheduled', 'ongoing', 'ended', 'cancelled')),
  CHECK (link_status IN ('linked', 'unlinked', 'ignored'))
);

CREATE INDEX IF NOT EXISTS idx_meetings_project_time ON meetings(project_id, start_time);
CREATE INDEX IF NOT EXISTS idx_meetings_status ON meetings(status);
CREATE INDEX IF NOT EXISTS idx_meetings_link_status ON meetings(link_status);
CREATE INDEX IF NOT EXISTS idx_meetings_code ON meetings(meeting_code);

CREATE TABLE IF NOT EXISTS meeting_transcripts (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  meeting_id        INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  project_id        INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  record_file_id    TEXT,
  transcript_text   TEXT NOT NULL,
  transcript_raw    TEXT NOT NULL DEFAULT '{}',
  raw_source_id     INTEGER REFERENCES raw_sources(id) ON DELETE SET NULL,
  fetched_at        TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE(meeting_id)
);

CREATE INDEX IF NOT EXISTS idx_meeting_transcripts_project ON meeting_transcripts(project_id);

CREATE TABLE IF NOT EXISTS meeting_minutes (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  meeting_id         INTEGER NOT NULL UNIQUE REFERENCES meetings(id) ON DELETE CASCADE,
  project_id         INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  transcript_id      INTEGER REFERENCES meeting_transcripts(id) ON DELETE SET NULL,
  content_md         TEXT NOT NULL,
  official_minutes   TEXT,
  decisions_json     TEXT NOT NULL DEFAULT '[]',
  todos_json         TEXT NOT NULL DEFAULT '[]',
  file_path          TEXT NOT NULL,
  product_id         INTEGER REFERENCES products(id) ON DELETE SET NULL,
  generator          TEXT NOT NULL DEFAULT 'stakeholder-comms',
  model_used         TEXT,
  generated_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_meeting_minutes_project ON meeting_minutes(project_id, generated_at);
",
            )
            .map_err(|e| format!("创建会议表失败: {}", e))?;
        Ok(())
    }

    // ── 会议 CRUD ─────────────────────────────────────────────────────

    /// 从腾讯会议 MCP 数据 upsert，project_id 可为空（未归属缓存）
    pub fn upsert_from_tencent(
        &self,
        input: &TencentMeetingUpsert,
        project_id: Option<i64>,
    ) -> Result<i64, String> {
        let link_status = if project_id.is_some() {
            "linked"
        } else {
            "unlinked"
        };

        self.db
            .execute(
                "INSERT INTO meetings (project_id, meeting_id, meeting_code, subject, host_user_id,
                    invitees_json, start_time, end_time, duration_minutes, status,
                    link_status, source, raw_payload_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'tencent_mcp', ?12, datetime('now'))
                 ON CONFLICT(meeting_id) DO UPDATE SET
                    project_id = COALESCE(excluded.project_id, meetings.project_id),
                    meeting_code = excluded.meeting_code,
                    subject = excluded.subject,
                    host_user_id = excluded.host_user_id,
                    invitees_json = excluded.invitees_json,
                    start_time = excluded.start_time,
                    end_time = excluded.end_time,
                    duration_minutes = excluded.duration_minutes,
                    status = excluded.status,
                    link_status = CASE
                        WHEN meetings.link_status = 'ignored' THEN 'ignored'
                        WHEN excluded.project_id IS NOT NULL AND meetings.project_id IS NULL THEN 'linked'
                        ELSE meetings.link_status
                    END,
                    raw_payload_json = excluded.raw_payload_json,
                    updated_at = datetime('now')",
                params![
                    project_id,
                    input.meeting_id,
                    input.meeting_code,
                    input.subject,
                    input.host_user_id,
                    input.invitees_json,
                    input.start_time,
                    input.end_time,
                    input.duration_minutes,
                    input.status,
                    link_status,
                    input.raw_payload_json,
                ],
            )
            .map_err(|e| format!("upsert 会议失败: {}", e))?;

        // 返回本地 id
        let id: i64 = self
            .db
            .query_row(
                "SELECT id FROM meetings WHERE meeting_id = ?1",
                params![input.meeting_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("查询 upsert 后的会议 id 失败: {}", e))?;
        Ok(id)
    }

    /// 按过滤条件查询会议列表
    pub fn list(&self, filter: &MeetingFilter) -> Result<Vec<Meeting>, String> {
        let mut sql = String::from("SELECT * FROM meetings WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(pid) = filter.project_id {
            sql.push_str(&format!(" AND project_id = ?{}", param_values.len() + 1));
            param_values.push(Box::new(pid));
        }
        if let Some(ref status) = filter.status {
            sql.push_str(&format!(" AND status = ?{}", param_values.len() + 1));
            param_values.push(Box::new(status.clone()));
        }
        if let Some(ref ls) = filter.link_status {
            sql.push_str(&format!(" AND link_status = ?{}", param_values.len() + 1));
            param_values.push(Box::new(ls.clone()));
        }
        if let Some(ref q) = filter.query {
            sql.push_str(&format!(
                " AND (subject LIKE ?{} OR meeting_code LIKE ?{})",
                param_values.len() + 1,
                param_values.len() + 2
            ));
            let pattern = format!("%{}%", q);
            param_values.push(Box::new(pattern.clone()));
            param_values.push(Box::new(pattern));
        }

        sql.push_str(" ORDER BY start_time DESC");

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        if let Some(offset) = filter.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.db.prepare(&sql).map_err(|e| format!("准备查询失败: {}", e))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| row_to_meeting(row))
            .map_err(|e| format!("查询会议失败: {}", e))?;

        let mut meetings = Vec::new();
        for row in rows {
            meetings.push(row.map_err(|e| format!("解析会议行失败: {}", e))?);
        }
        Ok(meetings)
    }

    /// 按本地 id 获取会议
    pub fn get(&self, id: i64) -> Result<Option<Meeting>, String> {
        self.db
            .query_row("SELECT * FROM meetings WHERE id = ?1", params![id], |row| {
                row_to_meeting(row)
            })
            .optional()
            .map_err(|e| format!("查询会议失败: {}", e))
    }

    /// 按腾讯 meeting_id 获取会议
    pub fn get_by_tencent_id(&self, meeting_id: &str) -> Result<Option<Meeting>, String> {
        self.db
            .query_row(
                "SELECT * FROM meetings WHERE meeting_id = ?1",
                params![meeting_id],
                |row| row_to_meeting(row),
            )
            .optional()
            .map_err(|e| format!("查询会议失败: {}", e))
    }

    /// 关联项目（link_status → linked）
    pub fn link_project(&self, meeting_id: i64, project_id: i64) -> Result<(), String> {
        // 校验项目存在且未归档
        let project_status: String = self
            .db
            .query_row(
                "SELECT status FROM projects WHERE id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("查询项目失败: {}", e))?
            .ok_or_else(|| format!("项目 id={} 不存在", project_id))?;

        if project_status == "archived" {
            return Err("不能关联已归档的项目".to_string());
        }

        self.db
            .execute(
                "UPDATE meetings SET project_id = ?1, link_status = 'linked', updated_at = datetime('now') WHERE id = ?2",
                params![project_id, meeting_id],
            )
            .map_err(|e| format!("关联项目失败: {}", e))?;
        Ok(())
    }

    /// 取消项目关联（link_status → unlinked, project_id → NULL）
    pub fn unlink_project(&self, meeting_id: i64) -> Result<(), String> {
        // 检查是否已有纪要——有纪要不允许取消关联
        let has_minutes: bool = self
            .db
            .query_row(
                "SELECT COUNT(*) > 0 FROM meeting_minutes WHERE meeting_id = ?1",
                params![meeting_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("查询纪要失败: {}", e))?;

        if has_minutes {
            return Err("已生成纪要的会议不能取消项目关联，请先删除纪要".to_string());
        }

        self.db
            .execute(
                "UPDATE meetings SET project_id = NULL, link_status = 'unlinked', updated_at = datetime('now') WHERE id = ?1",
                params![meeting_id],
            )
            .map_err(|e| format!("取消关联失败: {}", e))?;
        Ok(())
    }

    /// 标记为忽略
    pub fn ignore(&self, meeting_id: i64) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE meetings SET link_status = 'ignored', updated_at = datetime('now') WHERE id = ?1",
                params![meeting_id],
            )
            .map_err(|e| format!("标记忽略失败: {}", e))?;
        Ok(())
    }

    // ── 转写 ──────────────────────────────────────────────────────────

    /// 保存转写（project_id 必须非空）
    pub fn save_transcript(&self, input: &SaveTranscript) -> Result<i64, String> {
        // 校验会议的项目归属一致
        let meeting_project_id: Option<i64> = self
            .db
            .query_row(
                "SELECT project_id FROM meetings WHERE id = ?1",
                params![input.meeting_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("查询会议失败: {}", e))?
            .ok_or_else(|| format!("会议 id={} 不存在", input.meeting_id))?;

        if meeting_project_id != Some(input.project_id) {
            // 如果会议尚未关联项目，自动关联
            if meeting_project_id.is_none() {
                self.db
                    .execute(
                        "UPDATE meetings SET project_id = ?1, link_status = 'linked', updated_at = datetime('now') WHERE id = ?2",
                        params![input.project_id, input.meeting_id],
                    )
                    .map_err(|e| format!("自动关联项目失败: {}", e))?;
            } else {
                return Err(format!(
                    "转写的 project_id({}) 与会议的 project_id({:?}) 不一致",
                    input.project_id, meeting_project_id
                ));
            }
        }

        self.db
            .execute(
                "INSERT INTO meeting_transcripts (meeting_id, project_id, record_file_id, transcript_text, transcript_raw, raw_source_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(meeting_id) DO UPDATE SET
                    record_file_id = excluded.record_file_id,
                    transcript_text = excluded.transcript_text,
                    transcript_raw = excluded.transcript_raw,
                    raw_source_id = excluded.raw_source_id,
                    fetched_at = datetime('now')",
                params![
                    input.meeting_id,
                    input.project_id,
                    input.record_file_id,
                    input.transcript_text,
                    input.transcript_raw,
                    input.raw_source_id,
                ],
            )
            .map_err(|e| format!("保存转写失败: {}", e))?;

        let id: i64 = self
            .db
            .query_row(
                "SELECT id FROM meeting_transcripts WHERE meeting_id = ?1",
                params![input.meeting_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("查询转写 id 失败: {}", e))?;
        Ok(id)
    }

    /// 获取会议的转写
    pub fn get_transcript(&self, meeting_id: i64) -> Result<Option<MeetingTranscript>, String> {
        self.db
            .query_row(
                "SELECT * FROM meeting_transcripts WHERE meeting_id = ?1",
                params![meeting_id],
                |row| {
                    Ok(MeetingTranscript {
                        id: row.get(0)?,
                        meeting_id: row.get(1)?,
                        project_id: row.get(2)?,
                        record_file_id: row.get(3)?,
                        transcript_text: row.get(4)?,
                        transcript_raw: row.get(5)?,
                        raw_source_id: row.get(6)?,
                        fetched_at: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(|e| format!("查询转写失败: {}", e))
    }

    // ── 纪要 ──────────────────────────────────────────────────────────

    /// 保存纪要（project_id 必须非空）
    pub fn save_minutes(&self, input: &SaveMinutes) -> Result<i64, String> {
        // 校验会议存在，并检查项目一致性
        let meeting = self
            .get(input.meeting_id)?
            .ok_or_else(|| format!("会议 id={} 不存在", input.meeting_id))?;

        if let Some(meeting_project_id) = meeting.project_id {
            if meeting_project_id != input.project_id {
                return Err(format!(
                    "纪要的 project_id({}) 与会议的 project_id({}) 不一致",
                    input.project_id, meeting_project_id
                ));
            }
        }

        self.db
            .execute(
                "INSERT INTO meeting_minutes (meeting_id, project_id, transcript_id, content_md,
                    official_minutes, decisions_json, todos_json, file_path,
                    product_id, generator, model_used)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(meeting_id) DO UPDATE SET
                    transcript_id = excluded.transcript_id,
                    content_md = excluded.content_md,
                    official_minutes = excluded.official_minutes,
                    decisions_json = excluded.decisions_json,
                    todos_json = excluded.todos_json,
                    file_path = excluded.file_path,
                    product_id = excluded.product_id,
                    generator = excluded.generator,
                    model_used = excluded.model_used,
                    generated_at = datetime('now')",
                params![
                    input.meeting_id,
                    input.project_id,
                    input.transcript_id,
                    input.content_md,
                    input.official_minutes,
                    input.decisions_json,
                    input.todos_json,
                    input.file_path,
                    input.product_id,
                    input.generator,
                    input.model_used,
                ],
            )
            .map_err(|e| format!("保存纪要失败: {}", e))?;

        let id: i64 = self
            .db
            .query_row(
                "SELECT id FROM meeting_minutes WHERE meeting_id = ?1",
                params![input.meeting_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("查询纪要 id 失败: {}", e))?;
        Ok(id)
    }

    /// 获取会议的纪要
    pub fn get_minutes(&self, meeting_id: i64) -> Result<Option<MeetingMinutes>, String> {
        self.db
            .query_row(
                "SELECT * FROM meeting_minutes WHERE meeting_id = ?1",
                params![meeting_id],
                |row| {
                    Ok(MeetingMinutes {
                        id: row.get(0)?,
                        meeting_id: row.get(1)?,
                        project_id: row.get(2)?,
                        transcript_id: row.get(3)?,
                        content_md: row.get(4)?,
                        official_minutes: row.get(5)?,
                        decisions_json: row.get(6)?,
                        todos_json: row.get(7)?,
                        file_path: row.get(8)?,
                        product_id: row.get(9)?,
                        generator: row.get(10)?,
                        model_used: row.get(11)?,
                        generated_at: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(|e| format!("查询纪要失败: {}", e))
    }

    // ── 复合查询 ──────────────────────────────────────────────────────

    /// 获取会议及其转写和纪要
    pub fn get_with_assets(&self, id: i64) -> Result<Option<MeetingWithAssets>, String> {
        let meeting = match self.get(id)? {
            Some(m) => m,
            None => return Ok(None),
        };

        let transcript = self.get_transcript(id)?;
        let minutes = self.get_minutes(id)?;

        // 查询项目名
        let project_name = if let Some(pid) = meeting.project_id {
            self.db
                .query_row(
                    "SELECT name FROM projects WHERE id = ?1",
                    params![pid],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| format!("查询项目名失败: {}", e))?
        } else {
            None
        };

        Ok(Some(MeetingWithAssets {
            meeting,
            transcript,
            minutes,
            project_name,
        }))
    }

    /// 最近纪要列表
    pub fn list_recent_minutes(
        &self,
        project_id: Option<i64>,
        limit: u32,
    ) -> Result<Vec<MeetingMinutes>, String> {
        let mut sql = String::from("SELECT * FROM meeting_minutes WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(pid) = project_id {
            sql.push_str(&format!(" AND project_id = ?{}", param_values.len() + 1));
            param_values.push(Box::new(pid));
        }

        sql.push_str(&format!(
            " ORDER BY generated_at DESC LIMIT {}",
            limit
        ));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self
            .db
            .prepare(&sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(MeetingMinutes {
                    id: row.get(0)?,
                    meeting_id: row.get(1)?,
                    project_id: row.get(2)?,
                    transcript_id: row.get(3)?,
                    content_md: row.get(4)?,
                    official_minutes: row.get(5)?,
                    decisions_json: row.get(6)?,
                    todos_json: row.get(7)?,
                    file_path: row.get(8)?,
                    product_id: row.get(9)?,
                    generator: row.get(10)?,
                    model_used: row.get(11)?,
                    generated_at: row.get(12)?,
                })
            })
            .map_err(|e| format!("查询纪要列表失败: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("解析纪要行失败: {}", e))?);
        }
        Ok(results)
    }
}

// ── MCP JSON 解析（共享纯函数，供 commands 和 sync 复用） ──────────────────

/// MCP 会议数据来源接口，用于在 `status` 字段缺失时按接口语义兜底状态。
///
/// 腾讯会议两个列表接口的返回字段不同：
/// - 进行中/待开始接口（get_user_meetings）：返回 `status` 字段（如 MEETING_STATE_STARTED）
/// - 已结束接口（get_user_ended_meetings）：完全不返回状态字段
/// 仅靠字段判断会导致已结束会议被降级为 scheduled，因此需由调用方传入来源提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingDataSource {
    /// 来自 get_user_meetings（进行中/待开始）
    Upcoming,
    /// 来自 get_user_ended_meetings（已结束）
    Ended,
    /// 来源未知（如手动构造）
    Unknown,
}

/// 从腾讯会议 MCP 返回的 JSON 中提取会议列表。
///
/// MCP 响应结构在不同接口下嵌套层级不同，按优先级尝试多种键名。
pub fn extract_meeting_list(payload: &serde_json::Value) -> Vec<serde_json::Value> {
    // 去掉一层 MCP 包装
    let root = payload.get("result").unwrap_or(payload);

    if let Some(list) = root.get("meeting_info_list").and_then(|v| v.as_array()) {
        return list.clone();
    }
    if let Some(list) = root.get("meeting_list").and_then(|v| v.as_array()) {
        return list.clone();
    }
    if let Some(list) = root.get("meetings").and_then(|v| v.as_array()) {
        return list.clone();
    }
    if let Some(arr) = root.as_array() {
        return arr.clone();
    }
    Vec::new()
}

/// 将 MCP 单条会议 JSON 转为 `TencentMeetingUpsert`。
///
/// 状态映射规则（按腾讯会议 REST API 真实字段）：
/// 1. 优先读取 `status` 字段（进行中接口返回，值如 MEETING_STATE_STARTED/MEETING_STATE_ENDED）
/// 2. `status` 缺失时按 `source_hint` 兜底：Ended → ended，Upcoming/Unknown → scheduled
///
/// 注意：已结束接口（get_user_ended_meetings）不返回任何状态字段，
/// 必须由调用方传入 `source_hint = Ended` 才能正确识别为 ended。
pub fn mcp_json_to_upsert(
    m: &serde_json::Value,
    source_hint: MeetingDataSource,
) -> Option<TencentMeetingUpsert> {
    let meeting_id = m.get("meeting_id")?.as_str()?.to_string();
    let subject = m
        .get("subject")
        .and_then(|v| v.as_str())
        .unwrap_or("未命名会议")
        .to_string();

    // 状态解析：先看 status 字段，缺失则按来源兜底
    let status = match m.get("status").and_then(|v| v.as_str()) {
        Some("MEETING_STATE_INIT") | Some("MEETING_STATE_STARTED") => "ongoing".to_string(),
        Some("MEETING_STATE_ENDED") => "ended".to_string(),
        Some("MEETING_STATE_CANCELLED") => "cancelled".to_string(),
        Some(s) if s.contains("ENDED") => "ended".to_string(),
        Some(s) if s.contains("CANCEL") => "cancelled".to_string(),
        // status 字段缺失或无法识别：按数据来源接口兜底
        _ => match source_hint {
            MeetingDataSource::Ended => "ended".to_string(),
            MeetingDataSource::Upcoming | MeetingDataSource::Unknown => "scheduled".to_string(),
        },
    };

    Some(TencentMeetingUpsert {
        meeting_id,
        meeting_code: m
            .get("meeting_code")
            .and_then(|v| v.as_str())
            .map(String::from),
        subject,
        host_user_id: m
            .get("host")
            .and_then(|h| h.get("user_id"))
            .and_then(|v| v.as_str())
            .map(String::from),
        invitees_json: m
            .get("invitees")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "[]".to_string()),
        start_time: m
            .get("start_time")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        end_time: m
            .get("end_time")
            .and_then(|v| v.as_str())
            .map(String::from),
        duration_minutes: m.get("duration").and_then(|v| v.as_i64()),
        status,
        raw_payload_json: m.to_string(),
    })
}

// ── transcript_raw 编解码（official_minutes 的持久化载体） ─────────────────

/// 构造 transcript_raw JSON，把腾讯会议官方纪要附带在转写记录中。
///
/// 结构：`{"official_minutes": "..."}`。无官方纪要时返回 `"{}"`。
/// `generate_meeting_minutes` 通过 `parse_official_minutes` 读回。
pub fn build_transcript_raw(official_minutes: Option<&str>) -> String {
    match official_minutes.filter(|s| !s.trim().is_empty()) {
        Some(m) => serde_json::json!({ "official_minutes": m }).to_string(),
        None => "{}".to_string(),
    }
}

/// 从 transcript_raw 解析官方纪要。
///
/// 容错：非 JSON 或缺键时返回 None，不阻断纪要生成。
pub fn parse_official_minutes(transcript_raw: &str) -> Option<String> {
    if transcript_raw.trim().is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(transcript_raw).ok()?;
    value
        .get("official_minutes")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
}

// ── 辅助函数 ──────────────────────────────────────────────────────────────

fn row_to_meeting(row: &rusqlite::Row) -> rusqlite::Result<Meeting> {
    Ok(Meeting {
        id: row.get(0)?,
        project_id: row.get(1)?,
        meeting_id: row.get(2)?,
        meeting_code: row.get(3)?,
        subject: row.get(4)?,
        host_user_id: row.get(5)?,
        invitees_json: row.get(6)?,
        start_time: row.get(7)?,
        end_time: row.get(8)?,
        duration_minutes: row.get(9)?,
        status: row.get(10)?,
        link_status: row.get(11)?,
        source: row.get(12)?,
        raw_payload_json: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}
