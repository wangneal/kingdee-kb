//! 双引擎文档分析 — Step 1 分析阶段
//!
//! 提供两个分析引擎：
//! - **RustAnalysisEngine**：纯本地 TF-IDF 关键词、标题层级树、语言检测
//! - **LlmAnalysisEngine**：LLM 语义分析（命名实体、关键概念、交叉引用、矛盾检测）
//! - **AnalysisOrchestrator**：引擎选择 + 缓存集成 + 超时降级

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::Duration;
use tracing::{info, warn};

// ─── 静态正则（LazyLock，避免每次调用重新编译） ───

/// 匹配 # 一级标题
static RE_H1_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^#\s+(.+)$").expect("编译正则失败: RE_H1_TITLE"));

/// 匹配 Markdown 标题标记（1-6 级）
static RE_HEADINGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(#{1,6})\s+(.+)$").expect("编译正则失败: RE_HEADINGS"));

/// 匹配中文书名号内容《xxx》
static RE_BOOK_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"《([^》]+)》").expect("编译正则失败: RE_BOOK_TITLE"));

/// 匹配中文组织/产品名模式（XX公司、XX系统等）
static RE_ORG_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\u4e00-\u9fff]{2,}(公司|集团|系统|模块|平台|软件|产品|方案|服务)")
        .expect("编译正则失败: RE_ORG_NAME")
});

/// 匹配大写英文缩写（ERP、CRM、SQL等）
static RE_ABBREVIATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^A-Za-z])([A-Z]{2,10})(?:$|[^A-Za-z])")
        .expect("编译正则失败: RE_ABBREVIATION")
});

/// 匹配 JSON 代码块 ```json ... ```
static RE_JSON_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)```(?:json)?\s*\n?(.*?)```").expect("编译正则失败: RE_JSON_BLOCK")
});

use crate::services::analysis_cache::{AnalysisCacheStore, CreateAnalysisCache};
use crate::services::llm_providers::LLMProviderManager;

// ─── 常量 ───

/// LLM 分析超时时间（30 秒）
const LLM_ANALYSIS_TIMEOUT_SECS: u64 = 30;

/// 分析器版本号，缓存失效用
const ANALYZER_VERSION: &str = "1";

/// 中文常见停用词（精简版）
const STOP_WORDS_ZH: &[&str] = &[
    "的", "了", "在", "是", "我", "有", "和", "就", "不", "人",
    "都", "一", "一个", "上", "也", "很", "到", "说", "要", "去",
    "你", "会", "着", "没有", "看", "好", "自己", "这", "他", "她",
    "它", "们", "那", "些", "什么", "怎么", "因为", "所以", "但是",
    "如果", "虽然", "而且", "或者", "但是", "然而", "因此", "可以",
    "这个", "那个", "已经", "通过", "进行", "使用", "需要", "能够",
    "以及", "其中", "之后", "之前", "同时", "此外", "对于", "关于",
    "按照", "根据", "作为", "从", "到", "与", "为", "以", "被", "将",
    "把", "让", "向", "往", "由", "于", "之", "所", "其", "该",
];

/// TF-IDF 关键词提取数量
const KEYWORD_COUNT: usize = 20;

/// 最小词长（跳过单字词）
const MIN_WORD_LEN: usize = 2;

/// 中文 Unicode 范围
const CJK_UNIFIED_START: u32 = 0x4E00;
const CJK_UNIFIED_END: u32 = 0x9FFF;
const CJK_EXT_START: u32 = 0x3400;
const CJK_EXT_END: u32 = 0x4DBF;

// ─── 结构体定义 ───

/// 文档分析结果（兼容两种引擎输出）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentAnalysis {
    /// 关联 raw_sources.identity
    pub source_identity: String,
    /// 源文件 SHA256
    pub sha256: String,
    /// 提取的标题
    pub title: String,
    /// 标题层级树（两引擎均产出）
    pub headings: Vec<Heading>,
    /// TF-IDF 关键词（两引擎均产出）
    pub keywords: Vec<KeywordScore>,
    pub word_count: usize,
    pub char_count: usize,
    /// 检测到的语言
    pub language: String,
    /// LLM 引擎专属字段（Rust 引擎输出为空数组）
    pub entities: Vec<String>,
    /// 关键概念
    pub key_concepts: Vec<String>,
    /// 与已有 Wiki 的交叉引用
    pub cross_references: Vec<CrossRef>,
    /// 检测到的矛盾
    pub contradictions: Vec<String>,
}

/// 标题节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heading {
    /// 标题级别（1-6）
    pub level: u8,
    /// 标题文本
    pub text: String,
    /// 子标题列表
    pub children: Vec<Heading>,
}

/// 关键词权重
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordScore {
    pub keyword: String,
    pub score: f32,
}

/// 交叉引用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossRef {
    /// 引用的目标标题或 Wiki 页面 slug
    pub target: String,
    /// 引用上下文片段
    pub context: String,
    /// 引用类型（mention / link / see_also）
    pub ref_type: String,
}

/// 分析引擎的类型标识
#[derive(Debug, Clone, PartialEq)]
pub enum EngineType {
    Llm,
    Rust,
}

/// 分析结果 + 引擎来源
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub analysis: DocumentAnalysis,
    pub engine: EngineType,
}

// ─── Rust 分析引擎 ───

/// Rust 本地分析引擎（纯词法/正则分析，不依赖 LLM）
pub struct RustAnalysisEngine;

impl RustAnalysisEngine {
    /// 对清理后的文档文本执行全面分析
    pub fn analyze(text: &str, source_identity: &str, sha256: &str) -> DocumentAnalysis {
        let title = Self::extract_title(text);
        let headings = Self::parse_headings(text);
        let keywords = Self::extract_keywords_tfidf(text);
        // 规格要求：entities 是 LLM 引擎专属字段，Rust 引擎输出为空数组
        let entities = Vec::new();
        let (word_count, char_count) = Self::count_text(text);
        let language = Self::detect_language(text);

        DocumentAnalysis {
            source_identity: source_identity.to_string(),
            sha256: sha256.to_string(),
            title,
            headings,
            keywords,
            word_count,
            char_count,
            language,
            entities,
            key_concepts: Vec::new(),
            cross_references: Vec::new(),
            contradictions: Vec::new(),
        }
    }

    /// 从文本中提取标题（首个一级标题或第一行非空文本）
    fn extract_title(text: &str) -> String {
        // 尝试匹配 # 一级标题
        if let Some(cap) = RE_H1_TITLE.captures(text) {
            return cap[1].trim().to_string();
        }
        // 退回到第一行非空文本
        text.lines().find(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .unwrap_or_default()
    }

    /// 解析 Markdown 标题标记，构建标题层级树
    fn parse_headings(text: &str) -> Vec<Heading> {
        let matches: Vec<(u8, String)> = RE_HEADINGS.captures_iter(text)
            .map(|cap| {
                let level = cap[1].len() as u8;
                let text = cap[2].trim().to_string();
                (level, text)
            })
            .collect();

        Self::build_heading_tree(&matches)
    }

    /// 将扁平的标题列表构建为层级树
    fn build_heading_tree(headings: &[(u8, String)]) -> Vec<Heading> {
        // 使用索引模拟栈，避免自引用借用问题
        let mut nodes: Vec<Heading> = Vec::new();
        // 栈中存储节点的索引
        let mut stack: Vec<usize> = Vec::new();

        for (level, text) in headings {
            let idx = nodes.len();
            nodes.push(Heading {
                level: *level,
                text: text.clone(),
                children: Vec::new(),
            });

            // 从栈顶弹出直到找到合适的父节点
            while let Some(&top_idx) = stack.last() {
                if nodes[top_idx].level < *level {
                    break;
                }
                stack.pop();
            }

            if let Some(&parent_idx) = stack.last() {
                // 将当前节点作为父节点的子节点
                let child = nodes.swap_remove(idx);
                nodes[parent_idx].children.push(child);
                // 重新入栈的是父节点 children 中最后一个（刚添加的）引用
                // 由于我们之后不会再修改已添加的子节点，只需记住父节点的索引
                // 但我们需要子节点在栈顶以供后续更深层级的标题定位
                // 所以找一种方式：把新添加的子节点放到末尾并记住索引
                // 更好的方式：重新设计——先构建完树再返回
            } else {
                // 根级别节点
            }
        }

        // 上述方法实际上有问题，需要完全重写
        // 改用递归构建方式
        Self::build_heading_tree_recursive(headings, 0).0
    }

    /// 递归构建标题层级树
    fn build_heading_tree_recursive(
        headings: &[(u8, String)],
        start: usize,
    ) -> (Vec<Heading>, usize) {
        Self::build_heading_tree_inner(headings, start, 0)
    }

    /// 内部递归：parent_level 是父节点的标题级别，用于判断何时返回
    fn build_heading_tree_inner(
        headings: &[(u8, String)],
        start: usize,
        parent_level: u8,
    ) -> (Vec<Heading>, usize) {
        let mut result: Vec<Heading> = Vec::new();
        let mut i = start;

        while i < headings.len() {
            let (level, text) = &headings[i];

            if *level <= parent_level {
                // 遇到了父级或更高级别的标题，返回上一层
                break;
            }

            // 当前标题是 parent_level 的子级
            // 递归收集它的子标题（level > *level 的标题）
            let (children, next_i) =
                Self::build_heading_tree_inner(headings, i + 1, *level);

            result.push(Heading {
                level: *level,
                text: text.clone(),
                children,
            });
            i = next_i;
        }

        (result, i)
    }

    /// 基于词频的 TF-IDF 近似关键词提取
    fn extract_keywords_tfidf(text: &str) -> Vec<KeywordScore> {
        // 中文分词：基于字的二元组和常用词模式
        let words = Self::segment_chinese(text);

        // 词频统计
        let mut freq: HashMap<String, usize> = HashMap::new();
        let mut total: usize = 0;
        for word in &words {
            if word.len() < MIN_WORD_LEN {
                continue;
            }
            if STOP_WORDS_ZH.contains(&word.as_str()) {
                continue;
            }
            *freq.entry(word.clone()).or_insert(0) += 1;
            total += 1;
        }

        if total == 0 {
            return Vec::new();
        }

        // 计算 TF-IDF 分数（简化版本：TF * IDF，IDF 用 log(N/df) 近似）
        // 此处单文档 IDF 用 log(1 + 总词数 / 词频) 近似
        let mut scored: Vec<KeywordScore> = freq
            .into_iter()
            .map(|(keyword, count)| {
                let tf = count as f32 / total as f32;
                let idf = (1.0 + total as f32 / count as f32).ln();
                let score = tf * idf;
                KeywordScore { keyword, score }
            })
            .collect();

        // 按分数降序排列
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // 取前 N 个
        scored.truncate(KEYWORD_COUNT);
        scored
    }

    /// 简单中文分词（基于字符二元组 + 常见多字词模式）
    fn segment_chinese(text: &str) -> Vec<String> {
        let mut words: Vec<String> = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();

        // 提取中文连续段落中的二元组
        let mut i = 0;
        while i < len {
            if Self::is_cjk(chars[i]) {
                let mut j = i;
                while j < len && Self::is_cjk(chars[j]) {
                    j += 1;
                }
                let cjk_segment: String = chars[i..j].iter().collect();
                // 二元组滑动窗口
                let seg_chars: Vec<char> = cjk_segment.chars().collect();
                for k in 0..seg_chars.len().saturating_sub(1) {
                    let mut bigram = String::new();
                    bigram.push(seg_chars[k]);
                    bigram.push(seg_chars[k + 1]);
                    words.push(bigram);
                }
                // 三元组（捕获更长的词组）
                for k in 0..seg_chars.len().saturating_sub(2) {
                    let mut trigram = String::new();
                    trigram.push(seg_chars[k]);
                    trigram.push(seg_chars[k + 1]);
                    trigram.push(seg_chars[k + 2]);
                    words.push(trigram);
                }
                i = j;
            } else if chars[i].is_ascii_alphabetic() {
                let mut j = i;
                while j < len && chars[j].is_ascii_alphanumeric() {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                words.push(word.to_lowercase());
                i = j;
            } else {
                i += 1;
            }
        }

        words
    }

    fn is_cjk(c: char) -> bool {
        let cp = c as u32;
        (CJK_UNIFIED_START..=CJK_UNIFIED_END).contains(&cp)
            || (CJK_EXT_START..=CJK_EXT_END).contains(&cp)
    }

    /// 简单命名实体提取（正则匹配常见模式）
    fn extract_entities_simple(text: &str) -> Vec<String> {
        let mut entities: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // 模式1：中文引号内的内容（可能为书名、文章标题）
        for cap in RE_BOOK_TITLE.captures_iter(text) {
            let entity = cap[1].trim().to_string();
            if entity.len() >= 2 && seen.insert(entity.clone()) {
                entities.push(entity);
            }
        }

        // 模式2：匹配常见组织/产品名模式（XX公司、XX系统、XX模块）
        for cap in RE_ORG_NAME.captures_iter(text) {
            let entity = cap[0].trim().to_string();
            if entity.len() >= 2 && seen.insert(entity.clone()) {
                entities.push(entity);
            }
        }

        // 模式3：匹配大写英文缩写（ERP、CRM、SQL等）
        // regex crate 不支持 lookahead/lookbehind，用捕获组处理
        for cap in RE_ABBREVIATION.captures_iter(text) {
            let entity = cap[1].to_string();
            if seen.insert(entity.clone()) {
                entities.push(entity);
            }
        }

        entities
    }

    /// 统计字数和词数
    fn count_text(text: &str) -> (usize, usize) {
        let char_count = text.chars().count();
        // 词数：中文按连续 CJK 段计数，英文按空格分词
        let mut word_count = 0;
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if Self::is_cjk(chars[i]) {
                word_count += 1;
                while i < chars.len() && Self::is_cjk(chars[i]) {
                    i += 1;
                }
            } else if chars[i].is_ascii_alphanumeric() {
                word_count += 1;
                while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        (word_count, char_count)
    }

    /// 检测文本的主要语言
    fn detect_language(text: &str) -> String {
        let total_chars = text.chars().count();
        if total_chars == 0 {
            return "unknown".to_string();
        }

        let cjk_count = text.chars().filter(|c| Self::is_cjk(*c)).count();
        let cjk_ratio = cjk_count as f64 / total_chars as f64;

        if cjk_ratio > 0.3 {
            "zh".to_string()
        } else if cjk_ratio > 0.05 {
            "zh-en".to_string()
        } else {
            "en".to_string()
        }
    }
}

// ─── LLM 分析引擎 ───

/// LLM 语义分析引擎（调用 LLM 提取实体、概念、引用、矛盾）
pub struct LlmAnalysisEngine;

impl LlmAnalysisEngine {
    /// 组装 LLM 分析提示词
    pub fn build_prompt(text: &str, source_identity: &str) -> String {
        format!(
            r#"你是一个文档分析专家。请分析以下文档内容，以 JSON 格式输出分析结果。

## 输出格式
必须严格按以下 JSON 结构输出（不包含任何额外文本或 markdown 代码块标记）：

```json
{{
    "title": "文档标题",
    "headings": [
        {{"level": 1, "text": "标题文本", "children": []}}
    ],
    "keywords": [
        {{"keyword": "关键词", "score": 0.95}}
    ],
    "word_count": 0,
    "char_count": 0,
    "language": "zh",
    "entities": ["实体1", "实体2"],
    "key_concepts": ["概念1", "概念2"],
    "cross_references": [
        {{"target": "引用目标", "context": "上下文", "ref_type": "mention"}}
    ],
    "contradictions": ["矛盾描述"]
}}

## 字段说明
- `title`：文档标题
- `headings`：标题层级树（level 1-6，children 为子标题列表）
- `keywords`：关键词列表（score 0-1 的浮点数）
- `word_count`、`char_count`：文档字数/字符数
- `language`：文档语言代码（zh/en/zh-en）
- `entities`：命名实体（人名、组织名、产品名、系统名）
- `key_concepts`：关键概念或主题
- `cross_references`：与外部文档、标准或系统的交叉引用
- `contradictions`：文档内部或与常见知识之间的矛盾

## 文档内容
source_identity: {source_identity}

---
{text}
---"#,
            source_identity = source_identity,
            text = text,
        )
    }

    /// 从 LLM 响应 JSON 中提取 DocumentAnalysis
    pub fn parse_response(
        json_str: &str,
        source_identity: &str,
        sha256: &str,
    ) -> Result<DocumentAnalysis, String> {
        // 尝试从响应中提取 JSON（可能被 markdown 代码块包裹）
        let cleaned = Self::extract_json(json_str);

        let mut analysis: DocumentAnalysis = serde_json::from_str(&cleaned)
            .map_err(|e| format!("解析 LLM 响应 JSON 失败: {}", e))?;

        analysis.source_identity = source_identity.to_string();
        analysis.sha256 = sha256.to_string();

        // 确保字段不为空
        if analysis.title.is_empty() {
            analysis.title = source_identity.to_string();
        }

        Ok(analysis)
    }

    /// 从 LLM 响应文本中提取 JSON 部分（去除 markdown 代码块等包裹）
    fn extract_json(text: &str) -> String {
        let text = text.trim();

        // 尝试提取 ```json ... ``` 块中的内容
        if let Some(cap) = RE_JSON_BLOCK.captures(text) {
            return cap[1].trim().to_string();
        }

        // 尝试直接解析为 JSON 对象（从第一个 { 到最后一个 }）
        if let Some(start) = text.find('{') {
            if let Some(end) = text.rfind('}') {
                return text[start..=end].to_string();
            }
        }

        text.to_string()
    }

    /// 通过 LLM 分析文档（直接提供配置信息）
    pub async fn analyze_with_config(
        text: &str,
        source_identity: &str,
        sha256: &str,
        base_url: &str,
        api_key: &str,
        model_name: &str,
    ) -> Result<DocumentAnalysis, String> {
        let prompt = Self::build_prompt(text, source_identity);

        let body = serde_json::json!({
            "model": model_name,
            "messages": [
                {"role": "system", "content": "你是一个文档分析专家。请严格按照要求的 JSON 格式输出分析结果。"},
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.1,
            "max_tokens": 4096
        });

        let client = reqwest::Client::new();
        let chat_url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

        let response = client
            .post(&chat_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("LLM 请求失败: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            return Err(format!("LLM API 错误 ({}): {}", status, err_text));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("解析 LLM 响应失败: {}", e))?;

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| "LLM 响应中未找到 content".to_string())?;

        Self::parse_response(content, source_identity, sha256)
    }
}

// ─── 分析编排器 ───

/// 分析编排器 — 引擎选择、缓存集成、超时降级
pub struct AnalysisOrchestrator {
    /// 分析缓存存储
    cache_store: Arc<Mutex<AnalysisCacheStore>>,
    /// LLM 供应商管理器
    provider_manager: Arc<RwLock<LLMProviderManager>>,
}

impl AnalysisOrchestrator {
    pub fn new(
    cache_store: Arc<Mutex<AnalysisCacheStore>>,
    provider_manager: Arc<RwLock<LLMProviderManager>>,
    ) -> Self {
        Self {
            cache_store,
            provider_manager,
        }
    }

    /// 分析文档并返回结果（带缓存 + 双引擎降级）
    ///
    /// 流程：
    /// 1. 检查 `analysis_cache` 是否命中且版本匹配
    /// 2. 若未命中，尝试 LLM 引擎（30s 超时）
    /// 3. LLM 不可用/超时 → 自动降级到 Rust 引擎
    /// 4. 写入 analysis_cache
    pub async fn analyze(
        &self,
        project: &str,
        source_identity: &str,
        sha256: &str,
        text: &str,
        enable_kb_compilation: bool,
    ) -> AnalysisResult {
        // 1. 检查缓存
        if let Some(cached) = self.check_cache(project, source_identity, sha256) {
            info!(
                "分析缓存命中: source={}, sha256={}",
                source_identity, sha256
            );
            return AnalysisResult {
                analysis: cached,
                engine: EngineType::Rust,
            };
        }

        // 2. 尝试 LLM 引擎
        if enable_kb_compilation {
            let llm_config: Option<(String, String, String)> = {
                let mgr = match self.provider_manager.read() {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("获取 provider 管理器锁失败: {}，降级到 Rust 引擎", e);
                        return self.fallback_rust(text, source_identity, sha256);
                    }
                };
                match mgr.get_default_provider() {
                    Some(p) if p.is_configured() => Some((
                        p.base_url.clone(),
                        p.get_default_key_value(),
                        p.get_default_model_name(),
                    )),
                    _ => None,
                }
            };

            let (base_url, api_key, model_name) = match llm_config {
                Some(c) => c,
                None => {
                    warn!("LLM 供应商未配置，降级到 Rust 引擎");
                    return self.fallback_rust(text, source_identity, sha256);
                }
            };

            let chat_url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
            let prompt = LlmAnalysisEngine::build_prompt(text, source_identity);
            let body = serde_json::json!({
                "model": model_name,
                "messages": [
                    {"role": "system", "content": "你是一个文档分析专家。请严格按照要求的 JSON 格式输出分析结果。"},
                    {"role": "user", "content": prompt}
                ],
                "temperature": 0.1,
                "max_tokens": 4096
            });

            let client = reqwest::Client::new();
            let result = tokio::time::timeout(
                Duration::from_secs(LLM_ANALYSIS_TIMEOUT_SECS),
                Self::call_llm_inner(&client, &chat_url, &api_key, &body, source_identity, sha256),
            )
            .await;

            match result {
                Ok(Ok(analysis)) => {
                    info!("LLM 分析成功: source={}", source_identity);
                            self.write_cache(project, source_identity, sha256, &analysis);
                    return AnalysisResult {
                        analysis,
                        engine: EngineType::Llm,
                    };
                }
                Ok(Err(e)) => {
                    warn!("LLM 分析失败: {}，降级到 Rust 引擎", e);
                }
                Err(_) => {
                    warn!("LLM 分析超时 ({}s)，降级到 Rust 引擎", LLM_ANALYSIS_TIMEOUT_SECS);
                }
            }
        }

        // 3. Rust 引擎降级
        self.fallback_rust(text, source_identity, sha256)
    }

    /// 内部 LLM HTTP 调用辅助（无 self，可传递给 timeout）
    async fn call_llm_inner(
        client: &reqwest::Client,
        chat_url: &str,
        api_key: &str,
        body: &serde_json::Value,
        source_identity: &str,
        sha256: &str,
    ) -> Result<DocumentAnalysis, String> {
        let response = client
            .post(chat_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| format!("LLM 请求失败: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            return Err(format!("LLM API 错误 ({}): {}", status, err_text));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("解析 LLM 响应失败: {}", e))?;

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| "LLM 响应中未找到 content".to_string())?;

        LlmAnalysisEngine::parse_response(content, source_identity, sha256)
    }

    /// Rust 引擎降级分析
    fn fallback_rust(
        &self,
        text: &str,
        source_identity: &str,
        sha256: &str,
    ) -> AnalysisResult {
        info!("使用 Rust 引擎分析: source={}", source_identity);
        let analysis = RustAnalysisEngine::analyze(text, source_identity, sha256);
        AnalysisResult {
            analysis,
            engine: EngineType::Rust,
        }
    }

    /// 从 analysis_cache 读取缓存
    fn check_cache(
        &self,
        project: &str,
        source_identity: &str,
        sha256: &str,
    ) -> Option<DocumentAnalysis> {
        let store = match self.cache_store.lock() {
            Ok(s) => s,
            Err(e) => {
                warn!("获取 cache_store 锁失败: {}", e);
                return None;
            }
        };

        let cached = match store.get_by_key(project, source_identity, sha256) {
            Ok(Some(c)) => c,
            _ => return None,
        };

        if cached.analyzer_version != ANALYZER_VERSION {
            return None;
        }

        match serde_json::from_str::<DocumentAnalysis>(&cached.analysis_json) {
            Ok(analysis) => Some(analysis),
            Err(e) => {
                warn!("解析缓存 JSON 失败: {}", e);
                None
            }
        }
    }

    /// 写入 analysis_cache
    fn write_cache(
        &self,
        project: &str,
        source_identity: &str,
        sha256: &str,
        analysis: &DocumentAnalysis,
    ) {
        let analysis_json = match serde_json::to_string(analysis) {
            Ok(s) => s,
            Err(e) => {
                warn!("序列化分析结果失败: {}", e);
                return;
            }
        };

        let store = match self.cache_store.lock() {
            Ok(s) => s,
            Err(e) => {
                warn!("获取 cache_store 锁失败: {}", e);
                return;
            }
        };

        let input = CreateAnalysisCache {
            project: project.to_string(),
            source_identity: source_identity.to_string(),
            sha256: sha256.to_string(),
            analysis_json,
            analyzer_version: Some(ANALYZER_VERSION.to_string()),
        };

        if let Err(e) = store.upsert(&input) {
            warn!("写入 analysis_cache 失败: {}", e);
        }
    }
}

// ─── 辅助函数 ───

/// 快速分析文档（单一入口，适用于不需要缓存和编排的场景）
pub fn quick_analyze(text: &str, source_identity: &str, sha256: &str) -> DocumentAnalysis {
    RustAnalysisEngine::analyze(text, source_identity, sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title() {
        let text = "# 测试标题\n\n一些内容";
        assert_eq!(RustAnalysisEngine::extract_title(text), "测试标题");
    }

    #[test]
    fn test_parse_headings() {
        let text = "# 一级\n## 二级\n### 三级\n## 二级二";
        let headings = RustAnalysisEngine::parse_headings(text);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].level, 1);
        assert_eq!(headings[0].text, "一级");
        assert_eq!(headings[0].children.len(), 2);
        assert_eq!(headings[0].children[0].text, "二级");
        assert_eq!(headings[0].children[0].children[0].text, "三级");
        assert_eq!(headings[0].children[1].text, "二级二");
    }

    #[test]
    fn test_keyword_extraction() {
        let text = "金蝶云星空 ERP 系统提供了财务管理、供应链管理、生产管理等功能模块。\
                    财务管理包括总账、应收应付、固定资产等子模块。\
                    供应链管理包括采购、销售、库存等子模块。";
        let keywords = RustAnalysisEngine::extract_keywords_tfidf(text);
        assert!(!keywords.is_empty());
        assert!(keywords[0].score > 0.0);
    }

    #[test]
    fn test_entity_extraction() {
        let text = "金蝶公司开发的ERP系统包括CRM模块和SCM模块。\
                    配套《实施方法论》文档已发布。";
        let entities = RustAnalysisEngine::extract_entities_simple(text);
        // Rust 引擎不再提取 entities（规格要求，LLM 专属字段）
        // extract_entities_simple 仍保留以供外部使用
        assert!(entities.contains(&"ERP".to_string()));
        assert!(entities.contains(&"CRM".to_string()));
        assert!(entities.contains(&"SCM".to_string()));
    }

    #[test]
    fn test_language_detection() {
        assert_eq!(RustAnalysisEngine::detect_language("中文测试"), "zh");
        assert_eq!(RustAnalysisEngine::detect_language("Hello world"), "en");
    }

    #[test]
    fn test_llm_prompt_assembly() {
        let prompt = LlmAnalysisEngine::build_prompt("测试内容", "test-id");
        assert!(prompt.contains("test-id"));
        assert!(prompt.contains("测试内容"));
        assert!(prompt.contains("entities"));
        assert!(prompt.contains("key_concepts"));
    }

    #[test]
    fn test_json_extraction() {
        let raw = "```json\n{\"title\": \"测试\"}\n```";
        let extracted = LlmAnalysisEngine::extract_json(raw);
        assert_eq!(extracted, "{\"title\": \"测试\"}");

        let raw2 = "一些文本 {\"title\": \"测试\"} 结尾";
        let extracted2 = LlmAnalysisEngine::extract_json(raw2);
        assert_eq!(extracted2, "{\"title\": \"测试\"}");
    }

    #[test]
    fn test_build_heading_tree() {
        let headings = vec![
            (1u8, "A".to_string()),
            (2u8, "B".to_string()),
            (3u8, "C".to_string()),
            (2u8, "D".to_string()),
        ];
        let tree = RustAnalysisEngine::build_heading_tree(&headings);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].children.len(), 1);
    }

    #[test]
    fn test_count_text() {
        let text = "Hello 世界";
        let (words, chars) = RustAnalysisEngine::count_text(text);
        assert_eq!(chars, 8); // H,e,l,l,o, ,世,界
        assert!(words > 0);
    }

    #[test]
    fn test_empty_text() {
        let analysis = RustAnalysisEngine::analyze("", "empty", "000");
        assert_eq!(analysis.title, "");
        assert_eq!(analysis.word_count, 0);
        assert_eq!(analysis.char_count, 0);
    }
}
