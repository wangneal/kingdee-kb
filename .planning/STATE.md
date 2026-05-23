# Project State: KingdeeKB

**Project:** KingdeeKB — 金蝶ERP实施顾问本地 RAG 知识管理工具
**Milestone:** v0.1 MVP
**Started:** 2026-05-23
**Last Updated:** 2026-05-23

---

## Project Reference

**Core Value:** 让金蝶ERP实施顾问能快速检索历史案例并基于检索结果进行 AI 辅助问答，把分散的项目经验转化为可复用的结构化知识。

**Current Focus:** Phase 5 完成 — 混合检索引擎（RRFR 融合向量+BM25）已就绪。下一步：Phase 6（LLM 集成与 AI 问答）。

**Tech Stack:** Tauri 2.x + React 19 + TypeScript + TailwindCSS + usearch (HNSW) + rusqlite + fastembed-rs (bge-small-zh-v1.5) + tantivy (BM25 + jieba) + OpenAI API

---

## Current Position

| Metric | Value |
|--------|-------|
| **Phase** | 5 — 混合检索引擎 ✅ |
| **Plan** | 05-hybrid-search (completed) |
| **Status** | Ready for Phase 6 |
| **Progress** | 5/8 phases complete |

```
Phase 1 [██████████] 100% ✅
Phase 2 [██████████] 100% ✅
Phase 3 [██████████] 100% ✅
Phase 4 [██████████] 100% ✅
Phase 5 [██████████] 100% ✅
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
| Phases executed | 8 | 5 |
| UAT passed | — | — |

---

## Accumulated Context

### Key Decisions (from research)

1. 放弃 ChromaDB → 采用 `usearch` HNSW + `rusqlite` 元数据（避免 sidecar 进程 + Python 运行时 ~200MB）
2. Embedding 模型切换：`all-MiniLM-L6-v2` → `bge-small-zh-v1.5`（中文语义退化风险消除，C-MTEB 61.77）
3. 中文分块使用中文感知分隔符（`。！？；，`），避免英文分隔符导致语义断裂
4. API Key 通过 Windows Credential Manager 存储，不落盘明文 JSON
5. 检索默认按项目隔离，防止多项目知识混淆

### Key Decisions (Phase 2 execution)

6. usearch 禁用 numkong feature（默认关闭）以避免 MSVC C99 编译错误
7. HNSW 使用 BF16 量化（vs F32）减少 ~50% 内存占用
8. bge-small-zh-v1.5 ONNX 模型下载需 HuggingFace 代理或预打包方案
9. MetadataStore 使用 SHA256 UNIQUE 约束做文档去重

### Active Todos

- [x] Phase 2 Spike: usearch + bge-small-zh-v1.5 ONNX 在 Windows 上的可行性验证
- [ ] 解决 bge-small-zh-v1.5 ONNX 模型下载（HuggingFace 被墙）
- [ ] 替换 splash.png 为品牌 logo + "KingdeeKB" 文字
- [ ] 在中文 Windows 环境验证 Keyring Store 兼容性

### Blockers

_(None)_

### Open Questions

- bge-small-zh-v1.5 模型下载方案：HF_ENDPOINT 镜像 vs 预打包 vs Modelscope？
- Phase 5 需评估数据集：准备 50-100 个中文 ERP 查询-答案对用于检索评估
- Tauri Plugin Keyring Store 在中文 Windows 环境的兼容性待验证

---

## Session Continuity

### Last Session

- **Date:** 2026-05-23
- **Action:** Phase 2 执行完成 — 9 个任务全部提交，嵌入与向量存储引擎就绪
- **Summary:** `.planning/phases/02-embedding-engine/02-02-SUMMARY.md`
- **Next:** Phase 3 知识入库 (`/gsd-plan-phase 3`)

### Handoff Notes

- Phase 1-5 全部完成，后端基础设施就绪
- Phase 3 入库流程已验证，Phase 4 BM25 + Phase 5 混合检索已就绪
- 下一步进入 LLM 集成（Phase 6），需要 OpenAI API Key 配置

---

*State updated: 2026-05-23*
