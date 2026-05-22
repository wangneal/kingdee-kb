# Phase 2: 嵌入与向量存储引擎 — Context

**Gathered:** 2026-05-23
**Status:** Ready for planning
**Mode:** Auto-generated (--auto)

<domain>
## Phase Boundary

**Goal**: 中文文本能成功向量化并存储——bge-small-zh-v1.5 embedding 模型自动下载、usearch HNSW 索引可读写、rusqlite 元数据表就绪。此阶段是入库（Phase 3）和检索（Phase 4-5）的共同上游依赖。

**Requirements**: SRCH-01, INFR-06

**Success Criteria**:
1. bge-small-zh-v1.5 模型在首次使用时自动下载到 `~/.kingdee-kb/models/`，下载过程可观测进度
2. 输入中文文本后返回 512 维向量，语义相近的中文句子向量余弦相似度 ≥ 0.7
3. 向量能写入 usearch HNSW 索引并持久化到磁盘（`~/.kingdee-kb/index/`），重启后索引可读
4. 通过余弦相似度从索引中检索 Top-K 向量，结果按相似度降序返回

**Dependencies**: Phase 1（需要 Tauri Rust 后端框架和目录结构）

**⚠️ SPIKE REQUIRED**: 验证 usearch + bge-small-zh-v1.5 ONNX 在 Windows 桌面端的可行性。这是 Phase 2 的第一个任务。
</domain>

<decisions>
## Implementation Decisions

### Auto-Selected Decisions (--auto mode)

[auto] **Embedding 模型** — Q: "bge-small-zh-v1.5 vs all-MiniLM-L6-v2 vs bge-base-zh-v1.5?" → Selected: bge-small-zh-v1.5 — 中文优化（C-MTEB retrieval 61.77），512 维，Q4 量化仅 ~15MB，比 all-MiniLM-L6-v2（纯英文训练）中文召回率高 30-40%

[auto] **Embedding 运行时** — Q: "fastembed-rs (ONNX) vs candle vs ort directly?" → Selected: fastembed-rs with ONNX backend — 原生 Rust 库，无需 Python 依赖，内置模型下载/缓存/批处理，同时支持 TextRerank 用于后续重排序

[auto] **向量存储** — Q: "usearch + rusqlite vs ChromaDB sidecar vs lancedb?" → Selected: usearch (HNSW) + rusqlite — 纯 Rust，零外部进程依赖（ChromaDB Rust 客户端仅支持 HTTP/客户端模式，嵌入式需 sidecar + Python ~200MB）

[auto] **HNSW 参数** — Q: "默认参数 vs 调优?" → Selected: usearch 默认参数（M=16, ef_construction=200）作为起点，Phase 5 混合检索时根据实际召回率调优

[auto] **向量维度** — Q: "384 vs 512 vs 768?" → Selected: 512 维（bge-small-zh-v1.5 原生维度），不降维以避免精度损失

[auto] **批处理大小** — Q: "32 vs 64 vs 128 chunks/batch?" → Selected: 64 chunks/batch — 在桌面端 4 核 CPU 上平衡吞吐与内存

[auto] **模型下载策略** — Q: "首次自动下载 vs 预打包?" → Selected: 首次自动下载，带进度回调 — 简单且灵活，允许后续升级模型

[auto] **索引持久化** — Q: "启动时加载全量索引 vs 按需加载?" → Selected: 启动时加载全量索引（usearch 内存占用小，HNSW 索引加载速度快）
</decisions>

<code_context>
## Existing Code

Phase 1 is under execution — provides Tauri project scaffold and ~/.kingdee-kb/ directory structure.

**Reference patterns**:
- `.planning/research/STACK.md` — usearch, fastembed-rs, bge-small-zh-v1.5 版本和用法
- `.planning/research/ARCHITECTURE.md` — §4 向量引擎规格、§5 检索引擎规格
- `.planning/research/PITFALLS.md` — §1 embedding 模型陷阱、§2 存储陷阱
</code_context>

<specifics>
## Specific Ideas

- bge-small-zh-v1.5 Q4 量化后仅 ~15MB，可考虑预打包进安装包（减少首次下载等待）
- Embedding 模型缓存在 `~/.kingdee-kb/models/bge-small-zh-v1.5/`（fastembed-rs 默认缓存路径）
- 索引文件存储在 `~/.kingdee-kb/index/`（usearch .usearch 文件）
- 元数据存储在 `~/.kingdee-kb/metadata.db`（rusqlite）
- 提供 Tauri command `embed_text(text: String) -> Vec<f32>` 和 `search_similar(query: String, top_k: u32) -> Vec<SearchResult>`
- 余弦相似度阈值：0.3（低于此值的结果不返回）
</specifics>

<canonical_refs>
## Canonical References

- `.planning/ROADMAP.md` — Phase 2 成功标准（SRCH-01, INFR-06）
- `.planning/REQUIREMENTS.md` — SRCH-01, INFR-06 需求定义
- `.planning/research/STACK.md` — §2 Embedding 引擎、§3 向量存储
- `.planning/research/ARCHITECTURE.md` — §4 向量引擎规格
- `.planning/research/PITFALLS.md` — §1 Embedding pitfalls, §2 Storage pitfalls
- `.planning/research/SUMMARY.md` — Phase 2 关键风险标记
</canonical_refs>

<deferred>
## Deferred Ideas

- bge-base-zh-v1.5（768维，更高精度但 4x 更大）— v0.2 可选升级
- GPU 加速推理（CUDA/Metal）— v0.3
- 多模型支持（用户可选不同 embedding 模型）— v0.3
</deferred>
