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
| **Phase** | — (not started) |
| **Plan** | — |
| **Status** | Design complete, pending implementation |
| **Progress** | 0/7 phases |

```
Phase 9  [··········] 0%
Phase 10 [··········] 0%
Phase 11 [··········] 0%
Phase 12 [··········] 0%
Phase 13 [··········] 0%
Phase 14 [··········] 0%
Phase 15 [··········] 0%
```

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

*State updated: 2026-05-24 — v0.2 合并为智能调研与文档生成*
