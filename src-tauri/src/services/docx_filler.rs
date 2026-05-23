//! Docx template filler
//!
//! Takes a .docx template and a HashMap of field values, replaces `{field_name}`
//! placeholders with actual values, and saves the result as a new .docx file.
//!
//! Handles Word's split-run problem by merging adjacent runs within each paragraph
//! before performing replacements.

use quick_xml::events::Event;
use quick_xml::Reader;
use quick_xml::Writer;
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use zip::read::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};

/// Fill a .docx template with field values.
///
/// Opens the template, replaces all `{field_name}` placeholders with corresponding
/// values from `fields`, and saves the result to `output_path`.
///
/// Returns the number of field replacements made.
pub fn fill_docx(
    template_path: &Path,
    fields: &HashMap<String, String>,
    output_path: &Path,
) -> Result<usize, String> {
    let template_file = File::open(template_path)
        .map_err(|e| format!("Failed to open template {}: {}", template_path.display(), e))?;
    let mut archive = ZipArchive::new(template_file)
        .map_err(|e| format!("Failed to read template zip: {}", e))?;

    let output_file = File::create(output_path)
        .map_err(|e| format!("Failed to create output {}: {}", output_path.display(), e))?;
    let mut writer = ZipWriter::new(output_file);
    let options = SimpleFileOptions::default();

    let mut total_replaced = 0;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry {}: {}", i, e))?;
        let name = entry.name().to_string();

        if name == "word/document.xml" {
            let mut xml = String::new();
            entry
                .read_to_string(&mut xml)
                .map_err(|e| format!("Failed to read document.xml: {}", e))?;

            let (processed_xml, count) = process_document_xml(&xml, fields)?;
            total_replaced = count;

            writer
                .start_file(&name, options)
                .map_err(|e| format!("Failed to start file in output zip: {}", e))?;
            writer
                .write_all(processed_xml.as_bytes())
                .map_err(|e| format!("Failed to write document.xml: {}", e))?;
        } else {
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .map_err(|e| format!("Failed to read zip entry {}: {}", name, e))?;

            writer
                .start_file(&name, options)
                .map_err(|e| format!("Failed to start file in output zip: {}", e))?;
            writer
                .write_all(&content)
                .map_err(|e| format!("Failed to write zip entry {}: {}", name, e))?;
        }
    }

    writer
        .finish()
        .map_err(|e| format!("Failed to finalize output zip: {}", e))?;

    Ok(total_replaced)
}

/// Process document.xml, replacing field placeholders within paragraphs.
///
/// Strategy: buffer raw XML bytes per paragraph, then process the paragraph
/// string to merge runs and replace fields.
fn process_document_xml(
    xml: &str,
    fields: &HashMap<String, String>,
) -> Result<(String, usize), String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut paragraph_buf: Vec<u8> = Vec::new();
    let mut in_paragraph = false;
    let mut _depth: u32 = 0;
    let mut total_replaced = 0;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) if e.name().as_ref() == b"w:p" => {
                in_paragraph = true;
                _depth = 1;
                paragraph_buf.clear();
                // Write the start tag to buffer
                let mut temp_writer = Writer::new(Cursor::new(Vec::new()));
                temp_writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(|e| format!("XML write error: {}", e))?;
                paragraph_buf.extend_from_slice(&temp_writer.into_inner().into_inner());
            }
            Ok(Event::Start(ref e)) if in_paragraph => {
                _depth += 1;
                let mut temp_writer = Writer::new(Cursor::new(Vec::new()));
                temp_writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(|e| format!("XML write error: {}", e))?;
                paragraph_buf.extend_from_slice(&temp_writer.into_inner().into_inner());
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"w:p" && in_paragraph => {
                // Write the end tag to buffer
                let mut temp_writer = Writer::new(Cursor::new(Vec::new()));
                temp_writer
                    .write_event(Event::End(e.clone()))
                    .map_err(|e| format!("XML write error: {}", e))?;
                paragraph_buf.extend_from_slice(&temp_writer.into_inner().into_inner());

                // Process paragraph: merge runs, replace fields, write to main writer
                let paragraph_xml = String::from_utf8_lossy(&paragraph_buf).to_string();
                let (processed, count) = process_paragraph_str(&paragraph_xml, fields)?;
                total_replaced += count;

                writer
                    .write_event(Event::Text(quick_xml::events::BytesText::new(&processed)))
                    .map_err(|e| format!("XML write error: {}", e))?;

                in_paragraph = false;
                paragraph_buf.clear();
            }
            Ok(Event::End(ref e)) if in_paragraph => {
                _depth -= 1;
                let mut temp_writer = Writer::new(Cursor::new(Vec::new()));
                temp_writer
                    .write_event(Event::End(e.clone()))
                    .map_err(|e| format!("XML write error: {}", e))?;
                paragraph_buf.extend_from_slice(&temp_writer.into_inner().into_inner());
            }
            Ok(Event::Eof) => break,
            Ok(ref event) => {
                if in_paragraph {
                    let mut temp_writer = Writer::new(Cursor::new(Vec::new()));
                    temp_writer
                        .write_event(event.clone())
                        .map_err(|e| format!("XML write error: {}", e))?;
                    paragraph_buf.extend_from_slice(&temp_writer.into_inner().into_inner());
                } else {
                    writer
                        .write_event(event.clone())
                        .map_err(|e| format!("XML write error: {}", e))?;
                }
            }
            Err(e) => return Err(format!("XML parse error: {}", e)),
        }
    }

    let result = writer.into_inner().into_inner();
    let xml_str =
        String::from_utf8(result).map_err(|e| format!("UTF-8 conversion error: {}", e))?;

    Ok((xml_str, total_replaced))
}

/// Process a paragraph XML string, merging split runs and replacing field placeholders.
///
/// Handles Word's split-run problem by:
/// 1. Collecting all `<w:t>` text content from runs
/// 2. Merging into a single string
/// 3. Replacing `{field_name}` patterns with values
/// 4. Writing merged text back to the first `<w:t>`, clearing subsequent ones
///
/// Returns the processed paragraph XML and the number of replacements made.
fn process_paragraph_str(
    paragraph_xml: &str,
    fields: &HashMap<String, String>,
) -> Result<(String, usize), String> {
    // -- Pass 1: collect all <w:t> text content --
    let mut reader = Reader::from_str(paragraph_xml);
    reader.config_mut().trim_text(true);

    let mut text_parts: Vec<String> = Vec::new();
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) if e.name().as_ref() == b"w:t" => {
                in_text = true;
            }
            Ok(Event::Text(ref e)) if in_text => {
                let text = e
                    .unescape()
                    .map_err(|e| format!("XML unescape error: {}", e))?;
                text_parts.push(text.to_string());
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"w:t" => {
                in_text = false;
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
    }

    if text_parts.is_empty() {
        return Ok((paragraph_xml.to_string(), 0));
    }

    let merged: String = text_parts.join("");
    let (replaced, count) = replace_fields(&merged, fields);

    if count == 0 {
        return Ok((paragraph_xml.to_string(), 0));
    }

    // -- Pass 2: write replaced text back --
    let mut reader = Reader::from_str(paragraph_xml);
    reader.config_mut().trim_text(true);

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut text_node_index = 0usize;
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) if e.name().as_ref() == b"w:t" => {
                in_text = true;
                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(|e| format!("XML write error: {}", e))?;
            }
            Ok(Event::Text(_)) if in_text => {
                if text_node_index == 0 {
                    writer
                        .write_event(Event::Text(quick_xml::events::BytesText::new(&replaced)))
                        .map_err(|e| format!("XML write error: {}", e))?;
                } else {
                    writer
                        .write_event(Event::Text(quick_xml::events::BytesText::new("")))
                        .map_err(|e| format!("XML write error: {}", e))?;
                }
                text_node_index += 1;
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"w:t" => {
                in_text = false;
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(|e| format!("XML write error: {}", e))?;
            }
            Ok(Event::Eof) => break,
            Ok(ref event) => {
                writer
                    .write_event(event.clone())
                    .map_err(|e| format!("XML write error: {}", e))?;
            }
            Err(e) => return Err(format!("XML parse error: {}", e)),
        }
    }

    let result = writer.into_inner().into_inner();
    let xml_str =
        String::from_utf8(result).map_err(|e| format!("UTF-8 conversion error: {}", e))?;

    Ok((xml_str, count))
}

/// Replace `{field_name}` patterns in text with values from the fields map.
///
/// Returns the replaced text and the number of replacements made.
/// Fields not found in the map are left as-is (original placeholder preserved).
fn replace_fields(text: &str, fields: &HashMap<String, String>) -> (String, usize) {
    let re = Regex::new(r"\{([^}]+)\}").expect("Invalid regex");
    let mut count = 0usize;

    let result = re
        .replace_all(text, |caps: &regex::Captures| {
            let field_name = &caps[1];
            match fields.get(field_name) {
                Some(value) => {
                    count += 1;
                    value.clone()
                }
                None => caps[0].to_string(),
            }
        })
        .to_string();

    (result, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_fields_basic() {
        let mut fields = HashMap::new();
        fields.insert("项目名称".to_string(), "星达铜业".to_string());
        fields.insert("调研日期".to_string(), "2026-05-23".to_string());

        let text = "项目名称：{项目名称}，调研日期：{调研日期}";
        let (result, count) = replace_fields(text, &fields);
        assert_eq!(result, "项目名称：星达铜业，调研日期：2026-05-23");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_replace_fields_partial() {
        let mut fields = HashMap::new();
        fields.insert("项目名称".to_string(), "星达铜业".to_string());

        let text = "项目名称：{项目名称}，负责人：{负责人}";
        let (result, count) = replace_fields(text, &fields);
        assert_eq!(result, "项目名称：星达铜业，负责人：{负责人}");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_replace_fields_empty_map() {
        let fields = HashMap::new();
        let text = "项目名称：{项目名称}";
        let (result, count) = replace_fields(text, &fields);
        assert_eq!(result, "项目名称：{项目名称}");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_process_document_xml_split_runs() {
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

        let mut fields = HashMap::new();
        fields.insert("项目名称".to_string(), "星达铜业".to_string());

        let (result, count) = process_document_xml(xml, &fields).unwrap();
        assert_eq!(count, 1);
        assert!(result.contains("星达铜业"));
        assert!(result.contains("项目名称：星达铜业"));
    }

    #[test]
    fn test_process_document_xml_no_placeholders() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:r><w:t>普通文本，没有占位符。</w:t></w:r>
    </w:p>
  </w:body>
</w:document>"#;

        let fields = HashMap::new();
        let (result, count) = process_document_xml(xml, &fields).unwrap();
        assert_eq!(count, 0);
        assert!(result.contains("普通文本，没有占位符。"));
    }

    #[test]
    fn test_process_document_xml_multiple_paragraphs() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:r><w:t>项目：{项目名称}</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:t>日期：{调研日期}</w:t></w:r>
    </w:p>
  </w:body>
</w:document>"#;

        let mut fields = HashMap::new();
        fields.insert("项目名称".to_string(), "星达铜业".to_string());
        fields.insert("调研日期".to_string(), "2026-05-23".to_string());

        let (result, count) = process_document_xml(xml, &fields).unwrap();
        assert_eq!(count, 2);
        assert!(result.contains("星达铜业"));
        assert!(result.contains("2026-05-23"));
    }
}
