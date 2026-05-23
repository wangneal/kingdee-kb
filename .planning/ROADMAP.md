# Roadmap: KingdeeKB v0.1 MVP

**Milestone:** v0.1 — 本地 RAG 知识管理 MVP
**Granularity:** Fine（8 阶段）
**Created:** 2026-05-23
**Requirements:** 35 v1 requirements → 100% mapped

---

## Phases

- [x] **Phase 1: 项目脚手架与基础设施** ✅ — Tauri 2.x 脚手架、Splash Screen、OS Keyring、本地存储目录
- [x] **Phase 2: 嵌入与向量存储引擎** ✅ — bge-small-zh-v1.5 embedding + usearch HNSW 索引 + rusqlite 元数据
- [x] **Phase 3: 入库流水线** ✅ — 粘贴/拖拽/文件夹导入、递归分块、标签提取、SHA256 去重
- [x] **Phase 4: BM25 全文检索** ✅ — tantivy + jieba 中文分词关键词检索
- [x] **Phase 5: 混合检索引擎** ✅ — RRFR 融合向量+BM25 结果、项目级知识隔离
- [ ] **Phase 6: LLM 集成与 AI 问答** — OpenAI API 流式 RAG 问答、token 感知上下文管理
- [ ] **Phase 7: 知识管理与检索前端** — 树形目录、内容预览、搜索框、结果高亮与标注
- [ ] **Phase 8: AI 对话与设置前端** — 多轮对话面板、API 配置、连接测试、存储统计

---

## Phase Details

### Phase 1: 项目脚手架与基础设施

**Goal**: 应用能成功启动并具备基础框架——Tauri 2.x 桌面窗口、Splash Screen 消除冷启动白屏、本地数据目录就绪、API Key 安全存储框架就绪。

**Depends on**: 无（第一阶段）

**Requirements**: INFR-01, INFR-03, INFR-07, INFR-08

**Success Criteria** (what must be TRUE):
1. 应用在 Windows 上以 Tauri 2.x 桌面窗口成功启动，启动过程显示 Splash Screen
2. Splash Screen 平滑过渡到 React 前端主界面（消除 WebView2 冷启动白屏）
3. 首次启动自动创建 `~/.kingdee-kb/` 目录结构（knowledge/、index/、models/、bm25_index/、metadata.db）
4. API Key 通过 Windows Credential Manager 安全存储与读取，不落盘为明文 JSON
5. WebView2 fixedRuntime 捆绑打包，安装包体积 < 30MB（不含业务依赖）

**Plans**: TBD

**Research flag**: 标准模式 — Tauri 2 脚手架有成熟模板，tauri-plugin-keyring-store 需在 Windows 中文环境验证兼容性

---

### Phase 2: 嵌入与向量存储引擎

**Goal**: 中文文本能成功向量化并存储——bge-small-zh-v1.5 embedding 模型自动下载、usearch HNSW 索引可读写、rusqlite 元数据表就绪。此阶段是入库和检索的共同上游依赖。

**Depends on**: Phase 1（需要 Tauri Rust 后端框架和目录结构）

**Requirements**: SRCH-01, INFR-06

**Success Criteria** (what must be TRUE):
1. bge-small-zh-v1.5 模型在首次使用时自动下载到 `~/.kingdee-kb/models/`，下载过程可观测进度
2. 输入中文文本后返回 512 维向量，语义相近的中文句子向量余弦相似度 ≥ 0.7
3. 向量能写入 usearch HNSW 索引并持久化到磁盘（`~/.kingdee-kb/index/`），重启后索引可读
4. 通过余弦相似度从索引中检索 Top-K 向量，结果按相似度降序返回

**Plans**: TBD

**Research flag**: ⚠️ **强烈建议在规划前执行 `/gsd-spike`** — 验证 `usearch` + `bge-small-zh-v1.5` + ONNX Runtime 在 Windows 上的端到端可行性（当前置信度 MEDIUM-HIGH）
**Plans**: `02-embedding-engine` (9 tasks, completed 2026-05-23)

---

### Phase 3: 入库流水线

**Goal**: 知识能通过多种方式导入并自动完成解析→清洗→分块→向量化→存储的全流程。递归分块保留文档结构，中文感知分隔符防止语义断裂，SHA256 去重避免重复索引。

**Depends on**: Phase 2（需要向量化能力和存储引擎）

**Requirements**: KNOW-01, KNOW-02, KNOW-03, KNOW-04, KNOW-05, INFR-10, INFR-11

**Success Criteria** (what must be TRUE):
1. 系统接收粘贴文本后自动完成清洗、分块、向量化、存储全流程
2. 系统接收 .md/.txt 文件后自动解析——提取文件名作标题、按 `#` 层级提取章节结构
3. 系统接收文件夹路径后批量扫描并导入所有 .md/.txt 文件（含子目录）
4. 自动提取标签（从文件名、章节路径推断），相同内容（SHA256）重复导入时跳过不产生重复数据
5. 分块使用中文感知分隔符（`\n## ` → `\n\n` → `。` → `！` → `？` → `；` → `，`），保留层级元数据（source_file / section_path / tags / line_no）

**Plans**: TBD

**Research flag**: 标准模式 — 文本分块算法已在 SPEC 中定义，langchain 有参考实现

---

### Phase 4: BM25 全文检索

**Goal**: 中文关键词检索可用——tantivy 引擎 + jieba 搜索引擎模式分词，支持增量更新。为混合检索提供另一半能力。

**Depends on**: Phase 2（需要 rusqlite 元数据 Schema），可与 Phase 3 并行开发（用 Mock 数据）

**Requirements**: SRCH-02

**Success Criteria** (what must be TRUE):
1. 输入中文关键词（如「期货点价」）后返回匹配的知识片段，按 BM25 相关性得分排序
2. BM25 使用 jieba 搜索引擎模式（`cut_for_search`）分词，非简单字符或空格切分
3. BM25 索引支持增量更新——新增知识后自动增量索引，删除知识后索引同步移除

**Plans**: TBD

**Research flag**: 标准模式 — tantivy + jieba 方案成熟

---

### Phase 5: 混合检索引擎

**Goal**: 向量检索与 BM25 检索经 RRFR 算法融合，提供兼顾语义和关键词的混合检索能力。检索默认按项目隔离，防止多项目知识混淆。

**Depends on**: Phase 3（需要已入库 chunk 数据） + Phase 4（需要 BM25 检索能力）

**Requirements**: SRCH-03, SRCH-08

**Success Criteria** (what must be TRUE):
1. 同一查询同时触发向量检索（Top-30）和 BM25 检索（Top-30），经 RRFR 算法融合排序后返回统一结果列表
2. 检索结果默认按 project 字段隔离——查询项目 A 不返回项目 B 的知识片段
3. 每条检索结果包含相关性得分和来源标注（文件名 + 章节路径）
4. 支持分页返回——结果超过 50 条时分页加载

**Plans**: TBD

**Research flag**: 轻度研究 — RRF 参数（k 值）需网格搜索确定最优值，需准备中文 ERP 测试查询集

---

### Phase 6: LLM 集成与 AI 问答

**Goal**: AI 能基于知识库检索上下文生成回答——OpenAI 协议兼容、流式响应、来源标注、token 感知上下文管理。RAG 价值链的最终输出环节。

**Depends on**: Phase 5（需要混合检索提供上下文）

**Requirements**: AIQA-02, AIQA-03, AIQA-05, AIQA-06

**Success Criteria** (what must be TRUE):
1. 调用 OpenAI Chat Completions API（兼容协议），流式返回（SSE）AI 回答
2. AI 回答基于检索到的知识库上下文生成（RAG 模式），回答中标注参考来源（文件名 + 章节路径）
3. 知识库中无相关内容时 AI 明确告知「知识库中未找到相关信息」而非编造答案
4. 支持 token 感知的动态上下文窗口管理——检索片段按 token 预算截断，不超出模型限制
5. LLM 不可用时优雅降级——仅展示检索结果，不做 AI 生成

**Plans**: TBD

**Research flag**: 轻度研究 — OpenAI 流式 API 已充分文档化，重点是 tiktoken-rs 中文 token 计数校验

---

### Phase 7: 知识管理与检索前端

**Goal**: 用户能浏览、管理和搜索自己的知识库——树形目录导航、Markdown 内容预览、知识编辑与删除、搜索框输入自然语言查询、结果高亮与来源标注。这是用户可见的第一个完整闭环体验。

**Depends on**: Phase 3（需要入库数据） + Phase 5（需要检索后端 API）

**Requirements**: KNOW-06, KNOW-07, KNOW-08, KNOW-09, KNOW-10, SRCH-04, SRCH-05, SRCH-06, SRCH-07

**Success Criteria** (what must be TRUE):
1. 用户看到左侧树形目录（按标签/来源组织），点击节点在右侧预览区查看 Markdown 渲染的完整内容
2. 用户能编辑知识（修改标题、标签、内容）和删除单条知识
3. 用户可通过标签/来源筛选浏览范围
4. 用户在搜索框输入自然语言查询（如「客户要做期货点价怎么处理」）后展示相关知识片段，命中关键词高亮
5. 每条检索结果标注来源（文件名 + 章节路径）和相关性得分，支持按标签过滤检索范围
6. 导入操作实时展示进度（进度条/状态提示），避免界面冻结

**Plans**: TBD

**UI hint**: yes

---

### Phase 8: AI 对话与设置前端

**Goal**: 用户能进行 AI 辅助问答并配置系统——多轮对话面板（流式回答 + 来源引用）、API Key/Endpoint/Model 设置、连接测试、存储统计。CPO 核心价值的最终呈现。

**Depends on**: Phase 6（需要 LLM 后端） + Phase 7（共享前端 UI 框架和组件）

**Requirements**: AIQA-01, AIQA-04, INFR-02, INFR-04, INFR-05, INFR-09

**Success Criteria** (what must be TRUE):
1. 用户打开对话面板输入问题后获得 AI 流式回答（逐字显示），回答中内联引用可跳转到原文
2. 支持多轮追问——在同一会话内 AI 能记住之前的对话上下文
3. 用户在设置中填写 OpenAI API Key（通过 OS Keyring 安全存储）、自定义 Endpoint 和 Model
4. 用户点击「测试连接」按钮能验证 API 可用性——成功显示绿色确认，失败显示具体错误原因
5. 用户能查看当前知识库的存储空间占用（总文档数、总 chunk 数、索引大小）

**Plans**: TBD

**UI hint**: yes

---

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. 项目脚手架与基础设施 | 1/1 | Completed ✅ | 2026-05-23 |
| 2. 嵌入与向量存储引擎 | 1/1 | Completed ✅ | 2026-05-23 |
| 3. 入库流水线 | 1/1 | Completed ✅ | 2026-05-23 |
| 4. BM25 全文检索 | 0/1 | Completed ✅ | 2026-05-23 |
| 5. 混合检索引擎 | 0/1 | Completed ✅ | 2026-05-23 |
| 6. LLM 集成与 AI 问答 | 0/1 | Not started | — |
| 7. 知识管理与检索前端 | 0/1 | Not started | — |
| 8. AI 对话与设置前端 | 0/1 | Not started | — |

---

## Dependency Graph

```
Phase 1 (Scaffold) 
  └─ Phase 2 (Embedding/Storage) 
       ├─ Phase 3 (Ingestion) ──────────────┐
       │    └─ Phase 4 (BM25) ──────┐        │
       │         └─ Phase 5 (Hybrid) ┤        │
       │              └─ Phase 6 (LLM)        │
       │                   │                 │
       │                   ▼                 │
       └───────────── Phase 7 (KM+Search UI) │
                           │                 │
                           ▼                 │
                      Phase 8 (Chat+Settings)│
```

- **Phase 4 可与 Phase 3 并行开发**（Phase 3 产出 chunk 数据，Phase 4 可用 Mock 数据先行）
- **Phase 7 和 Phase 8 不可并行于 Phase 1-6**（前端依赖后端 API 稳定后开发以减少返工）
- **所有 P1 陷阱均在对应阶段中预防**：P4(Phase 1)、P1(Phase 2)、P3(Phase 3)、P5(Phase 5)、P6/P8(Phase 6)

---

*Roadmap created: 2026-05-23*
*Next step: `/gsd-plan-phase 1`*
