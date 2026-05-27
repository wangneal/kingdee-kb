//! Docx field placeholder extractor
//!
//! Parses .docx files to find field placeholders in document.xml.
//! Supports three Kingdee template formats:
//!   1. `{field_name}` — brace-style placeholders
//!   2. `XXXX字段名` — Kingdee XXXX-style placeholders (most common in 金蝶 templates)
//!   3. SDT content controls with alias/tag/title attributes
//!
//! Handles Word's split-run problem by merging adjacent runs before regex matching.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Field placeholder extracted from a docx template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    /// Field name (e.g., "客户名称", "调研日期")
    pub name: String,
    /// Inferred field type: "text", "number", "date"
    pub field_type: String,
    /// Context snippet around the field for display
    pub context: String,
    /// Occurrence count in the document
    pub count: usize,
    /// Source format: "brace", "xxxx", "sdt"
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "brace".to_string()
}

/// Extract all field placeholders from a .docx file.
///
/// Strategy:
/// 1. Open .docx as ZIP archive
/// 2. Extract word/document.xml
/// 3. Parse XML to collect all `<w:t>` text nodes in order (merging across runs)
/// 4. Extract fields from multiple formats: `{name}`, `XXXX名称`, SDT controls
/// 5. Return deduplicated field list with context
pub fn extract_docx_fields(file_path: &Path) -> Result<Vec<FieldInfo>, String> {
    let file = File::open(file_path)
        .map_err(|e| format!("Failed to open {}: {}", file_path.display(), e))?;

    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Failed to read zip: {}", e))?;

    let mut document_xml = String::new();
    let mut xml_parts = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {}", e))?;
        let name = entry.name().to_string();
        if !is_word_text_part(&name) {
            continue;
        }
        let mut xml = String::new();
        entry
            .read_to_string(&mut xml)
            .map_err(|e| format!("Failed to read {}: {}", name, e))?;
        if name == "word/document.xml" {
            document_xml = xml.clone();
        }
        xml_parts.push(xml);
    }

    if document_xml.is_empty() {
        return Err("document.xml not found".to_string());
    }

    let all_xml = xml_parts.join("\n");

    // Extract all text content, handling split runs
    let merged_text = merge_runs(&all_xml)?;

    // Collect fields from all formats
    let mut fields: BTreeMap<String, (usize, String, String)> = BTreeMap::new();

    // 1. Brace-style: {field_name}
    extract_brace_fields(&merged_text, &mut fields);

    // 2. Kingdee XXXX-style: XXXX字段名
    extract_xxxx_fields(&merged_text, &mut fields);

    // 3. SDT content controls with alias/tag/title
    extract_sdt_fields(&all_xml, &mut fields);

    // 4. Word form fields / merge fields
    extract_word_field_codes(&all_xml, &mut fields);

    let result: Vec<FieldInfo> = fields
        .into_iter()
        .map(|(name, (count, context, source))| FieldInfo {
            field_type: infer_field_type(&name),
            name,
            context,
            count,
            source,
        })
        .collect();

    Ok(result)
}

fn is_word_text_part(name: &str) -> bool {
    name == "word/document.xml"
        || name.starts_with("word/header")
        || name.starts_with("word/footer")
        || name.starts_with("word/footnotes")
        || name.starts_with("word/endnotes")
        || name.starts_with("word/comments")
}

/// Extract `{field_name}` brace-style placeholders.
fn extract_brace_fields(merged_text: &str, fields: &mut BTreeMap<String, (usize, String, String)>) {
    let re = Regex::new(r"\{([^}]+)\}").unwrap();

    for cap in re.captures_iter(merged_text) {
        let field_name = cap[1].trim().to_string();
        if field_name.is_empty() {
            continue;
        }
        let context = extract_context(
            merged_text,
            cap.get(0).map_or(0, |m| m.start()),
            cap.get(0).map_or(0, |m| m.len()),
        );

        let entry = fields
            .entry(field_name)
            .or_insert((0, context, "brace".to_string()));
        entry.0 += 1;
    }
}

/// Extract `XXXX字段名` Kingdee-style placeholders.
///
/// Kingdee templates use `XXXX` as a visual placeholder marker followed by the field name
/// in Chinese characters. The field name extends until a non-field character is encountered.
///
/// Examples from real templates:
///   - "XXXX客户名称" → field "客户名称"
///   - "XXXX项目风险跟踪记录表" → field "项目风险跟踪记录表" (but too long, likely a title)
///   - "XXXX系统" → field "系统"
fn extract_xxxx_fields(merged_text: &str, fields: &mut BTreeMap<String, (usize, String, String)>) {
    // Match XXXX followed by 1-20 Chinese/word characters (the field name)
    let re = Regex::new(r"XXXX([\u4e00-\u9fff][\u4e00-\u9fff\w/（）()\-]{0,19})").unwrap();

    for cap in re.captures_iter(merged_text) {
        let field_name = cap[1].trim().to_string();
        // Filter out very long names (>10 chars) which are likely titles/sentences, not field names
        // Also filter out single-char names (too ambiguous)
        if field_name.is_empty() || field_name.len() > 30 || field_name.chars().count() > 10 {
            continue;
        }
        // Skip if already found via brace pattern (brace takes priority)
        if fields.contains_key(&field_name) {
            continue;
        }

        let context = extract_context(
            merged_text,
            cap.get(0).map_or(0, |m| m.start()),
            cap.get(0).map_or(0, |m| m.len()),
        );

        let entry = fields
            .entry(field_name)
            .or_insert((0, context, "xxxx".to_string()));
        entry.0 += 1;
    }
}

/// Extract SDT content control field names from alias, tag, or title attributes.
///
/// Note: In real Kingdee templates, SDT blocks are typically used for TOC (table of contents)
/// rather than form fields, so this extractor often returns nothing useful.
fn extract_sdt_fields(document_xml: &str, fields: &mut BTreeMap<String, (usize, String, String)>) {
    // Match SDT alias: <w:alias w:val="..."/>
    let alias_re = Regex::new(r#"<w:alias[^>]*w:val="([^"]+)""#).unwrap();
    for cap in alias_re.captures_iter(document_xml) {
        let name = cap[1].trim().to_string();
        if !name.is_empty() && !fields.contains_key(&name) {
            fields.insert(name, (1, "SDT alias".to_string(), "sdt".to_string()));
        }
    }

    // Match SDT tag: <w:tag w:val="..."/> (only if value looks like a field name, not a GUID)
    let tag_re = Regex::new(r#"<w:tag[^>]*w:val="([^"]+)""#).unwrap();
    for cap in tag_re.captures_iter(document_xml) {
        let val = cap[1].trim().to_string();
        // Skip GUIDs and numeric-only tags
        if val.is_empty() || val.contains('-') && val.len() > 20 {
            continue;
        }
        if !fields.contains_key(&val) {
            fields.insert(val, (1, "SDT tag".to_string(), "sdt".to_string()));
        }
    }

    // Match SDT title: <w:title w:val="..."/>
    let title_re = Regex::new(r#"<w:title[^>]*w:val="([^"]+)""#).unwrap();
    for cap in title_re.captures_iter(document_xml) {
        let name = cap[1].trim().to_string();
        if !name.is_empty() && !fields.contains_key(&name) {
            fields.insert(name, (1, "SDT title".to_string(), "sdt".to_string()));
        }
    }
}

fn extract_word_field_codes(
    document_xml: &str,
    fields: &mut BTreeMap<String, (usize, String, String)>,
) {
    let form_name_re = Regex::new(r#"<w:ffData\b(?s:.*?)<w:name[^>]*w:val="([^"]+)""#).unwrap();
    for cap in form_name_re.captures_iter(document_xml) {
        let name = normalize_field_name(&cap[1]);
        if should_keep_field_name(&name) {
            let entry = fields.entry(name).or_insert((
                0,
                "Word form field".to_string(),
                "form".to_string(),
            ));
            entry.0 += 1;
        }
    }

    let instr_re = Regex::new(r#"<w:instrText[^>]*>(?s:.*?)</w:instrText>"#).unwrap();
    let text_tag_re = Regex::new(r"<[^>]+>").unwrap();
    let merge_re = Regex::new(r#"MERGEFIELD\s+["']?([^"'\s\\]+)"#).unwrap();
    for cap in instr_re.captures_iter(document_xml) {
        let raw = text_tag_re.replace_all(&cap[0], "");
        let decoded = decode_xml_entities(&raw);
        for merge in merge_re.captures_iter(&decoded) {
            let name = normalize_field_name(&merge[1]);
            if should_keep_field_name(&name) {
                let entry = fields.entry(name).or_insert((
                    0,
                    "MERGEFIELD".to_string(),
                    "mergefield".to_string(),
                ));
                entry.0 += 1;
            }
        }
    }
}

fn normalize_field_name(value: &str) -> String {
    decode_xml_entities(value)
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '«' | '»' | '《' | '》'))
        .trim()
        .to_string()
}

fn should_keep_field_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "toc" | "hyperlink" | "page" | "numPages" | "ref"
    ) {
        return false;
    }
    name.chars().count() <= 40
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Merge text runs from document.xml to handle Word's split-run problem.
///
/// Word sometimes splits a single placeholder like `{项目名称}` across
/// multiple `<w:r><w:t>...</w:t></w:r>` elements. This function extracts
/// all text content in document order, producing a single merged string.
fn merge_runs(xml: &str) -> Result<String, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut text_buffer = String::new();
    let mut in_text_node = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name_bytes = e.name();
                let name_ref = name_bytes.as_ref();
                // Track when we're inside a <w:t> element
                if name_ref == b"w:t" {
                    in_text_node = true;
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_text_node {
                    let text = e
                        .unescape()
                        .map_err(|e| format!("XML unescape error: {}", e))?;
                    text_buffer.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let name_bytes = e.name();
                let name_ref = name_bytes.as_ref();
                if name_ref == b"w:t" {
                    in_text_node = false;
                }
                // At paragraph boundaries, add a separator to avoid merging
                // text across paragraphs (which could create false positives)
                if name_ref == b"w:p" {
                    text_buffer.push('\n');
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {}", e)),
            _ => {}
        }
    }

    Ok(text_buffer)
}

/// Extract a context window around a match position, safe for multi-byte (UTF-8) strings.
///
/// Takes ~30 chars before and after the match, handling char boundaries correctly.
fn extract_context(text: &str, match_start: usize, match_len: usize) -> String {
    // Convert byte positions to char positions for safe slicing
    let chars: Vec<char> = text.chars().collect();
    let total_chars = chars.len();

    // Find the char index corresponding to the byte position
    let mut byte_pos = 0;
    let mut char_match_start = 0;
    let mut char_match_end = 0;
    let mut found_start = false;

    for (i, ch) in chars.iter().enumerate() {
        if byte_pos >= match_start && !found_start {
            char_match_start = i;
            found_start = true;
        }
        if byte_pos >= match_start + match_len && char_match_end == 0 {
            char_match_end = i;
            break;
        }
        byte_pos += ch.len_utf8();
    }
    if char_match_end == 0 {
        char_match_end = total_chars;
    }

    let ctx_start = char_match_start.saturating_sub(30);
    let ctx_end = (char_match_end + 30).min(total_chars);

    chars[ctx_start..ctx_end]
        .iter()
        .collect::<String>()
        .replace('\n', " ")
}

/// Infer field type from the field name.
///
/// Simple heuristic based on common Chinese naming patterns.
fn infer_field_type(name: &str) -> String {
    let name_lower = name.to_lowercase();

    // Date patterns
    if name_lower.contains("日期")
        || name_lower.contains("时间")
        || name_lower.contains("date")
        || name_lower.contains("time")
    {
        return "date".to_string();
    }

    // Number patterns
    if name_lower.contains("数量")
        || name_lower.contains("金额")
        || name_lower.contains("价格")
        || name_lower.contains("比例")
        || name_lower.contains("百分")
        || name_lower.contains("天数")
        || name_lower.contains("number")
        || name_lower.contains("amount")
        || name_lower.contains("price")
        || name_lower.contains("count")
        || name_lower.contains("ratio")
    {
        return "number".to_string();
    }

    // Default to text
    "text".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_field_type() {
        assert_eq!(infer_field_type("项目名称"), "text");
        assert_eq!(infer_field_type("调研日期"), "date");
        assert_eq!(infer_field_type("预计金额"), "number");
        assert_eq!(infer_field_type("人员数量"), "number");
        assert_eq!(infer_field_type("开始时间"), "date");
    }

    #[test]
    fn test_merge_runs_simple() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:r><w:t>项目名称：</w:t></w:r>
      <w:r><w:t>{项目</w:t></w:r>
      <w:r><w:t>名称}</w:t></w:r>
    </w:p>
  </w:body>
</w:document>"#;

        let merged = merge_runs(xml).unwrap();
        assert!(merged.contains("项目名称：{项目名称}"));
    }
}
