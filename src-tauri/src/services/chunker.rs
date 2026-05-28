//! Recursive chunker: split text respecting document structure and Chinese sentence boundaries.
//!
//! Split hierarchy: H2 headings → paragraphs → Chinese sentences.
//! Small chunks (< MIN_CHUNK_CHARS) are merged with adjacent chunks.
//! Large chunks (> MAX_CHUNK_CHARS) are force-split at sentence boundaries.

use serde::{Deserialize, Serialize};

/// Target chunk size in characters (~384 tokens ≈ 500-700 chars for Chinese)
const TARGET_CHUNK_CHARS: usize = 600;
/// Minimum chunk size — merge with adjacent if smaller
const MIN_CHUNK_CHARS: usize = 100;
/// Maximum chunk size — force split if larger
const MAX_CHUNK_CHARS: usize = 1500;

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

/// Chunk a single section by paragraphs, then by sentences
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

    for para in &paragraphs {
        let para_trimmed = para.trim();
        if para_trimmed.is_empty() {
            continue;
        }

        if current_text.len() + para_trimmed.len() + 2 <= TARGET_CHUNK_CHARS {
            if !current_text.is_empty() {
                current_text.push_str("\n\n");
            }
            current_text.push_str(para_trimmed);
        } else {
            // Flush current buffer
            if !current_text.is_empty() {
                chunks.push(make_chunk(
                    &current_text,
                    meta,
                    section_path,
                    current_offset,
                    current_offset + current_text.len(),
                ));
                current_offset += current_text.len();
                current_text.clear();
            }

            // If paragraph itself is too large, split by sentences
            if para_trimmed.len() > TARGET_CHUNK_CHARS {
                let sentence_chunks =
                    split_by_sentences(para_trimmed, meta, section_path, current_offset);
                let para_end = current_offset + para_trimmed.len();
                chunks.extend(sentence_chunks);
                current_offset = para_end;
            } else {
                current_text = para_trimmed.to_string();
            }
        }
    }

    // Flush remaining
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

/// Create a Chunk with metadata
fn make_chunk(
    content: &str,
    meta: &ChunkInputMeta,
    section_path: Option<&str>,
    line_start: usize,
    line_end: usize,
) -> Chunk {
    Chunk {
        content: content.to_string(),
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
        // Each chunk must be > 100 bytes (MIN_CHUNK_CHARS) to avoid merging
        // Chinese chars are 3 bytes each, so need ~35+ chars per section
        let text = "前言内容，这里是一段较长的文字用于测试分块功能，需要足够长才能避免被合并。\n\n## 第一章\n\n第一章的内容，这里是一段较长的文字用于测试分块功能，需要足够长才能避免被合并。\n\n## 第二章\n\n第二章的内容，这里是一段较长的文字用于测试分块功能，需要足够长才能避免被合并。";
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
