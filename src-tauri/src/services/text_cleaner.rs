//! Text cleaner: remove Markdown noise, normalize whitespace, preserve code blocks
//!
//! Cleans raw text before chunking — strips link/image syntax, HTML tags,
//! and normalizes whitespace while preserving fenced code blocks.

use regex::Regex;
use std::sync::LazyLock;

/// Fenced code block placeholder prefix
const CODE_BLOCK_PLACEHOLDER: &str = "\x00CODEBLOCK_";

// Pre-compiled regexes (compiled once, reused across calls)
// SAFE: hardcoded regex patterns verified at code review — guaranteed valid
static RE_CODE_FENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^```[^\n]*\n[\s\S]*?^```").unwrap()
});
// SAFE: hardcoded regex pattern for Markdown image syntax
static RE_IMAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"!\[([^\]]*)\]\([^)]*\)").unwrap()
});
// SAFE: hardcoded regex pattern for Markdown link syntax
static RE_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap()
});
// SAFE: hardcoded regex pattern for HTML tags
static RE_HTML_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<[^>]+>").unwrap()
});
// SAFE: hardcoded regex pattern for multiple newlines
static RE_MULTI_NEWLINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\n{3,}").unwrap()
});
// SAFE: hardcoded regex pattern for non-newline whitespace
static RE_MULTI_SPACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[^\S\n]+").unwrap()
});
// SAFE: hardcoded regex pattern for trailing whitespace
static RE_TRAILING_SPACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)\s+$").unwrap()
});

/// Clean raw text for ingestion.
///
/// - Preserves fenced code blocks as opaque tokens
/// - Strips Markdown image/link syntax (keeps display text)
/// - Removes HTML tags
/// - Keeps heading markers (for chunker to split on)
/// - Normalizes whitespace (collapse spaces, trim lines, reduce blank lines)
pub fn clean_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }

    let mut code_blocks: Vec<String> = Vec::new();

    // Step 1: Extract and replace fenced code blocks with placeholders
    let without_code = RE_CODE_FENCE.replace_all(raw, |caps: &regex::Captures| {
        let idx = code_blocks.len();
        code_blocks.push(caps[0].to_string());
        format!("{}{}", CODE_BLOCK_PLACEHOLDER, idx)
    });

    // Step 2: Strip images (remove entirely)
    let no_images = RE_IMAGE.replace_all(&without_code, "");

    // Step 3: Strip links → keep display text
    let no_links = RE_LINK.replace_all(&no_images, "$1");

    // Step 4: Remove HTML tags
    let no_html = RE_HTML_TAG.replace_all(&no_links, "");

    // Step 5: Keep heading markers (# ) — they're structural signals for chunker
    // (Don't strip RE_HEADING_MARKER — chunker needs them)

    // Step 6: Normalize whitespace
    let spaces_collapsed = RE_MULTI_SPACE.replace_all(&no_html, " ");
    let trailing_trimmed = RE_TRAILING_SPACE.replace_all(&spaces_collapsed, "");
    let newlines_normalized = RE_MULTI_NEWLINE.replace_all(&trailing_trimmed, "\n\n");

    let mut result = newlines_normalized.to_string();

    // Step 7: Restore code blocks
    for (idx, block) in code_blocks.iter().enumerate() {
        let placeholder = format!("{}{}", CODE_BLOCK_PLACEHOLDER, idx);
        result = result.replace(&placeholder, block);
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_empty() {
        assert_eq!(clean_text(""), "");
        assert_eq!(clean_text("   "), "");
    }

    #[test]
    fn test_clean_heading_preserved() {
        let input = "# Title\n\n## Section\n\nSome content";
        let result = clean_text(input);
        assert!(result.contains("# Title"));
        assert!(result.contains("## Section"));
        assert!(result.contains("Some content"));
    }

    #[test]
    fn test_clean_links() {
        let input = "See [this link](https://example.com) for details.";
        let result = clean_text(input);
        assert_eq!(result, "See this link for details.");
    }

    #[test]
    fn test_clean_images() {
        let input = "Before\n![alt text](image.png)\nAfter";
        let result = clean_text(input);
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
        assert!(!result.contains("!["));
    }

    #[test]
    fn test_clean_html_tags() {
        let input = "<p>Hello</p> <b>world</b>";
        let result = clean_text(input);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_clean_code_blocks_preserved() {
        let input = "Text before\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\nText after";
        let result = clean_text(input);
        assert!(result.contains("```rust"));
        assert!(result.contains("println!(\"hi\");"));
        assert!(result.contains("Text before"));
        assert!(result.contains("Text after"));
    }

    #[test]
    fn test_clean_whitespace_normalization() {
        let input = "  multiple   spaces  \n\n\n\n\nextra newlines";
        let result = clean_text(input);
        assert!(!result.contains("   "));
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn test_clean_chinese_text() {
        let input = "# 金蝶ERP\n\n## 期货点价\n\n客户要做期货点价，需要在系统中配置[相关参数](http://example.com)。\n\n> 注意：此功能需要管理员权限。";
        let result = clean_text(input);
        assert!(result.contains("金蝶ERP"));
        assert!(result.contains("期货点价"));
        assert!(result.contains("相关参数"));
        assert!(!result.contains("[相关参数]"));
    }

    #[test]
    fn test_clean_mixed_content() {
        let input = r#"# 文档标题

## 第一章

这是一段文字，包含[链接](http://x.com)和![图片](img.png)。

```python
print("hello")
```

## 第二章

另一段文字。

<div>HTML内容</div>"#;
        let result = clean_text(input);
        assert!(result.contains("# 文档标题"));
        assert!(result.contains("## 第一章"));
        assert!(result.contains("链接"));
        assert!(!result.contains("!["));
        assert!(result.contains("```python"));
        assert!(result.contains("另一段文字"));
        assert!(!result.contains("<div>"));
    }
}
