//! Pptx field placeholder extractor
//!
//! Parses .pptx files to find `{field_name}` placeholders in slide text boxes.
//! PPTX is a ZIP archive; text content lives in `ppt/slides/slide*.xml` as `<a:t>` elements.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::template_common;

/// Field placeholder extracted from a pptx template (slide text)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PptxFieldInfo {
    /// Field name (e.g., "项目名称", "汇报日期")
    pub name: String,
    /// Inferred field type: "text", "number", "date"
    pub field_type: String,
    /// Slide index where the field appears (1-based)
    pub slide_index: usize,
    /// Context snippet around the field for display
    pub context: String,
    /// Occurrence count in the document
    pub count: usize,
}

/// Extract all `{field_name}` placeholders from a .pptx file.
///
/// Strategy:
/// 1. Open .pptx as ZIP archive
/// 2. Iterate all `ppt/slides/slide*.xml` files
/// 3. Parse each slide's XML to collect `<a:t>` text nodes
/// 4. Apply regex `\{([^}]+)\}` on each slide's merged text to find placeholders
/// 5. Return deduplicated field list with slide index and context
pub fn extract_pptx_fields(file_path: &Path) -> Result<Vec<PptxFieldInfo>, String> {
    let file =
        File::open(file_path).map_err(|e| format!("Failed to open {}: {}", file_path.display(), e))?;

    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Failed to read pptx zip: {}", e))?;

    let re = Regex::new(r"\{([^}]+)\}").map_err(|e| format!("Regex error: {}", e))?;

    // Collect all slide file names sorted
    let mut slide_names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| format!("ZIP index error: {}", e))?;
        let name = entry.name().to_string();
        // Match ppt/slides/slideN.xml (not slide layouts, not notes, not masters)
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") && !name.contains("notes") && !name.contains("layout") && !name.contains("master") {
            slide_names.push(name);
        }
    }
    // NatSort: slide1, slide2, ..., slide10
    slide_names.sort_by(|a, b| {
        let a_num = extract_slide_number(a).unwrap_or(0);
        let b_num = extract_slide_number(b).unwrap_or(0);
        a_num.cmp(&b_num)
    });

    if slide_names.is_empty() {
        return Err("PPTX文件中未找到幻灯片 (ppt/slides/slide*.xml)".to_string());
    }

    let mut fields: BTreeMap<String, (usize, String, usize)> = BTreeMap::new();
    // ^^ field_name -> (slide_index, context, count)

    for (slide_idx, slide_name) in slide_names.iter().enumerate() {
        let slide_num = slide_idx + 1; // 1-based for user display
        let mut slide_xml = String::new();
        archive
            .by_name(slide_name)
            .map_err(|e| format!("Failed to read {}: {}", slide_name, e))?
            .read_to_string(&mut slide_xml)
            .map_err(|e| format!("Failed to read {}: {}", slide_name, e))?;

        // Extract all text from this slide
        let slide_text = extract_text_from_slide_xml(&slide_xml)?;

        // Find all {field_name} patterns
        for cap in re.captures_iter(&slide_text) {
            let field_name = cap[1].to_string();
            if field_name.trim().is_empty() {
                continue;
            }

            // Extract context: 40 chars before/after
            let full_match = cap.get(0).map_or("", |m| m.as_str());
            let match_start = cap.get(0).map_or(0, |m| m.start());
            let ctx_start = match_start.saturating_sub(40);
            let ctx_end = (match_start + full_match.len() + 40).min(slide_text.len());
            let context = if ctx_start >= ctx_end {
                field_name.clone()
            } else {
                let ctx = &slide_text[ctx_start..ctx_end];
                ctx.trim().to_string()
            };

            let entry = fields.entry(field_name.clone()).or_insert_with(|| (slide_num, context.clone(), 0));
            entry.2 += 1;
            // Keep the first slide occurrence as primary
        }
    }

    let result: Vec<PptxFieldInfo> = fields
        .into_iter()
        .map(|(name, (slide_idx, context, count))| PptxFieldInfo {
            field_type: template_common::infer_field_type(&name),
            name,
            slide_index: slide_idx,
            context,
            count,
        })
        .collect();

    Ok(result)
}

/// Extract the slide number from a path like "ppt/slides/slide12.xml"
fn extract_slide_number(path: &str) -> Option<usize> {
    let name = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())?;
    let num_str = name.trim_start_matches("slide");
    num_str.parse::<usize>().ok()
}

/// Extract all text content from a single slide's XML.
///
/// PPTX slide XML uses `a:t` elements (drawingml text) within shapes:
/// ```xml
/// <p:sp>
///   <p:txBody>
///     <a:p>
///       <a:r>
///         <a:t>Hello {field_name}</a:t>
///       </a:r>
///     </a:p>
///   </p:txBody>
/// </p:sp>
/// ```
fn extract_text_from_slide_xml(xml: &str) -> Result<String, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut text_buffer = String::new();
    let mut in_text_node = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                // Check for both a:t (drawingml text) and w:t (wordml - some pptx files embed word tables)
                let name = e.name().as_ref().to_vec();
                if name == b"a:t" || name == b"w:t" {
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
                let name = e.name().as_ref().to_vec();
                if name == b"a:t" || name == b"w:t" {
                    in_text_node = false;
                }
                // Paragraph boundary -> add newline (for a:p or w:p)
                if name == b"a:p" || name == b"w:p" {
                    text_buffer.push('\n');
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error in slide: {}", e)),
            _ => {}
        }
    }

    Ok(text_buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_slide_number() {
        assert_eq!(extract_slide_number("ppt/slides/slide1.xml"), Some(1));
        assert_eq!(extract_slide_number("ppt/slides/slide12.xml"), Some(12));
        assert_eq!(extract_slide_number("ppt/slides/slide0.xml"), Some(0));
        assert_eq!(extract_slide_number("ppt/slides/slide.xml"), None);
        assert_eq!(extract_slide_number("ppt/slides/layout1.xml"), None);
    }

    #[test]
    fn test_extract_text_from_slide_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <p:slide xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <p:sp>
                <p:txBody>
                    <a:p>
                        <a:r>
                            <a:t>项目名称：{项目名称}</a:t>
                        </a:r>
                    </a:p>
                    <a:p>
                        <a:r>
                            <a:t>汇报日期：{汇报日期}</a:t>
                        </a:r>
                    </a:p>
                </p:txBody>
            </p:sp>
        </p:slide>"#;

        let text = extract_text_from_slide_xml(xml).unwrap();
        assert!(text.contains("项目名称：{项目名称}"));
        assert!(text.contains("汇报日期：{汇报日期}"));
    }

    #[test]
    fn test_infer_field_type_pptx() {
        assert_eq!(template_common::infer_field_type("项目名称"), "text");
        assert_eq!(template_common::infer_field_type("汇报日期"), "date");
        assert_eq!(template_common::infer_field_type("项目预算"), "number");
    }
}
