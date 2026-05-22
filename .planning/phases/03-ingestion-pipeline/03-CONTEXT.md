# Phase 3: 入库流水线 — Context

**Gathered:** 2026-05-23
**Status:** Ready for planning
**Mode:** Auto-generated (--auto)

<domain>
## Phase Boundary

**Goal**: 知识能通过多种方式（粘贴文本、拖拽文件、选择文件夹）导入，系统自动完成解析、清洗、递归分块、标签提取、去重检测和向量化入库。

**Requirements**: KNOW-01, KNOW-02, KNOW-03, KNOW-04, KNOW-05, INFR-10, INFR-11

**Success Criteria**:
1. 用户粘贴文本后系统自动完成分块、向量化、存储全流程
2. 用户拖入 .md/.txt 文件后自动解析（提取文件名作标题、按 # 层级提取章节结构）
3. 用户选择文件夹后批量扫描并导入所有 .md/.txt 文件
4. 自动提取标签（从文件名、章节路径推断），相同内容重复导入时 SHA256 去重
5. 分块使用中文感知分隔符（。！？；，），保留层级元数据

**Dependencies**: Phase 2（需要 embedding 引擎和向量存储；需要 rusqlite 元数据库 schema）
</domain>

<decisions>
## Implementation Decisions

### Auto-Selected Decisions (--auto mode)

[auto] **分块策略** — Q: "递归分块 vs 固定大小分块 vs 语义分块?" → Selected: 递归分块（H2→段落→句子）— 保留文档结构，对 ERP 知识库场景最优（PITFALLS §3）

[auto] **中文分隔符** — Q: "默认分隔符 vs 中文感知分隔符?" → Selected: 中文感知分隔符 `["\n\n", "\n", "。", "！", "？", "；", "，"]` — 避免英文句点切割中文（PITFALLS §3）

[auto] **目标 chunk 大小** — Q: "256 vs 384 vs 512 tokens?" → Selected: 384 tokens（bge 模型最大输入 512，384 留空间给上下文）

[auto] **chunk 重叠** — Q: "0 vs 50 vs 100 tokens?" → Selected: 50 tokens 重叠 — 平衡上下文连续性和存储效率

[auto] **去重方式** — Q: "SHA256 内容哈希 vs 标题匹配 vs 全文比较?" → Selected: SHA256 内容哈希 — 精确、快速、O(1) 查重

[auto] **标签提取** — Q: "自动推断 vs 手动标注 vs 混合?" → Selected: 自动推断（文件名 token + 章节路径 token）— v0.1 MVP 简化实现

[auto] **批量处理** — Q: "串行 vs 并行?" → Selected: 串行批处理（64 chunks/batch）— 桌面端 4 核环境，向量化 CPU 密集型，串行避免内存峰值

[auto] **Markdown 清洗** — Q: "保留 vs 移除代码块?" → Selected: 保留代码块结构标记（不参与分块但保留结构）— ERP 场景可能包含配置示例
</decisions>

<code_context>
## Existing Code

Phase 1: Tauri 2.x scaffold + ~/.kingdee-kb/ directory structure
Phase 2: (pending) embedding engine + usearch index + rusqlite metadata DB

**Reference patterns**:
- `.planning/research/ARCHITECTURE.md` — §3 Ingestion Pipeline 数据流
- `.planning/research/PITFALLS.md` — §3 分块陷阱
- SPEC.md §5.2-5.3 — 解析引擎和分块引擎规格
</code_context>

<specifics>
## Specific Ideas

- ChunkMetadata struct: source_file, title, section_path, heading, line_start, line_end, tags, created_at
- 最小 chunk 合并：< 100 tokens 的 chunk 合并到相邻块
- 最大 chunk 截断：> 1024 tokens 强制截断
- Tauri commands: `ingest_text(text: String)`, `ingest_file(path: String)`, `ingest_directory(path: String)`
- 进度回调：通过 Tauri event 向前端报告入库进度
</specifics>

<canonical_refs>
## Canonical References

- `.planning/ROADMAP.md` — Phase 3 需求映射
- `.planning/REQUIREMENTS.md` — KNOW-01~05, INFR-10, INFR-11
- `.planning/research/ARCHITECTURE.md` — §3 Ingestion Pipeline
- `.planning/research/PITFALLS.md` — §3 分块策略陷阱
- `SPEC.md` — §5.2 解析引擎、§5.3 分块引擎
</canonical_refs>

<deferred>
## Deferred Ideas

- .docx / .pdf 解析 — v0.3
- 文件系统监视自动入库 — Phase 10
- 语义分块（基于 embedding 相似度动态切分）— v0.3
- 自定义分块规则编辑器 — 远期
</deferred>
