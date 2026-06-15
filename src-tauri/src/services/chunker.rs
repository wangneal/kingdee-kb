//! Recursive chunker: split text respecting document structure and Chinese sentence boundaries.
//!
//! Split hierarchy: H2 headings → paragraphs → Chinese sentences.
//! Small chunks (< MIN_CHUNK_CHARS) are merged with adjacent chunks.
//! Large chunks (> MAX_CHUNK_CHARS) are force-split at sentence boundaries.

use serde::{Deserialize, Serialize};

/// 目标分块字符数（中文近似值，英文实际会更大）
const TARGET_CHUNK_CHARS: usize = 800;
/// 最小分块字符数 — 过小时合并到相邻块
const MIN_CHUNK_CHARS: usize = 160;
/// 子块目标字符数（Small-to-Big 检索，~150 tokens，约 300 中文字符）
const CHILD_TARGET_CHARS: usize = 300;
/// 子块最小字符数 — 低于此值不拆分（保持语义完整性）
const CHILD_MIN_CHARS: usize = 100;
/// 最大分块字符数 — 过大时强制拆分
const MAX_CHUNK_CHARS: usize = 2400;
/// 重叠比例：每个 chunk 前缀包含前一个 chunk 末尾的 15%
/// 业界标准 10-20% 重叠，防止边界上下文丢失
const OVERLAP_RATIO: f32 = 0.15;

/// Chinese sentence separators (in priority order)
const SENTENCE_SEPARATORS: &[&str] = &["。", "！", "？", "；"];

/// Input metadata for chunking
#[derive(Debug, Clone)]
pub struct ChunkInputMeta {
    /// Source file path (optional for pasted text)
    pub source_file: Option<String>,
    /// Document title
    pub title: String,
    /// Tags to attach to all chunks
    pub tags: Vec<String>,
}

/// A single chunk with its content and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    /// Chunk text content
    pub content: String,
    /// Contextual Retrieval 前缀：文档级上下文，嵌入/索引时前置到 content 前
    /// 格式示例："文档《实施方法论》第三章 采购管理"
    /// 业界标准（Anthropic Contextual Retrieval）：上下文前缀可降低 67% 检索失败率
    pub context_prefix: Option<String>,
    /// 子块内容列表（Small-to-Big 检索）
    /// 每个父块拆分为 2-3 个子块（~150 tokens），子块独立嵌入索引，
    /// 检索命中时扩展为父块完整内容。空 Vec 表示未拆分（短文本）。
    pub child_contents: Vec<String>,
    /// Metadata for this chunk
    pub metadata: ChunkMetadata,
}

/// Metadata attached to each chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Source file path
    pub source_file: Option<String>,
    /// Document title
    pub title: String,
    /// Section path (e.g., "第一章 > 第一节")
    pub section_path: Option<String>,
    /// Heading at this chunk's location
    pub heading: Option<String>,
    /// Character offset start in original text
    pub line_start: usize,
    /// Character offset end in original text
    pub line_end: usize,
    /// Tags (from filename, section path, etc.)
    pub tags: Vec<String>,
}

/// Recursively chunk cleaned text into structured pieces.
///
/// Split strategy:
/// 1. Split on `\n## ` (H2 headings) — each section gets its own section_path
/// 2. Within each section, split on `\n\n` (paragraphs)
/// 3. Within each paragraph, split on Chinese sentence separators if still too large
/// 4. Merge small chunks, truncate large ones
pub fn recursive_chunk(text: &str, meta: &ChunkInputMeta) -> Vec<Chunk> {
    if text.trim().is_empty() {
        return vec![];
    }

    let sections = split_by_headings(text);
    let mut all_chunks: Vec<Chunk> = Vec::new();

    for section in sections {
        let section_path = section.heading.as_ref().map(|h| h.clone());
        let section_chunks = chunk_section(
            &section.content,
            meta,
            section_path.as_deref(),
            section.offset,
        );

        for mut chunk in section_chunks {
            if let Some(ref heading) = section.heading {
                chunk.metadata.heading = Some(heading.clone());
                // Build section_path from heading hierarchy
                chunk.metadata.section_path = Some(heading.clone());
            }
            all_chunks.push(chunk);
        }
    }

    // Post-process: merge small chunks, truncate large ones
    merge_small_chunks(&mut all_chunks);
    truncate_large_chunks(&mut all_chunks);

    // Small-to-Big: generate child chunks for each parent chunk
    for chunk in &mut all_chunks {
        chunk.child_contents = generate_child_chunks(&chunk.content);
    }

    all_chunks
}

/// A section split by heading
struct Section {
    heading: Option<String>,
    content: String,
    offset: usize,
}

/// Split text by H2 headings (`\n## `)
fn split_by_headings(text: &str) -> Vec<Section> {
    let mut sections = Vec::new();

    // Find all H2 heading positions
    let mut heading_positions: Vec<(usize, usize, String)> = Vec::new();

    for (i, line) in text.lines().enumerate() {
        let line_offset = text.lines().take(i).map(|l| l.len() + 1).sum::<usize>();
        if line.starts_with("## ") {
            let heading = line.trim_start_matches("## ").trim().to_string();
            heading_positions.push((line_offset, line_offset + line.len() + 1, heading));
        }
    }

    if heading_positions.is_empty() {
        // No headings — treat entire text as one section
        sections.push(Section {
            heading: None,
            content: text.to_string(),
            offset: 0,
        });
        return sections;
    }

    // Content before first heading
    if heading_positions[0].0 > 0 {
        let pre_content = text[..heading_positions[0].0].trim();
        if !pre_content.is_empty() {
            sections.push(Section {
                heading: None,
                content: pre_content.to_string(),
                offset: 0,
            });
        }
    }

    // Split at each heading
    for (idx, &(_, end, ref heading)) in heading_positions.iter().enumerate() {
        let content_start = end;
        let content_end = if idx + 1 < heading_positions.len() {
            heading_positions[idx + 1].0
        } else {
            text.len()
        };

        let content = text[content_start..content_end].trim();
        if !content.is_empty() {
            sections.push(Section {
                heading: Some(heading.clone()),
                content: content.to_string(),
                offset: content_start,
            });
        }
    }

    sections
}

/// 对一个段落缓冲区执行分块，带重叠
///
/// 每个 chunk（除第一个外）的前缀包含前一个 chunk 末尾的 OVERLAP_RATIO 比例文字，
/// 防止语义在 chunk 边界丢失。重叠部分不计算在偏移量中（重叠内容指向原文位置）。
fn chunk_section(
    text: &str,
    meta: &ChunkInputMeta,
    section_path: Option<&str>,
    base_offset: usize,
) -> Vec<Chunk> {
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .collect();
    let mut chunks = Vec::new();

    let mut current_text = String::new();
    let mut current_offset = base_offset;
    // 上一个已输出 chunk 的末尾内容，用作下一个 chunk 的重叠前缀
    let mut overlap_text: Option<String> = None;

    for para in &paragraphs {
        let para_trimmed = para.trim();
        if para_trimmed.is_empty() {
            continue;
        }

        // 计算当前缓冲区的有效长度（不含重叠部分）
        let overlap_len = overlap_text.as_ref().map(|o| o.len() + 2).unwrap_or(0);
        let effective_len = current_text.len().saturating_sub(overlap_len);

        if effective_len + para_trimmed.len() + 2 <= TARGET_CHUNK_CHARS {
            if !current_text.is_empty() {
                current_text.push_str("\n\n");
            }
            current_text.push_str(para_trimmed);
        } else {
            // 输出当前缓冲区为一个 chunk
            if !current_text.is_empty() {
                // 计算重叠文本用于下一个 chunk
                let overlap_chars = (current_text.chars().count() as f32 * OVERLAP_RATIO) as usize;
                let overlap: String = if overlap_chars > 0 {
                    let char_count = current_text.chars().count();
                    let skip = char_count.saturating_sub(overlap_chars);
                    current_text.chars().skip(skip).collect()
                } else {
                    String::new()
                };

                chunks.push(make_chunk(
                    &current_text,
                    meta,
                    section_path,
                    current_offset,
                    current_offset + current_text.len(),
                ));
                current_offset += current_text.len();
                current_text.clear();

                // 为下一个 chunk 设置重叠前缀
                if !overlap.is_empty() {
                    overlap_text = Some(overlap);
                } else {
                    overlap_text = None;
                }
            }

            // 大段落 → 按句子拆分
            if para_trimmed.len() > TARGET_CHUNK_CHARS {
                let sentence_chunks =
                    split_by_sentences(para_trimmed, meta, section_path, current_offset);
                let para_end = current_offset + para_trimmed.len();
                chunks.extend(sentence_chunks);
                current_offset = para_end;
                overlap_text = None; // 句子拆分后重置重叠
            } else {
                // 新缓冲区以重叠前缀开头（如果有的话）
                if let Some(ref overlap) = overlap_text {
                    current_text.push_str(overlap);
                    current_text.push_str("\n\n");
                }
                current_text.push_str(para_trimmed);
            }
        }
    }

    // 输出剩余内容
    if !current_text.is_empty() {
        chunks.push(make_chunk(
            &current_text,
            meta,
            section_path,
            current_offset,
            current_offset + current_text.len(),
        ));
    }

    chunks
}

/// Split a large paragraph by Chinese sentence separators
fn split_by_sentences(
    text: &str,
    meta: &ChunkInputMeta,
    section_path: Option<&str>,
    base_offset: usize,
) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_offset = base_offset;

    // Split on sentence separators while keeping the separator
    let sentences = split_keeping_separator(text, SENTENCE_SEPARATORS);

    for sentence in &sentences {
        if current.len() + sentence.len() > TARGET_CHUNK_CHARS && !current.is_empty() {
            chunks.push(make_chunk(
                &current,
                meta,
                section_path,
                current_offset,
                current_offset + current.len(),
            ));
            current_offset += current.len();
            current.clear();
        }
        current.push_str(sentence);
    }

    if !current.is_empty() {
        chunks.push(make_chunk(
            &current,
            meta,
            section_path,
            current_offset,
            current_offset + current.len(),
        ));
    }

    chunks
}

/// Split text on separators, keeping the separator attached to the preceding fragment
fn split_keeping_separator<'a>(text: &'a str, separators: &[&str]) -> Vec<&'a str> {
    let mut result = Vec::new();
    let mut last_end = 0;

    for (i, _) in text.char_indices() {
        for sep in separators {
            if text[i..].starts_with(sep) {
                let end = i + sep.len();
                if end > last_end {
                    result.push(&text[last_end..end]);
                    last_end = end;
                }
                break;
            }
        }
    }

    if last_end < text.len() {
        result.push(&text[last_end..]);
    }

    if result.is_empty() && !text.is_empty() {
        result.push(text);
    }

    result
}

/// Build a contextual prefix for Contextual Retrieval (Anthropic, 2024).
///
/// Format: "文档《{title}》{section}" to provide document-level context
/// for each chunk during embedding/indexing, reducing retrieval failures by up to 67%.
fn build_context_prefix(title: &str, section_path: Option<&str>) -> Option<String> {
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    let mut prefix = format!("文档《{}》", title);
    if let Some(section) = section_path {
        let section = section.trim();
        if !section.is_empty() {
            prefix.push_str(&format!(" {}章节", section));
        }
    }
    Some(prefix)
}

/// Create a Chunk with metadata
fn make_chunk(
    content: &str,
    meta: &ChunkInputMeta,
    section_path: Option<&str>,
    line_start: usize,
    line_end: usize,
) -> Chunk {
    let context_prefix = build_context_prefix(&meta.title, section_path);
    Chunk {
        content: content.to_string(),
        context_prefix,
        child_contents: Vec::new(), // populated later by generate_child_chunks()
        metadata: ChunkMetadata {
            source_file: meta.source_file.clone(),
            title: meta.title.clone(),
            section_path: section_path.map(|s| s.to_string()),
            heading: None,
            line_start,
            line_end,
            tags: meta.tags.clone(),
        },
    }
}

/// 从父块内容生成子块（Small-to-Big 检索）
///
/// 将父块（~400 tokens）按句子边界拆分为 2-3 个子块（~150 tokens），
/// 子块独立嵌入索引以提高检索精度，命中时扩展为父块完整内容。
/// 短文本（<200 chars）不拆分，返回空 Vec。
fn generate_child_chunks(parent_content: &str) -> Vec<String> {
    let parent_len = parent_content.chars().count();
    // 太短的父块不拆分（无法产生有意义的子块）
    if parent_len < CHILD_MIN_CHARS * 2 {
        return Vec::new();
    }

    let sentences = split_keeping_separator(parent_content, SENTENCE_SEPARATORS);
    if sentences.len() < 2 {
        return Vec::new();
    }

    let mut children = Vec::new();
    let mut current = String::new();

    for sentence in &sentences {
        if current.len() + sentence.len() > CHILD_TARGET_CHARS && !current.is_empty() {
            // 当前句子会使子块过大，先输出当前子块
            if current.chars().count() >= CHILD_MIN_CHARS {
                children.push(current.clone());
            }
            current.clear();
        }
        current.push_str(sentence);
    }

    // 输出最后一个子块
    if !current.is_empty() && current.chars().count() >= CHILD_MIN_CHARS {
        children.push(current);
    }

    // 限制为最多 3 个子块
    children.truncate(3);
    children
}

/// Merge chunks smaller than MIN_CHUNK_CHARS with their neighbor
fn merge_small_chunks(chunks: &mut Vec<Chunk>) {
    if chunks.len() < 2 {
        return;
    }

    let mut i = 0;
    while i < chunks.len() {
        if chunks[i].content.len() < MIN_CHUNK_CHARS {
            // Try to merge with next chunk
            if i + 1 < chunks.len() {
                let separator = "\n\n";
                let merged_content = format!(
                    "{}{}{}",
                    chunks[i].content,
                    separator,
                    chunks[i + 1].content
                );

                if merged_content.len() <= MAX_CHUNK_CHARS {
                    chunks[i].content = merged_content;
                    chunks[i].metadata.line_end = chunks[i + 1].metadata.line_end;
                    chunks.remove(i + 1);
                    continue; // Re-check this index
                }
            }
            // If can't merge forward, try merging with previous
            if i > 0 {
                let separator = "\n\n";
                let merged_content = format!(
                    "{}{}{}",
                    chunks[i - 1].content,
                    separator,
                    chunks[i].content
                );

                if merged_content.len() <= MAX_CHUNK_CHARS {
                    chunks[i - 1].content = merged_content;
                    chunks[i - 1].metadata.line_end = chunks[i].metadata.line_end;
                    chunks.remove(i);
                    continue; // Re-check this index (now points to next)
                }
            }
        }
        i += 1;
    }
}

/// Force-split chunks larger than MAX_CHUNK_CHARS at sentence boundaries
fn truncate_large_chunks(chunks: &mut Vec<Chunk>) {
    let mut i = 0;
    while i < chunks.len() {
        if chunks[i].content.len() > MAX_CHUNK_CHARS {
            let original = chunks[i].content.clone();
            let meta_backup = chunks[i].metadata.clone();
            let original_context = chunks[i].context_prefix.clone();

            // Split at sentence boundaries
            let sentences = split_keeping_separator(&original, SENTENCE_SEPARATORS);
            let mut parts = Vec::new();
            let mut current = String::new();

            for sentence in &sentences {
                if current.len() + sentence.len() > MAX_CHUNK_CHARS && !current.is_empty() {
                    parts.push(current.clone());
                    current.clear();
                }
                current.push_str(sentence);
            }
            if !current.is_empty() {
                parts.push(current);
            }

            if parts.len() > 1 {
                // Replace original chunk with split parts
                chunks.remove(i);
                for (j, part) in parts.iter().enumerate() {
                    let mut chunk = Chunk {
                        content: part.clone(),
                        context_prefix: original_context.clone(),
                        child_contents: Vec::new(),
                        metadata: meta_backup.clone(),
                    };
                    // Adjust offsets
                    chunk.metadata.line_start = meta_backup.line_start;
                    chunk.metadata.line_end = meta_backup.line_start + part.len();
                    chunks.insert(i + j, chunk);
                }
                i += parts.len();
                continue;
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_meta() -> ChunkInputMeta {
        ChunkInputMeta {
            source_file: Some("test.md".to_string()),
            title: "测试文档".to_string(),
            tags: vec!["test".to_string()],
        }
    }

    #[test]
    fn test_chunk_empty() {
        let chunks = recursive_chunk("", &test_meta());
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_short_text() {
        let text = "这是一段短文本，不需要分块。";
        let chunks = recursive_chunk(text, &test_meta());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, text);
    }

    #[test]
    fn test_chunk_by_heading() {
        // Each chunk must be > 160 chars (MIN_CHUNK_CHARS) to avoid merging
        let text = "前言部分。这里是一段相当长的文字，主要是为了增加字符数量。我们需要确保每个段落的字符总数都能够大于一百六十个字符，这样分块算法在执行合并步骤时，才不会把它们强制合并成一个大段落。这里是一段相当长的文字，主要是为了增加字符数量。我们需要确保每个段落的字符总数都能够大于一百六十个字符，这样分块算法在执行合并步骤时，才不会把它们强制合并成一个大段落。\n\n\
                    ## 第一章\n\n\
                    第一章内容。这里是一段相当长的文字，主要是为了增加字符数量。我们需要确保每个段落的字符总数都能够大于一百六十个字符，这样分块算法在执行合并步骤时，才不会把它们强制合并成一个大段落。这里是一段相当长的文字，主要是为了增加字符数量。我们需要确保每个段落的字符总数都能够大于一百六十个字符，这样分块算法在执行合并步骤时，才不会把它们强制合并成一个大段落。\n\n\
                    ## 第二章\n\n\
                    第二章内容。这里是一段相当长的文字，主要是为了增加字符数量。我们需要确保每个段落的字符总数都能够大于一百六十个字符，这样分块算法在执行合并步骤时，才不会把它们强制合并成一个大段落。这里是一段相当长的文字，主要是为了增加字符数量。我们需要确保每个段落的字符总数都能够大于一百六十个字符，这样分块算法在执行合并步骤时，才不会把它们强制合并成一个大段落。";
        let chunks = recursive_chunk(text, &test_meta());
        // Should have at least 2 chunks (one per section after heading)
        assert!(
            chunks.len() >= 2,
            "Expected >= 2 chunks, got {}",
            chunks.len()
        );
        // Check section paths are extracted
        let headings: Vec<Option<&String>> =
            chunks.iter().map(|c| c.metadata.heading.as_ref()).collect();
        assert!(headings.iter().any(|h| h.is_some()));
    }

    #[test]
    fn test_chunk_paragraph_split() {
        let mut text = String::new();
        for i in 0..10 {
            text.push_str(&format!(
                "这是第{}段落，包含足够的文字来进行分块测试。每个段落都有一定的长度。\n\n",
                i
            ));
        }
        let chunks = recursive_chunk(&text, &test_meta());
        assert!(chunks.len() > 1);
    }

    #[test]
    fn test_chunk_chinese_sentences() {
        // A very long paragraph that should be split by sentence separators
        let mut text = String::new();
        for i in 0..20 {
            text.push_str(&format!("这是第{}个句子，包含中文标点符号。", i));
        }
        let chunks = recursive_chunk(&text, &test_meta());
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn test_chunk_metadata_preserved() {
        let text = "## 章节标题\n\n这是章节内容。";
        let chunks = recursive_chunk(text, &test_meta());
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].metadata.title, "测试文档");
        assert_eq!(chunks[0].metadata.source_file, Some("test.md".to_string()));
        assert_eq!(chunks[0].metadata.tags, vec!["test"]);
    }

    #[test]
    fn test_merge_small_chunks() {
        // Create text that produces small chunks
        let text = "短。\n\n也短。";
        let chunks = recursive_chunk(text, &test_meta());
        // Small chunks should be merged
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_chunk_long_chinese_text() {
        // Simulate a real Chinese document
        let mut text = "# 金蝶ERP系统\n\n".to_string();
        text.push_str("## 期货点价管理\n\n");
        for i in 0..30 {
            text.push_str(&format!(
                "期货点价是金蝶ERP系统中的重要功能模块。在第{}步操作中，用户需要在系统中配置相关参数。\
                 这些参数包括价格策略、交割方式、结算周期等。每个参数都需要仔细配置以确保业务流程的正确性。\n\n",
                i
            ));
        }
        let chunks = recursive_chunk(&text, &test_meta());
        // Should produce multiple chunks
        assert!(chunks.len() > 1);
        // All chunks should have reasonable size
        for chunk in &chunks {
            assert!(
                chunk.content.len() >= 10,
                "Chunk too small: {}",
                chunk.content.len()
            );
            // After merge, chunks should be within reasonable bounds
            // (may exceed MAX_CHUNK_CHARS if a single sentence is very long, which is acceptable)
        }
    }

    #[test]
    fn test_split_keeping_separator() {
        let text = "句子一。句子二！句子三？";
        let parts = split_keeping_separator(text, SENTENCE_SEPARATORS);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "句子一。");
        assert_eq!(parts[1], "句子二！");
        assert_eq!(parts[2], "句子三？");
    }
}
