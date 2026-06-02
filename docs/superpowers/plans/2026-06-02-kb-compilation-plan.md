# 知识编译 Phase 2 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 subagent-driven-development（推荐）或 executing-plans 逐任务实现此计划。

**目标：** 将 VerificationPipeline 集成到知识编译管道中，实现 Step 2.5 验证 pass 和 chunk_id 来源追踪。

**现状：** 知识编译的 80% 基础设施已存在（wiki_page CRUD、ingestion_pipeline、ingest_cache、页面合并保护）。需要补全验证集成和来源追踪。

**技术栈：** Rust + tokio async

---

## 文件结构

| 文件 | 职责 | 状态 |
|------|------|------|
| `services/ingestion_pipeline.rs` | 两步摄入管道编排 | ✅ 已有，需集成验证 |
| `services/wiki_page.rs` | Wiki 页面 CRUD + schema | ✅ 已有 |
| `services/ingest_cache.rs` | SHA256 增量缓存 | ✅ 已有 |
| `services/ingestion.rs` | 主摄入流 | ✅ 已有 |
| `services/verification/pipeline.rs` | VerificationPipeline | ✅ 已有（Phase 1） |
| `services/verification/types.rs` | VerificationInput, ScenarioType | ✅ 已有 |

---

### 任务 1：验证集成到知识编译管道

**文件：**
- 修改：`src-tauri/src/services/ingestion_pipeline.rs`
- 修改：`src-tauri/src/services/verification/types.rs`（仅当需要新增编译场景类型时）

- [ ] **步骤 1：阅读现有验证管道和摄入管道**

需要理解：
1. `VerificationPipeline::verify()` 接受 `VerificationInput`，内有 `generated_text`, `retrieved_chunks`, `chunk_titles`, `available_chunk_ids`, `query`, `scenario` 字段
2. `run_llm_compilation()` 生成 wiki 页面内容后返回 slug 列表
3. 编译后的 wiki 页面内容存储在 `content_candidate` 字段

- [ ] **步骤 2：在 ingestion_pipeline.rs 中添加验证导入**

```rust
// 在现有 use 语句区域添加
use crate::services::verification::pipeline::VerificationPipeline;
use crate::services::verification::types::{ScenarioType, VerificationInput};
```

- [ ] **步骤 3：在 run_llm_compilation 返回后插入验证**

找到 `ingestion_pipeline.rs` 中的 `process_with_kb_compilation` 函数，在 Step 2（LLM 编译）成功后，增加 Step 2.5 验证：

```rust
    // Step 2.5: 验证编译结果
    if compilation_done && !generated_pages.is_empty() {
        let verifier = VerificationPipeline::default_with_all();
        for slug in &generated_pages {
            // 读回刚写入的 wiki 页面内容
            if let Ok(Some(page)) = wiki_pages.lock().map_err(|e| e.to_string()).and_then(|store| {
                store.get_by_slug(project, slug).map_err(|e| e.to_string())
            }) {
                let input = VerificationInput {
                    generated_text: page.content_candidate.clone().unwrap_or_default(),
                    retrieved_chunks: vec![],  // 编译场景不需要检索 chunk 校验
                    chunk_titles: vec![],
                    available_chunk_ids: vec![],
                    query: format!("知识编译验证: {}", page.title),
                    scenario: ScenarioType::KnowledgeCompilation,
                };
                let report = verifier.verify(&input).await;
                tracing::info!(
                    "编译验证: slug={}, level={:?}, confidence={}",
                    slug, report.level, report.overall_confidence
                );
                // 如果验证等级为 Failed，记录警告但不阻塞
                if report.level == crate::services::verification::types::VerificationLevel::Failed {
                    tracing::warn!("编译验证未通过: slug={}, detail={:?}", slug, report.suggested_labels);
                }
            }
        }
    }
```

- [ ] **步骤 4：编译验证**

运行：`cd E:\projects\kingdee\KingdeeKB\src-tauri && cargo check 2>&1`
预期：0 errors

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/services/ingestion_pipeline.rs
git commit -m "feat: 知识编译 Step 2.5 验证 pass（集成 VerificationPipeline）"
```

---

### 任务 2：Wiki 页面来源追踪增强

**文件：**
- 修改：`src-tauri/src/services/ingestion_pipeline.rs`
- 修改：`src-tauri/src/services/wiki_page.rs`（确认 sources 字段结构）

- [ ] **步骤 1：确认 sources 字段结构**

阅读 `wiki_page.rs` 中 `WikiPage` 和 `CreateWikiPage` 的 `sources` 字段类型。当前为 `String`（JSON 格式）。

需确认 sources JSON 格式包含 `chunk_ids` 数组：
```json
[
  {"source_id": 42, "document_id": 7, "chunks": [451, 452, 453]}
]
```

- [ ] **步骤 2：在编译时传入 chunk_id 列表**

在 `ingestion_pipeline.rs` 的 `process_with_kb_compilation` 中，已经有 `text` 参数（原始文档内容）。如果调用方能提供对应的 `chunk_ids`，则将其传入 sources 字段。

找到创建 wiki 页面的位置（`run_llm_compilation` 内部），在 `CreateWikiPage` 的 `sources` 字段中添加上下文 chunk_id 列表。

```rust
// 在 run_llm_compilation 函数中，构建 sources JSON
let sources_json = serde_json::json!([{
    "source_id": null,
    "document_id": null,
    "chunks": chunk_ids,  // 从上下文传入
}]);

let create = CreateWikiPage {
    sources: Some(sources_json.to_string()),
    // ... 其他字段
};
```

- [ ] **步骤 3：验证编译通过**

运行：`cd E:\projects\kingdee\KingdeeKB\src-tauri && cargo check 2>&1`
预期：0 errors

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/src/services/ingestion_pipeline.rs
git commit -m "feat: 知识编译来源追踪关联 chunk_id"
```

---

### 任务 3：验证 + 全量测试

- [ ] **步骤 1：运行所有测试**

运行：`cd E:\projects\kingdee\KingdeeKB\src-tauri && cargo test -- --nocapture 2>&1`
预期：全部 PASS（或只有 pre-existing failures）

- [ ] **步骤 2：cargo check 最终验证**

运行：`cd E:\projects\kingdee\KingdeeKB\src-tauri && cargo check 2>&1`
预期：0 errors

- [ ] **步骤 3：Commit**

```bash
git add -A
git commit -m "chore: Phase 2 知识编译验证集成完成"
```
