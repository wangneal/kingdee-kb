# Project State: KingdeeKB

**Project:** KingdeeKB — 金蝶ERP实施顾问本地 RAG 知识管理工具
**Milestone:** v0.1 MVP
**Started:** 2026-05-23
**Last Updated:** 2026-05-23

---

## Project Reference

**Core Value:** 让金蝶ERP实施顾问能快速检索历史案例并基于检索结果进行 AI 辅助问答，把分散的项目经验转化为可复用的结构化知识。

**Current Focus:** Phase 1 完成 — Tauri 2.x 项目骨架就绪。下一步：Phase 2（嵌入与向量存储引擎）技术验证。

**Tech Stack:** Tauri 2.x + React 19 + TypeScript + TailwindCSS + usearch (HNSW) + rusqlite + fastembed-rs (bge-small-zh-v1.5) + tantivy (BM25 + jieba) + OpenAI API

---

## Current Position

| Metric | Value |
|--------|-------|
| **Phase** | 1 — 项目脚手架与基础设施 ✅ |
| **Plan** | 01-scaffold (12/12 tasks) |
| **Status** | Ready for Phase 2 |
| **Progress** | 1/8 phases complete |

```
Phase 1 [██████████] 100% ✅
Phase 2 [··········] 0%
Phase 3 [··········] 0%
Phase 4 [··········] 0%
Phase 5 [··········] 0%
Phase 6 [··········] 0%
Phase 7 [··········] 0%
Phase 8 [··········] 0%
```

---

## Performance Metrics

| Metric | Target | Current |
|--------|--------|---------|
| Requirements covered | 35/35 | 35/35 ✓ |
| Phases planned | 8 | 8 |
| Phases executed | 8 | 1 |
| UAT passed | — | — |

---

## Accumulated Context

### Key Decisions (from research)

1. 放弃 ChromaDB → 采用 `usearch` HNSW + `rusqlite` 元数据（避免 sidecar 进程 + Python 运行时 ~200MB）
2. Embedding 模型切换：`all-MiniLM-L6-v2` → `bge-small-zh-v1.5`（中文语义退化风险消除，C-MTEB 61.77）
3. 中文分块使用中文感知分隔符（`。！？；，`），避免英文分隔符导致语义断裂
4. API Key 通过 Windows Credential Manager 存储，不落盘明文 JSON
5. 检索默认按项目隔离，防止多项目知识混淆

### Active Todos

- [ ] Phase 2 Spike: 验证 `usearch` + `bge-small-zh-v1.5` + ONNX Runtime 在 Windows 上的端到端可行性
- [ ] 替换 splash.png 为品牌 logo + "KingdeeKB" 文字
- [ ] 在中文 Windows 环境验证 Keyring Store 兼容性

### Blockers

_(None)_

### Open Questions

- Phase 2 需 Spike 验证：`usearch` + `bge-small-zh-v1.5` + ONNX Runtime 在 Windows 上的端到端可行性
- Phase 5 需评估数据集：准备 50-100 个中文 ERP 查询-答案对用于检索评估
- Tauri Plugin Keyring Store 在中文 Windows 环境的兼容性待验证

---

## Session Continuity

### Last Session

- **Date:** 2026-05-23
- **Action:** Phase 1 执行完成 — 12 个任务全部提交，项目骨架就绪
- **Next:** Phase 2 技术验证 (`/gsd-spike`) 或 Phase 2 规划 (`/gsd-plan-phase 2`)

### Handoff Notes

- 路线图中 Phase 2 标注了 Spike 建议——在规划 Phase 2 前执行 `/gsd-spike` 验证向量方案
- 所有 v1 需求已映射到 8 个阶段，无遗漏
- v0.2 特性（Anthropic API、Git 知识包、macOS/Linux）在 REQUIREMENTS.md v2 区域跟踪，不纳入当前里程碑

---

*State initialized: 2026-05-23*
