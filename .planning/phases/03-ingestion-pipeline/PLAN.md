# Phase 3: 入库流水线 — PLAN

**Created:** 2026-05-23
**Status:** Executing

---

## Task Groups

### TG-1: Text Cleaner (`services/text_cleaner.rs`)
**Goal:** Clean raw text — remove Markdown noise, normalize whitespace, preserve code blocks.

**Atomic Tasks:**
1. Create `services/text_cleaner.rs` with `pub fn clean_text(raw: &str) -> String`
2. Strip Markdown noise: link syntax `[text](url)` → `text`, image `![alt](url)` → remove, HTML tags → remove
3. Normalize whitespace: collapse multiple spaces/newlines, trim lines
4. Preserve fenced code blocks (```...```) as opaque tokens — don't break content inside
5. Unit tests for Chinese text cleaning, mixed MD content

**Deliverable:** `text_cleaner.rs` with tests passing

---

### TG-2: Recursive Chunker (`services/chunker.rs`)
**Goal:** Split cleaned text into chunks respecting document structure and Chinese sentence boundaries.

**Atomic Tasks:**
1. Create `services/chunker.rs` with `ChunkMetadata` struct and `pub fn recursive_chunk(text: &str, meta: &ChunkInputMeta) -> Vec<Chunk>`
2. Implement `ChunkMetadata`: `source_file`, `title`, `section_path`, `heading`, `line_start`, `line_end`, `tags`
3. Level 1 split: `\n## ` (H2 headings) — extract section_path from heading hierarchy
4. Level 2 split: `\n\n` (paragraphs)
5. Level 3 split: Chinese sentence separators `。！？；，`
6. Merge small chunks: < 100 chars → merge with adjacent chunk
7. Truncate large chunks: > 1500 chars → force split at sentence boundary
8. Target chunk size: ~384 tokens (≈ 500-700 chars for Chinese), 50-char overlap
9. Unit tests for Chinese document chunking, edge cases

**Deliverable:** `chunker.rs` with tests passing

---

### TG-3: Dedup & Tag Extraction (`services/ingestion_helpers.rs`)
**Goal:** SHA256 content hashing for dedup, automatic tag extraction from filename and section path.

**Atomic Tasks:**
1. Create `services/ingestion_helpers.rs`
2. `pub fn compute_sha256(content: &str) -> String` — use existing `sha2` crate
3. `pub fn extract_tags(filename: &str, section_path: Option<&str>) -> Vec<String>` — tokenize filename (split on `-`, `_`, spaces) + section path segments
4. `pub fn extract_title_from_filename(filename: &str) -> String` — strip extension, replace `-`/`_` with spaces
5. Unit tests

**Deliverable:** `ingestion_helpers.rs` with tests passing

---

### TG-4: Ingestion Pipeline (`services/ingestion.rs`)
**Goal:** Orchestrate the full pipeline: read → clean → chunk → dedup → embed → store.

**Atomic Tasks:**
1. Create `services/ingestion.rs` with `IngestionPipeline` struct
2. `pub fn ingest_text(text: &str, title: &str, app_state: &AppState) -> Result<IngestResult, String>` — full pipeline for pasted text
3. `pub fn ingest_file(path: &Path, app_state: &AppState) -> Result<IngestResult, String>` — read file → extract title/filename → ingest
4. `pub fn ingest_directory(path: &Path, app_state: &AppState) -> Result<Vec<IngestResult>, String>` — walk dir, filter .md/.txt, ingest each
5. `IngestResult` struct: `document_id`, `chunk_count`, `skipped` (dedup), `tags`
6. Progress event emission via `AppHandle::emit("ingestion-progress", payload)`
7. Integration: wire into `AppState` services (embedding, vector_index, metadata)

**Deliverable:** `ingestion.rs` compiling

---

### TG-5: Tauri Commands
**Goal:** Expose ingestion as Tauri commands callable from frontend.

**Atomic Tasks:**
1. Add `ingest_text(text: String, title: String)` command in `lib.rs`
2. Add `ingest_file(path: String)` command in `lib.rs`
3. Add `ingest_directory(path: String)` command in `lib.rs`
4. Register all in `invoke_handler`
5. `cargo check` passes

**Deliverable:** Commands registered and compiling

---

### TG-6: Module Wiring & Build Verification
**Goal:** All modules properly exported, build clean.

**Atomic Tasks:**
1. Add `pub mod text_cleaner; pub mod chunker; pub mod ingestion_helpers; pub mod ingestion;` to `services/mod.rs`
2. `cargo build` passes
3. `cargo test` passes (existing + new tests)

**Deliverable:** Full build + test pass

---

## Verification Checklist

- [ ] `clean_text()` strips MD noise, preserves code blocks, normalizes whitespace
- [ ] `recursive_chunk()` splits on H2 → paragraph → sentence with Chinese separators
- [ ] Chunks < 100 chars merged, > 1500 chars truncated
- [ ] `compute_sha256()` produces consistent hashes, dedup skips duplicate content
- [ ] `extract_tags()` infers tags from filename + section path
- [ ] `ingest_text()` full pipeline: clean → chunk → embed → store
- [ ] `ingest_file()` reads .md/.txt, extracts title from filename
- [ ] `ingest_directory()` recursively scans, imports all .md/.txt
- [ ] Progress events emitted during ingestion
- [ ] `cargo test` all pass
- [ ] `cargo build` succeeds
