# Phase 4: BM25 全文检索 — Context

**Gathered:** 2026-05-23 | **Status:** Ready for planning | **Mode:** Auto

<domain>
## Phase Boundary
**Goal**: 中文关键词检索可用。使用 tantivy + jieba 实现中文 BM25 全文检索。

**Requirements**: SRCH-02 | **Dependencies**: Phase 2（需要 rusqlite metadata DB schema；可并行 Phase 3）

**Success Criteria**:
1. 输入中文关键词返回匹配的知识片段，按 BM25 得分排序
2. BM25 使用 jieba cut_for_search 分词，非简单字符切分
3. BM25 索引支持增量更新（新增/删除知识后索引同步）
</domain>

<decisions>
### Auto-Selected
[auto] **BM25 引擎**: tantivy（Rust 原生全文搜索引擎）| [auto] **分词器**: jieba-rs（cut_for_search 搜索引擎模式）| [auto] **索引更新**: 增量更新（监听 chunk 增删事件）
</decisions>

<canonical_refs>
- `.planning/ROADMAP.md` | `.planning/RESEARCH/STACK.md` | `.planning/RESEARCH/PITFALLS.md` §3 中文分块
</canonical_refs>

<deferred>BM25 参数调优（k1, b）— Phase 5 统一调优</deferred>
