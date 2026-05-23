# Project State: KingdeeKB

**Project:** KingdeeKB — 金蝶ERP实施顾问本地 RAG 知识管理 + 智能文档生成工具
**Milestone:** v0.2 — 智能文档生成
**Started:** 2026-05-23
**Last Updated:** 2026-05-23

---

## Project Reference

**Core Value:** 让金蝶ERP实施顾问能快速检索历史案例、AI 问答、并按实施方法论模板自动生成标准化交付物。

**Current Focus:** v0.2 需求定义 — 基于 85 个实施方法论 V10.0 模板的智能文档生成。

**Tech Stack:** Tauri 2.x + React 19 + TypeScript + TailwindCSS + usearch (HNSW) + rusqlite + fastembed-rs (bge-small-zh-v1.5) + tantivy (BM25 + jieba) + OpenAI API

---

## Current Position

| Metric | Value |
|--------|-------|
| **Phase** | — (defining requirements) |
| **Plan** | — |
| **Status** | Requirements |
| **Progress** | 0/6 phases |

```
Phase 9  [··········] 0%
Phase 10 [··········] 0%
Phase 11 [··········] 0%
Phase 12 [··········] 0%
Phase 13 [··········] 0%
Phase 14 [··········] 0%
```

---

## Previous Milestone

**v0.1 — 本地 RAG 知识管理 MVP**: ✅ Complete (8/8 phases, 35 requirements, 35 commits)

---

## Accumulated Context

### Key Decisions (from v0.1)

1. 放弃 ChromaDB → `usearch` HNSW + `rusqlite` 元数据
2. Embedding: `bge-small-zh-v1.5` (C-MTEB 61.77)
3. 中文分块使用中文感知分隔符（`。！？；，`）
4. API Key 通过 Windows Credential Manager 存储
5. 检索默认按项目隔离

### Blockers

_(None)_

### Open Questions (v0.2)

- .docx 模板解析方案：`docx-rs` vs `python-docx` via sidecar？
- .xlsx 模板解析方案：`calamine` vs `rust_xlsxwriter`？
- 产物生成：LLM 填文本字段 + Rust 填结构化字段的混合方案？
- 模板的字段占位符约定：`{字段名}` 还是自定义标记？

---

*State updated: 2026-05-23 — v0.2 milestone started*
