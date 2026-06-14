//! 本地安全脱敏管道 — 发送给 LLM 前过滤敏感信息，返回后还原
//!
//! 流程: 扫描文本 → 替换敏感信息为带类型标签的占位符 → 发给 LLM → 还原原文
//! 支持: 财务金额、身份证号、手机号、邮箱、自定义敏感词（可配置类型）
//!
//! 全部在 Rust 本地执行，无需联网，不依赖第三方 NER 服务。
//!
//! ## 语义保留设计
//!
//! 占位符带类型标签（如 `[$_NAME_1]`、`[$_PHONE_1]`、`[$_AMT_1]`），
//! 让 LLM 仍能识别"这是人名/金额/手机号"，正常推理上下文语义。
//!
//! ## 脱敏的能力边界（权衡）
//!
//! - **保留**：类型语义（LLM 知道"这是个人名""这是个金额"）、个体区分（`NAME_1` ≠ `NAME_2`）
//! - **丢失**：具体数值（无法比较金额大小）、具体归属（无法判断手机号运营商/邮箱域名）
//! - 这是安全性与可用性的权衡。敏感词类型配置越准确，LLM 推理质量越高。
//!
//! ## 持久化
//!
//! 自定义敏感词持久化到 `sensitive_keywords` 表（text + kind），
//! 应用启动时从 DB 加载到内存，避免重启丢失。

use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// 脱敏结果：脱敏后的文本 + 还原映射表
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesensitizeResult {
    /// 脱敏后的文本（可安全发给 LLM）
    pub safe_text: String,
    /// 占位符 → 原始值的映射（用于还原）
    pub mapping: HashMap<String, String>,
}

/// 自定义敏感词的语义类型，决定占位符的类型标签。
///
/// 类型越准确，LLM 推理质量越高（例：项目代号标成 Term 而非 Name，
/// LLM 不会误以为是人名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveKind {
    /// 人名/高管姓名 → `[$_NAME_N]`
    Name,
    /// 项目代号/产品名/术语 → `[$_TERM_N]`
    Term,
    /// 合同号/订单号/内部编号 → `[$_CODE_N]`
    Code,
    /// 其他自定义 → `[$_CUSTOM_N]`
    Custom,
}

impl SensitiveKind {
    /// 占位符中的类型标签（如 NAME / TERM / CODE / CUSTOM）
    fn tag(&self) -> &'static str {
        match self {
            Self::Name => "NAME",
            Self::Term => "TERM",
            Self::Code => "CODE",
            Self::Custom => "CUSTOM",
        }
    }

    /// 从字符串解析类型，未知值降级为 Custom
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "name" => Self::Name,
            "term" => Self::Term,
            "code" => Self::Code,
            _ => Self::Custom,
        }
    }
}

/// 自定义敏感词条目（词文本 + 类型）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitiveKeyword {
    pub text: String,
    pub kind: SensitiveKind,
}

/// 脱敏管道（线程安全）
pub struct Desensitizer {
    /// 用户自定义敏感词库（带类型标签）
    custom_keywords: Mutex<Vec<SensitiveKeyword>>,
    /// 金额模式：¥1,234.56 / 1234.56万元 / 1234.56元
    amount_re: Regex,
    /// 身份证号：18位数字（含x结尾）
    id_re: Regex,
    /// 手机号：1xx-xxxx-xxxx 或 1xxxxxxxxxx
    phone_re: Regex,
    /// 邮箱
    email_re: Regex,
    /// 银行卡号：16-19位数字
    bank_re: Regex,
    /// 还原容错：匹配形如占位符的片段（用于 LLM 改写后的归一化重试）
    placeholder_re: Regex,
}

impl Desensitizer {
    /// 创建新的脱敏器，加载内置规则
    pub fn new() -> Self {
        Self {
            custom_keywords: Mutex::new(Vec::new()),
            amount_re: Regex::new(r"[¥￥]?\d{1,3}(?:,\d{3})*(?:\.\d+)?(?:万?元)?(?:人民币)?")
                .expect("Invalid amount regex"),
            id_re: Regex::new(
                r"\b[1-9]\d{5}(?:19|20)\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])\d{3}[\dXx]\b",
            )
            .expect("Invalid ID regex"),
            phone_re: Regex::new(r"\b1[3-9]\d{9}\b").expect("Invalid phone regex"),
            email_re: Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")
                .expect("Invalid email regex"),
            bank_re: Regex::new(r"\b\d{16,19}\b").expect("Invalid bank card regex"),
            // 匹配带类型标签的占位符：[$_TYPE_N]，容许内部空白和大小写差异
            placeholder_re: Regex::new(r"\[\s*\$\$_?[A-Za-z]+_?\d+\s*\]")
                .expect("Invalid placeholder regex"),
        }
    }

    /// 添加自定义敏感词（带类型）。
    ///
    /// 同一词重复添加会更新其类型。
    pub fn add_typed_keyword(&self, keyword: &str, kind: SensitiveKind) {
        let keyword = keyword.trim().to_string();
        if keyword.is_empty() {
            return;
        }
        if let Ok(mut kw) = self.custom_keywords.lock() {
            if let Some(existing) = kw.iter_mut().find(|k| k.text == keyword) {
                existing.kind = kind;
            } else {
                kw.push(SensitiveKeyword {
                    text: keyword,
                    kind,
                });
            }
        }
    }

    /// 添加自定义敏感词（兼容旧调用，默认 Custom 类型）
    pub fn add_keyword(&self, keyword: &str) {
        self.add_typed_keyword(keyword, SensitiveKind::Custom);
    }

    /// 批量添加敏感词（带类型）
    pub fn add_typed_keywords(&self, keywords: &[SensitiveKeyword]) {
        for k in keywords {
            self.add_typed_keyword(&k.text, k.kind);
        }
    }

    /// 获取当前敏感词列表（带类型）
    pub fn get_keywords(&self) -> Vec<SensitiveKeyword> {
        self.custom_keywords
            .lock()
            .map(|kw| kw.clone())
            .unwrap_or_default()
    }

    /// 清除自定义敏感词
    pub fn clear_keywords(&self) {
        if let Ok(mut kw) = self.custom_keywords.lock() {
            kw.clear();
        }
    }

    /// 删除指定的自定义敏感词（按文本匹配）
    pub fn remove_keyword(&self, keyword: &str) -> bool {
        if let Ok(mut kw) = self.custom_keywords.lock() {
            let before = kw.len();
            kw.retain(|k| k.text != keyword);
            kw.len() < before
        } else {
            false
        }
    }

    /// 执行脱敏：扫描文本，替换敏感信息为占位符
    pub fn desensitize(&self, text: &str) -> DesensitizeResult {
        let mut safe_text = text.to_string();
        let mut mapping = HashMap::new();
        let mut counter = 0usize;

        // 1. 自定义敏感词（按文本长度降序，避免短词部分匹配长词）
        if let Ok(keywords) = self.custom_keywords.lock() {
            let mut sorted = keywords.clone();
            sorted.sort_by(|a, b| b.text.len().cmp(&a.text.len()));
            for kw in &sorted {
                if kw.text.chars().count() < 2 {
                    continue;
                }
                let tag = kw.kind.tag();
                let placeholder = format!("[$_{}_{}]", tag, counter);
                let count = safe_text.matches(&kw.text).count();
                if count > 0 {
                    safe_text = safe_text.replace(&kw.text, &placeholder);
                    mapping.insert(placeholder.clone(), kw.text.clone());
                    counter += 1;
                }
            }
        }

        // 2. 身份证号
        let mut new_text = String::new();
        let mut last_end = 0;
        for cap in self.id_re.find_iter(&safe_text) {
            let placeholder = format!("[$$_ID_{}]", counter);
            new_text.push_str(&safe_text[last_end..cap.start()]);
            new_text.push_str(&placeholder);
            mapping
                .entry(placeholder)
                .or_insert_with(|| cap.as_str().to_string());
            last_end = cap.end();
            counter += 1;
        }
        new_text.push_str(&safe_text[last_end..]);
        safe_text = new_text;

        // 3. 手机号
        let mut new_text = String::new();
        let mut last_end = 0;
        for cap in self.phone_re.find_iter(&safe_text) {
            // 跳过银行卡号匹配范围内的手机号
            if self.bank_re.is_match(cap.as_str()) {
                continue;
            }
            let placeholder = format!("[$$_PHONE_{}]", counter);
            new_text.push_str(&safe_text[last_end..cap.start()]);
            new_text.push_str(&placeholder);
            mapping
                .entry(placeholder)
                .or_insert_with(|| cap.as_str().to_string());
            last_end = cap.end();
            counter += 1;
        }
        new_text.push_str(&safe_text[last_end..]);
        safe_text = new_text;

        // 4. 邮箱
        let mut new_text = String::new();
        let mut last_end = 0;
        for cap in self.email_re.find_iter(&safe_text) {
            let placeholder = format!("[$$_EMAIL_{}]", counter);
            new_text.push_str(&safe_text[last_end..cap.start()]);
            new_text.push_str(&placeholder);
            mapping
                .entry(placeholder)
                .or_insert_with(|| cap.as_str().to_string());
            last_end = cap.end();
            counter += 1;
        }
        new_text.push_str(&safe_text[last_end..]);
        safe_text = new_text;

        // 5. 金额（最后处理，避免干扰其他匹配）
        let mut new_text = String::new();
        let mut last_end = 0;
        for cap in self.amount_re.find_iter(&safe_text) {
            let placeholder = format!("[$$_AMT_{}]", counter);
            new_text.push_str(&safe_text[last_end..cap.start()]);
            new_text.push_str(&placeholder);
            mapping
                .entry(placeholder)
                .or_insert_with(|| cap.as_str().to_string());
            last_end = cap.end();
            counter += 1;
        }
        new_text.push_str(&safe_text[last_end..]);
        safe_text = new_text;

        DesensitizeResult { safe_text, mapping }
    }

    /// 还原：将 LLM 返回文本中的占位符替换为原始值。
    ///
    /// 两阶段策略：
    /// 1. 精确匹配（按长度降序，避免部分匹配）
    /// 2. 容错扫描：LLM 可能改写占位符（加空格、改大小写、漏字符），
    ///    用正则扫描形如占位符的片段，归一化（去空白、统一大写）后重试匹配。
    ///    仍无法还原的占位符保留原文并记录 warn 日志，便于排查。
    pub fn restore(&self, text: &str, mapping: &HashMap<String, String>) -> String {
        let mut result = text.to_string();
        // 阶段 1：精确匹配（按占位符长度降序，避免部分匹配）
        let mut pairs: Vec<(String, String)> = mapping
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        for (placeholder, original) in &pairs {
            result = result.replace(placeholder, original);
        }

        // 阶段 2：容错扫描——LLM 改写过的占位符归一化重试
        // 构建归一化 mapping：key 去空白并转大写
        let normalized_map: HashMap<String, String> = mapping
            .iter()
            .map(|(k, v)| (normalize_placeholder(k), v.clone()))
            .collect();

        if !normalized_map.is_empty() {
            let mut unreplaced: Vec<String> = Vec::new();
            // 对每个仍存在的形如占位符的片段，尝试归一化匹配
            result = self
                .placeholder_re
                .replace_all(&result, |caps: &regex::Captures| -> String {
                    let raw = &caps[0];
                    let norm = normalize_placeholder(raw);
                    if let Some(original) = normalized_map.get(&norm) {
                        original.clone()
                    } else {
                        // 记录无法还原的占位符（便于排查 LLM 输出异常）
                        if unreplaced.iter().all(|u: &String| u != raw) {
                            unreplaced.push(raw.to_string());
                        }
                        raw.to_string()
                    }
                })
                .to_string();

            if !unreplaced.is_empty() {
                tracing::warn!(
                    "[Desensitize] {} 个占位符无法还原（LLM 可能改写），保留原文: {:?}",
                    unreplaced.len(),
                    unreplaced
                );
            }
        }

        result
    }
}

/// 归一化占位符：去空白、转大写，用于 LLM 改写后的容错匹配。
///
/// 例：`[ $_name_1 ]` → `[$_NAME_1]`
fn normalize_placeholder(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

// ── 持久化 ────────────────────────────────────────────────────────────────

/// 敏感词持久化存储（sensitive_keywords 表）。
///
/// 解决自定义敏感词重启丢失问题：用户配置的敏感词写入 DB，
/// 应用启动时由 `AppState` 加载到 `Desensitizer` 内存。
pub struct SensitiveKeywordStore {
    db: Connection,
}

impl SensitiveKeywordStore {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, String> {
        let db = Connection::open(db_path).map_err(|e| format!("打开敏感词数据库失败: {}", e))?;
        Self::ensure_table(&db)?;
        Ok(Self { db })
    }

    /// 从已有连接创建（共享 metadata.db）
    pub fn with_conn(db: Connection) -> Result<Self, String> {
        Self::ensure_table(&db)?;
        Ok(Self { db })
    }

    fn ensure_table(db: &Connection) -> Result<(), String> {
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS sensitive_keywords (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                text       TEXT NOT NULL UNIQUE,
                kind       TEXT NOT NULL DEFAULT 'custom',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_sensitive_keywords_text ON sensitive_keywords(text);",
        )
        .map_err(|e| format!("创建 sensitive_keywords 表失败: {}", e))?;
        Ok(())
    }

    /// 新增或更新敏感词（text 唯一，已存在则更新 kind）
    pub fn upsert(&self, text: &str, kind: SensitiveKind) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("敏感词不能为空".to_string());
        }
        self.db
            .execute(
                "INSERT INTO sensitive_keywords (text, kind) VALUES (?1, ?2)
                 ON CONFLICT(text) DO UPDATE SET kind = excluded.kind",
                params![text, kind.tag().to_lowercase()],
            )
            .map_err(|e| format!("保存敏感词失败: {}", e))?;
        Ok(())
    }

    /// 查询全部敏感词（带类型）
    pub fn list(&self) -> Result<Vec<SensitiveKeyword>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT text, kind FROM sensitive_keywords ORDER BY id")
            .map_err(|e| format!("查询敏感词失败: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                let text: String = row.get(0)?;
                let kind: String = row.get(1)?;
                Ok(SensitiveKeyword {
                    text,
                    kind: SensitiveKind::from_str_lossy(&kind),
                })
            })
            .map_err(|e| format!("解析敏感词失败: {}", e))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("解析敏感词行失败: {}", e))?);
        }
        Ok(result)
    }

    /// 删除敏感词（按 text）。返回是否删除了记录。
    pub fn delete(&self, text: &str) -> Result<bool, String> {
        let affected = self
            .db
            .execute(
                "DELETE FROM sensitive_keywords WHERE text = ?1",
                params![text.trim()],
            )
            .map_err(|e| format!("删除敏感词失败: {}", e))?;
        Ok(affected > 0)
    }
}

impl Default for Desensitizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desensitize_phone() {
        let d = Desensitizer::new();
        let result = d.desensitize("请联系手机 13800138000 确认。");
        assert!(
            !result.safe_text.contains("13800138000"),
            "safe_text: {}",
            result.safe_text
        );
        assert!(result.mapping.values().any(|v| v == "13800138000"));
    }

    #[test]
    fn test_desensitize_id_card() {
        let d = Desensitizer::new();
        let id = "110101199001011234";
        let result = d.desensitize(&format!("身份证号：{}", id));
        assert!(
            !result.safe_text.contains(id),
            "safe_text: {}",
            result.safe_text
        );
        assert!(result.mapping.values().any(|v| v == id));
    }

    #[test]
    fn test_desensitize_email() {
        let d = Desensitizer::new();
        let result = d.desensitize("邮箱是 ceo@company.com。");
        assert!(
            !result.safe_text.contains("ceo@company.com"),
            "safe_text: {}",
            result.safe_text
        );
        assert!(result.mapping.values().any(|v| v == "ceo@company.com"));
    }

    #[test]
    fn test_desensitize_amount() {
        let d = Desensitizer::new();
        let result = d.desensitize("合同金额 1,234,567.89 元。");
        assert!(
            !result.safe_text.contains("1,234,567.89"),
            "safe_text: {}",
            result.safe_text
        );
    }

    #[test]
    fn test_custom_keyword() {
        let d = Desensitizer::new();
        d.add_keyword("张三丰");
        let result = d.desensitize("财务总监张三丰确认了此事。");
        assert!(
            !result.safe_text.contains("张三丰"),
            "safe_text: {}",
            result.safe_text
        );
        assert!(result.mapping.len() >= 1, "mapping: {:?}", result.mapping);
    }

    #[test]
    fn test_empty_text() {
        let d = Desensitizer::new();
        let result = d.desensitize("");
        assert_eq!(result.safe_text, "");
        assert!(result.mapping.is_empty());
    }

    #[test]
    fn test_no_sensitive_data() {
        let d = Desensitizer::new();
        let result = d.desensitize("今天天气不错，适合做ERP实施。");
        assert_eq!(result.safe_text, "今天天气不错，适合做ERP实施。");
    }

    #[test]
    fn test_roundtrip_preserves_meaning() {
        let d = Desensitizer::new();
        d.add_keyword("王明");
        let original =
            "请联系财务总监王明（手机13800138000，邮箱 ming@corp.com）确认付款金额500,000元。";
        let result = d.desensitize(original);
        assert!(
            !result.safe_text.contains("13800138000"),
            "safe_text: {}",
            result.safe_text
        );
        assert!(
            !result.safe_text.contains("ming@corp.com"),
            "safe_text: {}",
            result.safe_text
        );
        assert!(
            !result.safe_text.contains("王明"),
            "safe_text: {}",
            result.safe_text
        );
    }

    #[test]
    fn test_multiple_keywords() {
        let d = Desensitizer::new();
        d.add_keyword("李总");
        d.add_keyword("张经理");
        let result = d.desensitize("李总同意方案，张经理负责执行。");
        let count_li = result.safe_text.matches("李总").count();
        let count_zhang = result.safe_text.matches("张经理").count();
        eprintln!("safe_text: {:?}", result.safe_text);
        eprintln!("mapping: {:?}", result.mapping);
        eprintln!("李总 count in safe_text: {}", count_li);
        eprintln!("张经理 count in safe_text: {}", count_zhang);
        assert!(
            count_li == 0,
            "李总 should be replaced: safe_text={:?}",
            result.safe_text
        );
        assert!(
            count_zhang == 0,
            "张经理 should be replaced: safe_text={:?}",
            result.safe_text
        );
    }

    #[test]
    fn test_typed_keyword_tags() {
        // 不同类型的自定义敏感词应生成对应的类型标签占位符
        let d = Desensitizer::new();
        d.add_typed_keyword("王明", SensitiveKind::Name);
        d.add_typed_keyword("苍穹V4", SensitiveKind::Term);
        d.add_typed_keyword("HT20260614", SensitiveKind::Code);
        let result = d.desensitize("王明负责苍穹V4项目，合同号HT20260614。");
        assert!(
            result.safe_text.contains("[$_NAME_"),
            "人名应为 NAME 标签: {}",
            result.safe_text
        );
        assert!(
            result.safe_text.contains("[$_TERM_"),
            "术语应为 TERM 标签: {}",
            result.safe_text
        );
        assert!(
            result.safe_text.contains("[$_CODE_"),
            "编号应为 CODE 标签: {}",
            result.safe_text
        );
        assert!(!result.safe_text.contains("王明"));
        assert!(!result.safe_text.contains("苍穹V4"));
        assert!(!result.safe_text.contains("HT20260614"));
    }

    #[test]
    fn test_restore_normalizes_modified_placeholder() {
        // LLM 可能改写占位符（加空格、改大小写），restore 应容错还原
        let d = Desensitizer::new();
        d.add_typed_keyword("王明", SensitiveKind::Name);
        let result = d.desensitize("联系王明。");
        // 模拟 LLM 改写：占位符加空格 + 小写
        let llm_output = result
            .safe_text
            .replace("[$_NAME_0]", "[ $_name_0 ]");
        let restored = d.restore(&llm_output, &result.mapping);
        assert!(
            restored.contains("王明") && !restored.contains("[$_"),
            "改写后的占位符应被还原: restored={}",
            restored
        );
    }

    #[test]
    fn test_sensitive_keyword_store_persistence() {
        // 敏感词持久化：写入后重新打开应能读回
        let tmp = std::env::temp_dir().join(format!(
            "test_desensitize_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // 写入
        {
            let store = SensitiveKeywordStore::new(&tmp).unwrap();
            store.upsert("王明", SensitiveKind::Name).unwrap();
            store.upsert("苍穹V4", SensitiveKind::Term).unwrap();
        }
        // 重新打开读回
        {
            let store = SensitiveKeywordStore::new(&tmp).unwrap();
            let keywords = store.list().unwrap();
            assert_eq!(keywords.len(), 2);
            assert!(keywords.iter().any(|k| k.text == "王明" && k.kind == SensitiveKind::Name));
            assert!(keywords.iter().any(|k| k.text == "苍穹V4" && k.kind == SensitiveKind::Term));
        }
        let _ = std::fs::remove_file(&tmp);
    }
}
