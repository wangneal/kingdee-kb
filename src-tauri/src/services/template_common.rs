//! 模板处理共享工具函数
//!
//! 集中管理 template_docx、template_xlsx、template_pptx 共用的逻辑，
//! 消除代码重复，确保行为一致。


/// 推断字段类型（date / number / text）
///
/// 合并了 docx、xlsx、pptx 三份实现的关键词列表，确保同一字段名在不同模板类型中
/// 推断结果一致。
pub fn infer_field_type(name: &str) -> String {
    let name_lower = name.to_lowercase();

    // ── 日期模式 ──
    if name_lower.contains("日期")
        || name_lower.contains("时间")
        || name_lower.contains("年月日")
        || name_lower.contains("date")
        || name_lower.contains("time")
    {
        return "date".to_string();
    }

    // ── 数字模式 ──
    if name_lower.contains("数量")
        || name_lower.contains("金额")
        || name_lower.contains("价格")
        || name_lower.contains("预算")
        || name_lower.contains("成本")
        || name_lower.contains("费用")
        || name_lower.contains("比例")
        || name_lower.contains("百分")
        || name_lower.contains("人数")
        || name_lower.contains("天数")
        || name_lower.contains("次数")
        || name_lower.contains("率")
        || name_lower.contains("number")
        || name_lower.contains("amount")
        || name_lower.contains("price")
        || name_lower.contains("count")
        || name_lower.contains("ratio")
    {
        return "number".to_string();
    }

    // ── 默认文本 ──
    "text".to_string()
}

/// 从 LLM 响应中提取 JSON 对象或数组
///
/// 处理常见情况：LLM 可能在 JSON 前后添加说明文字、markdown 代码块标记等。
/// 如果未找到有效 JSON，返回原始响应的 trim 版本（容错模式）。
pub fn extract_json_from_response(response: &str) -> String {
    let trimmed = response.trim();

    // 尝试直接解析整个响应
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return trimmed.to_string();
    }

    // 尝试提取 ```json ... ``` 代码块
    if let Some(start) = trimmed.find("```json") {
        let json_start = start + 7;
        if let Some(end) = trimmed[json_start..].find("```") {
            let json_str = trimmed[json_start..json_start + end].trim();
            if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
                return json_str.to_string();
            }
        }
    }

    // 尝试提取 ``` ... ``` 代码块（无语言标记）
    if let Some(start) = trimmed.find("```") {
        let json_start = start + 3;
        let content_start = if let Some(newline) = trimmed[json_start..].find('\n') {
            json_start + newline + 1
        } else {
            json_start
        };
        if let Some(end) = trimmed[content_start..].find("```") {
            let json_str = trimmed[content_start..content_start + end].trim();
            if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
                return json_str.to_string();
            }
        }
    }

    // 尝试提取第一个 { ... } 或 [ ... ]
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                let json_str = &trimmed[start..=end];
                if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
                    return json_str.to_string();
                }
            }
        }
    }
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if end > start {
                let json_str = &trimmed[start..=end];
                if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
                    return json_str.to_string();
                }
            }
        }
    }

    // 未找到有效 JSON，返回原始响应（容错）
    trimmed.to_string()
}

/// 从知识库搜索结果组装上下文字符串
///
/// 将搜索结果格式化为 LLM 可理解的上下文块，包含来源标题和内容。
/// 可选 max_tokens 限制上下文长度。
pub fn assemble_kb_context(
    results: &[super::hybrid_search::HybridSearchResult],
    max_tokens: Option<u32>,
) -> String {
    if results.is_empty() {
        return String::new();
    }

    let mut context = String::new();
    context.push_str("以下是从知识库中检索到的相关内容：\n\n");

    for (i, result) in results.iter().enumerate() {
        let section = result
            .section_path
            .as_deref()
            .unwrap_or("（无章节信息）");
        context.push_str(&format!(
            "【来源 {}】{} | {}\n{}\n\n",
            i + 1,
            result.title,
            section,
            result.content
        ));
    }

    // 按 token 预算截断
    if let Some(max) = max_tokens {
        super::llm_service::truncate_to_tokens(&context, max)
    } else {
        context
    }
}

/// 字段占位符正则表达式（共享，避免重复编译）
pub fn field_placeholder_regex() -> regex::Regex {
    regex::Regex::new(r"\{([^}]+)\}").expect("field placeholder regex should always compile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_field_type_date() {
        assert_eq!(infer_field_type("项目日期"), "date");
        assert_eq!(infer_field_type("开始时间"), "date");
        assert_eq!(infer_field_type("年月日"), "date");
        assert_eq!(infer_field_type("start_date"), "date");
    }

    #[test]
    fn test_infer_field_type_number() {
        assert_eq!(infer_field_type("项目预算"), "number");
        assert_eq!(infer_field_type("人员数量"), "number");
        assert_eq!(infer_field_type("完成比例"), "number");
        assert_eq!(infer_field_type("total_amount"), "number");
    }

    #[test]
    fn test_infer_field_type_text() {
        assert_eq!(infer_field_type("项目名称"), "text");
        assert_eq!(infer_field_type("调研背景"), "text");
    }

    #[test]
    fn test_extract_json_from_response_pure_json() {
        let response = r#"{"key": "value"}"#;
        assert_eq!(extract_json_from_response(response), response);
    }

    #[test]
    fn test_extract_json_from_response_markdown_block() {
        let response = "这是分析结果：\n```json\n{\"key\": \"value\"}\n```\n以上是结果。";
        assert_eq!(
            extract_json_from_response(response),
            r#"{"key": "value"}"#
        );
    }

    #[test]
    fn test_extract_json_from_response_with_text() {
        let response = "根据分析，结果如下：{\"key\": \"value\"}，请参考。";
        assert_eq!(
            extract_json_from_response(response),
            r#"{"key": "value"}"#
        );
    }

    #[test]
    fn test_extract_json_from_response_array() {
        let response = "结果：[1, 2, 3] 完成。";
        assert_eq!(
            extract_json_from_response(response),
            "[1, 2, 3]"
        );
    }

    #[test]
    fn test_extract_json_from_response_no_json() {
        // 现在返回原始响应（容错模式）
        assert_eq!(extract_json_from_response("这里没有 JSON"), "这里没有 JSON");
    }
}
