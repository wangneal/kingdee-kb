//! 文件内容提取器：根据文件扩展名分派到对应的文本提取策略
//!
//! 支持格式：
//! - Markdown / TXT：直接读取纯文本
//! - HTML：读取后去除标签（由 text_cleaner 处理）
//! - PDF：使用 pdf-extract crate 提取文本
//! - DOCX：解压 ZIP → 解析 word/document.xml → 提取 <w:t> 文本
//! - XLSX/XLS：使用 umya-spreadsheet 读取每个 sheet 的单元格文本

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// 根据文件扩展名提取纯文本内容
pub fn extract_text(file_path: &Path) -> Result<String, String> {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        // 纯文本格式：直接读取
        "md" | "txt" | "text" | "markdown" => extract_plain_text(file_path),

        // HTML：直接读取（后续由 text_cleaner 去除标签）
        "html" | "htm" => extract_plain_text(file_path),

        // PDF：使用 pdf-extract
        "pdf" => extract_pdf_text(file_path),

        // DOCX：解压 ZIP 并提取 XML 文本
        "docx" => extract_docx_text(file_path),

        // Excel：使用 umya-spreadsheet
        "xlsx" | "xls" => extract_xlsx_text(file_path),

        // Video/Audio: require transcription pipeline, not direct text extraction
        "mp4" | "webm" | "avi" | "mov" | "mkv" | "flv" | "wmv" | "m4a" | "mp3" | "wav" => {
            Err(format!(
                "视频/音频文件 (.{}) 需要通过转写管道处理，请使用视频转写功能",
                extension
            ))
        }

        _ => Err(format!("不支持的文件格式：.{}", extension)),
    }
}

/// 获取所有支持的文件扩展名
pub fn supported_extensions() -> &'static [&'static str] {
    &[
        "md", "txt", "text", "markdown", "html", "htm", "pdf", "docx", "xlsx", "xls",
        // Phase 14: Video/Audio formats (require transcription pipeline)
        "mp4", "webm", "avi", "mov", "mkv", "flv", "wmv", "m4a", "mp3", "wav",
    ]
}

/// 检查文件扩展名是否受支持
pub fn is_supported(file_path: &Path) -> bool {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    supported_extensions().contains(&extension.as_str())
}

/// Check if the file is a video/audio format that requires the transcription pipeline.
pub fn is_video_format(file_path: &Path) -> bool {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    matches!(
        extension.as_str(),
        "mp4" | "webm" | "avi" | "mov" | "mkv" | "flv" | "wmv" | "m4a" | "mp3" | "wav"
    )
}

// ── 内部实现 ──────────────────────────────────────────────────────────────────

/// 读取纯文本文件
fn extract_plain_text(file_path: &Path) -> Result<String, String> {
    std::fs::read_to_string(file_path)
        .map_err(|e| format!("读取文件失败 {:?}: {}", file_path.display(), e))
}

/// 从 PDF 文件提取文本
fn extract_pdf_text(file_path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(file_path)
        .map_err(|e| format!("读取 PDF 失败 {:?}: {}", file_path.display(), e))?;

    let text = pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| format!("解析 PDF 失败 {:?}: {}", file_path.display(), e))?;

    Ok(text)
}

/// 从 DOCX 文件提取文本
///
/// DOCX 本质是 ZIP 压缩包，文本内容在 word/document.xml 的 <w:t> 节点中。
/// 复用了 template_docx 模块的 merge_runs 逻辑思路。
fn extract_docx_text(file_path: &Path) -> Result<String, String> {
    let file = File::open(file_path)
        .map_err(|e| format!("打开 DOCX 失败 {:?}: {}", file_path.display(), e))?;

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("解析 DOCX ZIP 失败 {:?}: {}", file_path.display(), e))?;

    // 列出 ZIP 内所有文件，帮助诊断
    let file_list: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();
    eprintln!("[DOCX] ZIP 内容: {:?}", file_list);

    // 读取 word/document.xml
    let mut document_xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|e| format!("DOCX 中未找到 word/document.xml: {}", e))?
        .read_to_string(&mut document_xml)
        .map_err(|e| format!("读取 document.xml 失败: {}", e))?;

    eprintln!("[DOCX] document.xml 长度: {} bytes", document_xml.len());

    // 解析 XML 提取文本
    let text = extract_text_from_docx_xml(&document_xml)?;

    eprintln!("[DOCX] 提取文本长度: {} chars, 前200字: {:?}", 
        text.len(), 
        text.chars().take(200).collect::<String>());

    if text.trim().is_empty() {
        return Err(format!("DOCX 文件内容为空: {:?}", file_path.display()));
    }

    Ok(text)
}

/// 从 DOCX 的 document.xml 提取所有文本
fn extract_text_from_docx_xml(xml: &str) -> Result<String, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut text_buffer = String::new();
    let mut in_text_node = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                if e.name().as_ref() == b"w:t" {
                    in_text_node = true;
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_text_node {
                    let text = e
                        .unescape()
                        .map_err(|e| format!("XML 反转义错误: {}", e))?;
                    text_buffer.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == b"w:t" {
                    in_text_node = false;
                }
                // 段落边界添加换行
                if e.name().as_ref() == b"w:p" {
                    text_buffer.push('\n');
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML 解析错误: {}", e)),
            _ => {}
        }
    }

    Ok(text_buffer)
}

/// 从 XLSX/XLS 文件提取文本
///
/// 遍历所有工作表的所有单元格，将非空单元格的值拼接为文本。
/// 每个 sheet 之间用分隔线隔开。
fn extract_xlsx_text(file_path: &Path) -> Result<String, String> {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if extension == "xls" {
        // .xls 旧格式使用 calamine 直接提取文本
        return extract_xls_text(file_path);
    }

    // .xlsx 新格式使用 umya-spreadsheet 读取
    let book = umya_spreadsheet::reader::xlsx::read(file_path)
        .map_err(|e| format!("读取 Excel 失败 {:?}: {}", file_path.display(), e))?;

    let mut text_buffer = String::new();
    let sheet_count = book.get_sheet_count();
    eprintln!("[Excel] 工作表数量: {}", sheet_count);

    for sheet_idx in 0..sheet_count {
        let sheet = match book.get_sheet(&sheet_idx) {
            Some(s) => s,
            None => continue,
        };

        let sheet_name = sheet.get_name().to_string();

        if !text_buffer.is_empty() {
            text_buffer.push_str("\n\n");
        }
        text_buffer.push_str(&format!("## Sheet: {}\n\n", sheet_name));

        let cells = sheet.get_cell_collection();
        let mut has_content = false;

        // Group cells by row for tabular output
        use std::collections::BTreeMap;
        let mut rows: BTreeMap<u32, BTreeMap<u32, String>> = BTreeMap::new();
        let mut max_col: u32 = 0;

        for cell in cells.iter() {
            let value = cell.get_value().to_string();
            if value.is_empty() {
                continue;
            }
            let coord = cell.get_coordinate();
            let row = *coord.get_row_num();
            let col = *coord.get_col_num();
            rows.entry(row).or_default().insert(col, value);
            if col > max_col {
                max_col = col;
            }
            has_content = true;
        }

        if !has_content {
            text_buffer.push_str("（空工作表）\n");
            continue;
        }

        // Output rows in order
        for (_row, row_cells) in &rows {
            let mut row_texts: Vec<String> = Vec::new();
            for col in 1..=max_col {
                let val = row_cells.get(&col).cloned().unwrap_or_default();
                row_texts.push(val);
            }
            // Trim trailing empty cells
            while row_texts.last().map(|s| s.as_str()) == Some("") {
                row_texts.pop();
            }
            if !row_texts.is_empty() {
                text_buffer.push_str(&row_texts.join("\t"));
                text_buffer.push('\n');
            }
        }
    }

    eprintln!("[Excel] 提取文本长度: {} chars", text_buffer.len());

    if text_buffer.trim().is_empty() {
        return Err(format!("Excel 文件内容为空: {:?}", file_path.display()));
    }

    Ok(text_buffer)
}

/// 使用 calamine 读取旧版 .xls 格式并直接提取文本
fn extract_xls_text(file_path: &Path) -> Result<String, String> {
    use calamine::{Reader, Xls};

    let mut workbook: Xls<_> = calamine::open_workbook(file_path)
        .map_err(|e| format!("打开 XLS 文件失败 {:?}: {}", file_path.display(), e))?;

    let sheet_names = workbook.sheet_names().to_owned();
    eprintln!("[Excel/XLS] 工作表: {:?}", sheet_names);

    let mut text_buffer = String::new();

    for sheet_name in &sheet_names {
        let range = workbook.worksheet_range(sheet_name)
            .map_err(|e| format!("读取工作表 {} 失败: {}", sheet_name, e))?;

        if !text_buffer.is_empty() {
            text_buffer.push_str("\n\n");
        }
        text_buffer.push_str(&format!("## Sheet: {}\n\n", sheet_name));

        let mut has_content = false;

        for row in range.rows() {
            let mut row_texts: Vec<String> = Vec::new();
            for cell in row.iter() {
                // calamine 的 Data 类型直接 to_string 即可
                let value = cell.to_string();
                row_texts.push(value);
            }
            // Trim trailing empty cells
            while row_texts.last().map(|s| s.is_empty()) == Some(true) {
                row_texts.pop();
            }
            if !row_texts.is_empty() {
                text_buffer.push_str(&row_texts.join("\t"));
                text_buffer.push('\n');
                has_content = true;
            }
        }

        if !has_content {
            text_buffer.push_str("（空工作表）\n");
        }
    }

    eprintln!("[Excel/XLS] 提取文本长度: {} chars", text_buffer.len());

    if text_buffer.trim().is_empty() {
        return Err(format!("XLS 文件内容为空: {:?}", file_path.display()));
    }

    Ok(text_buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported() {
        assert!(is_supported(Path::new("test.md")));
        assert!(is_supported(Path::new("test.txt")));
        assert!(is_supported(Path::new("test.pdf")));
        assert!(is_supported(Path::new("test.docx")));
        assert!(is_supported(Path::new("test.xlsx")));
        assert!(is_supported(Path::new("test.html")));
        assert!(is_supported(Path::new("test.htm")));
        assert!(!is_supported(Path::new("test.exe")));
        assert!(!is_supported(Path::new("test.zip")));
    }

    #[test]
    fn test_extract_docx_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:body>
                <w:p>
                    <w:r><w:t>你好</w:t></w:r>
                    <w:r><w:t>世界</w:t></w:r>
                </w:p>
                <w:p>
                    <w:r><w:t>第二段</w:t></w:r>
                </w:p>
            </w:body>
        </w:document>"#;

        let text = extract_text_from_docx_xml(xml).unwrap();
        assert!(text.contains("你好世界"));
        assert!(text.contains("第二段"));
    }
}
