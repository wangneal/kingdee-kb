# Phase 5: 混合检索引擎 — Context

**Gathered:** 2026-05-23 | **Status:** Ready for planning | **Mode:** Auto

<domain>
## Phase Boundary
**Goal**: 向量检索 + BM25 检索经 RRFR 融合排序，按项目隔离结果。

**Requirements**: SRCH-03, SRCH-08 | **Dependencies**: Phase 3（chunk 数据）, Phase 4（BM25 索引）

**Success Criteria**:
1. 同一查询同时触发向量（top-30）和 BM25（top-30），经 RRFR 融合后返回结果
2. 检索结果默认按 project 字段隔离
3. 结果含相关性得分和来源标注
</domain>

<decisions>
### Auto-Selected
[auto] **融合算法**: RRFR（k=60，Phase 5 做网格搜索最优值）| [auto] **隔离策略**: ChromaDB where 过滤 或 rusqlite project_id 字段 | [auto] **向量 top_n**: 30, BM25 top_n: 30, 最终 top_k: 5
</decisions>

<canonical_refs>
- `.planning/ROADMAP.md` | `.planning/RESEARCH/ARCHITECTURE.md` §5 | `.planning/RESEARCH/PITFALLS.md` §5
</canonical_refs>

<deferred>RRFR k 值网格搜索 — 需中文 ERP 评估数据集</deferred>
