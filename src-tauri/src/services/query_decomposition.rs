//! 查询分解 — 将复杂问题拆解为多个原子子问题
//!
//! 用 LLM 对用户查询进行分解，然后将子查询并行检索后合并。

use crate::services::llm_service::{ChatMessage, LLMService};

/// 尝试将查询分解为子问题。返回子问题列表。
/// 如果查询不需要分解（简单问题），返回包含原查询的 vec。
pub async fn decompose_query(
    llm: &LLMService,
    query: &str,
) -> Result<Vec<String>, String> {
    let word_count = query.chars().filter(|c| c.is_whitespace()).count() + 1;
    if word_count <= 5 {
        return Ok(vec![query.to_string()]);
    }

    let config = llm.get_active_config()?;
    let prompt = format!(
        "你是一个查询分解助手。请将以下复杂问题拆解为 2-3 个独立的原子子问题，\
         每个子问题应该只涉及一个主题。\n\n\
         如果问题不需要分解（只有一个主题），请只返回原问题。\n\n\
         输入：{}\n\n\
         输出格式：每行一个子问题，不要编号。",
        query
    );

    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "你是查询分解助手。只返回子问题列表，每行一个。不解释，不问候。".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: prompt,
        },
    ];

    let response = llm.chat_completion(&messages, &config).await?;

    let sub_queries: Vec<String> = response
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && l.len() > 3)
        .collect();

    if sub_queries.is_empty() {
        Ok(vec![query.to_string()])
    } else {
        Ok(sub_queries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_query_not_decomposed() {
        let result = decompose_short_query();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "金蝶是什么");
    }

    fn decompose_short_query() -> Vec<String> {
        let query = "金蝶是什么";
        let word_count = query.chars().filter(|c| c.is_whitespace()).count() + 1;
        if word_count <= 5 {
            vec![query.to_string()]
        } else {
            vec![]
        }
    }

    #[test]
    fn test_word_count_logic() {
        let query = "金蝶是什么";
        let word_count = query.chars().filter(|c| c.is_whitespace()).count() + 1;
        assert_eq!(word_count, 1);

        let query = "金蝶 云 星辰 是什么 怎么 用";
        let word_count = query.chars().filter(|c| c.is_whitespace()).count() + 1;
        assert_eq!(word_count, 6);
    }

    #[test]
    fn test_empty_response_falls_back() {
        let lines = "";
        let sub_queries: Vec<String> = lines
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && l.len() > 3)
            .collect();
        let result = if sub_queries.is_empty() {
            vec!["原问题".to_string()]
        } else {
            sub_queries
        };
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "原问题");
    }
}
