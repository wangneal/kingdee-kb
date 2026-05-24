# Phase 11: PLAN

## Overview
实现问题推荐 + 智能补全引擎的 5 个子任务，新建 `question_recommend.rs`，增强 `smart_completion.rs`，添加 3 个 Tauri 命令。

---

## Task 1: 问题检索内核 — `question_recommend.rs`

**File:** `src-tauri/src/services/question_recommend.rs` (NEW)

**Steps:**
1. 定义 `RecommendedQuestion`, `RecommendRequest` 结构体
2. 实现 `recommend_questions(state, req)`:
   - 从 `RecommendRequest` 构建 query
   - 调用 `hybrid_search()` 对 research_questions 索引检索
   - 将 `HybridSearchResult` 映射为 `RecommendedQuestion`
   - 从 `research_indexer` 获取问题详情补充 module_name/section/category
   - 排除 `answered_question_ids` 中的已回答问题
   - 返回 top_k 结果

**Success Criteria:** `recommend_questions()` 返回按相关性排序的推荐问题列表

---

## Task 2: Edition 过滤

**File:** `src-tauri/src/services/question_recommend.rs`

**Steps:**
1. `RecommendRequest` 增加 `edition: Option<String>` 字段
2. 在 `recommend_questions()` 中调用 `research_indexer` 查询时添加 edition WHERE 条件
3. 新增 `ResearchIndexer::get_questions_by_edition_and_ids(edition, exclude_ids, limit)` 方法
4. 使用 `AppState.edition_config.current()` 获取当前 Edition

**Success Criteria:** 推荐结果只包含当前 Edition 匹配的问题

---

## Task 3: 上下文累积匹配

**File:** `src-tauri/src/services/question_recommend.rs`

**Steps:**
1. 将 `answered_texts` 拼接为上下文字符串：`ctx = answered_texts.join("; ")`
2. 增强搜索 query：`enhanced_query = format!("{} {}", req.query, ctx)`
3. 对 enhanced_query 执行 hybrid_search
4. 对结果做已回答问题去重（基于 question_id）
5. 对结果做内容去重（相似问题文本去重，similarity > 0.8 的只保留得分最高的）

**Success Criteria:** 已回答问题不出现在推荐中，上下文增强后推荐质量提升

---

## Task 4: 知识库智能补全增强

**File:** `src-tauri/src/services/smart_completion.rs` (MODIFY)

**Steps:**
1. 新增 `smart_fill_for_question(state, question_text, project_name)` 函数
2. 以 question_text 为 query 调用 `hybrid_search()` 获取 KB 上下文
3. 组装 LLM prompt："基于以下知识库内容，为调研问题提供答案草稿"
4. 调用 `llm_service` 生成答案草稿
5. 返回 `SmartFillResult` 格式结果（复用现有类型）

**Success Criteria:** 选中推荐问题后可自动获取 KB 辅助的答案草稿

---

## Task 5: 信息追问

**File:** `src-tauri/src/services/question_recommend.rs`

**Steps:**
1. 定义 `FollowUpRequest`, `FollowUpResult`, `KBSource` 结构体
2. 实现 `generate_followup_questions(state, req)`:
   - 将已回答 Q&A 拼接为上下文
   - 如有 project_name，调用 `hybrid_search()` 获取 KB 上下文
   - 组装 LLM prompt："根据已回答的调研问题和知识库，生成 3-5 个追问"
   - 解析 LLM 输出为追问列表
   - 返回 `FollowUpResult`
3. LLM 失败降级返回空列表 + 日志警告

**Success Criteria:** LLM 能基于已有回答+KB上下文生成相关追问

---

## Tauri Commands (跨任务)

**File:** `src-tauri/src/lib.rs` (MODIFY)

1. `recommend_questions_cmd` — 调用 `recommend_questions()`
2. `smart_fill_for_question_cmd` — 调用 `smart_fill_for_question()`
3. `generate_followup_questions_cmd` — 调用 `generate_followup_questions()`

---

## Verification
- `cargo check` 0 errors
- 单元测试：问题推荐核心逻辑
- 模块注册：`mod.rs` + `app_state.rs` 无遗漏
