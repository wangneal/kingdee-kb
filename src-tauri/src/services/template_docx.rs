//! Docx field placeholder extractor
//!
//! Parses .docx files to find `{field_name}` placeholders in document.xml.
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
    /// Field name (e.g., "项目名称", "调研日期")
    pub name: String,
    /// Inferred field type: "text", "number", "date"
    pub field_type: String,
    /// Context snippet around the field for display
    pub context: String,
    /// Occurrence count in the document
    pub count: usize,
}

/// Extract all `{field_name}` placeholders from a .docx file.
///
/// Strategy:
/// 1. Open .docx as ZIP archive
/// 2. Extract word/document.xml
/// 3. Parse XML to collect all `<w:t>` text nodes in order (merging across runs)
/// 4. Apply regex `\{([^}]+)\}` on the merged text to find placeholders
/// 5. Return deduplicated field list with context
pub fn extract_docx_fields(file_path: &Path) -> Result<Vec<FieldInfo>, String> {
    let file =
        File::open(file_path).map_err(|e| format!("Failed to open {}: {}", file_path.display(), e))?;

    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Failed to read zip: {}", e))?;

    // Read word/document.xml
    let mut document_xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|e| format!("document.xml not found: {}", e))?
        .read_to_string(&mut document_xml)
        .map_err(|e| format!("Failed to read document.xml: {}", e))?;

    // Extract all text content, handling split runs
    let merged_text = merge_runs(&document_xml)?;

    // Find all {field_name} patterns
    let re = Regex::new(r"\{([^}]+)\}").map_err(|e| format!("Regex error: {}", e))?;

    let mut fields: BTreeMap<String, (usize, String)> = BTreeMap::new();

    for cap in re.captures_iter(&merged_text) {
        let field_name = cap[1].to_string();
        // SAFE: cap.get(0) always exists in captures_iter (full match)
        let match_start = cap.get(0).map_or(0, |m| m.start());

        // Extract context: ~50 chars before and after
        let ctx_start = match_start.saturating_sub(30);
        let ctx_end = (match_start + cap[0].len() + 30).min(merged_text.len());
        let context = merged_text[ctx_start..ctx_end]
            .chars()
            .collect::<String>()
            .replace('\n', " ");

        let entry = fields.entry(field_name.clone()).or_insert((0, context));
        entry.0 += 1;
    }

    let result: Vec<FieldInfo> = fields
        .into_iter()
        .map(|(name, (count, context))| FieldInfo {
            field_type: infer_field_type(&name),
            name,
            context,
            count,
        })
        .collect();

    Ok(result)
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
