# Project State: KingdeeKB

**Project:** KingdeeKB — 金蝶ERP实施顾问本地 RAG 知识管理 + 智能调研与文档生成工具
**Milestone:** v0.2 — 智能调研与文档生成（合并版）
**Started:** 2026-05-24
**Last Updated:** 2026-05-24

---

## Project Reference

**Core Value:** 让金蝶ERP实施顾问能快速检索历史案例、AI 问答、调研辅助（实时问题推荐+录音转写）、并按实施方法论模板自动生成标准化交付物。

**Current Focus:** v0.2 实现调研辅助（问题推荐、Whisper 语音、调研记录）和文档生成（模板解析、LLM 填充、交付物生成），打通调研→报告闭环。

**Tech Stack:** Tauri 2.x + React 19 + TypeScript + TailwindCSS + usearch (HNSW) + rusqlite + fastembed-rs (bge-small-zh-v1.5) + tantivy (BM25 + jieba) + whisper-rs + OpenAI API

---

## Current Position

| Metric | Value |
|--------|-------|
| **Phase** | 15 (complete) |
| **Plan** | `.planning/phases/15-integration` ✅ |
| **Status** | **Phase 13-15 COMPLETE** — Research session management + unified frontend + integration tests |
| **Progress** | 7/7 phases |

```
Phase 9  [██████████] 100% ✅
Phase 10 [██████████] 100% ✅
Phase 11 [██████████] 100% ✅
Phase 12 [██████████] 100% ✅
Phase 13 [██████████] 100% ✅
Phase 14 [██████████] 100% ✅
Phase 15 [██████████] 100% ✅
```

## v0.2 Milestone: 智能调研与文档生成 — ✅ COMPLETE

All 7 phases done. 121 unit tests pass (0 failures), frontend builds clean.

### Phase 12 Summary
- whisper-rs 0.16 integration (WhisperService)
- cpal 0.15 microphone capture (AudioCapture with dedicated audio thread)
- Chinese post-processing (punctuation restoration, duplicate removal, short sentence merge)
- Model management: download + lazy load
- Tauri commands: load_whisper_model, get_whisper_status, start/stop_whisper_recording

### Phase 13 Summary
- ResearchSessionStore: CRUD for interview sessions and Q&A records
- Export: CSV + Markdown
- SQLite tables: research_sessions, session_qa_records

### Phase 14 Summary
- ResearchAssistant.tsx: session list/detail/new views
- VoiceRecorder integration: mic → Whisper → question text
- Q&A CRUD with inline editing
- Route + sidebar navigation

### Phase 15 Summary
- 121 unit tests all passing
- Frontend build verified
- chinese_postprocess bugs fixed (regex backreference, whitespace loop)

---

## Previous Milestone

**v0.1 — 本地 RAG 知识管理 MVP**: ✅ Complete (8/8 phases, 35 requirements, 35 commits)

---

## Accumulated Context

### Key Decisions (from v0.1)

1. 放弃 ChromaDB → usearch HNSW + rusqlite 元数据
2. Embedding: bge-small-zh-v1.5 (C-MTEB 61.77)
3. 中文分块使用中文感知分隔符（。！？；，）
4. API Key 通过 Windows Credential Manager 存储
5. 检索默认按项目隔离

### Key Decisions (v0.2)

1. 合并原 v0.2（文档生成）和原 v0.3（调研助手）为一个版本
2. 企业版先做（25 份调研提纲），旗舰版架构预留
3. STT 使用 Whisper 本地模型（whisper-rs），零费用离线运行
4. 腾讯会议集成通过网页扩展应用（H5 侧边栏）
5. Edition 架构：同一索引 + 版本 metadata filter

### Blockers

_(None)_

### Open Questions (v0.2)

- Whisper tiny vs small 模型在目标硬件上的 RTF 表现？
- 腾讯会议侧边栏 H5 与桌面端通信协议细节？
- 调研报告模板字段映射方案？

---

*State updated: 2026-05-24 — v0.2 全部 7 阶段完成*
