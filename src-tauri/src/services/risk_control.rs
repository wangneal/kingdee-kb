//! 双轨风险把控舱 — Scope Creep 预警 + 爆雷预警 + 防身话术库
//!
//! P1.1 需求蔓延警报器: 新需求 vs 合同范围 → 红黄绿评级
//! P1.2 实施爆雷预警: 周报/缺席率/数据延迟 → 延期概率计算
//! P1.3 顾问防身话术库: 场景匹配 → 专业话术生成

use serde::{Deserialize, Serialize};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

use super::llm_service::{ChatMessage, LLMService};

// ─── Types ───

/// 合同范围条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractScopeItem {
    pub id: i64,
    pub category: String,       // 模块/功能分类
    pub description: String,    // 范围描述
    pub is_in_scope: bool,      // true=在范围内, false=明确排除
    pub detail: String,         // 详细说明/合同条款引用
    pub created_at: String,
}

/// 需求蔓延检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeCreepResult {
    pub risk_level: String,     // "red" / "yellow" / "green"
    pub risk_label: String,     // "超范围" / "需评估" / "范围内"
    pub explanation: String,    // 详细解释
    pub matched_items: Vec<String>, // 匹配的合同条款
    pub suggestion: String,     // 建议行动
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
    pub overall_score: f64,     // 0-100, 越高越危险
    pub risk_level: String,     // "low" / "medium" / "high" / "critical"
    pub dimensions: Vec<HealthDimension>,
    pub trend: String,          // 趋势描述
    pub alert_count: u32,       // 需要关注的告警数
}

/// 健康维度评分
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthDimension {
    pub name: String,
    pub score: f64,
    pub weight: f64,
    pub detail: String,
}

/// 防身话术请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefenseScriptRequest {
    pub scenario: String,       // 场景描述
    pub context: String,        // 上下文/背景
    pub tone: String,           // "push_back" / "guide" / "escalate"
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
    pub phase: String,          // 阶段：开场 / 核心 / 收尾
    pub content: String,        // 话术内容
    pub tip: String,            // 使用提示
}

// ─── Risk Control Store ───

pub struct RiskControlStore {
    conn: Mutex<Connection>,
}

impl RiskControlStore {
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("Failed to open risk control DB: {}", e))?;
        let store = Self { conn: Mutex::new(conn) };
        store.init_tables()?;
        Ok(store)
    }

    fn init_tables(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS contract_scope_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                category TEXT NOT NULL,
                description TEXT NOT NULL,
                is_in_scope INTEGER NOT NULL DEFAULT 1,
                detail TEXT DEFAULT '',
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS project_health_metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                indicator_type TEXT NOT NULL,
                value REAL NOT NULL,
                notes TEXT DEFAULT '',
                recorded_at TEXT DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_health_type ON project_health_metrics(indicator_type);",
        )
        .map_err(|e| format!("Failed to init risk control tables: {}", e))
    }

    // ─── 合同范围管理 ───

    pub fn add_scope_item(&self, category: &str, description: &str, is_in_scope: bool, detail: &str) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO contract_scope_items (category, description, is_in_scope, detail) VALUES (?1, ?2, ?3, ?4)",
            params![category, description, is_in_scope as i32, detail],
        )
        .map_err(|e| format!("Failed to add scope item: {}", e))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_scope_items(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<ContractScopeItem>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT id, category, description, is_in_scope, detail, created_at FROM contract_scope_items ORDER BY category, id LIMIT ?1 OFFSET ?2")
            .map_err(|e| format!("Failed to prepare: {}", e))?;
        let rows = stmt.query_map(params![limit.unwrap_or(-1), offset.unwrap_or(0)], |row| {
            Ok(ContractScopeItem {
                id: row.get(0)?,
                category: row.get(1)?,
                description: row.get(2)?,
                is_in_scope: row.get::<_, i32>(3)? != 0,
                detail: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query: {}", e))?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(items)
    }

    pub fn delete_scope_item(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute("DELETE FROM contract_scope_items WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete: {}", e))?;
        Ok(())
    }

    // ─── 健康指标管理 ───

    pub fn record_health_metric(&self, indicator_type: &str, value: f64, notes: &str) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO project_health_metrics (indicator_type, value, notes) VALUES (?1, ?2, ?3)",
            params![indicator_type, value, notes],
        )
        .map_err(|e| format!("Failed to record metric: {}", e))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_recent_metrics(&self, indicator_type: &str, limit: usize) -> Result<Vec<HealthMetric>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT id, indicator_type, value, notes, recorded_at FROM project_health_metrics WHERE indicator_type = ?1 ORDER BY id DESC LIMIT ?2")
            .map_err(|e| format!("Failed to prepare: {}", e))?;
        let rows = stmt.query_map(params![indicator_type, limit as i64], |row| {
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

    pub fn get_all_recent_metrics(&self) -> Result<Vec<HealthMetric>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT id, indicator_type, value, notes, recorded_at FROM project_health_metrics ORDER BY recorded_at DESC LIMIT 100")
            .map_err(|e| format!("Failed to prepare: {}", e))?;
        let rows = stmt.query_map([], |row| {
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

    pub fn calculate_health_score(&self) -> Result<ProjectHealthScore, String> {
        let metrics = self.get_all_recent_metrics()?;

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
            let score = latest.get(*key).copied().unwrap_or(30.0); // 默认30
            weighted_sum += score * weight;
            total_weight += weight;
            dimensions.push(HealthDimension {
                name: label.to_string(),
                score,
                weight: *weight,
                detail: format!("最近记录值: {:.1}", score),
            });
        }

        let overall = if total_weight > 0.0 { weighted_sum / total_weight } else { 0.0 };

        let risk_level = if overall >= 70.0 { "critical" }
            else if overall >= 50.0 { "high" }
            else if overall >= 30.0 { "medium" }
            else { "low" };

        let alert_count = dimensions.iter().filter(|d| d.score >= 50.0).count() as u32;
        let trend = if alert_count >= 2 { "⚠️ 多项指标偏高，建议紧急干预" }
            else if alert_count >= 1 { "🔶 存在风险点，建议关注" }
            else { "✅ 项目整体健康" };

        Ok(ProjectHealthScore {
            overall_score: (overall * 10.0).round() / 10.0,
            risk_level: risk_level.to_string(),
            dimensions,
            trend: trend.to_string(),
            alert_count,
        })
    }

    // ─── LLM 驱动的风控逻辑 ───

    /// 检查需求是否超出合同范围 (P1.1)
    pub async fn check_scope_creep(
        &self,
        llm: &LLMService,
        requirement: &str,
    ) -> Result<ScopeCreepResult, String> {
        let scope_items = self.list_scope_items(None, None)?;

        let scope_desc: String = if scope_items.is_empty() {
            "暂无合同范围定义".to_string()
        } else {
            scope_items.iter().map(|item| {
                let scope = if item.is_in_scope { "[范围内]" } else { "[排除]" };
                format!("{} {} {}: {}", scope, item.category, item.description, item.detail)
            }).collect::<Vec<_>>().join("\n")
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
            ChatMessage { role: "system".to_string(), content: SYSTEM_RISK_PROMPT.to_string() },
            ChatMessage { role: "user".to_string(), content: prompt },
        ];

        let config = llm.get_config()?;
        let response = llm.chat_completion(&messages, &config).await?;

        // 解析JSON响应
        serde_json::from_str(&response)
            .map_err(|e| format!("LLM返回格式错误: {} — 原始响应: {}", e, response))
    }

    /// 生成爆雷预警报告 (P1.2)
    pub async fn generate_risk_report(
        &self,
        llm: &LLMService,
        additional_context: &str,
    ) -> Result<String, String> {
        let health = self.calculate_health_score()?;
        let metrics = self.get_all_recent_metrics()?;

        let metrics_summary: String = metrics.iter()
            .map(|m| format!("[{}] {}: {:.1} — {}", m.recorded_at, m.indicator_type, m.value, m.notes))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "当前项目健康评分：{:.1}/100（风险等级：{}）\n\
             告警数：{}\n\n\
             各维度评分：\n{}\n\n\
             原始指标记录：\n{}\n\n\
             补充上下文：\n{}\n\n\
             请生成一份简要的实施爆雷预警报告，包含：\n\
             1. 整体风险评估\n\
             2. 主要风险因子\n\
             3. 建议的缓解措施\n\
             4. 建议告知客户的沟通策略",
            health.overall_score, health.risk_level, health.alert_count,
            health.dimensions.iter().map(|d| format!("- {}: {:.1}/100", d.name, d.score)).collect::<Vec<_>>().join("\n"),
            metrics_summary,
            additional_context
        );

        let messages = vec![
            ChatMessage { role: "system".to_string(), content: SYSTEM_RISK_PROMPT.to_string() },
            ChatMessage { role: "user".to_string(), content: prompt },
        ];

        let config = llm.get_config()?;
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
            ChatMessage { role: "system".to_string(), content: SCRIPT_SYSTEM_PROMPT.to_string() },
            ChatMessage { role: "user".to_string(), content: prompt },
        ];

        let config = llm.get_config()?;
        let response = llm.chat_completion(&messages, &config).await?;

        serde_json::from_str(&response)
            .map_err(|e| format!("LLM返回格式错误: {} — 原始响应: {}", e, response))
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
        RiskControlStore::new(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_add_and_list_scope_items() {
        let store = new_store();
        let id = store.add_scope_item("FI", "总账模块实施", true, "合同第3.1条").unwrap();
        assert!(id > 0);
        store.add_scope_item("FI", "银企直连", false, "合同排除项清单第5条").unwrap();

        let items = store.list_scope_items(None, None).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_in_scope);
        assert!(!items[1].is_in_scope);
    }

    #[test]
    fn test_record_and_calculate_health() {
        let store = new_store();
        store.record_health_metric("attendance", 80.0, "项目经理连续2周缺席").unwrap();
        store.record_health_metric("data_delay", 60.0, "期初数据延迟2周").unwrap();

        let score = store.calculate_health_score().unwrap();
        assert!(score.overall_score > 0.0);
        assert_eq!(score.dimensions.len(), 4);
    }

    #[test]
    fn test_delete_scope_item() {
        let store = new_store();
        let id = store.add_scope_item("MM", "采购模块", true, "").unwrap();
        store.delete_scope_item(id).unwrap();
        assert_eq!(store.list_scope_items(None, None).unwrap().len(), 0);
    }

    #[test]
    fn test_health_empty_returns_default() {
        let store = new_store();
        let score = store.calculate_health_score().unwrap();
        // 空数据默认返回30分
        assert!(score.overall_score > 0.0);
    }

    #[test]
    fn test_get_recent_metrics() {
        let store = new_store();
        store.record_health_metric("attendance", 50.0, "测试").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        store.record_health_metric("attendance", 60.0, "测试2").unwrap();

        let metrics = store.get_recent_metrics("attendance", 2).unwrap();
        assert_eq!(metrics.len(), 2);
        // Most recent first
        assert!((metrics[0].value - 60.0).abs() < 0.01);
        assert!((metrics[1].value - 50.0).abs() < 0.01);
    }
}
