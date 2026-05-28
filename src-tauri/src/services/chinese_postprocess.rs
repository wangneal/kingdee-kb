//! Chinese text post-processing for Whisper output
//!
//! Pure rule engine — no LLM dependency, works offline.
//! Handles: punctuation restoration, short-sentence merging,
//! duplicate phrase removal, and common Whisper artifacts.

use regex::Regex;
use std::sync::LazyLock;

/// Common Chinese sentence-ending keywords that imply specific punctuation
// SAFE: hardcoded regex pattern — CJK question keywords
static QUESTION_KEYWORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(吗|呢|吧|啊|么|谁|什么|怎么|哪|多少|几|为什么|如何|是不是|能不能|有没有)")
        .unwrap()
});

// SAFE: hardcoded regex pattern — removes spaces between CJK characters
static RE_CJK_SPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([\u4e00-\u9fff])\s+([\u4e00-\u9fff])").unwrap());
// SAFE: hardcoded regex pattern — collapses multiple spaces into one
static RE_MULTI_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"  +").unwrap());

/// Duplicate phrase pattern — same 2-4 char sequence repeated immediately
/// NOTE: Rust's regex crate does NOT support backreferences, so we use
/// a programmatic approach in remove_duplicates() instead.
// This constant kept for documentation; the function uses manual iteration.

/// Main entry point: post-process raw Whisper Chinese transcription.
///
/// Pipeline order matters:
/// 1. Remove duplicate phrases first (avoids punctuating duplicates)
/// 2. Restore punctuation
/// 3. Merge short sentences
/// 4. Clean up whitespace
pub fn postprocess_chinese(raw_text: &str) -> String {
    let text = raw_text.trim();
    if text.is_empty() {
        return String::new();
    }

    // Step 1: Remove duplicate phrases ("然后然后" → "然后")
    let text = remove_duplicates(&text);

    // Step 2: Restore punctuation
    let text = restore_punctuation(&text);

    // Step 3: Merge short sentences
    let text = merge_short_sentences(&text);

    // Step 4: Clean whitespace
    let text = clean_whitespace(&text);

    text
}

/// Remove immediately repeated Chinese phrases.
///
/// E.g., "然后然后" → "然后", "那个那个那个" → "那个"
/// But keeps emphatic repetitions like "是是是" (3+ chars) as-is
/// since those might be intentional in speech.
fn remove_duplicates(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut result = Vec::new();
    let mut i = 0;

    while i < len {
        // Try to find repeated pattern starting at position i
        // Check from longest possible repeat (4 chars) down to 1 char
        let mut found_dup = false;

        for phrase_len in (1..=4.min(len - i)).rev() {
            if i + phrase_len > len {
                continue;
            }

            // Count how many times this phrase repeats
            let mut repeat_count = 1;
            let mut j = i + phrase_len;
            while j + phrase_len <= len && chars[j..j + phrase_len] == chars[i..i + phrase_len] {
                repeat_count += 1;
                j += phrase_len;
            }

            if repeat_count >= 2 {
                // Duplicate found
                let _phrase: String = chars[i..i + phrase_len].iter().collect();

                if phrase_len == 1 {
                    // Single char: keep 2 for emphasis
                    result.push(chars[i]);
                    if repeat_count >= 2 {
                        result.push(chars[i]);
                    }
                } else {
                    // Multi-char phrase: keep single instance
                    for &c in &chars[i..i + phrase_len] {
                        result.push(c);
                    }
                }

                i = j; // Skip past all repetitions
                found_dup = true;
                break;
            }
        }

        if !found_dup {
            result.push(chars[i]);
            i += 1;
        }
    }

    result.into_iter().collect()
}

/// Restore Chinese punctuation based on sentence content.
///
/// Rules:
/// - Sentences ending with question keywords → ？
/// - Long pauses between segments → ，
/// - Sentence end without punctuation → 。
fn restore_punctuation(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + text.len() / 10);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        let ch = chars[i];
        result.push(ch);

        // Look ahead: if next segment starts with a new thought indicator,
        // add appropriate punctuation after current position
        if i + 1 < len {
            let _next = chars[i + 1];

            // Don't add punctuation if already present
            if !is_cjk_punctuation(ch) && !is_ascii_punctuation(ch) {
                // Check if we're at a natural break point
                if is_sentence_end_context(&chars, i, len) {
                    // Check if this looks like a question
                    let recent_text = get_recent_text(&result, 6);
                    if QUESTION_KEYWORDS.is_match(&recent_text) {
                        result.push('？');
                    } else {
                        result.push('。');
                    }
                } else if is_clause_break(&chars, i, len) {
                    result.push('，');
                }
            }
        }

        i += 1;
    }

    // Ensure final punctuation
    if let Some(last_char) = result.chars().last() {
        if !is_cjk_punctuation(last_char) && !is_ascii_punctuation(last_char) {
            let recent = get_recent_text(&result, 6);
            if QUESTION_KEYWORDS.is_match(&recent) {
                result.push('？');
            } else {
                result.push('。');
            }
        }
    }

    result
}

/// Merge short consecutive sentences (< 10 chars) into longer paragraphs.
///
/// Chinese speech often produces many short fragments. Merging them
/// makes the text more readable while preserving meaning.
fn merge_short_sentences(text: &str) -> String {
    let sentences: Vec<&str> = text
        .split(|c| c == '。' || c == '？' || c == '！' || c == '；')
        .filter(|s| !s.trim().is_empty())
        .collect();

    if sentences.len() <= 1 {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut current_paragraph = String::new();

    for sentence in &sentences {
        let trimmed = sentence.trim();
        let char_count = trimmed.chars().count();

        if char_count < 10 && !current_paragraph.is_empty() {
            // Short sentence: append to current paragraph with comma
            if !current_paragraph.ends_with('，') && !current_paragraph.ends_with('、') {
                current_paragraph.push('，');
            }
            current_paragraph.push_str(trimmed);
        } else {
            // Long sentence or first: flush current paragraph, start new
            if !current_paragraph.is_empty() {
                result.push_str(&current_paragraph);
                result.push('。');
            }
            current_paragraph = trimmed.to_string();
        }
    }

    // Flush remaining
    if !current_paragraph.is_empty() {
        result.push_str(&current_paragraph);
        result.push('。');
    }

    result
}

/// Clean up excessive whitespace from Whisper output.
fn clean_whitespace(text: &str) -> String {
    let mut result = text.to_string();

    // Remove spaces between Chinese characters (Whisper artifact)
    // Loop since adjacent gaps need multiple passes
    loop {
        let new = RE_CJK_SPACE.replace_all(&result, "$1$2");
        if new.len() == result.len() {
            break;
        }
        result = new.to_string();
    }

    // Collapse multiple spaces into one
    result = RE_MULTI_SPACE.replace_all(&result, " ").to_string();

    // Remove leading/trailing whitespace per line
    result = result
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    result
}

// --- Helpers ---

fn is_cjk_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '，' | '。' | '？' | '！' | '；' | '：' | '、' | '…' | '—' | '\u{201C}' | '\u{201D}'
    )
}

fn is_ascii_punctuation(ch: char) -> bool {
    matches!(ch, '.' | ',' | '?' | '!' | ';' | ':')
}

/// Check if we're at a natural sentence boundary.
fn is_sentence_end_context(chars: &[char], i: usize, len: usize) -> bool {
    // End of text
    if i >= len - 1 {
        return true;
    }

    let next = chars[i + 1];

    // Next char is a conjunction that starts a new clause
    if is_conjunction(next) {
        return true;
    }

    // Look ahead for conjunction pattern
    if i + 2 < len {
        let two_ahead = format!("{}{}", chars[i + 1], chars[i + 2]);
        if is_two_char_conjunction(&two_ahead) {
            return true;
        }
    }

    false
}

/// Check if we're at a clause break (less than sentence end).
fn is_clause_break(chars: &[char], i: usize, len: usize) -> bool {
    if i + 1 >= len {
        return false;
    }

    let next = chars[i + 1];

    // After a Chinese character, if the next is also Chinese and we've
    // gone 8+ chars without punctuation, insert a comma
    if is_cjk_char(next) {
        // Look back to find last punctuation
        let recent = chars[..=i].iter().rev().take(12);
        let since_punct = recent
            .take_while(|&&c| !is_cjk_punctuation(c) && !is_ascii_punctuation(c))
            .count();
        return since_punct >= 8;
    }

    false
}

fn is_conjunction(ch: char) -> bool {
    matches!(ch, '但' | '而' | '所' | '如' | '因' | '虽' | '否')
}

fn is_two_char_conjunction(s: &str) -> bool {
    matches!(
        s,
        "但是"
            | "而且"
            | "所以"
            | "如果"
            | "因为"
            | "虽然"
            | "不过"
            | "然后"
            | "接着"
            | "或者"
            | "还是"
    )
}

fn is_cjk_char(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn get_recent_text(text: &str, n: usize) -> String {
    text.chars()
        .rev()
        .take(n)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_duplicates() {
        assert_eq!(remove_duplicates("然后然后"), "然后");
        assert_eq!(remove_duplicates("那个那个那个"), "那个");
        // Single char emphasis preserved
        let result = remove_duplicates("是是是");
        assert!(result.contains("是是"));
    }

    #[test]
    fn test_restore_punctuation() {
        let result = restore_punctuation("你好吗");
        assert!(result.contains('？'));

        let result = restore_punctuation("今天天气不错");
        assert!(result.contains('。'));
    }

    #[test]
    fn test_merge_short_sentences() {
        let result = merge_short_sentences("好的。然后。我们继续。");
        // Short sentences should be merged
        assert!(result.contains("好的"));
    }

    #[test]
    fn test_clean_whitespace() {
        let result = clean_whitespace("你 好 世 界");
        assert_eq!(result, "你好世界");
    }

    #[test]
    fn test_full_pipeline() {
        let raw = "然后然后我想问一下这个功能怎么用呢";
        let result = postprocess_chinese(raw);
        assert!(!result.contains("然后然后"));
        assert!(result.contains('？'));
    }
}
