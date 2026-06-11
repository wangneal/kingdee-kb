//! Wiki 链接解析共享工具
//!
//! 从 markdown 文本中提取 Obsidian 风格的 `[[slug]]` 或 `[[slug|显示文本]]` 形式链接。
//!
//! 设计要点：
//! - 用 char-by-char 字节扫描（不依赖 regex 库），避免重复实现
//! - 排除空 slug
//! - 排除自引用（slug == current_slug）
//! - 防御 LLM 幻觉：仅返回 `valid_slugs` 中已存在的 slug（`valid_slugs` 为空时不验证）
//! - 去重 + 排序（保证写入顺序稳定）
//!
//! 调用方：
//! - `wiki_page.rs::approve_candidate`（批准候选时重新计算 wikilinks）
//! - `ingestion_pipeline.rs::parse_wikilinks_from_markdown`（LLM 输出后处理）
//! - `knowledge_graph.rs::backfill_empty_wikilinks`（历史 wikilinks 回填）

/// 从 markdown 文本中提取 `[[slug]]` 形式的 wiki 链接
///
/// # 参数
/// - `markdown`: 待扫描的 markdown 文本
/// - `current_slug`: 当前页面的 slug（用于排除自引用）
/// - `valid_slugs`: 项目已有页面的 slug 集合（用于防御 LLM 幻觉）；
///                  传空切片时跳过验证（向后兼容：未传项目 slugs 时也能提取）
///
/// # 返回
/// 去重 + 排序后的 slug 列表
pub fn extract_wikilinks(
    markdown: &str,
    current_slug: &str,
    valid_slugs: &std::collections::HashSet<String>,
) -> Vec<String> {
    let bytes = markdown.as_bytes();
    let mut found: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(end) = find_double_close(bytes, i + 2) {
                let inner = &markdown[i + 2..end];
                // 提取 slug 部分（| 之前的）
                let slug = inner.split('|').next().unwrap_or("").trim();
                if !slug.is_empty()
                    && slug != current_slug
                    && (valid_slugs.is_empty() || valid_slugs.contains(slug))
                {
                    found.push(slug.to_string());
                }
                i = end + 2;
                continue;
            }
        }
        i += 1;
    }
    found.sort();
    found.dedup();
    found
}

/// 查找下一个 `]]` 位置（从 start 开始，匹配 `]]` 双字符）
fn find_double_close(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn simple_form() {
        let valid: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let result = extract_wikilinks("参考 [[a]] 和 [[b]]", "current", &valid);
        assert_eq!(result, vec!["a", "b"]);
    }

    #[test]
    fn alias_form() {
        let valid: HashSet<String> = ["a"].iter().map(|s| s.to_string()).collect();
        let result = extract_wikilinks("参考 [[a|显示文本]]", "current", &valid);
        assert_eq!(result, vec!["a"]);
    }

    #[test]
    fn filters_self_reference() {
        let valid: HashSet<String> = ["a", "self"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let result = extract_wikilinks("[[self]] [[a]]", "self", &valid);
        assert_eq!(result, vec!["a"]);
    }

    #[test]
    fn filters_invalid_slugs() {
        let valid: HashSet<String> = ["real"].iter().map(|s| s.to_string()).collect();
        let result = extract_wikilinks("[[real]] [[hallucinated]]", "current", &valid);
        assert_eq!(result, vec!["real"]);
    }

    #[test]
    fn empty_valid_skips_validation() {
        let valid: HashSet<String> = HashSet::new();
        let result = extract_wikilinks("[[any-page]]", "current", &valid);
        assert_eq!(result, vec!["any-page"]);
    }

    #[test]
    fn dedup_and_sort() {
        let valid: HashSet<String> = ["a", "b", "c"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let result = extract_wikilinks("[[c]] [[a]] [[b]] [[a]]", "x", &valid);
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn handles_chinese() {
        let valid: HashSet<String> = ["金蝶云星空"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let result = extract_wikilinks("参考 [[金蝶云星空]]", "current", &valid);
        assert_eq!(result, vec!["金蝶云星空"]);
    }

    #[test]
    fn ignores_empty_brackets() {
        let valid: HashSet<String> = ["a"].iter().map(|s| s.to_string()).collect();
        let result = extract_wikilinks("[[]] [[ ]] [[ |xxx]]", "current", &valid);
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn ignores_single_brackets() {
        let valid: HashSet<String> = ["a"].iter().map(|s| s.to_string()).collect();
        let result = extract_wikilinks("[a] 不是 wikilink", "current", &valid);
        assert_eq!(result, Vec::<String>::new());
    }
}
