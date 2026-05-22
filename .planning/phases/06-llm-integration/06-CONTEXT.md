# Phase 6: LLM 集成与 AI 问答 — Context

**Gathered:** 2026-05-23 | **Status:** Ready for planning | **Mode:** Auto

<domain>
## Phase Boundary
**Goal**: AI 能基于知识库检索上下文生成带来源标注的回答。核心价值交付阶段。

**Requirements**: AIQA-02, AIQA-03, AIQA-05, AIQA-06
**Dependencies**: Phase 5（混合检索）

**Success Criteria**:
1. 调用 OpenAI Chat Completions API 并流式返回（SSE）
2. AI 回答基于检索上下文生成（RAG），标注来源（文件名+章节）
3. 知识库无相关内容时明确告知而非编造
4. Token 感知动态上下文窗口管理
</domain>

<decisions>
### Auto-Selected
[auto] **LLM SDK**: reqwest（Tauri 内置 HTTP 客户端）| [auto] **流式**: SSE（Server-Sent Events）逐字返回 | [auto] **System Prompt**: 不可修改的 EPR 顾问专用（见 SPEC.md §5.6）| [auto] **Token 计数**: tiktoken-rs | [auto] **Context Window**: 默认 4096 tokens，可配置 | [auto] **Fallback**: LLM 不可用时降级为纯检索模式
</decisions>

<canonical_refs>
- `.planning/ROADMAP.md` | `.planning/REQUIREMENTS.md` AIQA-02/03/05/06 | `SPEC.md` §5.6
- `.planning/RESEARCH/PITFALLS.md` §6 LLM pitfalls
</canonical_refs>
