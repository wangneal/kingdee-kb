# KingdeeKB

## What This Is

KingdeeKB 是一款面向金蝶ERP实施顾问的**本地知识管理工具**，基于 RAG（检索增强生成）技术。v0.1 实现了知识库检索与 AI 问答，v0.2 新增基于金蝶实施方法论 V10.0 模板的**标准化文档智能生成**能力。完全开源（MIT License），所有数据本地存储。

## Core Value

让金蝶ERP实施顾问能**快速检索历史案例、基于检索结果 AI 问答、并根据模板自动生成标准化交付物**，把分散的项目经验转化为可复用的结构化知识和符合方法论标准的产物。

## Current Milestone: v0.2 智能文档生成

**Goal:** 基于金蝶实施方法论 V10.0 的 85 个交付物模板，通过 LLM + 用户输入 + 知识库检索，自动生成标准化产物（调研报告、业务蓝图、PCR、上线单、验收单等）。

**Target features:**
- 模板解析引擎：解析 .docx/.xlsx 模板的字段、章节结构
- 文档生成核心：LLM 填充模板，信息追问缺失字段
- 智能补全：从知识库检索相关历史内容辅助填充
- 产物管理：产物浏览、导出、历史记录
- 向导式生成 UI：分步引导用户输入并输出产物

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] 知识添加：粘贴文本、拖入 .md/.txt 文件
- [ ] 知识浏览：左侧树形目录 + 右侧内容预览
- [ ] 知识删除：单条删除
- [ ] 向量检索：基于本地 embedding（bge-small-zh-v1.5）的语义检索
- [ ] 关键词检索：BM25 关键词匹配
- [ ] 混合检索：RRFR 融合向量+关键词结果
- [ ] AI 问答：兼容 OpenAI Chat Completions API，基于检索上下文生成回答
- [ ] API 配置：用户自填 API Key，支持自定义 Endpoint 和 Model
- [ ] 跨平台客户端：Windows x64（首发），macOS/Linux 后续

### Out of Scope

- Anthropic API 兼容 — v0.2 规划
- Git 知识包导入 — v0.2 规划
- .docx / .pdf 解析 — v0.3 规划
- 深色模式 — v0.3 规划
- 团队协作/同步 — 远期规划
- 云端知识库 / LLM 代理服务 — 永久不做
- 官方知识包服务器 — 永久不做

## Context

- **目标用户**：金蝶ERP实施顾问（个人为主，小团队为辅）
- **使用场景**：项目实施过程中快速检索历史案例、沉淀新经验
- **本地优先**：所有数据（知识库、向量库、配置）存储在 `~/.kingdee-kb/`，不上传任何服务器
- **用户自备 LLM**：用户填入自己的 OpenAI API Key，应用不做代理
- **社区知识包**：知识包由社区贡献者自行托管（GitHub 等），用户通过 git clone 导入
- **开源协议**：MIT License，客户端完全开源

## Constraints

- **Tech stack**: Tauri 2.x (Rust + WebView2) + React 19 + TypeScript + TailwindCSS 4 + Biome + usearch (HNSW) + rusqlite
- **Embedding**: `bge-small-zh-v1.5` 本地模型（512维，~90MB），首次自动下载，无 API 费用，C-MTEB 61.77
- **LLM**: 用户自备 API Key，兼容 OpenAI 协议（v0.1），Anthropic 兼容（v0.2）
- **Platform**: Windows x64 首发，macOS/Linux 后续
- **Storage**: 本地文件系统 + usearch HNSW 索引 + rusqlite 元数据库
- **License**: MIT（开源）
- **Budget**: 零服务器成本（纯客户端）

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Tauri 而非 Electron | 更小的包体积、更好的性能、Rust 原生能力 | ✅ Phase 1 — 脚手架就绪 |
| usearch + rusqlite 而非 ChromaDB | 本地优先、零运维、避免 Python sidecar ~200MB | ✅ Phase 2 — 已集成 |
| bge-small-zh-v1.5 本地 embedding | C-MTEB 61.77，中文语义优于 all-MiniLM-L6-v2 | ✅ Phase 2 — ONNX 模型就绪 |
| 先支持 OpenAI 协议（v0.1），Anthropic 后续（v0.2） | 降低 MVP 复杂度 | Pending (Phase 6) |
| 递归分块策略（H2→段落→中文分隔符） | 保留文档结构，中文感知分隔符防止语义断裂 | ✅ Phase 3 — 已实现 |
| 混合检索（向量+BM25 + RRFR） | 兼顾语义和关键词匹配，提升召回率 | ✅ Phase 5 — RRFR 融合就绪 |
| Windows x64 首发 | 金蝶顾问主要使用 Windows 环境 | ✅ 持续验证中 |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-05-23 — v0.1 complete, v0.2 milestone started*
