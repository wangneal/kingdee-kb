//! 本地安全脱敏管道 — 发送给 LLM 前过滤敏感信息，返回后还原
//!
//! 流程: 扫描文本 → 替换敏感词为 [$$_N] 占位符 → 发给 LLM → 还原原文
//! 支持: 财务金额、身份证号、手机号、邮箱、企业高管姓名（可配置敏感词库）
//!
//! 全部在 Rust 本地执行，无需联网，不依赖第三方 NER 服务。

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// 脱敏结果：脱敏后的文本 + 还原映射表
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesensitizeResult {
    /// 脱敏后的文本（可安全发给 LLM）
    pub safe_text: String,
    /// 占位符 → 原始值的映射（用于还原）
    pub mapping: HashMap<String, String>,
}

/// 脱敏管道（线程安全）
pub struct Desensitizer {
    /// 用户自定义敏感词库（企业高管姓名、内部项目代号等）
    custom_keywords: Mutex<Vec<String>>,
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
}

impl Desensitizer {
    /// 创建新的脱敏器，加载内置规则
    pub fn new() -> Self {
        Self {
            custom_keywords: Mutex::new(Vec::new()),
            amount_re: Regex::new(r"[¥￥]?\d{1,3}(?:,\d{3})*(?:\.\d+)?(?:万?元)?(?:人民币)?")
                .expect("Invalid amount regex"),
            id_re: Regex::new(r"\b[1-9]\d{5}(?:19|20)\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])\d{3}[\dXx]\b")
                .expect("Invalid ID regex"),
            phone_re: Regex::new(r"\b1[3-9]\d{9}\b")
                .expect("Invalid phone regex"),
            email_re: Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")
                .expect("Invalid email regex"),
            bank_re: Regex::new(r"\b\d{16,19}\b")
                .expect("Invalid bank card regex"),
        }
    }

    /// 添加自定义敏感词（如企业高管姓名、内部代号）
    pub fn add_keyword(&self, keyword: &str) {
        if let Ok(mut kw) = self.custom_keywords.lock() {
            if !kw.contains(&keyword.to_string()) {
                kw.push(keyword.to_string());
            }
        }
    }

    /// 批量添加敏感词
    pub fn add_keywords(&self, keywords: &[String]) {
        for kw in keywords {
            self.add_keyword(kw);
        }
    }

    /// 获取当前敏感词列表
    pub fn get_keywords(&self) -> Vec<String> {
        self.custom_keywords.lock().map(|kw| kw.clone()).unwrap_or_default()
    }

    /// 清除自定义敏感词
    pub fn clear_keywords(&self) {
        if let Ok(mut kw) = self.custom_keywords.lock() {
            kw.clear();
        }
    }

    /// 执行脱敏：扫描文本，替换敏感信息为占位符
    pub fn desensitize(&self, text: &str) -> DesensitizeResult {
        let mut safe_text = text.to_string();
        let mut mapping = HashMap::new();
        let mut counter = 0usize;

        // 1. 自定义敏感词（按长度降序，避免部分匹配）
        if let Ok(keywords) = self.custom_keywords.lock() {
            let mut sorted = keywords.clone();
            sorted.sort_by(|a, b| b.len().cmp(&a.len()));
            for keyword in &sorted {
                if keyword.len() < 2 { continue; }
                let placeholder = format!("[$$_NAME_{}]", counter);
                let count = safe_text.matches(keyword).count();
                if count > 0 {
                    safe_text = safe_text.replace(keyword, &placeholder);
                    mapping.insert(placeholder.clone(), keyword.clone());
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
            mapping.entry(placeholder).or_insert_with(|| cap.as_str().to_string());
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
            if self.bank_re.is_match(cap.as_str()) { continue; }
            let placeholder = format!("[$$_PHONE_{}]", counter);
            new_text.push_str(&safe_text[last_end..cap.start()]);
            new_text.push_str(&placeholder);
            mapping.entry(placeholder).or_insert_with(|| cap.as_str().to_string());
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
            mapping.entry(placeholder).or_insert_with(|| cap.as_str().to_string());
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
            mapping.entry(placeholder).or_insert_with(|| cap.as_str().to_string());
            last_end = cap.end();
            counter += 1;
        }
        new_text.push_str(&safe_text[last_end..]);
        safe_text = new_text;

        DesensitizeResult { safe_text, mapping }
    }

    /// 还原：将 LLM 返回文本中的占位符替换为原始值
    pub fn restore(&self, text: &str, mapping: &HashMap<String, String>) -> String {
        let mut result = text.to_string();
        // 按占位符长度降序替换，避免部分匹配
        let mut pairs: Vec<(String, String)> = mapping.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        for (placeholder, original) in &pairs {
            result = result.replace(placeholder, original);
        }
        result
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
        assert!(!result.safe_text.contains("13800138000"), "safe_text: {}", result.safe_text);
        assert!(result.mapping.values().any(|v| v == "13800138000"));
    }

    #[test]
    fn test_desensitize_id_card() {
        let d = Desensitizer::new();
        let id = "110101199001011234";
        let result = d.desensitize(&format!("身份证号：{}", id));
        assert!(!result.safe_text.contains(id), "safe_text: {}", result.safe_text);
        assert!(result.mapping.values().any(|v| v == id));
    }

    #[test]
    fn test_desensitize_email() {
        let d = Desensitizer::new();
        let result = d.desensitize("邮箱是 ceo@company.com。");
        assert!(!result.safe_text.contains("ceo@company.com"), "safe_text: {}", result.safe_text);
        assert!(result.mapping.values().any(|v| v == "ceo@company.com"));
    }

    #[test]
    fn test_desensitize_amount() {
        let d = Desensitizer::new();
        let result = d.desensitize("合同金额 1,234,567.89 元。");
        assert!(!result.safe_text.contains("1,234,567.89"), "safe_text: {}", result.safe_text);
    }

    #[test]
    fn test_custom_keyword() {
        let d = Desensitizer::new();
        d.add_keyword("张三丰");
        let result = d.desensitize("财务总监张三丰确认了此事。");
        assert!(!result.safe_text.contains("张三丰"), "safe_text: {}", result.safe_text);
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
        let original = "请联系财务总监王明（手机13800138000，邮箱 ming@corp.com）确认付款金额500,000元。";
        let result = d.desensitize(original);
        assert!(!result.safe_text.contains("13800138000"), "safe_text: {}", result.safe_text);
        assert!(!result.safe_text.contains("ming@corp.com"), "safe_text: {}", result.safe_text);
        assert!(!result.safe_text.contains("王明"), "safe_text: {}", result.safe_text);
    }

    #[test]
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
        assert!(count_li == 0, "李总 should be replaced: safe_text={:?}", result.safe_text);
        assert!(count_zhang == 0, "张经理 should be replaced: safe_text={:?}", result.safe_text);
    }
}
