//! 文件内容提取器：根据文件扩展名分派到对应的文本提取策略
//!
//! 支持格式：
//! - Markdown / TXT：直接读取纯文本
//! - HTML：读取后去除标签
//! - PDF：使用 PDF 提取库读取文本
//! - DOC：使用本机 Microsoft Word 自动化提取文本（需要安装 Word）
//! - DOCX：解压压缩包，解析正文 XML 并提取文本
//! - XLSX/XLS：读取每个工作表的单元格文本
//! - VSDX：解压压缩包，解析 Visio 页面 XML 并提取形状文字
//! - VSD：使用本机 Microsoft Visio 自动化提取形状文字（需要安装 Visio）
//! - 图片（PNG/JPG/GIF/BMP/WEBP）：需通过图片处理服务异步识别文本

use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::Path;
use std::process::Command;
use tracing;

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

        // 网页文本：直接读取，后续由清洗模块去除标签
        "html" | "htm" => extract_plain_text(file_path),

        // PDF：使用提取库读取文本
        "pdf" => extract_pdf_text(file_path),

        // Word 文档：新版直接解包，旧版走本机 Word 自动化
        "doc" => super::research_outline::parse_doc_file(file_path),
        "docx" => extract_docx_text(file_path),

        // 表格文档：读取单元格文本
        "xlsx" | "xls" => extract_xlsx_text(file_path),

        // Visio 蓝图：提取形状文本
        "vsdx" => extract_vsdx_text(file_path),
        "vsd" => extract_vsd_text(file_path),

        // 视频和音频需要转写管道处理
        "mp4" | "webm" | "avi" | "mov" | "mkv" | "flv" | "wmv" | "m4a" | "mp3" | "wav" => {
            Err(format!(
                "视频/音频文件 (.{}) 需要通过转写管道处理，请使用视频转写功能",
                extension
            ))
        }

        // 图片格式：需异步 OCR 处理，不能同步提取文本
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" => Err(format!(
            "图片文件 (.{}) 需要通过 ImageProcessor 异步处理",
            extension
        )),

        _ => Err(format!("不支持的文件格式：.{}", extension)),
    }
}

/// 获取所有支持的文件扩展名
pub fn supported_extensions() -> &'static [&'static str] {
    &[
        "md", "txt", "text", "markdown", "html", "htm", "pdf", "doc", "docx", "xlsx", "xls",
        "vsdx", "vsd", "mp4", "webm", "avi", "mov", "mkv", "flv", "wmv", "m4a", "mp3", "wav",
        "png", "jpg", "jpeg", "gif", "bmp", "webp",
    ]
}

/// 检查文件是否为图片格式（需异步 OCR 处理）
pub fn is_image_format(file_path: &Path) -> bool {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp"
    )
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

/// 检查文件是否为需要转写管道处理的视频或音频格式
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

/// 检查文件是否为 Visio 格式
pub fn is_visio_format(file_path: &Path) -> bool {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(extension.as_str(), "vsdx" | "vsd")
}

/// 判断错误是否来自 DOCX 中不可直接读取的内嵌 OLE/Visio 对象。
pub fn is_unreadable_docx_embedded_object_error(error: &str) -> bool {
    error.contains("嵌入 OLE/Visio 对象但无法直接提取文字")
}

/// 提取 DOCX 中的媒体预览图，供异步 OCR/视觉识别使用。
pub fn extract_docx_preview_images(
    file_path: &Path,
    output_dir: &Path,
) -> Result<Vec<std::path::PathBuf>, String> {
    let file = File::open(file_path)
        .map_err(|e| format!("打开 DOCX 失败 {:?}: {}", file_path.display(), e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("解析 DOCX ZIP 失败 {:?}: {}", file_path.display(), e))?;
    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("创建 DOCX 预览图目录失败 {:?}: {}", output_dir.display(), e))?;

    let mut image_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|name| {
            name.starts_with("word/media/")
                && preview_image_extension(name)
                    .map(|extension| {
                        matches!(
                            extension.as_str(),
                            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "emf" | "wmf"
                        )
                    })
                    .unwrap_or(false)
        })
        .collect();
    image_names.sort();

    let mut output_paths = Vec::new();
    for (index, name) in image_names.iter().enumerate() {
        let extension = preview_image_extension(name).unwrap_or_else(|| "png".to_string());
        let raw_path = output_dir.join(format!("docx-preview-{}.{}", index + 1, extension));
        {
            let mut image_file = archive
                .by_name(name)
                .map_err(|e| format!("读取 DOCX 预览图失败 {}: {}", name, e))?;
            let mut bytes = Vec::new();
            image_file
                .read_to_end(&mut bytes)
                .map_err(|e| format!("读取 DOCX 预览图字节失败 {}: {}", name, e))?;
            std::fs::write(&raw_path, bytes)
                .map_err(|e| format!("写入 DOCX 预览图失败 {:?}: {}", raw_path.display(), e))?;
        }

        if matches!(extension.as_str(), "emf" | "wmf") {
            match convert_metafile_to_png(&raw_path) {
                Ok(path) => output_paths.push(path),
                Err(error) => tracing::warn!("转换 DOCX 预览图失败 {:?}: {}", raw_path, error),
            }
        } else {
            output_paths.push(raw_path);
        }
    }

    if output_paths.is_empty() {
        return Err("DOCX 中未找到可用于 OCR 的预览图".to_string());
    }

    Ok(output_paths)
}

// ── 内部实现 ──────────────────────────────────────────────────────────────────

fn preview_image_extension(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
}

fn convert_metafile_to_png(path: &Path) -> Result<std::path::PathBuf, String> {
    let source = path
        .to_str()
        .ok_or_else(|| "预览图路径包含无效字符".to_string())?
        .replace('\'', "''");
    let target_path = path.with_extension("png");
    let target = target_path
        .to_str()
        .ok_or_else(|| "PNG 输出路径包含无效字符".to_string())?
        .replace('\'', "''");
    let script = format!(
        concat!(
            "Add-Type -AssemblyName System.Drawing; ",
            "$source = '{source}'; ",
            "$target = '{target}'; ",
            "$meta = New-Object System.Drawing.Imaging.Metafile($source); ",
            "$bitmap = New-Object System.Drawing.Bitmap($meta.Width, $meta.Height); ",
            "$graphics = [System.Drawing.Graphics]::FromImage($bitmap); ",
            "try {{ ",
            "$graphics.Clear([System.Drawing.Color]::White); ",
            "$graphics.DrawImage($meta, 0, 0, $meta.Width, $meta.Height); ",
            "$bitmap.Save($target, [System.Drawing.Imaging.ImageFormat]::Png); ",
            "}} finally {{ ",
            "$graphics.Dispose(); ",
            "$bitmap.Dispose(); ",
            "$meta.Dispose(); ",
            "}}",
        ),
        source = source,
        target = target
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map_err(|e| format!("启动 PowerShell 转换预览图失败: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "预览图转 PNG 失败: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(target_path)
}

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
/// DOCX 本质是 ZIP 压缩包，正文文本在 word/document.xml 的 <w:t> 节点中。
/// 蓝图文档可能把 Visio 作为嵌入对象放在 word/embeddings 下，需要额外提取。
fn extract_docx_text(file_path: &Path) -> Result<String, String> {
    let file = File::open(file_path)
        .map_err(|e| format!("打开 DOCX 失败 {:?}: {}", file_path.display(), e))?;

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("解析 DOCX ZIP 失败 {:?}: {}", file_path.display(), e))?;

    // 列出 ZIP 内所有文件，帮助诊断
    let file_list: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();
    tracing::debug!("[DOCX] ZIP 内容: {:?}", file_list);

    // 读取 word/document.xml
    let mut document_xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|e| format!("DOCX 中未找到 word/document.xml: {}", e))?
        .read_to_string(&mut document_xml)
        .map_err(|e| format!("读取 document.xml 失败: {}", e))?;

    tracing::debug!("[DOCX] document.xml 长度: {} bytes", document_xml.len());

    let text = extract_text_from_docx_xml(&document_xml)?;
    let embedded_visio_text = extract_embedded_visio_text_from_docx(&mut archive, &file_list)?;
    let has_ole_embedding = file_list.iter().any(|name| {
        name.starts_with("word/embeddings/")
            && (name.ends_with(".bin") || name.to_lowercase().contains("oleobject"))
    });

    let mut combined = String::new();
    if !text.trim().is_empty() {
        combined.push_str(text.trim());
        combined.push('\n');
    }
    if !embedded_visio_text.trim().is_empty() {
        if !combined.trim().is_empty() {
            combined.push('\n');
        }
        combined.push_str("--- 嵌入 Visio 对象 ---\n");
        combined.push_str(embedded_visio_text.trim());
        combined.push('\n');
    }

    tracing::debug!(
        "[DOCX] 提取文本长度: {} chars, 前200字: {:?}",
        combined.len(),
        combined.chars().take(200).collect::<String>()
    );

    if combined.trim().is_empty() {
        if has_ole_embedding {
            return Err(format!(
                "DOCX 正文为空，且检测到嵌入 OLE/Visio 对象但无法直接提取文字: {:?}。请将 Visio 对象另存为 VSDX 后导入，或把蓝图导出为 PDF/图片后走 OCR。",
                file_path.display()
            ));
        }
        return Err(format!("DOCX 文件内容为空: {:?}", file_path.display()));
    }

    Ok(combined)
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
                    let text = e.unescape().map_err(|e| format!("XML 反转义错误: {}", e))?;
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

/// 提取 DOCX 中可解包的嵌入 Visio 对象文字
fn extract_embedded_visio_text_from_docx<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    file_list: &[String],
) -> Result<String, String> {
    let mut text_buffer = String::new();
    let embedding_names: Vec<&String> = file_list
        .iter()
        .filter(|name| name.starts_with("word/embeddings/"))
        .collect();

    for name in embedding_names {
        let lower = name.to_lowercase();
        let mut bytes = Vec::new();
        archive
            .by_name(name)
            .map_err(|e| format!("读取 DOCX 嵌入对象失败 {}: {}", name, e))?
            .read_to_end(&mut bytes)
            .map_err(|e| format!("读取 DOCX 嵌入对象字节失败 {}: {}", name, e))?;

        let extracted = if lower.ends_with(".vsdx") {
            extract_vsdx_text_from_bytes(&bytes).ok()
        } else if lower.ends_with(".bin") || lower.contains("oleobject") {
            extract_vsdx_text_from_ole_payload(&bytes).ok()
        } else {
            None
        };

        if let Some(text) = extracted {
            if !text.trim().is_empty() {
                text_buffer.push_str(&format!("--- {} ---\n", name));
                text_buffer.push_str(text.trim());
                text_buffer.push('\n');
            }
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
    tracing::debug!("[Excel] 工作表数量: {}", sheet_count);

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

        // 按行分组，输出接近表格的文本
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

        // 按行号顺序输出
        for (_row, row_cells) in &rows {
            let mut row_texts: Vec<String> = Vec::new();
            for col in 1..=max_col {
                let val = row_cells.get(&col).cloned().unwrap_or_default();
                row_texts.push(val);
            }
            // 去掉行尾空单元格
            while row_texts.last().map(|s| s.as_str()) == Some("") {
                row_texts.pop();
            }
            if !row_texts.is_empty() {
                text_buffer.push_str(&row_texts.join("\t"));
                text_buffer.push('\n');
            }
        }
    }

    tracing::debug!("[Excel] 提取文本长度: {} chars", text_buffer.len());

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
    tracing::debug!("[Excel/XLS] 工作表: {:?}", sheet_names);

    let mut text_buffer = String::new();

    for sheet_name in &sheet_names {
        let range = workbook
            .worksheet_range(sheet_name)
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
            // 去掉行尾空单元格
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

    tracing::debug!("[Excel/XLS] 提取文本长度: {} chars", text_buffer.len());

    if text_buffer.trim().is_empty() {
        return Err(format!("XLS 文件内容为空: {:?}", file_path.display()));
    }

    Ok(text_buffer)
}

/// 从 VSDX 文件提取形状文本
fn extract_vsdx_text(file_path: &Path) -> Result<String, String> {
    let file = File::open(file_path)
        .map_err(|e| format!("打开 VSDX 失败 {:?}: {}", file_path.display(), e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("解析 VSDX ZIP 失败 {:?}: {}", file_path.display(), e))?;
    extract_vsdx_text_from_archive(&mut archive)
}

fn extract_vsdx_text_from_bytes(bytes: &[u8]) -> Result<String, String> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("解析嵌入 VSDX ZIP 失败: {}", e))?;
    extract_vsdx_text_from_archive(&mut archive)
}

fn extract_vsdx_text_from_ole_payload(bytes: &[u8]) -> Result<String, String> {
    if let Ok(text) = extract_vsdx_text_from_contained_zip(bytes) {
        return Ok(text);
    }
    if let Ok(text) = extract_vsdx_text_from_cfb_payload(bytes) {
        return Ok(text);
    }
    Err("OLE 对象中未找到可解包的 VSDX 内容".to_string())
}

fn extract_vsdx_text_from_contained_zip(bytes: &[u8]) -> Result<String, String> {
    for offset in find_zip_offsets(bytes) {
        if let Ok(text) = extract_vsdx_text_from_bytes(&bytes[offset..]) {
            if !text.trim().is_empty() {
                return Ok(text);
            }
        }
    }
    Err("字节流中未找到可解包的 VSDX 内容".to_string())
}

fn extract_vsdx_text_from_cfb_payload(bytes: &[u8]) -> Result<String, String> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut compound =
        cfb::CompoundFile::open(cursor).map_err(|e| format!("解析 OLE 复合文件失败: {}", e))?;
    let stream_paths: Vec<String> = compound
        .walk()
        .filter(|entry| entry.is_stream())
        .map(|entry| entry.path().to_string_lossy().to_string())
        .collect();

    for path in stream_paths {
        let mut stream_bytes = Vec::new();
        compound
            .open_stream(path.as_str())
            .map_err(|e| format!("打开 OLE 流失败 {}: {}", path, e))?
            .read_to_end(&mut stream_bytes)
            .map_err(|e| format!("读取 OLE 流失败 {}: {}", path, e))?;
        if let Ok(text) = extract_vsdx_text_from_contained_zip(&stream_bytes) {
            if !text.trim().is_empty() {
                return Ok(text);
            }
        }
    }

    Err("OLE 复合文件流中未找到 VSDX 内容".to_string())
}

fn find_zip_offsets(bytes: &[u8]) -> Vec<usize> {
    bytes
        .windows(4)
        .enumerate()
        .filter_map(|(index, window)| {
            if window == [0x50, 0x4B, 0x03, 0x04] {
                Some(index)
            } else {
                None
            }
        })
        .collect()
}

fn extract_vsdx_text_from_archive<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<String, String> {
    let mut page_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|name| {
            name.starts_with("visio/pages/page")
                && name.ends_with(".xml")
                && !name.contains("_rels/")
        })
        .collect();
    page_names.sort();

    if page_names.is_empty() {
        return Err("VSDX 中未找到页面 XML：visio/pages/page*.xml".to_string());
    }

    let mut text_buffer = String::new();
    for (index, name) in page_names.iter().enumerate() {
        let mut xml = String::new();
        archive
            .by_name(name)
            .map_err(|e| format!("读取 VSDX 页面失败 {}: {}", name, e))?
            .read_to_string(&mut xml)
            .map_err(|e| format!("读取 VSDX 页面 XML 失败 {}: {}", name, e))?;
        let page_text = extract_text_from_visio_page_xml(&xml)?;
        if !page_text.trim().is_empty() {
            text_buffer.push_str(&format!("--- page-{} ---\n", index + 1));
            text_buffer.push_str(&page_text);
            text_buffer.push('\n');
        }
    }

    if text_buffer.trim().is_empty() {
        return Err("VSDX 未提取到形状文字；如果蓝图主要是图片，请先导出为 PDF/图片后走 OCR，或在图形中补充文本标注".to_string());
    }

    Ok(text_buffer)
}

/// 从 Visio 页面 XML 中提取 Text 节点内的文本
fn extract_text_from_visio_page_xml(xml: &str) -> Result<String, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut text_buffer = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) if e.name().as_ref() == b"Text" => {
                in_text = true;
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"Text" => {
                in_text = false;
                if !text_buffer.ends_with('\n') {
                    text_buffer.push('\n');
                }
            }
            Ok(Event::Text(e)) if in_text => {
                let text = e
                    .unescape()
                    .map_err(|e| format!("解析 Visio 文本失败: {}", e))?
                    .to_string();
                let text = text.trim();
                if !text.is_empty() {
                    if !text_buffer.ends_with('\n') && !text_buffer.is_empty() {
                        text_buffer.push(' ');
                    }
                    text_buffer.push_str(text);
                }
            }
            Ok(Event::CData(e)) if in_text => {
                let text = String::from_utf8_lossy(e.as_ref()).trim().to_string();
                if !text.is_empty() {
                    if !text_buffer.ends_with('\n') && !text_buffer.is_empty() {
                        text_buffer.push(' ');
                    }
                    text_buffer.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("解析 Visio 页面 XML 失败: {}", e)),
            _ => {}
        }
    }

    Ok(text_buffer)
}

/// 从旧版 VSD 文件提取形状文本
fn extract_vsd_text(file_path: &Path) -> Result<String, String> {
    let path_str = file_path
        .to_str()
        .ok_or_else(|| "VSD 文件路径包含无效字符".to_string())?;
    let escaped = path_str.replace('\'', "''");
    let script = format!(
        concat!(
            "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; ",
            "$OutputEncoding = [System.Text.Encoding]::UTF8; ",
            "$visio = New-Object -ComObject Visio.Application; ",
            "$visio.Visible = $false; ",
            "$doc = $null; ",
            "try {{ ",
            "$doc = $visio.Documents.Open('{path}'); ",
            "$items = New-Object System.Collections.Generic.List[string]; ",
            "foreach ($page in $doc.Pages) {{ ",
            "$items.Add('--- ' + $page.Name + ' ---'); ",
            "foreach ($shape in $page.Shapes) {{ ",
            "if ($shape.Text -and $shape.Text.Trim().Length -gt 0) {{ $items.Add($shape.Text.Trim()) }} ",
            "}} ",
            "}} ",
            "[Console]::Out.Write(($items -join [Environment]::NewLine)); ",
            "}} finally {{ ",
            "if ($doc -ne $null) {{ $doc.Close() | Out-Null; [System.Runtime.Interopservices.Marshal]::ReleaseComObject($doc) | Out-Null; }} ",
            "$visio.Quit(); ",
            "[System.Runtime.Interopservices.Marshal]::ReleaseComObject($visio) | Out-Null; ",
            "[System.GC]::Collect(); ",
            "[System.GC]::WaitForPendingFinalizers(); ",
            "}}",
        ),
        path = escaped
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map_err(|e| format!("启动 PowerShell 提取 VSD 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "VSD 提取失败，需要安装 Microsoft Visio: {}",
            stderr
        ));
    }

    let text =
        String::from_utf8(output.stdout).map_err(|e| format!("VSD 输出不是有效 UTF-8: {}", e))?;
    if text.trim().is_empty() {
        return Err("VSD 未提取到形状文字；如果蓝图主要是图片，请先导出为 PDF/图片后走 OCR，或在图形中补充文本标注".to_string());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    #[test]
    fn test_is_supported() {
        assert!(is_supported(Path::new("test.md")));
        assert!(is_supported(Path::new("test.txt")));
        assert!(is_supported(Path::new("test.pdf")));
        assert!(is_supported(Path::new("test.doc")));
        assert!(is_supported(Path::new("test.docx")));
        assert!(is_supported(Path::new("test.xlsx")));
        assert!(is_supported(Path::new("test.vsdx")));
        assert!(is_supported(Path::new("test.vsd")));
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

    #[test]
    fn test_extract_visio_page_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <PageContents xmlns="http://schemas.microsoft.com/office/visio/2012/main">
            <Shapes>
                <Shape ID="1">
                    <Text>开始<cp IX="0"/>销售订单</Text>
                </Shape>
                <Shape ID="2">
                    <Text>审核</Text>
                </Shape>
            </Shapes>
        </PageContents>"#;

        let text = extract_text_from_visio_page_xml(xml).unwrap();
        assert!(text.contains("开始"));
        assert!(text.contains("销售订单"));
        assert!(text.contains("审核"));
    }

    #[test]
    fn test_extract_vsdx_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blueprint.vsdx");
        std::fs::write(&path, build_vsdx_bytes("蓝图流程")).unwrap();

        let text = extract_vsdx_text(&path).unwrap();
        assert!(text.contains("蓝图流程"));
    }

    #[test]
    fn test_extract_docx_embedded_vsdx_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blueprint.docx");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body /></w:document>"#,
        )
        .unwrap();
        zip.start_file("word/embeddings/visio1.vsdx", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&build_vsdx_bytes("内嵌蓝图流程")).unwrap();
        zip.finish().unwrap();

        let text = extract_docx_text(&path).unwrap();
        assert!(text.contains("嵌入 Visio 对象"));
        assert!(text.contains("内嵌蓝图流程"));
    }

    #[test]
    fn test_extract_docx_embedded_ole_vsdx_payload_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blueprint.docx");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body /></w:document>"#,
        )
        .unwrap();
        zip.start_file(
            "word/embeddings/oleObject1.bin",
            SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"ole-prefix").unwrap();
        zip.write_all(&build_vsdx_bytes("OLE 蓝图流程")).unwrap();
        zip.finish().unwrap();

        let text = extract_docx_text(&path).unwrap();
        assert!(text.contains("OLE 蓝图流程"));
    }

    #[test]
    fn test_extract_docx_embedded_cfb_vsdx_package_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blueprint.docx");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body /></w:document>"#,
        )
        .unwrap();
        zip.start_file(
            "word/embeddings/oleObject1.bin",
            SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&build_cfb_package_bytes("CFB 蓝图流程"))
            .unwrap();
        zip.finish().unwrap();

        let text = extract_docx_text(&path).unwrap();
        assert!(text.contains("CFB 蓝图流程"));
    }

    #[test]
    fn test_extract_docx_unreadable_ole_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blueprint.docx");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body /></w:document>"#,
        )
        .unwrap();
        zip.start_file(
            "word/embeddings/oleObject1.bin",
            SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"unreadable-visio-object").unwrap();
        zip.finish().unwrap();

        let error = extract_docx_text(&path).unwrap_err();
        assert!(error.contains("嵌入 OLE/Visio 对象但无法直接提取文字"));
    }

    #[test]
    fn test_extract_docx_preview_images() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blueprint.docx");
        let output_dir = dir.path().join("preview");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body /></w:document>"#,
        )
        .unwrap();
        zip.start_file("word/media/image1.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"png-bytes").unwrap();
        zip.finish().unwrap();

        let images = extract_docx_preview_images(&path, &output_dir).unwrap();
        assert_eq!(images.len(), 1);
        assert!(images[0].exists());
        assert_eq!(std::fs::read(&images[0]).unwrap(), b"png-bytes");
    }

    fn build_vsdx_bytes(text: &str) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        zip.start_file("visio/pages/page1.xml", SimpleFileOptions::default())
            .unwrap();
        let xml = format!(
            "<PageContents><Shapes><Shape><Text>{}</Text></Shape></Shapes></PageContents>",
            text
        );
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn build_cfb_package_bytes(text: &str) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut compound = cfb::CompoundFile::create(cursor).unwrap();
        compound
            .create_stream("/Package")
            .unwrap()
            .write_all(&build_vsdx_bytes(text))
            .unwrap();
        compound.into_inner().into_inner()
    }
}
