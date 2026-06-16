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

/// 匹配 markdown 代码块 ```markdown ... ``` 或 LLM 简写 ```md ... ```
static RE_MD_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)```(?:markdown|md)\s*\n?(.*?)```")
        .expect("编译正则失败: RE_MD_BLOCK")
});

use crate::services::analysis_cache::AnalysisCacheStore;
use crate::services::document_analysis::{AnalysisOrchestrator, DocumentAnalysis, EngineType};
use crate::services::ingest_cache::{CreateIngestCache, IngestCacheStore};
use crate::services::llm_providers::LLMProviderManager;
use crate::services::verification::pipeline::VerificationPipeline;
use crate::services::verification::types::{ScenarioType, VerificationInput};
use crate::services::wiki_page::{CreateWikiPageWithCandidate, WikiPageStore};

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

/// 执行 KB 编译并返回 (engine, error) 元组。
///
/// 封装完整的两步编译流程：读取配置、自动提取参数、调用底层分析与编译、缓存更新与软错误处理。
/// 供主路径导入与后台队列导入复用，消除编排重复。
pub async fn run_kb_compilation_flow(
    state: &crate::app_state::AppState,
    text: &str,
    source_identity: &str,
    sha256: &str,
    project_id: i64,
    title: &str,
    document_id: i64,
    enable_kb_compilation: Option<bool>,
    force_recompile: bool,
) -> (Option<String>, Option<String>) {
    let kb_enabled = if let Some(v) = enable_kb_compilation {
        v
    } else {
        state
            .metadata
            .lock()
            .ok()
            .and_then(|m| m.get_kb_compilation_enabled().ok())
            .unwrap_or(false)
    };

    if !kb_enabled {
        return (None, None);
    }

    match process_with_kb_compilation(
        text,
        source_identity,
        sha256,
        project_id,
        title,
        true,
        state.analysis_cache.clone(),
        state.llm_providers.clone(),
        state.wiki_pages.clone(),
        state.ingest_cache_store.clone(),
        Some(document_id),
        force_recompile,
    )
    .await
    {
        Ok(compilation) => (Some(compilation.engine), None),
        Err(e) => {
            tracing::warn!("KB 编译失败（{}）: {}", title, e);
            (None, Some(format!("{}", e)))
        }
    }
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

    // 查询项目已有页面 slug，让 LLM 在正文中用 [[slug]] 引用它们
    let existing_slugs: Vec<(String, String)> = {
        let store = wiki_pages
            .lock()
            .map_err(|e| format!("wiki_pages 锁失败: {}", e))?;
        store.list_slugs(project_id)?
    };

    let prompt = build_compilation_prompt(analysis, &existing_slugs);

    let (generated_content, generated_tags) =
        call_llm_for_compilation(&prompt, &page_title, provider_manager).await?;

    // 从生成的 markdown 中提取 [[slug]] 形式的 wiki 链接
    // valid_slugs 用项目已有 slug（不含当前正在生成的 page_slug，避免自引用）
    let valid_slugs: std::collections::HashSet<String> = existing_slugs
        .iter()
        .map(|(s, _)| s.clone())
        .filter(|s| s != &page_slug)
        .collect();
    let wikilinks = crate::services::wikilink_parser::extract_wikilinks(
        &generated_content,
        "",
        &valid_slugs,
    );

    let wikilinks_json = serde_json::to_string(&wikilinks)
        .map_err(|e| format!("序列化 wikilinks 失败: {}", e))?;

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
        Some(wikilinks_json),
    )?;

    Ok(vec![slug])
}

/// 构造 LLM 编译提示词（从 DocumentAnalysis 生成 wiki 页面内容）
///
/// `existing_slugs` 项目已有页面 slug 列表，让 LLM 用 `[[slug]]` 引用它们
fn build_compilation_prompt(analysis: &DocumentAnalysis, existing_slugs: &[(String, String)]) -> String {
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

    // 项目已有页面列表（slug - 标题）。让 LLM 在正文中用 [[slug]] 引用
    let existing_pages_str: String = if existing_slugs.is_empty() {
        "（暂无，这是项目第一个页面）".to_string()
    } else {
        existing_slugs
            .iter()
            .map(|(s, t)| format!("- `{}` ({})", s, t))
            .collect::<Vec<String>>()
            .join("\n")
    };

    // 强制要求：LLM 必须在正文中使用 `[[slug]]` 引用至少 1 个已有页面，
    // 否则知识图谱 S1 (wikilink) 信号为零，只有 tag/source 边。
    let must_cite_section = if existing_slugs.is_empty() {
        String::new()
    } else {
        let top_list: String = existing_slugs
            .iter()
            .take(5)
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            r#"
## ⛔ 强制：正文必须含 `[[slug]]` 引用

候选 slug（前 5）：{top_list}

规则：至少 1 个 `[[xxx]]`；只能引用上述 slug，禁止编造。
"#
        )
    };

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

## 项目已有页面（可被引用）

{existing_pages}
{must_cite}

## 输出要求（严格遵守）

1. 生成标准 Markdown 格式内容
2. 第一段必须是页面概述（200 字以内）
3. 随后展开详细内容，引用文档中的要点
4. **必须**在正文中使用 `[[slug]]` 引用项目已有页面（见上方"强制要求"）
5. 输出格式（严格按此结构，``` 不可省略）：

```yaml
tags: [标签1, 标签2, 标签3]
```

```markdown
## 概述

（概述内容，必须含至少 1 个 `[[slug]]` 引用）

## 正文

（详细展开内容，每讨论一个相关概念都应使用 `[[slug]]` 引用）
```"#,
        title = analysis.title,
        keywords = keywords_str,
        concepts = concepts_str,
        entities = entities_str,
        word_count = analysis.word_count,
        language = analysis.language,
        headings = headings_str,
        cross_refs = cross_refs_str,
        existing_pages = existing_pages_str,
        must_cite = must_cite_section,
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
            content: r#"你是金蝶 ERP 实施知识库的维基页面生成助手。

## 核心要求

1. **严格按输出格式**：必须输出 ` ```yaml ` 代码块（标签）和 ` ```markdown ` 代码块（正文），不要省略
2. **必须生成 wiki 链接**：在正文中使用 `[[slug]]` 引用项目已有页面，**至少 1 个**。0 个引用 = 视为无效输出
3. **禁止编造**：只能引用用户提供的 slug 列表中的页面，不存在的 slug 一律不写
4. **忠于原文**：基于提供的文档分析结果（关键词/概念/实体）生成内容，不要凭空补充
5. **结构清晰**：使用二级标题分章节，正文段落完整、含具体细节
"#
            .to_string(),
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

/// 解析 LLM 编译响应，提取 tags 和 markdown 正文。
/// 兼容 ```yaml / ```markdown / ```md 三种代码块形式。
fn parse_compilation_response(text: &str) -> (String, String) {
    let text = text.trim();
    let tags = RE_YAML_BLOCK
        .captures(text)
        .and_then(|cap| {
            cap[1]
                .lines()
                .find(|l| l.trim().starts_with("tags:"))
                .map(|l| l.trim_start_matches("tags:").trim().to_string())
        })
        .unwrap_or_default();
    let body = RE_MD_BLOCK
        .captures(text)
        .map(|cap| cap[1].trim().to_string())
        .unwrap_or_else(|| {
            // 没有 markdown 块：去掉 yaml 块后剩下的就是正文
            RE_YAML_BLOCK.replace_all(text, "").trim().to_string()
        });
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
///
/// `wikilinks`: 从 LLM 生成的 markdown 中提取的 `[[slug]]` 引用列表（JSON 字符串）
/// - 新页面：直接写入 `wikilinks` 字段
/// - 已有页面：候选阶段不覆盖 `wikilinks`（与 sources 同样的设计：wikilinks 反映已批准 content）
///   新 wikilinks 应在 approve_candidate 时一起提交
fn write_or_update_wiki_page(
    wiki_pages: &Arc<Mutex<WikiPageStore>>,
    project_id: i64,
    slug: &str,
    title: &str,
    content: &str,
    tags: &str,
    sources: Option<String>,
    wikilinks: Option<String>,
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

        store.set_candidate(
            page.id,
            &content,
            candidate_status,
            sources.as_deref(),
            page.version + 1,
        )?;
    } else {
        // 新页面：一次 SQL 同时写入正式字段 + 候选字段
        // （之前是 create + update 两次写入，违反"重写与兼容原则"且性能浪费）
        // 关键修复：写入 LLM 从 markdown 中提取的 wikilinks（之前硬编码 "[]"）
        // → 知识图谱 S1 (wikilink) 信号能正确生成边
        store.create_with_candidate(&CreateWikiPageWithCandidate {
            project_id,
            slug: slug.to_string(),
            title: title.to_string(),
            page_type: "summary".to_string(),
            frontmatter: Some("{}".to_string()),
            sources: sources.clone().or(Some("[]".to_string())),
            wikilinks: wikilinks.clone().or(Some("[]".to_string())),
            tags: Some(tags.to_string()),
            page_metadata: Some("{}".to_string()),
            page_status: Some("draft".to_string()),
            content_candidate: content.to_string(),
            sources_candidate: sources.clone(),
            candidate_status: "pending".to_string(),
        })?;
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
    fn test_parse_compilation_response_fallback_to_md() {
        let text = r#"```yaml
tags: [a, b]
```

```md
## 概述
- 测试
```"#;
        let (tags, body) = parse_compilation_response(text);
        assert!(tags.contains("a"));
        assert!(body.contains("测试"));
    }

    #[test]
    fn test_parse_compilation_response_fallback_to_text() {
        let text = "## 概述\n\n这是概述内容。\n\n## 正文\n\n这是正文。";
        let (tags, body) = parse_compilation_response(text);
        // tags 应该是空的（没有 yaml 块）
        assert!(tags.is_empty());
        // body 应该包含正文
        assert!(body.contains("这是概述内容"));
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
    //
    // wikilink 提取单测已迁移到 `wikilink_parser::tests`，本模块不再保留重复覆盖。
}
