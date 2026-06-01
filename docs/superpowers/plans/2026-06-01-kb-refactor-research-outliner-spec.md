# 知识库重构 + 调研大纲编辑器及脑图视图 — 规格概要

> **状态**: 讨论稿，等待确认  
> **未获用户明确通知，不得进入任何实现阶段**  
> **前置文档**:  
>   - `2026-06-01-llm-wiki-research-report.md` — llm_wiki 深度调研（commit b86d81b，6 项核心机制拆解）  
>   - `2026-06-01-kb-refactor-design-decisions.md` — 8 项设计决策 + 取舍理由 + 暂不做清单  

---

本文档是快速导航入口。所有内容已迁移到上述两份文档。

## 调研摘要（详见调研报告）

| 维度 | llm_wiki 做法 | KingdeeKB 现状 |
|------|-------------|----------------|
| 知识存储 | 可读 Markdown 页面 (wiki/*.md) | 二进制 chunk |
| 摄入模式 | 两步 CoT（分析→生成） | 一步（extract→chunk→embed） |
| 增量缓存 | SHA256 + 文件存在性三重验证 | SHA256 文档级去重 |
| 队列 | 持久化 JSON + 崩溃恢复 + 3 次重试 | 无队列 |
| 知识图谱 | 4 信号 (wikilink/src overlap/AA/type) | 无 |
| 检索管道 | 4 阶段 (keyword+vector+graph+budget) | 2 阶段 (vector+BM25) |

## 设计决策摘要（详见设计决策文档）

| 决策 | 选项 | 选择 |
|------|------|------|
| 架构演进 | 推倒 / **叠加** / 迁移 | **叠加**：保留 chunk，新增 wiki_pages |
| 两步摄入 | 纯 Rust / **Rust+LLM 可选** / 纯 LLM | **Step 1 Rust，Step 2 LLM 可选(默认关)** |
| 原始资料 | 不保留 / **保留副本** / 仅引用 | **保留副本**到 raw/{project}/sources/ |
| 调研大纲 vs Q&A | 替代 / **并存** | **并存**：Q&A=原始记录，大纲=结构化整理，脑图=只读展示 |
| 大纲/脑图渲染 | markmap / mind-elixir / ReactFlow / D3 / AntV | **markmap(MVP)**：只读脑图；后续脑图内编辑优先评估 mind-elixir |

## 实现顺序

```
阶段一: 基础设施加固（2.5 天）     ← 待确认后可开始
阶段二: 原始资料+持久化队列（2 天） ← 依赖阶段一
阶段三: 编译知识层（4 天）         ← 依赖阶段二，可与阶段四并行
阶段四: 调研大纲编辑器 + 脑图视图（6 天） ← 不依赖阶段二/三，可与阶段三并行
阶段五: 知识图谱+图检索（4 天）    ← 依赖阶段三有足够 wiki_pages 数据
```

## 完整「暂不做」清单

Chrome 扩展、HTTP API 服务器、REVIEW 审查系统、Louvain 社区可视化、Deep Research、VLM caption、LLM 自动 wikilink 补全、长文档 checkpoint、Step 2.5 Review Suggestion、队列管理 UI、脑图内直接编辑（当前只读，后续评估 mind-elixir）、LLM 自动生成大纲

