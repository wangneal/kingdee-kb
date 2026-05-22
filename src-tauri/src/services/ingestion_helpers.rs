//! Ingestion helpers: SHA256 dedup, tag extraction, title extraction
//!
//! Utility functions used by the ingestion pipeline for content hashing,
//! automatic tag inference from filenames and section paths, and title extraction.

use sha2::{Digest, Sha256};

/// Compute SHA256 hash of content for dedup detection.
///
/// Returns a lowercase hex string (64 chars).
pub fn compute_sha256(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract tags from filename and section path.
///
/// - Tokenizes filename by `-`, `_`, spaces, and Chinese characters
/// - Adds section path segments as tags
/// - Deduplicates and sorts
pub fn extract_tags(filename: &str, section_path: Option<&str>) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();

    // Extract tags from filename
    let name = filename
        .rsplit_once('.')
        .map(|(n, _)| n) // Strip extension
        .unwrap_or(filename);

    for token in name.split(|c: char| c == '-' || c == '_' || c == ' ') {
        let t = token.trim();
        if !t.is_empty() && t.len() >= 2 {
            tags.push(t.to_string());
        }
    }

    // Extract tags from section path
    if let Some(path) = section_path {
        for segment in path.split(" > ") {
            let s = segment.trim();
            if !s.is_empty() && s.len() >= 2 {
                tags.push(s.to_string());
            }
        }
    }

    // Deduplicate
    tags.sort();
    tags.dedup();
    tags
}

/// Extract a human-readable title from a filename.
///
/// - Strips file extension
/// - Replaces `-`, `_` with spaces
/// - Trims and title-cases
pub fn extract_title_from_filename(filename: &str) -> String {
    let name = filename
        .rsplit_once('.')
        .map(|(n, _)| n)
        .unwrap_or(filename);

    name.replace(['-', '_'], " ")
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_consistency() {
        let hash1 = compute_sha256("hello world");
        let hash2 = compute_sha256("hello world");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn test_sha256_different_content() {
        let hash1 = compute_sha256("hello");
        let hash2 = compute_sha256("world");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_sha256_chinese() {
        let hash = compute_sha256("金蝶ERP系统");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_extract_tags_from_filename() {
        let tags = extract_tags("期货点价-操作指南.md", None);
        assert!(tags.contains(&"期货点价".to_string()));
        assert!(tags.contains(&"操作指南".to_string()));
    }

    #[test]
    fn test_extract_tags_with_section() {
        let tags = extract_tags("guide.md", Some("第一章 > 基础配置"));
        assert!(tags.contains(&"guide".to_string()));
        assert!(tags.contains(&"第一章".to_string()));
        assert!(tags.contains(&"基础配置".to_string()));
    }

    #[test]
    fn test_extract_tags_dedup() {
        let tags = extract_tags("test-test.md", Some("test"));
        let test_count = tags.iter().filter(|t| t.as_str() == "test").count();
        assert_eq!(test_count, 1);
    }

    #[test]
    fn test_extract_tags_short_tokens_filtered() {
        let tags = extract_tags("a-b-cc-dd.md", None);
        // "a" and "b" are too short (< 2 chars), should be filtered
        assert!(!tags.contains(&"a".to_string()));
        assert!(!tags.contains(&"b".to_string()));
        assert!(tags.contains(&"cc".to_string()));
        assert!(tags.contains(&"dd".to_string()));
    }

    #[test]
    fn test_extract_title_simple() {
        assert_eq!(extract_title_from_filename("hello-world.md"), "hello world");
    }

    #[test]
    fn test_extract_title_underscores() {
        assert_eq!(extract_title_from_filename("my_document.txt"), "my document");
    }

    #[test]
    fn test_extract_title_chinese() {
        assert_eq!(extract_title_from_filename("期货点价-操作指南.md"), "期货点价 操作指南");
    }

    #[test]
    fn test_extract_title_no_extension() {
        assert_eq!(extract_title_from_filename("README"), "README");
    }

    #[test]
    fn test_extract_title_multiple_dots() {
        assert_eq!(extract_title_from_filename("my.doc.v2.md"), "my.doc.v2");
    }
}
