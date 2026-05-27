//! Docx 模板填充器
//!
//! 接收 .docx 模板和字段值的 HashMap，将 `{field_name}`
//! 占位符替换为实际值，并将结果保存为新的 .docx 文件。
//!
//! 通过在每个段落中合并相邻的 run 来处理 Word 的 split-run 问题，
//! 然后执行替换。

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

/// 使用字段值填充 .docx 模板。
///
/// 打开模板，将所有 `{field_name}` 占位符替换为
/// `fields` 中的对应值，并将结果保存到 `output_path`。
///
/// 返回字段替换次数。
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

        if is_word_text_part(&name) {
            let mut xml = String::new();
            entry
                .read_to_string(&mut xml)
                .map_err(|e| format!("Failed to read {}: {}", name, e))?;

            let (processed_xml, count) = process_document_xml(&xml, fields)?;
            total_replaced += count;

            writer
                .start_file(&name, options)
                .map_err(|e| format!("Failed to start file in output zip: {}", e))?;
            writer
                .write_all(processed_xml.as_bytes())
                .map_err(|e| format!("Failed to write {}: {}", name, e))?;
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

/// 处理 document.xml，替换段落中的字段占位符。
///
/// 策略：每个段落缓冲原始 XML 字节，然后处理段落
/// 字符串以合并 run 并替换字段。
fn process_document_xml(
    xml: &str,
    fields: &HashMap<String, String>,
) -> Result<(String, usize), String> {
    let paragraph_re =
        Regex::new(r#"(?s)<w:p\b[^>]*>.*?</w:p>"#).map_err(|e| format!("Regex error: {}", e))?;
    let mut total_replaced = 0;
    let mut error: Option<String> = None;

    let result = paragraph_re
        .replace_all(xml, |caps: &regex::Captures| {
            if error.is_some() {
                return caps[0].to_string();
            }

            match process_paragraph_str(&caps[0], fields) {
                Ok((processed, count)) => {
                    total_replaced += count;
                    processed
                }
                Err(e) => {
                    error = Some(e);
                    caps[0].to_string()
                }
            }
        })
        .to_string();

    if let Some(e) = error {
        return Err(e);
    }

    Ok((result, total_replaced))
}

/// 处理段落 XML 字符串，合并 split run 并替换字段占位符。
///
/// 通过以下方式处理 Word 的 split-run 问题：
/// 1. 收集所有 `<w:t>` 文本内容
/// 2. 合并为单个字符串
/// 3. 将 `{field_name}` 模式替换为值
/// 4. 将合并后的文本写回第一个 `<w:t>`，清空后续的
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
    let (mut replaced, mut count) = replace_fields(&merged, fields);

    if count == 0 {
        if let Some((field_name, value)) = field_code_value(paragraph_xml, fields) {
            replaced = replace_field_code_display(&merged, &field_name, &value);
            count = 1;
        }
    }

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
    let xxxx_re = Regex::new(r"XXXX([\u4e00-\u9fff][\u4e00-\u9fff\w/（）()\-]{0,19})")
        .expect("Invalid regex");
    let mut count = 0usize;

    let result = re
        .replace_all(text, |caps: &regex::Captures| {
            let field_name = &caps[1];
            match get_field_value(fields, field_name) {
                Some(value) => {
                    count += 1;
                    value
                }
                None => caps[0].to_string(),
            }
        })
        .to_string();

    let result = xxxx_re
        .replace_all(&result, |caps: &regex::Captures| {
            let field_name = &caps[1];
            match get_field_value(fields, field_name) {
                Some(value) => {
                    count += 1;
                    value
                }
                None => caps[0].to_string(),
            }
        })
        .to_string();

    (result, count)
}

fn is_word_text_part(name: &str) -> bool {
    name == "word/document.xml"
        || (name.starts_with("word/header") && name.ends_with(".xml"))
        || (name.starts_with("word/footer") && name.ends_with(".xml"))
        || name == "word/footnotes.xml"
        || name == "word/endnotes.xml"
        || name == "word/comments.xml"
}

fn get_field_value(fields: &HashMap<String, String>, name: &str) -> Option<String> {
    let normalized = normalize_field_name(name);
    fields
        .get(name)
        .or_else(|| fields.get(name.trim()))
        .or_else(|| fields.get(&normalized))
        .cloned()
}

fn normalize_field_name(name: &str) -> String {
    name.trim()
        .trim_matches('《')
        .trim_matches('》')
        .trim_matches('«')
        .trim_matches('»')
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn field_code_value(
    paragraph_xml: &str,
    fields: &HashMap<String, String>,
) -> Option<(String, String)> {
    for name in field_code_names(paragraph_xml) {
        if let Some(value) = get_field_value(fields, &name) {
            return Some((name, value));
        }
    }
    None
}

fn field_code_names(paragraph_xml: &str) -> Vec<String> {
    let patterns = [
        r#"<w:name[^>]*w:val="([^"]+)""#,
        r#"<w:alias[^>]*w:val="([^"]+)""#,
        r#"<w:tag[^>]*w:val="([^"]+)""#,
        r#"MERGEFIELD\s+["']?([^"'\s\\]+)"#,
    ];

    let mut names = Vec::new();
    for pattern in patterns {
        let re = Regex::new(pattern).expect("Invalid regex");
        for cap in re.captures_iter(paragraph_xml) {
            let name = normalize_field_name(&decode_xml_entities(&cap[1]));
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn replace_field_code_display(text: &str, field_name: &str, value: &str) -> String {
    if text.trim().is_empty() {
        return value.to_string();
    }

    let guillemet_re = Regex::new(r"«[^»]+»").expect("Invalid regex");
    if guillemet_re.is_match(text) {
        return guillemet_re.replace_all(text, value).to_string();
    }

    if text.contains(field_name) {
        return text.replace(field_name, value);
    }

    if text.trim() == "XXXX" {
        return text.replace("XXXX", value);
    }

    value.to_string()
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
    fn test_replace_fields_xxxx() {
        let mut fields = HashMap::new();
        fields.insert("项目名称".to_string(), "罗孚项目".to_string());

        let (result, count) = replace_fields("项目：XXXX项目名称", &fields);
        assert_eq!(result, "项目：罗孚项目");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_process_document_xml_mergefield_display() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:instrText> MERGEFIELD ProjectName \* MERGEFORMAT </w:instrText></w:r><w:r><w:t>项目：«ProjectName»</w:t></w:r></w:p></w:body></w:document>"#;
        let mut fields = HashMap::new();
        fields.insert("ProjectName".to_string(), "罗孚项目".to_string());

        let (result, count) = process_document_xml(xml, &fields).unwrap();
        assert_eq!(count, 1);
        assert!(result.contains("项目：罗孚项目"), "result: {}", result);
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
