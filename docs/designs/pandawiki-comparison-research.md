# PandaWiki 对比调研报告

> 调研日期：2026-06-17
> 调研对象：PandaWiki（chaitin/PandaWiki，本仓库 owner 镜像 wangneal/PandaWiki fork）
> 目的：评估是否将 PandaWiki「完整搬到」KingdeeKB，并回答「检索是否成熟」「PandaWiki 是否有编译」「图片处理如何对比」三个具体问题。
> 结论先行：**不建议整体引入 PandaWiki**。KingdeeKB 检索底座已对标 raglite（PandaWiki 的 RAG 引擎）；PandaWiki 没有「知识编译」概念，该能力反为 KingdeeKB 独有；图片处理有差距但已立项增强（见第 6.1 项）。建议改为针对性补短板（见第 6 节）。

---

## 1. PandaWiki 实勘

### 1.1 形态与技术栈（基于源码与官方文档）

| 维度 | PandaWiki |
|---|---|
| 定位 | 自托管 Web 知识库搭建系统（产品文档 / 技术文档 / FAQ / 博客，对外发布） |
| 后端 | **Go**（`backend/`，严格分层 handler→usecase→repo→store），Go 1.24.3 |
| RAG 引擎 | **Python raglite**（`sdk/rag`，superlinear-ai/raglite），依赖 **PostgreSQL + pgvector** |
| 前端 | TypeScript monorepo：`web/admin`（React+Vite 控制台）+ `web/app`（Next.js 前台 Wiki 站） |
| 进程 | **api + consumer 两个独立服务**（`Dockerfile.api` / `Dockerfile.consumer`） |
| 部署 | Docker Compose 多容器，Linux + Docker 20.x，2核4G，2443 端口 Web |
| 许可证 | **AGPL-3.0**（强 copyleft，网络服务也须开源） |
| 内置模型 | bge-m3（向量）+ bge-reranker-v2-m3（重排），用户首次只需配 Chat 模型 |

### 1.2 核心能力

- AI 创作 / AI 问答（RAG）/ AI 搜索
- 富文本编辑（Markdown+HTML，导出 word/pdf/markdown，版本管理，协作编辑）
- 多渠道内容导入：**URL / Sitemap / RSS / 离线文件**
- 第三方集成：网页挂件、钉钉 / 飞书 / 企业微信机器人
- 文档级"生成当前文档摘要"

### 1.3 架构理念

官方/第三方提炼为三层：**内容层（Markdown 优先）→ 语义层（embedding + 文档图谱 + 上下文权重）→ 生成层（可控触发、不擅自改写）**。设计成熟，工程化程度高。

---

## 2. 为什么不建议整体引入

三个硬障碍，任一独立成立即可否决"整体搬"：

### 2.1 物理集成不成立

PandaWiki = Go 后端 + Python raglite + PostgreSQL/pgvector 多容器 Web 服务。塞进 Tauri 桌面单包只有两条路，都不通：

- **内嵌运行**：在一个桌面安装包里塞 Go runtime + Python 解释器 + PostgreSQL 服务 → 违背 Tauri 单包轻量定位，体积、启动复杂度、跨平台适配全崩。
- **Rust 重写**：那不叫"搬 PandaWiki"，而是"参考它的设计自己写"，等于回到自研。

### 2.2 AGPL-3.0 许可证污染

KingdeeKB 当前 **MIT**。PandaWiki 是 AGPL，传染性强于 GPL——**通过网络提供服务也必须开源全部衍生代码**。合入后法律上整个 KingdeeKB 被强制要求改 AGPL 开源，与 MIT 公开仓库定位直接冲突。这是法律红线，非技术偏好。

### 2.3 领域不匹配

PandaWiki 面向"对外发布文档给用户浏览"（每个知识库生成独立 Wiki 网站、网页挂件、机器人）。KingdeeKB 面向"金蝶实施顾问内部用"——项目隔离、调研蓝图、风险把控舱、腾讯会议 MCP、技能系统，这些 PandaWiki 完全没有，丢弃即丢失产品差异化。

---

## 3. 问题一：KingdeeKB 检索对标 raglite 成熟吗？

**底座成熟——工程实现逐项对标，但要诚实标注边界，不笼统吹。**

### 3.1 逐环节对标（均基于实测代码）

| raglite 环节 | KingdeeKB 实现 | 代码证据 | 评价 |
|---|---|---|---|
| Hybrid search（vector+BM25） | weighted RRF，向量权重 > BM25，project 过滤 | `services/knowledge/hybrid_search.rs` `rrf_fuse` | 持平 |
| Reranker | **同款 BAAI/bge-reranker-v2-m3**，fastembed 本地 ONNX，RRF 后精排 TOP K | `services/knowledge/rerank.rs:7,29` | 持平 |
| Chunk 策略 | Small-to-Big（子块独立嵌入、命中扩父块）+ Contextual Retrieval（Anthropic context_prefix，降 67% 失败率）+ 15% 重叠 + 中文句界 | `services/knowledge/chunker.rs:6-18` | **更细** |

> 小到大检索与上下文前缀均为业界验证过的最佳实践，KingdeeKB 已落地，分块策略实际比 raglite 默认更细致。

### 3.2 诚实标注的短板

1. **缺 self-query**：raglite 有 `self_query=True`（LLM 自动生成元数据过滤），KingdeeKB 未见。复杂过滤查询会偏弱。
2. **向量模型较轻**：BGE-Small-ZH vs bge-m3，召回上限略低（换来零外部依赖，桌面端合理取舍）。
3. **检索效果无量化基准**：成熟不能靠"读了像成熟"。两边均无公开 eval 集，是共同真空。严谨表述：**工程实现对标成熟，效果未做量化评测**。

### 3.3 结论

检索底座**不是该推倒重写的对象**。真正该补：self-query 能力 + 一套检索 eval 基准。

---

## 4. 问题二：PandaWiki 是否有"编译"？

**没有。它走完全不同的路径。** 这是决定性事实。

### 4.1 PandaWiki 的"文档处理"是什么

- 上传/导入文档 → consumer 进程**异步分块 + embedding 入 pgvector** → 供 RAG 检索。
- AI 能力是创作 / 问答 / 搜索三件套 + "生成当前文档摘要"。
- **不存在"知识编译"概念**：不把原始文档 LLM 提炼成结构化 Wiki 页、不维护 `[[slug]]` 页面互链、不做候选批准流、不构建多信号知识图谱。

### 4.2 对比

| | PandaWiki | KingdeeKB |
|---|---|---|
| 原始文档处理 | 分块+向量化（RAG 基础设施） | 同样有（documents / chunks / vectors） |
| LLM 提炼成 Wiki 页 | ❌ 无 | ✅ `ingestion_pipeline.rs` Step2 |
| 候选批准流 | ❌ 无 | ✅ auto / conflict / pending + `approve_candidate` |
| 页面互链 `[[slug]]` | ❌ 无（靠语义邻接） | ✅ `wikilink_parser` |
| 多信号知识图谱 | 文档图谱（语义层） | ✅ wikilink / tag / source / co_citation 四信号 |

### 4.3 含义

- "编译"恰是 **PandaWiki 没做、KingdeeKB 自己趟出来的差异化能力**。
- 问题不是"该不该搬 PandaWiki 的编译"（它没有），而是 **KingdeeKB 的编译是否需要打磨**——属产品增强，非"换底座"。
- 形态差异：PandaWiki 的"知识库=独立 Wiki 网站"面向**对外发布文档**；KingdeeKB 编译面向**顾问内部消化资料**。目的不同，编译对 KingdeeKB 不可替代。

---

## 5. 问题三：图片处理如何对比？

**两者都走"图片转文字喂下游文本流程"，但路线与成熟度有差距。KingdeeKB 有三处可借鉴 raglite 的成熟做法，已立项增强。**

### 5.1 PandaWiki/raglite 的图片处理

raglite 的图片处理是**可选增强**，核心是"把图片变成文本描述，纳入 Markdown 流程"，靠 Mistral OCR 完成：

- **默认（不装 mistralai）**：PDF 用 `pdftext`+`pypdfium2` 提文字，其他格式 Pandoc 转 Markdown，**图片基本被忽略**。
- **装 `mistralai` 后（高质量模式）**：调 **Mistral OCR**（mistral-ocr-latest）：
  1. **四分类**：`image_types` 把图片归为 graph（图表）/ text（文字截图）/ table（表格）/ image（普通图像），可用 `exclude_image_types` 过滤无价值类型。
  2. **生成文本描述**：每页返回 Markdown，图片**内联成交织文本描述**（interleaved text + images）。
  3. **纳入分块索引**：带描述的 Markdown 进入 level 4 语义分块 → embedding → 检索。
- 强项：表格/图表/公式/复杂版式理解强，输出结构化 Markdown，号称 2000 页/分钟。
- **关键特点**：图片本身不存、不向量化，**永远转文字描述**进文本流程；强依赖 Mistral 云 API，离线不可用。

### 5.2 KingdeeKB 现状

| | PandaWiki/raglite | KingdeeKB 现状 |
|---|---|---|
| 默认行为 | 图片被忽略（纯文本提取） | 独立图片文件主动 OCR/多模态；DOCX 内嵌 Visio 抽预览图 OCR |
| OCR 引擎 | Mistral OCR / Pixtral（云端多模态） | LLM 多模态探测复用 + 百度/腾讯 OCR 降级 |
| 主辅关系 | OCR 为主 | **LLM 多模态为主、OCR 降级（反了）** |
| 图片分类 | graph/text/table/image 四分类 + 可排除 | `classify_image` 按宽高比**伪分类**（ratio>1.5 判 Flowchart，否则 Mixed） |
| 图片→文本 | 文字描述**位置内联**进 Markdown | `--- DOCX 内嵌 Visio 预览图 ---` **段落拼接**，丢失位置上下文 |
| 表格/图表理解 | Mistral OCR 强 | 取决于所配 LLM/OCR，无专项 |
| Mistral 引擎 | ✅ | ❌（仅百度/腾讯/LLM） |

### 5.3 三处差距（均已在增强提案中解决）

1. **主辅关系反了**：OCR（专用视觉服务）在版式/表格/图表上专业性强、成本低，应为主，LLM 多模态为辅助。现状相反。
2. **分类是伪分类**：宽高比猜测无法区分表格/图表/文字截图/装饰图，无法过滤噪声。
3. **段落拼接丢位置**：流程图/需求矩阵这类位置敏感的图，描述需与前后文绑定才有意义。

### 5.4 结论

图片处理方向正确（本地优先 + 降级，符合桌面 + 涉密场景），但有差距。**已开 OpenSpec 提案** [`enhance-image-processing-mistral`](../OpenSpec/changes/enhance-image-processing-mistral/proposal.md) 落地：OCR 为主、LLM 多模态辅助；真四分类 + 可配置排除；图片描述位置内联 Markdown；集成 Mistral OCR 为同级可选引擎（不绑死云，保留离线能力）。详见第 6.1 项。

---

## 6. 建议的后续动作（针对性补短板）

均为现有 Rust 模块的小幅增强，每项可独立开 OpenSpec change（L2，需审批）。**已开提案的标注状态**：

1. **图片处理增强** ✅ **已开提案** [`enhance-image-processing-mistral`](../OpenSpec/changes/enhance-image-processing-mistral/)（proposal / design / tasks，待审批）：OCR 为主 + 四分类可排除 + 位置内联 Markdown + 集成 Mistral OCR。源于第 5 节图片处理对比。
2. **知识图谱前端可视化**（你已点名的痛点 6）：后端 `get_neighbors` / `traverse_graph` 数据已就绪，前端 `KnowledgeGraph.tsx` 接入图形库（react-flow / cytoscape / d3）渲染节点-连线即可。ROI 最高。
3. **self-query 元数据过滤**：借鉴 raglite，LLM 自动生成过滤条件，增强复杂查询召回（对应第 3.2 短板 1）。
4. **URL / Sitemap / RSS 导入**：借鉴 PandaWiki，对实施顾问抓取客户官网/文档站有用，补 `file_extractor` 之外的摄入源。
5. **检索 eval 基准**：建立一套带标注的 query-chunk 评测集，量化召回/精排效果，给"成熟"提供数据支撑，也为后续优化兜底（对应第 3.2 短板 3）。
6. **编译候选 UX 打磨**：候选批准流是独有能力，但 `conflict/pending` 的人工审批体验仍有提升空间。

> 不建议：移植 raglite 的 PG/pgvector 依赖（与 SQLite 本地优先冲突）、引入 PandaWiki 任何 AGPL 代码。

---

## 7. 参考来源

- chaitin/PandaWiki 仓库：https://github.com/chaitin/PandaWiki
- PandaWiki 官方文档：https://pandawiki.docs.baizhi.cloud
- PROJECT_STRUCTURE.md / AGENTS.md（raw.githubusercontent.com/chaitin/PandaWiki/main）
- raglite（superlinear-ai/raglite）：https://github.com/superlinear-ai/raglite
- Mistral OCR 文档：https://docs.mistral.ai/studio-api/document-processing/basic_ocr
- KingdeeKB 实测代码：`src-tauri/src/services/knowledge/{hybrid_search,rerank,chunker,knowledge_graph,ingestion_pipeline,wiki_page,wikilink_parser}.rs`、`services/media/image_processor.rs`、`commands/{kb_compilation,wiki_page,knowledge_graph,ingestion,llm_provider}.rs`
- 关联提案：[`OpenSpec/changes/enhance-image-processing-mistral/`](../OpenSpec/changes/enhance-image-processing-mistral/)
