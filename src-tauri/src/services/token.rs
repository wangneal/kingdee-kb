//! 统一 Token 计数模块
//! 全项目所有 token 计算统一通过此模块，替代现有三套标准。

/// Token 计数错误
#[derive(Debug)]
pub enum TokenError {
    TiktokenInitFailed(String),
}

/// 精确 token 计数（基于 tiktoken cl100k_base）
/// 失败返回 Result，不静默降级
pub fn count_tokens(text: &str) -> Result<u32, TokenError> {
    tiktoken_rs::cl100k_base()
        .map(|b| b.encode_with_special_tokens(text).len() as u32)
        .map_err(|e| TokenError::TiktokenInitFailed(e.to_string()))
}

/// 带回退的 token 计数（用于非关键路径）
/// 回退公式区分中英文比例：中文 ~1.5 字符/token，英文 ~4 字符/token
pub fn count_tokens_with_fallback(text: &str) -> u32 {
    count_tokens(text).unwrap_or_else(|_| {
        let chinese_chars = text.chars().filter(|c| !c.is_ascii()).count();
        let ascii_chars = text.len() - chinese_chars;
        (chinese_chars as f32 / 1.5 + ascii_chars as f32 / 4.0) as u32
    })
}

/// Token 级截断（二分查找，UTF-8 边界安全）
pub fn truncate_to_tokens(text: &str, max_tokens: u32) -> String {
    let total = count_tokens_with_fallback(text);
    if total <= max_tokens {
        return text.to_string();
    }

    let mut low = 0;
    let mut high = text.len();

    while low < high {
        let mid = (low + high + 1) / 2;
        let mut end = mid;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        let candidate = &text[..end];
        let tokens = count_tokens_with_fallback(candidate);
        if tokens <= max_tokens {
            low = end;
        } else {
            high = end - 1;
        }
    }

    let mut end = low;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens_empty() {
        let result = count_tokens_with_fallback("");
        assert_eq!(result, 0);
    }

    #[test]
    fn test_count_tokens_chinese() {
        let result = count_tokens_with_fallback("你好世界");
        assert!(result > 0, "Chinese text should return positive token count");
    }

    #[test]
    fn test_count_tokens_english() {
        let result = count_tokens_with_fallback("hello world");
        assert!(result > 0, "English text should return positive token count");
    }

    #[test]
    fn test_truncate_to_tokens_no_truncation() {
        let text = "short text";
        let result = truncate_to_tokens(text, 100);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_to_tokens_truncates() {
        let text = "a".repeat(10000);
        let result = truncate_to_tokens(&text, 10);
        assert!(result.len() < text.len());
        let tokens = count_tokens_with_fallback(&result);
        assert!(tokens <= 10, "truncated text should be within token budget");
    }
}
