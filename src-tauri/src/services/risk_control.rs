//! 双轨风险把控舱 — 范围蔓延预警 + 爆雷预警 + 防身话术库
//!
//! 第一项 需求蔓延警报器: 新需求对比合同范围 → 红黄绿评级
//! 第二项 实施爆雷预警: 周报/缺席率/数据延迟 → 延期概率计算
//! 第三项 顾问防身话术库: 场景匹配 → 专业话术生成

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Emitter;

use super::llm_service::{ChatMessage, LLMService};
use crate::services::verification::types::ScenarioType;

const MAX_SCOPE_EXTRACTION_BATCH_TOKENS: u32 = 900;
const MIN_SCOPE_EXTRACTION_BATCH_TOKENS: u32 = 450;
const SCOPE_EXTRACTION_PROMPT_OVERHEAD_TOKENS: u32 = 650;
const SCOPE_EXTRACTION_OUTPUT_RESERVE_TOKENS: u32 = 900;

/// 临时文件守卫：作用域结束时自动清理临时文件
struct TempFileGuard(PathBuf);
impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// ─── Types ───

/// 合同范围条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractScopeItem {
    pub id: i64,
    pub category: String,    // 模块/功能分类
    pub description: String, // 范围描述
    pub is_in_scope: bool,   // 是否属于合同范围内
    pub detail: String,      // 详细说明/合同条款引用
    pub created_at: String,
}

/// 需求蔓延检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeCreepResult {
    pub risk_level: String,         // "red" / "yellow" / "green"
    pub risk_label: String,         // "超范围" / "需评估" / "范围内"
    pub explanation: String,        // 详细解释
    pub matched_items: Vec<String>, // 匹配的合同条款
    pub suggestion: String,         // 建议行动
}

/// 项目健康指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMetric {
    pub id: i64,
    pub indicator_type: String, // "attendance" / "data_delay" / "issue_count" / "sentiment"
    pub value: f64,             // 指标值 (0-100, 越高越差)
    pub notes: String,          // 说明
    pub recorded_at: String,
}

/// 项目健康评分
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectHealthScore {
    pub overall_score: f64, // 0-100, 越高越危险
    pub risk_level: String, // "unknown" / "low" / "medium" / "high" / "critical"
    pub dimensions: Vec<HealthDimension>,
    pub trend: String,          // 趋势描述
    pub alert_count: u32,       // 需要关注的告警数
    pub metric_count: usize,    // 已录入的指标记录数
    pub data_completeness: f64, // 已有指标维度占比
}

/// 健康维度评分
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthDimension {
    pub name: String,
    pub score: f64,
    pub weight: f64,
    pub detail: String,
    pub has_data: bool,
}

/// 防身话术请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefenseScriptRequest {
    pub scenario: String, // 场景描述
    pub context: String,  // 上下文/背景
    pub tone: String,     // "push_back" / "guide" / "escalate"
}

/// 防身话术结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefenseScriptResult {
    pub scenario_label: String,
    pub scripts: Vec<ScriptItem>,
}

/// 单条话术
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptItem {
    pub phase: String,   // 阶段：开场 / 核心 / 收尾
    pub content: String, // 话术内容
    pub tip: String,     // 使用提示
}

/// 整库导入结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportDbResult {
    pub db_size_bytes: u64,
    pub document_count: i64,
    pub chunk_count: i64,
}

/// 候选范围条目（LLM 提取）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateScopeItem {
    pub category: String,
    pub description: String,
    pub is_in_scope: bool,
    pub detail: String,
    pub confidence: f64,
}

/// 合同范围提取进度事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractScopeProgress {
    pub project_id: i64,
    pub doc_id: i64,
    pub step: String,    // "reading" / "extracting" / "repairing" / "merging" / "done"
    pub current: u32,    // 当前批次（从 1 开始）
    pub total: u32,      // 总批次数
    pub message: String, // 人类可读的进度描述
}

/// 向前端发出合同范围提取进度事件
pub fn emit_scope_progress(
    app_handle: Option<&tauri::AppHandle>,
    project_id: i64,
    doc_id: i64,
    step: &str,
    current: u32,
    total: u32,
    message: &str,
) {
    if let Some(handle) = app_handle {
        let event = ContractScopeProgress {
            project_id,
            doc_id,
            step: step.to_string(),
            current,
            total,
            message: message.to_string(),
        };
        let _ = handle.emit("contract-scope-progress", &event);
    }
}

// ─── Risk Control Store ───

pub struct RiskControlStore {
    conn: Mutex<Connection>,
    pub db_path: PathBuf,
}

impl RiskControlStore {
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("Failed to open risk control DB: {}", e))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("Failed to set busy timeout on risk control store: {}", e))?;
        let store = Self {
            conn: Mutex::new(conn),
            db_path: db_path.to_path_buf(),
        };
        store.init_tables()?;
        Ok(store)
    }

    /// 创建内存存储，用于数据库损坏时兜底
    pub fn new_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("Failed to create in-memory risk control DB: {}", e))?;
        let store = Self {
            conn: Mutex::new(conn),
            db_path: PathBuf::from(":memory:"),
        };
        store.init_tables()?;
        Ok(store)
    }

    fn init_tables(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS contract_scope_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL DEFAULT -1,
                category TEXT NOT NULL,
                description TEXT NOT NULL,
                is_in_scope INTEGER NOT NULL DEFAULT 1,
                detail TEXT DEFAULT '',
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS project_health_metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL DEFAULT -1,
                indicator_type TEXT NOT NULL,
                value REAL NOT NULL,
                notes TEXT DEFAULT '',
                recorded_at TEXT DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_scope_project ON contract_scope_items(project_id);
            CREATE INDEX IF NOT EXISTS idx_health_project ON project_health_metrics(project_id);
            CREATE INDEX IF NOT EXISTS idx_health_type ON project_health_metrics(indicator_type);",
        )
        .map_err(|e| format!("Failed to init risk control tables: {}", e))?;

        // 分步执行 ALTER TABLE，忽略列已存在错误
        let alter_tables = [
            "ALTER TABLE contract_scope_items ADD COLUMN project_id INTEGER NOT NULL DEFAULT -1",
            "ALTER TABLE project_health_metrics ADD COLUMN project_id INTEGER NOT NULL DEFAULT -1",
        ];
        for sql in &alter_tables {
            let _ = conn.execute(sql, []);
        }
        drop(conn);
        self.migrate_legacy_risk_project_links()?;
        Ok(())
    }

    fn table_exists(&self, table_name: &str) -> Result<bool, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            params![table_name],
            |row| row.get::<_, i64>(0),
        )
        .map(|exists| exists == 1)
        .map_err(|e| format!("检查数据表失败: {}", e))
    }

    fn migrate_legacy_risk_project_links(&self) -> Result<(), String> {
        if !self.table_exists("risk_projects")? {
            return Ok(());
        }

        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute_batch(
            "UPDATE contract_scope_items
             SET project_id = (
                 SELECT kb_project_id FROM risk_projects
                 WHERE risk_projects.id = contract_scope_items.project_id
             )
             WHERE project_id IN (
                 SELECT id FROM risk_projects WHERE kb_project_id IS NOT NULL
             );

             UPDATE project_health_metrics
             SET project_id = (
                 SELECT kb_project_id FROM risk_projects
                 WHERE risk_projects.id = project_health_metrics.project_id
             )
             WHERE project_id IN (
                 SELECT id FROM risk_projects WHERE kb_project_id IS NOT NULL
             );",
        )
        .map_err(|e| format!("迁移旧风险项目关联失败: {}", e))
    }

    // ─── 合同范围管理 ───

    pub fn add_scope_item(
        &self,
        project_id: i64,
        category: &str,
        description: &str,
        is_in_scope: bool,
        detail: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO contract_scope_items (project_id, category, description, is_in_scope, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project_id, category, description, is_in_scope as i32, detail],
        )
        .map_err(|e| format!("Failed to add scope item: {}", e))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_scope_items(
        &self,
        project_id: i64,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<ContractScopeItem>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT id, category, description, is_in_scope, detail, created_at FROM contract_scope_items WHERE project_id = ?1 ORDER BY category, id LIMIT ?2 OFFSET ?3")
            .map_err(|e| format!("Failed to prepare: {}", e))?;
        let rows = stmt
            .query_map(
                params![project_id, limit.unwrap_or(-1), offset.unwrap_or(0)],
                |row| {
                    Ok(ContractScopeItem {
                        id: row.get(0)?,
                        category: row.get(1)?,
                        description: row.get(2)?,
                        is_in_scope: row.get::<_, i32>(3)? != 0,
                        detail: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )
            .map_err(|e| format!("Failed to query: {}", e))?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(items)
    }

    pub fn delete_scope_item(&self, project_id: i64, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let deleted = conn
            .execute(
                "DELETE FROM contract_scope_items WHERE id = ?1 AND project_id = ?2",
                params![id, project_id],
            )
            .map_err(|e| format!("Failed to delete: {}", e))?;
        if deleted == 0 {
            return Err(format!(
                "范围条目 {} 不存在或不属于当前项目 {}",
                id, project_id
            ));
        }
        Ok(())
    }

    // ─── 健康指标管理 ───

    pub fn record_health_metric(
        &self,
        project_id: i64,
        indicator_type: &str,
        value: f64,
        notes: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO project_health_metrics (project_id, indicator_type, value, notes) VALUES (?1, ?2, ?3, ?4)",
            params![project_id, indicator_type, value, notes],
        )
        .map_err(|e| format!("Failed to record metric: {}", e))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_recent_metrics(
        &self,
        project_id: i64,
        indicator_type: &str,
        limit: usize,
    ) -> Result<Vec<HealthMetric>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT id, indicator_type, value, notes, recorded_at FROM project_health_metrics WHERE project_id = ?1 AND indicator_type = ?2 ORDER BY id DESC LIMIT ?3")
            .map_err(|e| format!("Failed to prepare: {}", e))?;
        let rows = stmt
            .query_map(params![project_id, indicator_type, limit as i64], |row| {
                Ok(HealthMetric {
                    id: row.get(0)?,
                    indicator_type: row.get(1)?,
                    value: row.get(2)?,
                    notes: row.get(3)?,
                    recorded_at: row.get(4)?,
                })
            })
            .map_err(|e| format!("Failed to query: {}", e))?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(items)
    }

    pub fn get_all_recent_metrics(&self, project_id: i64) -> Result<Vec<HealthMetric>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT id, indicator_type, value, notes, recorded_at FROM project_health_metrics WHERE project_id = ?1 ORDER BY recorded_at DESC LIMIT 100")
            .map_err(|e| format!("Failed to prepare: {}", e))?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(HealthMetric {
                    id: row.get(0)?,
                    indicator_type: row.get(1)?,
                    value: row.get(2)?,
                    notes: row.get(3)?,
                    recorded_at: row.get(4)?,
                })
            })
            .map_err(|e| format!("Failed to query: {}", e))?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(items)
    }

    // ─── 健康评分计算 ───

    pub fn calculate_health_score(&self, project_id: i64) -> Result<ProjectHealthScore, String> {
        let metrics = self.get_all_recent_metrics(project_id)?;

        // 按类型分组取最新值
        let mut latest = std::collections::HashMap::new();
        for m in &metrics {
            latest.entry(m.indicator_type.clone()).or_insert(m.value);
        }

        // 各维度权重
        let weights: Vec<(&str, f64, &str)> = vec![
            ("attendance", 0.30, "客户关键岗位缺席率"),
            ("data_delay", 0.25, "期初数据延迟"),
            ("issue_count", 0.25, "未解决问题积压"),
            ("sentiment", 0.20, "客户配合度"),
        ];

        let mut dimensions = Vec::new();
        let mut weighted_sum = 0.0;
        let mut total_weight = 0.0;

        for (key, weight, label) in &weights {
            let value = latest.get(*key).copied();
            let score = value.unwrap_or(0.0);
            if value.is_some() {
                weighted_sum += score * weight;
                total_weight += weight;
            }
            dimensions.push(HealthDimension {
                name: label.to_string(),
                score,
                weight: *weight,
                detail: value
                    .map(|_| format!("最近记录值: {:.1}", score))
                    .unwrap_or_else(|| "暂无指标记录".to_string()),
                has_data: value.is_some(),
            });
        }

        let overall = if total_weight > 0.0 {
            weighted_sum / total_weight
        } else {
            0.0
        };

        let risk_level = if total_weight == 0.0 {
            "unknown"
        } else if overall >= 70.0 {
            "critical"
        } else if overall >= 50.0 {
            "high"
        } else if overall >= 30.0 {
            "medium"
        } else {
            "low"
        };

        let alert_count = dimensions
            .iter()
            .filter(|d| d.has_data && d.score >= 50.0)
            .count() as u32;
        let available_dimensions = dimensions.iter().filter(|d| d.has_data).count();
        let data_completeness = available_dimensions as f64 / dimensions.len() as f64;
        let trend = if available_dimensions == 0 {
            "暂无健康指标数据，无法判断趋势"
        } else if alert_count >= 2 {
            "⚠️ 多项指标偏高，建议紧急干预"
        } else if alert_count >= 1 {
            "🔶 存在风险点，建议关注"
        } else if available_dimensions < dimensions.len() {
            "部分指标缺失，当前判断仅供参考"
        } else {
            "✅ 项目整体健康"
        };

        Ok(ProjectHealthScore {
            overall_score: (overall * 10.0).round() / 10.0,
            risk_level: risk_level.to_string(),
            dimensions,
            trend: trend.to_string(),
            alert_count,
            metric_count: metrics.len(),
            data_completeness: (data_completeness * 1000.0).round() / 1000.0,
        })
    }

    // ─── LLM 驱动的风控逻辑 ───

    /// 检查需求是否超出合同范围 (P1.1)
    pub async fn check_scope_creep(
        &self,
        project_id: i64,
        llm: &LLMService,
        requirement: &str,
    ) -> Result<ScopeCreepResult, String> {
        let scope_items = self.list_scope_items(project_id, None, None)?;

        let scope_desc: String = if scope_items.is_empty() {
            "暂无合同范围定义".to_string()
        } else {
            scope_items
                .iter()
                .map(|item| {
                    let scope = if item.is_in_scope {
                        "[范围内]"
                    } else {
                        "[排除]"
                    };
                    format!(
                        "{} {} {}: {}",
                        scope, item.category, item.description, item.detail
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prompt = format!(
            "你是一个ERP实施项目的范围审计员。请判断以下新需求是否超出合同范围。\n\n\
             合同范围定义：\n{}\n\n\
             新需求：{}\n\n\
             请严格按照以下JSON格式回应，不要其他文字：\n\
             {{\n\
               \"risk_level\": \"red/yellow/green\",\n\
               \"risk_label\": \"超范围/需评估/范围内\",\n\
               \"explanation\": \"详细分析原因\",\n\
               \"matched_items\": [\"匹配的合同条款1\", \"条款2\"],\n\
               \"suggestion\": \"给顾问的建议行动\"\n\
             }}",
            scope_desc, requirement
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: SYSTEM_RISK_PROMPT.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];

        let config = llm.get_active_config()?;
        let (response, _report) = llm
            .verified_chat_completion(&messages, &config, ScenarioType::RiskReport)
            .await?;

        // 解析JSON响应
        serde_json::from_str(&response)
            .map_err(|e| format!("LLM返回格式错误: {} — 原始响应: {}", e, response))
    }

    /// 生成爆雷预警报告 (P1.2)
    pub async fn generate_risk_report(
        &self,
        project_id: i64,
        llm: &LLMService,
        additional_context: &str,
    ) -> Result<String, String> {
        let health = self.calculate_health_score(project_id)?;
        let metrics = self.get_all_recent_metrics(project_id)?;
        let scope_items = self.list_scope_items(project_id, None, None)?;

        let metrics_summary: String = metrics
            .iter()
            .map(|m| {
                format!(
                    "[{}] {}: {:.1} — {}",
                    m.recorded_at, m.indicator_type, m.value, m.notes
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let scope_summary = if scope_items.is_empty() {
            "暂无已确认的合同范围条目".to_string()
        } else {
            scope_items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    format!(
                        "【范围基线{}】[{}] {}：{}；依据：{}",
                        index + 1,
                        if item.is_in_scope {
                            "范围内"
                        } else {
                            "明确排除"
                        },
                        item.category,
                        item.description,
                        item.detail
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let health_score_summary = if health.risk_level == "unknown" {
            "暂无健康评分（未录入健康指标）".to_string()
        } else {
            format!(
                "{:.1}/100（风险等级：{}）",
                health.overall_score, health.risk_level
            )
        };

        let prompt = format!(
            "当前项目健康评分：{}\n\
             告警数：{}\n\n\
             健康指标数据完整度：{:.0}%（共 {} 条记录）\n\
             各维度评分：\n{}\n\n\
             原始指标记录：\n{}\n\n\
             已确认的合同范围基线：\n{}\n\n\
             项目文档检索证据与补充上下文：\n{}\n\n\
             请生成一份基于证据的实施爆雷预警报告，要求：\n\
             1. 首先说明分析覆盖范围、数据完整度和无法判断的事项\n\
             2. 分析合同/SOW范围风险、计划进度与超期风险、问题阻塞、交付与客户配合风险\n\
             3. 涉及日期时，以证据中的分析基准日期判断是否超期\n\
             4. 每个事实性结论必须引用对应的【证据N】、【范围基线N】或【阶段计划N】；没有依据时明确写“暂无证据，无法判断”\n\
             5. 严格区分文档事实与合理推断，禁止编造里程碑、日期、进度或风险数据\n\
             6. 给出按优先级排序的缓解措施、建议负责人和建议完成期限\n\
             7. 报告末尾列出实际引用的证据索引，包含证据编号、文档标题和章节",
            health_score_summary,
            health.alert_count,
            health.data_completeness * 100.0,
            health.metric_count,
            health
                .dimensions
                .iter()
                .map(|d| {
                    if d.has_data {
                        format!("- {}: {:.1}/100", d.name, d.score)
                    } else {
                        format!("- {}: 暂无数据", d.name)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            if metrics_summary.is_empty() {
                "暂无健康指标记录"
            } else {
                &metrics_summary
            },
            scope_summary,
            additional_context
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: SYSTEM_RISK_PROMPT.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];

        let config = llm.get_active_config()?;
        llm.chat_completion(&messages, &config).await
    }

    /// 生成防身话术 (P1.3)
    pub async fn generate_defense_script(
        &self,
        llm: &LLMService,
        request: &DefenseScriptRequest,
    ) -> Result<DefenseScriptResult, String> {
        let tone_guide = match request.tone.as_str() {
            "push_back" => "委婉但坚定地拒绝，给出专业理由",
            "guide" => "引导客户理解标准方案的价值，避免二开",
            "escalate" => "建议升级到更高级别的决策层讨论",
            _ => "专业、礼貌、有理有据",
        };

        let prompt = format!(
            "场景：{}\n上下文：{}\n沟通基调：{}\n\n\
             请生成三段式话术，严格按照以下JSON格式：\n\
             {{\n\
               \"scenario_label\": \"场景分类名称\",\n\
               \"scripts\": [\n\
                 {{\"phase\": \"开场\", \"content\": \"...\", \"tip\": \"使用时机\"}},\n\
                 {{\"phase\": \"核心\", \"content\": \"...\", \"tip\": \"关键话术\"}},\n\
                 {{\"phase\": \"收尾\", \"content\": \"...\", \"tip\": \"下一步行动\"}}\n\
               ]\n\
             }}",
            request.scenario, request.context, tone_guide
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: SCRIPT_SYSTEM_PROMPT.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];

        let config = llm.get_active_config()?;
        let (response, _report) = llm
            .verified_chat_completion(&messages, &config, ScenarioType::RiskReport)
            .await?;

        serde_json::from_str(&response)
            .map_err(|e| format!("LLM返回格式错误: {} — 原始响应: {}", e, response))
    }

    // ─── 范围提取 ───

    /// 从文档内容提取候选范围项（LLM 驱动）
    pub async fn extract_scope_from_document(
        &self,
        llm: &LLMService,
        chunks: &[super::metadata::ChunkMeta],
        app_handle: Option<&tauri::AppHandle>,
        project_id: i64,
        doc_id: i64,
    ) -> Result<Vec<CandidateScopeItem>, String> {
        let config = llm.get_active_config()?;
        let batch_token_budget = Self::scope_extraction_batch_token_budget(config.max_tokens);
        let contexts = Self::build_scope_extraction_batches(chunks, batch_token_budget);
        if contexts.is_empty() {
            return Err("文档内容为空，无法提取合同范围".to_string());
        }

        let total_batches = contexts.len() as u32;
        tracing::info!(total_batches, "开始合同范围提取，共 {} 个批次", total_batches);
        emit_scope_progress(
            app_handle, project_id, doc_id,
            "reading",
            0,
            total_batches,
            &format!("文档已分为 {} 个片段，准备提取", total_batches),
        );

        let mut extracted_items = Vec::new();
        let mut empty_response_count = 0usize;
        let mut parse_errors = Vec::new();

        for (index, context) in contexts.iter().enumerate() {
            tracing::info!(
                chunk_index = index + 1,
                total = contexts.len(),
                "正在提取第 {}/{} 段",
                index + 1,
                contexts.len()
            );
            emit_scope_progress(
                app_handle, project_id, doc_id,
                "extracting",
                (index + 1) as u32,
                total_batches,
                &format!("正在提取第 {}/{} 段", index + 1, contexts.len()),
            );

            let messages =
                Self::build_scope_extraction_messages(context, index + 1, contexts.len());
            let response = llm.chat_completion_unmasked(&messages, &config).await?;
            let response = if response.trim().is_empty() {
                tracing::warn!(
                    chunk_index = index + 1,
                    "第 {} 段 LLM 返回空响应，尝试重试",
                    index + 1
                );
                let retry_messages = Self::build_scope_extraction_retry_messages(context);
                llm.chat_completion_unmasked(&retry_messages, &config)
                    .await?
            } else {
                response
            };

            if response.trim().is_empty() {
                empty_response_count += 1;
                continue;
            }

            match Self::extract_json_from_llm_response(&response) {
                Ok(items) => extracted_items.extend(items),
                Err(parse_error) => {
                    tracing::warn!(
                        chunk_index = index + 1,
                        error = %parse_error,
                        "第 {} 段 JSON 解析失败，尝试修复",
                        index + 1
                    );
                    emit_scope_progress(
                        app_handle, project_id, doc_id,
                        "repairing",
                        (index + 1) as u32,
                        total_batches,
                        &format!("第 {} 段返回格式异常，正在修复", index + 1),
                    );

                    let repair_messages = Self::build_scope_json_repair_messages(&response);
                    let repaired = llm
                        .chat_completion_unmasked(&repair_messages, &config)
                        .await?;
                    if repaired.trim().is_empty() {
                        parse_errors.push(format!(
                            "片段 {} JSON 修复响应为空，原始响应预览: {}",
                            index + 1,
                            Self::response_preview(&response)
                        ));
                        continue;
                    }
                    match Self::extract_json_from_llm_response(&repaired) {
                        Ok(items) => extracted_items.extend(items),
                        Err(_) => parse_errors.push(format!(
                            "片段 {} {}，原始响应预览: {}",
                            index + 1,
                            parse_error,
                            Self::response_preview(&response)
                        )),
                    }
                }
            }
        }

        tracing::info!(
            total_items = extracted_items.len(),
            "原始提取 {} 项，正在去重归一化",
            extracted_items.len()
        );
        emit_scope_progress(
            app_handle, project_id, doc_id,
            "merging",
            total_batches,
            total_batches,
            &format!("提取完成，共 {} 项候选，正在去重归一化", extracted_items.len()),
        );

        let merged_items = Self::normalize_candidate_items(extracted_items);
        if !merged_items.is_empty() {
            tracing::info!(
                merged_count = merged_items.len(),
                "合同范围提取完成，归一化后 {} 项",
                merged_items.len()
            );
            emit_scope_progress(
                app_handle, project_id, doc_id,
                "done",
                total_batches,
                total_batches,
                &format!("提取完成，共 {} 项范围定义", merged_items.len()),
            );
            return Ok(merged_items);
        }

        if empty_response_count == contexts.len() {
            tracing::error!(
                empty_count = empty_response_count,
                total = contexts.len(),
                "LLM 对所有分块返回空响应"
            );
            return Err(
                "LLM 对所有 SOW 分块都返回空响应，无法提取合同范围。请先在设置中测试 LLM 连通性，或换用输出更稳定的模型。"
                    .to_string(),
            );
        }

        let diagnostics = if parse_errors.is_empty() {
            "模型未从任何分块中识别出明确范围项".to_string()
        } else {
            parse_errors.join("；")
        };
        tracing::error!(diagnostics = %diagnostics, "未提取到可确认的合同范围项");
        Err(format!("未提取到可确认的合同范围项。{}", diagnostics))
    }

    fn build_scope_extraction_messages(
        doc_content: &str,
        chunk_index: usize,
        total_chunks: usize,
    ) -> Vec<ChatMessage> {
        vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是 ERP 实施合同/SOW 范围审计专家。严格基于文档内容抽取范围定义，不编造信息，只输出 JSON 数组。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "请从以下 SOW/合同文档片段中提取合同范围基线。本片段是第 {}/{} 段。\n\n\
                     提取对象包括但不限于：实施范围、服务范围、工作范围、系统模块、功能点、接口、报表、数据迁移、培训、上线支持、交付物、客户责任、假设前提、明确排除项、不包含事项。\n\
                     不要依赖固定表头或固定写法，按语义判断；只提取片段中明确写出的内容，不要推断或补充。\n\
                     每项 detail 必须写出可追溯的原文依据或章节名称。\n\
                     最多提取 6 项，优先保留范围边界最清楚的条款。\n\
                     如果本片段没有明确范围条款，返回 []。\n\n\
                     文档片段：\n{}\n\n\
                     严格输出 JSON 数组，不要 Markdown，不要解释。数组元素格式：\n\
                     {{\"category\":\"实施范围/排除项/交付物/接口/培训/数据迁移/其他\",\"description\":\"范围描述\",\"is_in_scope\":true,\"detail\":\"原文依据：...\",\"confidence\":0.8}}",
                    chunk_index, total_chunks, doc_content
                ),
            },
        ]
    }

    fn build_scope_extraction_retry_messages(doc_content: &str) -> Vec<ChatMessage> {
        vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是合同/SOW 范围提取器。必须只输出 JSON 数组，不要解释。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "从以下文档摘录中提取 SOW/合同范围项。字段必须为 category、description、is_in_scope、detail、confidence。\
                     不要依赖固定表头，按语义判断；只输出 JSON 数组；没有结果输出 []；最多 6 项。\n\n{}",
                    doc_content
                ),
            },
        ]
    }

    fn build_scope_json_repair_messages(response: &str) -> Vec<ChatMessage> {
        vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是 JSON 修复器。只输出合法 JSON 数组，不要解释。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "把下面内容转换为合法 JSON 数组。数组元素字段必须为 category、description、is_in_scope、detail、confidence。\
                     如果没有可转换的范围项，输出 []。\n\n{}",
                    response
                ),
            },
        ]
    }

    fn response_preview(response: &str) -> String {
        let preview: String = response.chars().take(500).collect();
        if response.chars().count() > 500 {
            format!("{}...", preview)
        } else {
            preview
        }
    }

    fn scope_extraction_batch_token_budget(model_context_tokens: u32) -> u32 {
        let available = model_context_tokens
            .saturating_sub(SCOPE_EXTRACTION_PROMPT_OVERHEAD_TOKENS)
            .saturating_sub(SCOPE_EXTRACTION_OUTPUT_RESERVE_TOKENS);
        if available == 0 {
            MIN_SCOPE_EXTRACTION_BATCH_TOKENS
        } else {
            available.clamp(
                MIN_SCOPE_EXTRACTION_BATCH_TOKENS,
                MAX_SCOPE_EXTRACTION_BATCH_TOKENS,
            )
        }
    }

    fn build_scope_extraction_batches(
        chunks: &[super::metadata::ChunkMeta],
        max_context_tokens: u32,
    ) -> Vec<String> {
        let max_context_tokens = max_context_tokens.max(1);
        let mut batches = Vec::new();
        let mut current = String::new();

        for (index, chunk) in chunks.iter().enumerate() {
            let content = chunk.content.trim();
            if content.is_empty() {
                continue;
            }

            let section = chunk
                .section_path
                .as_deref()
                .filter(|section| !section.trim().is_empty())
                .unwrap_or("未标注章节");
            let prefix = format!("[文档分块 {}]\n章节：{}\n", index + 1, section);
            let prefix_tokens = Self::estimate_scope_tokens(&prefix);
            let content_budget = max_context_tokens
                .saturating_sub(prefix_tokens)
                .max(MIN_SCOPE_EXTRACTION_BATCH_TOKENS / 2);

            for part in Self::split_text_by_token_budget(content, content_budget) {
                let labeled = format!("{}{}\n", prefix, part.trim());
                if labeled.trim().is_empty() {
                    continue;
                }
                let labeled = if Self::estimate_scope_tokens(&labeled) > max_context_tokens {
                    Self::truncate_scope_text_to_token_budget(&labeled, max_context_tokens)
                } else {
                    labeled
                };
                let labeled_tokens = Self::estimate_scope_tokens(&labeled);
                let current_tokens = Self::estimate_scope_tokens(&current);

                if !current.trim().is_empty()
                    && current_tokens + labeled_tokens > max_context_tokens
                {
                    batches.push(current.trim().to_string());
                    current.clear();
                }
                current.push_str(&labeled);
                current.push('\n');
            }
        }

        if !current.trim().is_empty() {
            batches.push(current.trim().to_string());
        }

        batches
    }

    fn split_text_by_token_budget(text: &str, max_tokens: u32) -> Vec<String> {
        let max_tokens = max_tokens.max(1);
        let mut result = Vec::new();
        let mut current = String::new();

        for line in text.lines() {
            let line = format!("{}\n", line);
            let line_tokens = Self::estimate_scope_tokens(&line);
            if line_tokens > max_tokens {
                if !current.trim().is_empty() {
                    result.push(current.trim().to_string());
                    current.clear();
                }
                result.extend(Self::split_long_text_by_token_budget(&line, max_tokens));
                continue;
            }

            let current_tokens = Self::estimate_scope_tokens(&current);
            if !current.trim().is_empty() && current_tokens + line_tokens > max_tokens {
                result.push(current.trim().to_string());
                current.clear();
            }
            current.push_str(&line);
        }

        if !current.trim().is_empty() {
            result.push(current.trim().to_string());
        }
        result
    }

    fn split_long_text_by_token_budget(text: &str, max_tokens: u32) -> Vec<String> {
        let mut parts = Vec::new();
        let mut remaining = text.trim();

        while !remaining.is_empty() {
            let part = Self::truncate_scope_text_to_token_budget(remaining, max_tokens);
            if part.is_empty() {
                if let Some(ch) = remaining.chars().next() {
                    parts.push(ch.to_string());
                    remaining = remaining[ch.len_utf8()..].trim_start();
                } else {
                    break;
                }
                continue;
            }

            let consumed = part.len();
            parts.push(part.trim().to_string());
            remaining = remaining[consumed..].trim_start();
        }

        parts
    }

    fn estimate_scope_tokens(text: &str) -> u32 {
        let non_ascii_chars = text.chars().filter(|ch| !ch.is_ascii()).count();
        let ascii_chars = text.len().saturating_sub(non_ascii_chars);
        ((non_ascii_chars as f32 / 1.2) + (ascii_chars as f32 / 3.5)).ceil() as u32
    }

    fn truncate_scope_text_to_token_budget(text: &str, max_tokens: u32) -> String {
        let mut result = String::new();
        for ch in text.chars() {
            result.push(ch);
            if Self::estimate_scope_tokens(&result) > max_tokens {
                result.pop();
                break;
            }
        }
        result
    }

    /// 从 LLM 响应中提取 JSON（支持 markdown 代码块包裹）
    fn extract_json_from_llm_response(response: &str) -> Result<Vec<CandidateScopeItem>, String> {
        if response.trim().is_empty() {
            return Err("LLM 返回空响应".to_string());
        }
        // 尝试直接解析
        if let Ok(items) = Self::parse_scope_items_json(response) {
            return Ok(Self::normalize_candidate_items(items));
        }
        // 尝试提取 markdown 代码块中的 JSON
        if let Some(start) = response.find('[') {
            if let Some(end) = response.rfind(']') {
                let json_str = &response[start..=end];
                if let Ok(items) = Self::parse_scope_items_json(json_str) {
                    return Ok(Self::normalize_candidate_items(items));
                }
            }
        }
        if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                let json_str = &response[start..=end];
                if let Ok(items) = Self::parse_scope_items_json(json_str) {
                    return Ok(Self::normalize_candidate_items(items));
                }
            }
        }
        let partial_items = Self::parse_partial_scope_items(response);
        if !partial_items.is_empty() {
            return Ok(Self::normalize_candidate_items(partial_items));
        }
        Err(format!(
            "LLM 返回格式错误 — 原始响应预览: {}",
            Self::response_preview(response)
        ))
    }

    fn parse_scope_items_json(json: &str) -> Result<Vec<CandidateScopeItem>, serde_json::Error> {
        if let Ok(items) = serde_json::from_str::<Vec<CandidateScopeItem>>(json) {
            return Ok(items);
        }

        let value = serde_json::from_str::<serde_json::Value>(json)?;
        if let Some(array) = value.as_array() {
            return serde_json::from_value(serde_json::Value::Array(array.clone()));
        }

        if let Some(object) = value.as_object() {
            for key in ["items", "scope_items", "scopes", "data", "result"] {
                if let Some(items_value) = object.get(key) {
                    if items_value.is_array() {
                        return serde_json::from_value(items_value.clone());
                    }
                }
            }
            if let Ok(item) = serde_json::from_value::<CandidateScopeItem>(value.clone()) {
                return Ok(vec![item]);
            }
        }

        serde_json::from_value(value)
    }

    fn parse_partial_scope_items(response: &str) -> Vec<CandidateScopeItem> {
        let mut items = Vec::new();
        let mut object_start: Option<usize> = None;
        let mut brace_depth = 0i32;
        let mut in_string = false;
        let mut escaped = false;

        for (index, ch) in response.char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }

            match ch {
                '"' => in_string = true,
                '{' => {
                    if brace_depth == 0 {
                        object_start = Some(index);
                    }
                    brace_depth += 1;
                }
                '}' => {
                    if brace_depth > 0 {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            if let Some(start) = object_start.take() {
                                let end = index + ch.len_utf8();
                                if let Ok(item) = serde_json::from_str::<CandidateScopeItem>(
                                    &response[start..end],
                                ) {
                                    items.push(item);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        items
    }

    fn normalize_candidate_items(items: Vec<CandidateScopeItem>) -> Vec<CandidateScopeItem> {
        let mut seen = std::collections::BTreeSet::new();
        items
            .into_iter()
            .filter_map(|item| {
                let category = item.category.trim().to_string();
                let description = item.description.trim().to_string();
                let detail = item.detail.trim().to_string();
                if description.is_empty() {
                    return None;
                }
                let normalized_category = if category.is_empty() {
                    "未分类".to_string()
                } else {
                    category
                };
                let dedupe_key = format!(
                    "{}|{}|{}",
                    normalized_category.to_lowercase(),
                    description.to_lowercase(),
                    item.is_in_scope
                );
                if !seen.insert(dedupe_key) {
                    return None;
                }

                Some(CandidateScopeItem {
                    category: normalized_category,
                    description,
                    is_in_scope: item.is_in_scope,
                    detail,
                    confidence: item.confidence.clamp(0.0, 1.0),
                })
            })
            .collect()
    }

    /// 确认入库候选范围项（事务保护）
    pub fn confirm_scope_items(
        &self,
        project_id: i64,
        items: &[CandidateScopeItem],
    ) -> Result<usize, String> {
        let mut conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to begin transaction: {}", e))?;
        let mut count = 0usize;
        for item in items {
            tx.execute(
                "INSERT INTO contract_scope_items (project_id, category, description, is_in_scope, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![project_id, item.category, item.description, item.is_in_scope as i32, item.detail],
            ).map_err(|e| format!("Failed to insert scope item: {}", e))?;
            count += 1;
        }
        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {}", e))?;
        Ok(count)
    }

    // ─── 整库导出/导入 ───

    /// 导出整库（VACUUM INTO）
    pub fn export_database(&self, target_path: &str) -> Result<(), String> {
        let db_path = self.db_path.to_str().ok_or("Invalid db path")?.to_string();
        let backup_conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| format!("Failed to open DB for export: {}", e))?;
        backup_conn
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("Failed to set busy timeout on export connection: {}", e))?;
        backup_conn
            .execute_batch(&format!(
                "VACUUM INTO '{}';",
                target_path.replace('\'', "''")
            ))
            .map_err(|e| format!("VACUUM INTO failed: {}", e))?;
        Ok(())
    }

    /// 验证并导入数据库备份（返回统计信息）
    pub fn import_database(&self, backup_path: &str) -> Result<ImportDbResult, String> {
        use std::fs;
        use std::io::Read;

        // 1. 验证文件是合法 SQLite 数据库
        let header = {
            let mut f = fs::File::open(backup_path)
                .map_err(|e| format!("Cannot open backup file: {}", e))?;
            let mut buf = [0u8; 16];
            f.read_exact(&mut buf)
                .map_err(|e| format!("Cannot read backup file: {}", e))?;
            buf
        };
        if &header != b"SQLite format 3\0" {
            return Err("备份文件不是合法的 SQLite 数据库".to_string());
        }

        let db_path = self.db_path.as_path();

        // 2. 检查导入文件大小
        let meta = fs::metadata(backup_path).map_err(|e| format!("Cannot stat backup: {}", e))?;
        let db_size = meta.len();

        // 3. 备份当前 DB（安全措施），注册 RAII Guard 确保清理
        let temp_backup = db_path.with_extension("db.before_import");
        let _guard = TempFileGuard(temp_backup.clone());
        fs::copy(db_path, &temp_backup).map_err(|e| format!("Cannot backup current DB: {}", e))?;

        // 4. 替换当前 DB 文件 - 先释放连接，再复制，再重新打开
        // 注意：先用内存连接替换以释放文件锁，避免 Windows 文件锁定
        {
            let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
            *conn = rusqlite::Connection::open_in_memory()
                .map_err(|e| format!("Cannot open temp connection: {}", e))?;
        } // 锁守卫在此释放

        // 复制备份文件覆盖当前 DB
        fs::copy(backup_path, db_path).map_err(|e| format!("Cannot restore backup: {}", e))?;

        // 5. 重新打开连接并初始化表结构
        {
            let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
            let reopened = rusqlite::Connection::open(db_path)
                .map_err(|e| format!("Cannot reopen connection after import: {}", e))?;
            reopened
                .busy_timeout(std::time::Duration::from_secs(5))
                .map_err(|e| format!("Cannot set busy timeout after import: {}", e))?;
            *conn = reopened;
        } // 锁守卫在此释放
        self.init_tables()?;

        // 统计（临时备份由 TempFileGuard 自动清理）
        let (document_count, chunk_count) = {
            let conn = self.conn.lock().map_err(|e| e.to_string())?;
            let scope_count = conn
                .query_row("SELECT COUNT(*) FROM contract_scope_items", [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);
            let metric_count = conn
                .query_row("SELECT COUNT(*) FROM project_health_metrics", [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);
            (scope_count, metric_count)
        };

        Ok(ImportDbResult {
            db_size_bytes: db_size,
            document_count,
            chunk_count,
        })
    }
}

// ─── System Prompts ───

const SYSTEM_RISK_PROMPT: &str = "\
你是一个ERP实施项目的风险控制专家。你的职责是：\n\
1. 严格审查新需求是否超出合同范围\n\
2. 客观评估项目健康状态\n\
3. 给出专业的风险预警和行动建议\n\
\n\
核心原则：\n\
- 立场中立，基于合同条款和项目数据做判断\n\
- 不偏向客户或实施方\n\
- 建议必须具体可执行";

const SCRIPT_SYSTEM_PROMPT: &str = "\
你是一个ERP实施领域的沟通专家，擅长为实施顾问编写高情商的沟通话术。\n\
\n\
话术风格要求：\n\
- 专业：使用ERP行业术语，体现顾问经验\n\
- 得体：在不破坏客户关系的前提下坚持专业立场\n\
- 结构化：每个场景分为开场/核心/收尾三段\n\
- 有理有据：每个论点都要有合同条款、行业标准或技术理由支撑";

#[cfg(test)]
mod tests {
    use super::*;

    fn new_store() -> RiskControlStore {
        let store = RiskControlStore::new(Path::new(":memory:")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS projects (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    client_name TEXT NOT NULL DEFAULT '',
                    description TEXT NOT NULL DEFAULT '',
                    status TEXT NOT NULL DEFAULT 'active',
                    current_phase TEXT NOT NULL DEFAULT 'survey',
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                    CHECK (status IN ('active', 'archived'))
                );
                INSERT INTO projects (id, name, client_name) VALUES (1, '项目A', '客户A');
                INSERT INTO projects (id, name, client_name) VALUES (2, '项目B', '客户B');",
            )
            .unwrap();
        }
        store
    }

    fn test_chunk(id: i64, content: &str) -> crate::services::metadata::ChunkMeta {
        crate::services::metadata::ChunkMeta {
            id,
            vector_key: id,
            document_id: 1,
            content: content.to_string(),
            section_path: None,
            tags: None,
            line_no: None,
            created_at: "2026-06-07 00:00:00".to_string(),
        }
    }

    #[test]
    fn extract_json_accepts_items_wrapper() {
        let response = r#"{
            "items": [
                {
                    "category": "服务范围",
                    "description": "采购模块实施",
                    "is_in_scope": true,
                    "detail": "原文依据：SOW服务范围包含采购模块",
                    "confidence": 1.2
                }
            ]
        }"#;

        let items = RiskControlStore::extract_json_from_llm_response(response).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].category, "服务范围");
        assert_eq!(items[0].description, "采购模块实施");
        assert_eq!(items[0].confidence, 1.0);
    }

    #[test]
    fn extract_json_salvages_truncated_array_objects() {
        let response = r#"[
            {"category":"实施范围","description":"PLM模块-配方管理新增字段","is_in_scope":true,"detail":"原文依据：实施优化范围表格","confidence":1.0},
            {"category":"实施范围","description":"PLM模块-实验bom变更管理","is_in_scope":true,"detail":"原文依据：实施优化范围表格","confidence":1.0},
            {"category":"实施范围","description":"PLM模块-材料基础信息"#;

        let items = RiskControlStore::extract_json_from_llm_response(response).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].description, "PLM模块-配方管理新增字段");
        assert_eq!(items[1].description, "PLM模块-实验bom变更管理");
    }

    #[test]
    fn scope_batches_keep_unkeyworded_sections() {
        let chunks = vec![
            test_chunk(1, "封面\n项目名称\n版本记录\n目录"),
            test_chunk(2, "特殊约定：甲方需在上线前提供历史 BOM 清单和编码负责人。"),
            test_chunk(
                3,
                "实施范围：采购管理、库存管理、销售管理。\n交付物：蓝图文档、培训材料。\n不包含：银企直连接口。",
            ),
        ];

        let contexts = RiskControlStore::build_scope_extraction_batches(&chunks, 450);
        let joined = contexts.join("\n");
        assert!(joined.contains("特殊约定"));
        assert!(joined.contains("实施范围"));
    }

    #[test]
    fn scope_batches_respect_token_budget() {
        let content = (0..120)
            .map(|index| {
                format!(
                    "实施范围第{}项：这里是一段需要 LLM 语义判断的 SOW 范围描述。",
                    index
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = vec![test_chunk(1, &content)];

        let contexts = RiskControlStore::build_scope_extraction_batches(&chunks, 220);
        assert!(contexts.len() > 1);
        assert!(contexts
            .iter()
            .all(|context| RiskControlStore::estimate_scope_tokens(context) <= 220));
    }

    #[test]
    fn normalize_candidate_items_deduplicates_across_chunks() {
        let items = vec![
            CandidateScopeItem {
                category: "实施范围".to_string(),
                description: "PLM 配方管理新增字段".to_string(),
                is_in_scope: true,
                detail: "原文依据：片段 1".to_string(),
                confidence: 0.9,
            },
            CandidateScopeItem {
                category: "实施范围".to_string(),
                description: "PLM 配方管理新增字段".to_string(),
                is_in_scope: true,
                detail: "原文依据：片段 2".to_string(),
                confidence: 0.8,
            },
        ];

        let normalized = RiskControlStore::normalize_candidate_items(items);
        assert_eq!(normalized.len(), 1);
    }

    #[test]
    fn init_tables_does_not_create_risk_projects_table() {
        let store = new_store();
        let conn = store.conn.lock().unwrap();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'risk_projects'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(exists, 0);
    }

    #[test]
    fn test_add_and_list_scope_items() {
        let store = new_store();
        let pid = 1;
        let id = store
            .add_scope_item(pid, "FI", "总账模块实施", true, "合同第3.1条")
            .unwrap();
        assert!(id > 0);
        store
            .add_scope_item(pid, "FI", "银企直连", false, "合同排除项清单第5条")
            .unwrap();

        let items = store.list_scope_items(pid, None, None).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_in_scope);
        assert!(!items[1].is_in_scope);
    }

    #[test]
    fn test_record_and_calculate_health() {
        let store = new_store();
        let pid = 1;
        store
            .record_health_metric(pid, "attendance", 80.0, "项目经理连续2周缺席")
            .unwrap();
        store
            .record_health_metric(pid, "data_delay", 60.0, "期初数据延迟2周")
            .unwrap();

        let score = store.calculate_health_score(pid).unwrap();
        assert!((score.overall_score - 70.9).abs() < 0.1);
        assert_eq!(score.dimensions.len(), 4);
        assert_eq!(score.metric_count, 2);
        assert_eq!(score.data_completeness, 0.5);
        assert_eq!(score.dimensions.iter().filter(|d| d.has_data).count(), 2);
    }

    #[test]
    fn test_delete_scope_item() {
        let store = new_store();
        let pid = 1;
        let id = store
            .add_scope_item(pid, "MM", "采购模块", true, "")
            .unwrap();
        store.delete_scope_item(pid, id).unwrap();
        assert_eq!(store.list_scope_items(pid, None, None).unwrap().len(), 0);
    }

    #[test]
    fn test_delete_scope_item_rejects_other_project() {
        let store = new_store();
        let id = store.add_scope_item(1, "MM", "采购模块", true, "").unwrap();

        assert!(store.delete_scope_item(2, id).is_err());
        assert_eq!(store.list_scope_items(1, None, None).unwrap().len(), 1);
    }

    #[test]
    fn test_health_empty_returns_unknown() {
        let store = new_store();
        let pid = 1;
        let score = store.calculate_health_score(pid).unwrap();
        assert_eq!(score.overall_score, 0.0);
        assert_eq!(score.risk_level, "unknown");
        assert_eq!(score.metric_count, 0);
        assert_eq!(score.data_completeness, 0.0);
        assert!(score.dimensions.iter().all(|d| !d.has_data));
    }

    #[test]
    fn test_get_recent_metrics() {
        let store = new_store();
        let pid = 1;
        store
            .record_health_metric(pid, "attendance", 50.0, "测试")
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        store
            .record_health_metric(pid, "attendance", 60.0, "测试2")
            .unwrap();

        let metrics = store.get_recent_metrics(pid, "attendance", 2).unwrap();
        assert_eq!(metrics.len(), 2);
        // 最新记录排在前面
        assert!((metrics[0].value - 60.0).abs() < 0.01);
        assert!((metrics[1].value - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_confirm_scope_items() {
        let store = new_store();
        let pid = 1;

        // 构造候选范围项
        let candidates = vec![
            CandidateScopeItem {
                category: "FI".to_string(),
                description: "总账模块".to_string(),
                is_in_scope: true,
                detail: "合同第3.1条".to_string(),
                confidence: 0.95,
            },
            CandidateScopeItem {
                category: "FI".to_string(),
                description: "银企直连".to_string(),
                is_in_scope: false,
                detail: "排除项第5条".to_string(),
                confidence: 0.9,
            },
            CandidateScopeItem {
                category: "MM".to_string(),
                description: "采购模块".to_string(),
                is_in_scope: true,
                detail: "合同第3.2条".to_string(),
                confidence: 0.88,
            },
        ];

        // 批量确认入库
        let count = store.confirm_scope_items(pid, &candidates).unwrap();
        assert_eq!(count, 3);

        // 验证已入库
        let items = store.list_scope_items(pid, None, None).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].category, "FI");
        assert_eq!(items[0].description, "总账模块");
        assert!(items[0].is_in_scope);
        assert_eq!(items[1].description, "银企直连");
        assert!(!items[1].is_in_scope);
    }

    #[test]
    #[ignore = "涉及文件系统 VACUUM INTO 操作，需在集成测试环境中运行"]
    fn test_export_import_database() {
        let store = new_store();
        let pid = 1;
        store
            .add_scope_item(pid, "FI", "总账", true, "测试")
            .unwrap();

        // 导出到临时文件
        let export_path = std::env::temp_dir().join("risk_control_test_export.db");
        let export_str = export_path.to_str().unwrap();
        store.export_database(export_str).unwrap();
        assert!(export_path.exists());

        // 验证导入
        let result = store.import_database(export_str).unwrap();
        assert!(result.db_size_bytes > 0);
        assert_eq!(result.document_count, 1);

        // 清理
        let _ = std::fs::remove_file(&export_path);
    }
}
