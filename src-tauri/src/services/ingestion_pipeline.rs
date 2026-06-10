//! 两步摄入集成管道 — Step 1 分析 + Step 2 LLM 编译
//!
//! 将文档分析（AnalysisOrchestrator）与知识库编译（LLM 生成 wiki_pages）串联，
//! 并通过 ingest_cache 实现增量缓存（project_id + source_identity + sha256 三元组）。

use regex::Regex;
use serde::Serialize;
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use tracing::{info, warn};

// ─── 静态正则（LazyLock，避免每次调用重新编译） ───

/// 匹配 yaml 代码块 ```yaml ... ```
static RE_YAML_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)```yaml\s*\n?(.*?)```").expect("编译正则失败: RE_YAML_BLOCK")
});

/// 匹配 markdown 代码块 ```markdown ... ```
static RE_MD_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)```markdown\s*\n?(.*?)```").expect("编译正则失败: RE_MD_BLOCK")
});

use crate::services::analysis_cache::AnalysisCacheStore;
use crate::services::document_analysis::{AnalysisOrchestrator, DocumentAnalysis, EngineType};
use crate::services::ingest_cache::{CreateIngestCache, IngestCacheStore};
use crate::services::llm_providers::LLMProviderManager;
use crate::services::verification::pipeline::VerificationPipeline;
use crate::services::verification::types::{ScenarioType, VerificationInput};
use crate::services::wiki_page::{CreateWikiPage, UpdateWikiPage, WikiPageStore};

/// 两步摄入编译结果
#[derive(Debug, Clone, Serialize)]
pub struct KbCompilationResult {
    /// Step 1 文档分析结果
    pub analysis: DocumentAnalysis,
    /// 使用的分析引擎标识
    pub engine: String,
    /// 是否命中 ingest_cache（完全跳过）
    pub cache_hit: bool,
    /// 生成的 wiki 页面 slug 列表
    pub generated_pages: Vec<String>,
    /// 是否成功执行了 Step 2 编译
    pub compilation_done: bool,
}

// ─── 主入口 ───

/// 执行两步摄入流水线
///
/// 流程：
/// 1. 检查 ingest_cache（project_id + source_identity + sha256）→ 命中则跳过
/// 2. 通过 AnalysisOrchestrator 执行 Step 1 文档分析
/// 3. 若 enable_kb_compilation 为 true，执行 Step 2 LLM 编译生成 wiki_pages
/// 4. 将生成的 wiki 页面 slug 列表写入 ingest_cache
pub async fn process_with_kb_compilation(
    text: &str,
    source_identity: &str,
    sha256: &str,
    project_id: i64,
    title: &str,
    enable_kb_compilation: bool,
    cache_store: Arc<Mutex<AnalysisCacheStore>>,
    provider_manager: Arc<RwLock<LLMProviderManager>>,
    wiki_pages: Arc<Mutex<WikiPageStore>>,
    ingest_cache_store: Arc<Mutex<IngestCacheStore>>,
    document_id: Option<i64>,
    force_recompile: bool,
) -> Result<KbCompilationResult, String> {
    // Step 0: 检查 ingest_cache（force_recompile=true 时跳过，用于"删 wiki 后重生成"场景）
    if !force_recompile {
        if let Some(cached) =
            check_ingest_cache(&ingest_cache_store, project_id, source_identity, sha256)?
        {
            info!("ingest_cache 命中: source={}", source_identity);
            return Ok(KbCompilationResult {
                analysis: DocumentAnalysis {
                    source_identity: source_identity.to_string(),
                    sha256: sha256.to_string(),
                    title: title.to_string(),
                    headings: Vec::new(),
                    keywords: Vec::new(),
                    word_count: 0,
                    char_count: 0,
                    language: String::new(),
                    entities: Vec::new(),
                    key_concepts: Vec::new(),
                    cross_references: Vec::new(),
                    contradictions: Vec::new(),
                },
                engine: "cache".to_string(),
                cache_hit: true,
                generated_pages: cached,
                compilation_done: true,
            });
        }
    }

    // Step 1: 文档分析（同时用于双引擎降级 + analysis_cache）
    let orchestrator = AnalysisOrchestrator::new(cache_store, provider_manager.clone());
    let analysis_result = orchestrator
        .analyze(
            project_id,
            source_identity,
            sha256,
            text,
            enable_kb_compilation,
        )
        .await;

    let engine_label = match analysis_result.engine {
        EngineType::Llm => "llm",
        EngineType::Rust => "rust",
    };

    let mut generated_pages: Vec<String> = Vec::new();
    let mut compilation_done = false;

    // Step 2: LLM 知识库编译
    if enable_kb_compilation {
        match run_llm_compilation(
            &analysis_result.analysis,
            project_id,
            &[],
            &wiki_pages,
            &provider_manager,
            document_id,
        )
        .await
        {
            Ok(slugs) => {
                generated_pages = slugs;
                compilation_done = true;
                info!(
                    "LLM 编译完成: source={}, pages={:?}",
                    source_identity, generated_pages
                );
            }
            Err(e) => {
                warn!("LLM 编译失败: {}，跳过 Step 2", e);
            }
        }
    }

    // Step 2.5: 验证编译结果
    if compilation_done && !generated_pages.is_empty() {
        let verifier = VerificationPipeline::default_with_all();
        for slug in &generated_pages {
            if let Ok(Some(page)) = wiki_pages
                .lock()
                .map_err(|e| e.to_string())
                .and_then(|store| {
                    store
                        .get_by_slug(project_id, slug)
                        .map_err(|e| e.to_string())
                })
            {
                let input = VerificationInput {
                    generated_text: page.content_candidate.clone().unwrap_or_default(),
                    retrieved_chunks: vec![],
                    chunk_titles: vec![],
                    available_chunk_ids: vec![],
                    query: format!("知识编译验证: {}", page.title),
                    scenario: ScenarioType::KnowledgeCompilation,
                };
                let report = verifier.verify(&input).await;
                tracing::info!(
                    "编译验证: slug={}, level={:?}, confidence={}",
                    slug,
                    report.level,
                    report.overall_confidence
                );
                if report.level == crate::services::verification::types::VerificationLevel::Failed {
                    tracing::warn!(
                        "编译验证未通过: slug={}, detail={:?}",
                        slug,
                        report.suggested_labels
                    );
                }
            }
        }
    }

    // Step 3: 仅在编译成功时更新 ingest_cache（失败时不写缓存，以便下次重试）
    if compilation_done {
        update_ingest_cache(
            &ingest_cache_store,
            project_id,
            source_identity,
            sha256,
            &generated_pages,
        )?;
    } else {
        tracing::info!("LLM 编译未完成，跳过 ingest_cache 更新，下次导入将重试");
    }

    Ok(KbCompilationResult {
        analysis: analysis_result.analysis,
        engine: engine_label.to_string(),
        cache_hit: false,
        generated_pages,
        compilation_done,
    })
}

// ─── Step 2: LLM 知识库编译 ───

/// 根据文档分析结果，通过 LLM 生成 wiki 页面内容并写入 content_candidate
async fn run_llm_compilation(
    analysis: &DocumentAnalysis,
    project_id: i64,
    chunk_ids: &[i64],
    wiki_pages: &Arc<Mutex<WikiPageStore>>,
    provider_manager: &Arc<RwLock<LLMProviderManager>>,
    document_id: Option<i64>,
) -> Result<Vec<String>, String> {
    let page_title = if analysis.title.is_empty() {
        analysis.source_identity.clone()
    } else {
        analysis.title.clone()
    };
    let page_slug = slugify(&page_title);

    let prompt = build_compilation_prompt(analysis);

    let (generated_content, generated_tags) =
        call_llm_for_compilation(&prompt, &page_title, provider_manager).await?;

    let sources_json = serde_json::json!([{
        "source_id": serde_json::Value::Null,
        "document_id": document_id,
        "chunks": chunk_ids,
    }])
    .to_string();

    let slug = write_or_update_wiki_page(
        wiki_pages,
        project_id,
        &page_slug,
        &page_title,
        &generated_content,
        &generated_tags,
        Some(sources_json),
    )?;

    Ok(vec![slug])
}

/// 构造 LLM 编译提示词（从 DocumentAnalysis 生成 wiki 页面内容）
fn build_compilation_prompt(analysis: &DocumentAnalysis) -> String {
    let keywords_str = analysis
        .keywords
        .iter()
        .map(|k| k.keyword.as_str())
        .collect::<Vec<&str>>()
        .join("、");

    let concepts_str = analysis.key_concepts.join("、");
    let entities_str = analysis.entities.join("、");

    let cross_refs_str: String = analysis
        .cross_references
        .iter()
        .map(|r| format!("- {} ({}, {})", r.target, r.ref_type, r.context))
        .collect::<Vec<String>>()
        .join("\n");

    let headings_str: String = analysis
        .headings
        .iter()
        .map(|h| format!("{}. {}", h.level, h.text))
        .collect::<Vec<String>>()
        .join("\n");

    format!(
        r#"你是一个知识库维基页面生成专家。请根据以下文档分析结果，生成一篇维基百科风格的页面内容。

## 文档分析结果

标题：{title}
关键词：{keywords}
关键概念：{concepts}
命名实体：{entities}
字数：{word_count}
语言：{language}

## 标题结构
{headings}

## 交叉引用
{cross_refs}

## 输出要求

1. 生成标准 Markdown 格式内容，以 YAML 前置元数据开头
2. 第一段必须是页面概述（200 字以内）
3. 随后展开详细内容，引用文档中的要点
4. 输出格式（严格按此结构，``` 不可省略）：

```yaml
tags: [标签1, 标签2, 标签3]
```

```markdown
## 概述

（概述内容）

## 正文

（详细展开内容）
```"#,
        title = analysis.title,
        keywords = keywords_str,
        concepts = concepts_str,
        entities = entities_str,
        word_count = analysis.word_count,
        language = analysis.language,
        headings = headings_str,
        cross_refs = cross_refs_str,
    )
}

/// 调用 LLM 生成 wiki 页面内容，返回 (markdown_content, tags_json)
async fn call_llm_for_compilation(
    prompt: &str,
    _page_title: &str,
    provider_manager: &Arc<RwLock<LLMProviderManager>>,
) -> Result<(String, String), String> {
    // 获取默认的 LLM 供应商配置，避免硬编码直接发起请求
    let provider_config = {
        let mgr = provider_manager
            .read()
            .map_err(|e| format!("供应商管理器读取失败: {}", e))?;
        let provider = mgr
            .get_default_provider()
            .ok_or_else(|| "未配置默认 LLM 供应商".to_string())?;
        if !provider.is_configured() {
            return Err("默认 LLM 供应商未完成配置".to_string());
        }
        provider.clone()
    };

    // 使用系统统一封装的大模型服务执行请求
    // 统一复用协议路由、证书兼容、重试和密钥轮转逻辑
    let llm_service = crate::services::llm_service::LLMService::new(provider_manager.clone());

    let messages = vec![
        crate::services::llm_service::ChatMessage {
            role: "system".to_string(),
            content: "你是一个知识库维基页面生成专家。严格按照输出格式生成内容。".to_string(),
        },
        crate::services::llm_service::ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        },
    ];

    let content = llm_service
        .chat_completion(&messages, &provider_config)
        .await
        .map_err(|e| {
            format!(
                "LLM 编译请求失败: provider={}, protocol={:?}, base_url={}, model={}, error={}",
                provider_config.name,
                provider_config.protocol,
                provider_config.base_url,
                provider_config.get_default_model_name(),
                e
            )
        })?;

    // 解析响应：提取 tags 和正文
    let (tags_str, markdown_body) = parse_compilation_response(&content);
    let tags_json = normalize_tags_json(&tags_str)?;

    Ok((markdown_body, tags_json))
}

/// 解析 LLM 编译响应，提取 tags 和 markdown 正文
fn parse_compilation_response(text: &str) -> (String, String) {
    let text = text.trim();

    // 尝试提取 ```yaml ... ``` 块中的 tags
    let tags = if let Some(cap) = RE_YAML_BLOCK.captures(text) {
        let yaml_block = cap[1].trim();
        // 从 YAML 中提取 tags 行
        if let Some(tags_line) = yaml_block.lines().find(|l| l.trim().starts_with("tags:")) {
            tags_line.trim_start_matches("tags:").trim().to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // 提取 ```markdown ... ``` 块中的正文
    let body = if let Some(cap) = RE_MD_BLOCK.captures(text) {
        cap[1].trim().to_string()
    } else {
        // 退回到直接取全部文本
        text.to_string()
    };

    (tags, body)
}

/// 将 LLM 返回的 YAML 标签列表转换为持久化 JSON 字符串数组
fn normalize_tags_json(tags: &str) -> Result<String, String> {
    let trimmed = tags.trim();
    if trimmed.is_empty() {
        return Ok("[]".to_string());
    }

    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| format!("tags 必须是数组格式: {}", tags))?;

    let values: Vec<String> = inner
        .split(',')
        .map(|item| item.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|item| !item.is_empty())
        .collect();

    serde_json::to_string(&values).map_err(|e| format!("序列化 tags 失败: {}", e))
}

// ─── Wiki 页面写入 ───

/// 创建或更新 wiki 页面（写入 content_candidate，不直接修改 content）
fn write_or_update_wiki_page(
    wiki_pages: &Arc<Mutex<WikiPageStore>>,
    project_id: i64,
    slug: &str,
    title: &str,
    content: &str,
    tags: &str,
    sources: Option<String>,
) -> Result<String, String> {
    let store = wiki_pages
        .lock()
        .map_err(|e| format!("wiki_pages 锁失败: {}", e))?;

    // 检查页面是否已存在
    let existing = store.get_by_slug(project_id, slug)?;

    let final_slug = slug.to_string();

    if let Some(page) = existing {
        // 已有页面：计算 diff，设置 candidate_status
        let existing_content = if page.content.is_empty() {
            page.content_candidate.as_deref().unwrap_or("")
        } else {
            &page.content
        };

        let diff = calculate_diff_ratio(content, existing_content);
        let candidate_status = if existing_content.is_empty() {
            "pending"
        } else if diff <= 0.30 {
            "auto"
        } else {
            "conflict"
        };

        store.update(
            page.id,
            &UpdateWikiPage {
                // 设计要求：title/page_type/created_at 永不覆盖
                title: None,
                content: None,
                content_candidate: Some(content.to_string()),
                candidate_status: Some(candidate_status.to_string()),
                sources_candidate: sources.clone(),
                frontmatter: None,
                // 关键：候选阶段不覆盖正式 sources，只写 sources_candidate。
                // 理由：sources 是文档级引用元数据（指向 documents/chunks/vectors），
                // 与 wiki 正文版（content）一一对应。若在候选阶段就覆盖 sources：
                // 1. 拒绝候选时无法恢复（reject 只清候选字段）→ 元数据不一致
                // 2. 候选与已批准内容会暂时指向不同 documents → 中间态易错乱
                // 正确做法：sources 在 approve_candidate 时与 content 一起提交；拒绝时 sources 不变。
                sources: None,
                wikilinks: None,
                tags: None,
                page_metadata: None,
                candidate_version: Some(page.version + 1),
                page_status: None,
            },
        )?;
    } else {
        // 新页面：创建并设置 content_candidate + candidate_status
        store.create(&CreateWikiPage {
            project_id,
            slug: slug.to_string(),
            title: title.to_string(),
            page_type: "summary".to_string(),
            content: String::new(),
            frontmatter: Some("{}".to_string()),
            sources: sources.clone().or(Some("[]".to_string())),
            wikilinks: Some("[]".to_string()),
            tags: Some(tags.to_string()),
            page_metadata: Some("{}".to_string()),
            page_status: Some("draft".to_string()),
        })?;

        // 创建后立即更新 content_candidate
        // candidate_version 必须为 version + 1（满足 CHECK 约束）
        if let Some(new_page) = store.get_by_slug(project_id, slug)? {
            store.update(
                new_page.id,
                &UpdateWikiPage {
                    title: None,
                    content: None,
                    content_candidate: Some(content.to_string()),
                    candidate_status: Some("pending".to_string()),
                    sources_candidate: sources.clone(),
                    frontmatter: None,
                    sources: None,
                    wikilinks: None,
                    tags: None,
                    page_metadata: None,
                    candidate_version: Some(new_page.version + 1),
                    page_status: None,
                },
            )?;
        }
    }

    Ok(final_slug)
}

// ─── 差异计算 ───

/// 计算两个文本之间的差异比例（0.0 = 完全相同，1.0 = 完全不同）
///
/// 使用 UTF-8 字符级编辑距离（Levenshtein），按规格要求计算字符差异比例。
/// 对于超过 5000 字符的大文本，使用近似算法避免性能开销。
fn calculate_diff_ratio(new: &str, existing: &str) -> f64 {
    if existing.is_empty() {
        return 1.0;
    }
    if new == existing {
        return 0.0;
    }

    let new_chars: Vec<char> = new.chars().collect();
    let existing_chars: Vec<char> = existing.chars().collect();

    let max_len = new_chars.len().max(existing_chars.len());
    if max_len == 0 {
        return 1.0;
    }

    let distance = char_levenshtein(&new_chars, &existing_chars);
    distance as f64 / max_len as f64
}

/// 计算两个字符序列的 Levenshtein 编辑距离（滚动数组优化）
fn char_levenshtein(a: &[char], b: &[char]) -> usize {
    let m = a.len();
    let n = b.len();

    // 大文本使用快速近似：以最长公共前缀/后缀为锚点
    if m.max(n) > 5000 {
        let common = a.iter().zip(b.iter()).filter(|(x, y)| x == y).count();
        return a.len().max(b.len()) - common;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1) // 删除
                .min(curr[j - 1] + 1) // 插入
                .min(prev[j - 1] + cost); // 替换
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

// ─── Slug 生成 ───

/// 将标题转为 URL 安全的 slug
fn slugify(text: &str) -> String {
    let slug: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || (c as u32) > 0x7F {
                // 保留字母、数字、中文等字符
                c.to_ascii_lowercase()
            } else if c.is_whitespace() || c == '-' || c == '_' {
                '-'
            } else {
                '-'
            }
        })
        .collect();

    // 合并连续短横线并去除首尾
    let mut result = String::with_capacity(slug.len());
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push(c);
                prev_hyphen = true;
            }
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    result.trim_matches('-').to_string()
}

// ─── Ingest Cache 辅助 ───

/// 检查 ingest_cache，若命中返回已写入的文件列表
fn check_ingest_cache(
    store: &Arc<Mutex<IngestCacheStore>>,
    project_id: i64,
    source_identity: &str,
    sha256: &str,
) -> Result<Option<Vec<String>>, String> {
    let cache = store
        .lock()
        .map_err(|e| format!("ingest_cache 锁失败: {}", e))?;

    match cache.get_by_key(project_id, source_identity, sha256)? {
        Some(entry) if !entry.files_written.is_empty() => {
            let files: Vec<String> = serde_json::from_str(&entry.files_written).unwrap_or_default();
            if files.is_empty() {
                Ok(None)
            } else {
                Ok(Some(files))
            }
        }
        _ => Ok(None),
    }
}

/// 更新 ingest_cache 记录
fn update_ingest_cache(
    store: &Arc<Mutex<IngestCacheStore>>,
    project_id: i64,
    source_identity: &str,
    sha256: &str,
    files: &[String],
) -> Result<(), String> {
    let files_json = serde_json::to_string(files).unwrap_or_else(|_| "[]".to_string());

    let cache = store
        .lock()
        .map_err(|e| format!("ingest_cache 锁失败: {}", e))?;

    let input = CreateIngestCache {
        project_id,
        source_identity: source_identity.to_string(),
        sha256: sha256.to_string(),
        files_written: Some(files_json),
    };

    cache.upsert(&input)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_simple() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn test_slugify_chinese() {
        let result = slugify("金蝶云星空用户手册");
        assert!(result.contains("金蝶云星空用户手册"));
    }

    #[test]
    fn test_slugify_special_chars() {
        let result = slugify("ERP系统 V2.0-用户指南");
        assert!(result.contains("erp系统"));
        assert!(result.contains("v2"));
    }

    #[test]
    fn test_slugify_empty() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn test_diff_ratio_identical() {
        let text = "这是一段测试文本。";
        assert_eq!(calculate_diff_ratio(text, text), 0.0);
    }

    #[test]
    fn test_diff_ratio_completely_different() {
        let ratio = calculate_diff_ratio("abcdef", "xyz123");
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_diff_ratio_partial() {
        let a = "金蝶云星空ERP系统提供了财务管理功能";
        let b = "金蝶云星空ERP系统提供了供应链管理功能";
        let ratio = calculate_diff_ratio(a, b);
        assert!(ratio > 0.0);
        assert!(ratio < 1.0);
    }

    #[test]
    fn test_diff_ratio_empty_existing() {
        assert_eq!(calculate_diff_ratio("新内容", ""), 1.0);
    }

    #[test]
    fn test_parse_compilation_response_with_tags() {
        let input = "一些前文\n```yaml\ntags: [ERP, 财务, 管理]\n```\n```markdown\n## 概述\n测试内容\n```\n后文";
        let (tags, body) = parse_compilation_response(input);
        assert_eq!(tags, "[ERP, 财务, 管理]");
        assert!(body.contains("概述"));
        assert!(body.contains("测试内容"));
    }

    #[test]
    fn test_normalize_tags_json_yaml_array() {
        let tags = normalize_tags_json("[ERP, 财务, 管理]").unwrap();
        assert_eq!(tags, r#"["ERP","财务","管理"]"#);
    }

    #[test]
    fn test_normalize_tags_json_rejects_non_array() {
        assert!(normalize_tags_json("ERP, 财务").is_err());
    }

    #[test]
    fn test_parse_compilation_response_no_tags() {
        let input = "```markdown\n## 概述\n纯正文\n```";
        let (tags, body) = parse_compilation_response(input);
        assert_eq!(tags, "");
        assert!(body.contains("纯正文"));
    }

    // 单元测试说明：Issue 2 修复后，sources_has_valid_document_id + 8 个相关测试已移除
    // （设计决策改为 sources 永远不通过候选路径覆盖）
}
