# Phase 10: 文档生成核心 — PLAN.md

**Phase:** 10 — 文档生成核心
**Created:** 2026-05-24
**Depends on:** Phase 9 (✅ Complete)

---

## Goal

实现配方感知的文档生成流水线，打通"选择模板 → 配方路由 → KB检索 + LLM填充 → 产物保存"全链路。

---

## Tasks

### Task 1: `RecipeDocRequest` + `generate_recipe_doc()` in doc_generator.rs

**File:** `src-tauri/src/services/doc_generator.rs`

**Add:**
- `RecipeDocRequest` struct:
  ```rust
  pub struct RecipeDocRequest {
      pub recipe_id: String,           // e.g., "investigation_report"
      pub template_path: String,       // absolute path to template
      pub output_path: String,         // absolute path for output
      pub fields: HashMap<String, String>,  // user-provided fields
      pub schema_fields: Option<Vec<SchemaField>>,  // from template schema
      pub project_name: Option<String>,
      pub context: Option<String>,     // extra context (research Q&A, meeting notes, etc.)
      pub project_id: Option<String>,  // for KB search scoping
  }
  ```

- `RecipeDocResult` struct (extends GeneratedDoc):
  ```rust
  pub struct RecipeDocResult {
      pub doc: GeneratedDoc,
      pub recipe_name: String,
      pub kb_sources: Vec<KbSource>,   // KB citations used for kb-strategy fields
  }
  
  pub struct KbSource {
      pub field_name: String,
      pub sources: Vec<String>,  // titles of KB chunks used
  }
  ```

- `generate_recipe_doc()` async function:
  1. Look up recipe by `recipe_id` via `deliverable_recipes::get_recipe_by_template_id()`
  2. If `schema_fields` provided, apply recipe overrides: `deliverable_recipes::apply_recipe_overrides()`
  3. For each field with `fill_strategy == "kb"`:
     - Build search query from `project_name` + field hint
     - Call `hybrid_search(query, project_id, top_k=3)` 
     - Collect search results as KB context for this field
  4. Build enhanced LLM context: merge `request.context` + KB search results
  5. Call `generate_llm_fields_with_recipe()` — variant that uses recipe's `system_prompt` instead of generic one
  6. Merge all fields (user + kb + ai + defaults)
  7. Fill template via docx_filler/xlsx_filler
  8. Return `RecipeDocResult`

- `generate_llm_fields_with_recipe()` — same as `generate_llm_fields()` but accepts custom system_prompt:
  ```rust
  async fn generate_llm_fields_with_recipe(
      llm: &LLMService,
      fields: &[&SchemaField],
      project_name: &Option<String>,
      context: &Option<String>,
      system_prompt: &str,  // recipe's domain-specific prompt
  ) -> Result<HashMap<String, String>, String>
  ```

**Test:** Unit test for `generate_recipe_doc` with mock LLM (recipe lookup, overrides, KB context assembly)

### Task 2: KB Fill Strategy in generate_recipe_doc

**File:** `src-tauri/src/services/doc_generator.rs`

**Add inside `generate_recipe_doc()`:**
- KB field processing loop:
  ```rust
  // For each kb-strategy field, search KB and collect context
  let mut kb_context_parts: Vec<String> = Vec::new();
  let mut kb_sources: Vec<KbSource> = Vec::new();
  
  for field in &kb_fields {
      let query = format!("{} {}", project_name.as_deref().unwrap_or(""), field.description.as_deref().unwrap_or(""));
      let results = hybrid_search(&query, project_id.as_deref(), 3, embedding, vector_index, bm25, metadata)?;
      
      if !results.is_empty() {
          let source_titles: Vec<String> = results.iter().map(|r| r.title.clone()).collect();
          kb_sources.push(KbSource { field_name: field.name.clone(), sources: source_titles });
          
          let context_text: Vec<String> = results.iter()
              .map(|r| format!("[{}] {}", r.title, r.content))
              .collect();
          kb_context_parts.push(format!("## 知识库参考 - {}\n{}", field.name, context_text.join("\n")));
      }
  }
  ```

- Merge KB context into the LLM context:
  ```rust
  let enhanced_context = match kb_context_parts.is_empty() {
      true => request.context.clone(),
      false => {
          let mut ctx = kb_context_parts.join("\n\n");
          if let Some(ref user_ctx) = request.context {
              ctx.push_str(&format!("\n\n## 用户补充信息\n{}", user_ctx));
          }
          Some(ctx)
      }
  };
  ```

**Note:** `generate_recipe_doc()` needs references to embedding/vector_index/bm25/metadata for hybrid_search. These are passed from the Tauri command layer.

### Task 3: Tauri Commands in lib.rs

**File:** `src-tauri/src/lib.rs`

**Add 3 new commands:**

1. `generate_recipe_doc` — Main recipe-aware generation entry point
   ```rust
   #[tauri::command]
   async fn generate_recipe_doc(
       state: State<'_, AppState>,
       request: RecipeDocRequest,
   ) -> Result<RecipeDocResult, String>
   ```
   Calls `doc_generator::generate_recipe_doc()` with all service references.

2. `generate_from_research` — Convenience for research report generation
   ```rust
   #[tauri::command]
   async fn generate_from_research(
       state: State<'_, AppState>,
       recipe_id: String,       // "investigation_report"
       template_path: String,
       output_path: String,
       fields: HashMap<String, String>,
       schema_fields: Option<Vec<SchemaField>>,
       project_name: Option<String>,
       research_notes: String,  // aggregated Q&A text from research session
       project_id: Option<String>,
   ) -> Result<RecipeDocResult, String>
   ```
   Wraps `generate_recipe_doc` with `context = Some(research_notes)`.

3. `generate_from_meeting` — Convenience for meeting minutes generation
   ```rust
   #[tauri::command]
   async fn generate_from_meeting(
       state: State<'_, AppState>,
       recipe_id: String,       // "meeting_minutes"
       template_path: String,
       output_path: String,
       fields: HashMap<String, String>,
       schema_fields: Option<Vec<SchemaField>>,
       project_name: Option<String>,
       meeting_transcript: String,  // meeting notes / Whisper transcript
       project_id: Option<String>,
   ) -> Result<RecipeDocResult, String>
   ```
   Wraps `generate_recipe_doc` with `context = Some(meeting_transcript)`.

**Register all 3 in `invoke_handler`:**
```rust
generate_recipe_doc,
generate_from_research,
generate_from_meeting,
```

### Task 4: Product Store Integration in generate_recipe_doc

**File:** `src-tauri/src/services/doc_generator.rs` + `src-tauri/src/lib.rs`

**In `generate_recipe_doc` Tauri command:**
- After successful generation, save product to `product_store`:
  ```rust
  let product = store.create_product(
      &recipe.name,
      &request.template_path,
      &result.doc.output_path,
      &request.project_name.as_deref().unwrap_or("default"),
  )?;
  ```
- Store input_data JSON for regeneration support:
  ```rust
  let input_json = serde_json::to_string(&serde_json::json!({
      "recipe_id": request.recipe_id,
      "template_path": request.template_path,
      "fields": request.fields,
      "schema_fields": request.schema_fields,
      "project_name": request.project_name,
      "context": request.context,
      "project_id": request.project_id,
  }))?;
  store.add_version(product.id, &input_json, &result.doc.output_path)?;
  ```

### Task 5: Unit Tests

**File:** `src-tauri/src/services/doc_generator.rs` (tests module)

**Add tests:**
- `test_recipe_doc_request_serialization` — verify RecipeDocRequest round-trips through serde
- `test_kb_source_format` — verify KbSource struct serialization
- `test_generate_llm_fields_with_recipe_prompt` — verify custom system_prompt is used (mock LLM)

**File:** `src-tauri/src/services/deliverable_recipes.rs` (existing tests)
- No changes needed — existing tests already cover recipe lookup and override application

---

## Execution Order

```
Task 1 (RecipeDocRequest + generate_recipe_doc) ──→ Task 2 (KB fill) ──→ Task 3 (Tauri commands) ──→ Task 4 (Product store) ──→ Task 5 (Tests)
```

Tasks 1-2 are in the same file and must be sequential.
Task 3 depends on Task 1+2 (needs the new types).
Task 4 depends on Task 3.
Task 5 depends on Task 1+2.

---

## Verification

1. `cargo check` — no compilation errors
2. `cargo test -p kingdee-kb` — all existing + new tests pass
3. Manual: invoke `generate_recipe_doc` with recipe_id="investigation_report" → verify recipe system_prompt is used
4. Manual: invoke `generate_from_research` with research_notes → verify context is passed to LLM
5. Manual: invoke `generate_from_meeting` with meeting_transcript → verify context is passed

---

## Out of Scope

- Frontend UI for recipe selection (Phase 14)
- ResearchSession CRUD (Phase 13)
- Whisper integration for meeting transcript (Phase 12)
- Streaming progress events for generation (nice-to-have, not required)
- Draft product status lifecycle (Phase 13)

---

*PLAN.md created: 2026-05-24 — Phase 10 execution plan*