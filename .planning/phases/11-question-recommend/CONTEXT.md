# Phase 11: 问题推荐 + 智能补全引擎 — CONTEXT

**Phase:** 11
**Goal:** 语义匹配 + 问题推荐 + 知识库辅助填充 + 信息追问
**Depends on:** Phase 9 (research outline parsing + indexing)

---

## 1. Problem Statement

实施顾问在调研访谈时，需要快速找到与当前话题相关的调研问题。目前 `research_outline.rs` 已解析 25 份调研提纲为 `FlatQuestion` 结构并存入 SQLite（`research_indexer.rs`），但没有语义检索能力——顾问只能手动翻阅提纲列表。

此外，`smart_completion.rs` 已实现 KB+LLM 字段填充，但缺少：
- 基于调研上下文的问题推荐（当前 conversation context → 匹配相关问题）
- Edition 过滤（企业版/旗舰版切换）
- 上下文累积匹配（已回答问题影响后续推荐）
- 信息追问（LLM 根据已回答内容生成追问）

## 2. Existing Infrastructure

### 2a. Research Outline System
- `research_outline.rs`: `ResearchOutline` → `FlatQuestion` (edition, module_code, module_name, cloud_type, section, category, question_text, order)
- `research_indexer.rs`: SQLite 存储 `research_questions` 表，`get_questions_by_edition()`, `list_outlines()`, `import_directory()`
- `research_indexer.rs`: `build_vector_index()` + `build_bm25_index()` — 已有向量+BM25索引构建函数

### 2b. Hybrid Search
- `hybrid_search.rs`: `hybrid_search(query, project_id, top_k, ...)` → `Vec<HybridSearchResult>`
- RRF 融合常量 k=60, 候选 TOP_N=30
- 结果含 chunk_id, title, content, score, source, document_id, section_path, project
- `diversify_by_title()` 限制同文档最多2个结果

### 2c. Smart Completion
- `smart_completion.rs`: `smart_fill(SmartFillRequest, ...)` → `SmartFillResult`
- 已有 KB 搜索 → 上下文组装 → LLM 生成流程
- `SmartFillRequest` 含 template_id, user_input, manual_fields, schema_fields, project_name
- `SmartFillResult` 含 filled_fields, ai_fields, missing_fields, kb_sources

### 2d. Edition System
- `edition_config.rs`: `EditionConfig` in AppState，`current()` → Edition, `set()` 切换
- `Edition` enum: Enterprise / Flagship (serde snake_case)

### 2e. AppState
- `app_state.rs`: `edition_config: EditionConfig`, `research_indexer: ResearchIndexer` (无Mutex/Arc包装)
- KB 服务: embedding, vector_index, metadata, bm25 (均为 Arc<Mutex<>>)

## 3. Requirements Breakdown

| # | Task | Description |
|---|------|-------------|
| T1 | 问题检索内核 | 复用 hybrid_search 对 research_questions 向量+BM25索引进行检索，返回推荐问题列表 |
| T2 | Edition filter | 推荐结果按当前 Edition 过滤（enterprise/flagship） |
| T3 | 上下文累积匹配 | 接收已回答问题列表，从推荐结果中排除已回答的，同时利用已回答内容优化后续推荐 |
| T4 | 知识库智能补全 | 增强 smart_fill：当用户选中推荐问题时，自动搜索 KB 并预填充答案草稿 |
| T5 | 信息追问 | LLM 根据已回答内容 + KB 上下文，生成追问问题列表 |

## 4. Design Decisions

1. **新文件 `question_recommend.rs`**: 问题推荐引擎独立于 `smart_completion.rs`，职责清晰
2. **复用 research_indexer 的索引**: T1 直接用 `build_vector_index()` / `build_bm25_index()` 已构建的索引
3. **推荐函数签名**: `recommend_questions(query, edition, answered_ids, project_name, top_k)` → `Vec<RecommendedQuestion>`
4. **上下文累积**: 将已回答问题的 question_text 拼接为上下文字符串，加入搜索 query 增强语义匹配
5. **追问由 LLM 生成**: 输入已回答Q&A + KB上下文，输出 3-5 个追问
6. **Tauri commands**: `recommend_questions`, `smart_fill_for_question`, `generate_followup_questions`

## 5. Key Types

```rust
// question_recommend.rs
pub struct RecommendedQuestion {
    pub question_id: i64,        // research_questions.id
    pub question_text: String,
    pub module_name: String,
    pub section: String,
    pub category: String,
    pub score: f32,
    pub source: String,          // "vector" | "bm25" | "both"
    pub is_answered: bool,       // false always for recommended
}

pub struct RecommendRequest {
    pub query: String,           // 当前话题/用户输入
    pub answered_question_ids: Vec<i64>,  // 已回答问题的ID
    pub answered_texts: Vec<String>,      // 已回答问题的文本（用于上下文增强）
    pub project_name: Option<String>,
    pub top_k: Option<usize>,    // 默认10
}

pub struct FollowUpRequest {
    pub answered_qa: Vec<(String, String)>,  // (question, answer)
    pub project_name: Option<String>,
    pub module_name: Option<String>,
}

pub struct FollowUpResult {
    pub followup_questions: Vec<String>,
    pub kb_sources: Vec<KBSource>,
}
```

## 6. Constraints

- KB搜索失败不阻塞推荐（降级为纯问题库匹配）
- LLM调用失败不阻塞推荐（追问功能降级返回空列表）
- Edition过滤在数据库查询层完成（`WHERE edition = ?`），避免大量无效数据传输
- `ResearchIndexer` 非 Mutex 包装，但内部 `conn` 是 `Mutex<Connection>`，线程安全