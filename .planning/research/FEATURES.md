# Feature Research

**Domain:** 本地 RAG 知识管理桌面工具（面向 ERP 实施顾问）
**Researched:** 2026-05-23
**Confidence:** HIGH

## Feature Landscape

### Table Stakes (Users Expect These)

Features users assume exist. Missing these = product feels incomplete.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **知识添加（粘贴文本 + 文件拖入）** | 所有竞品（AnythingLLM、MaxKB、Cherry Studio）均支持，用户默认知识管理工具"能加东西" | LOW | .md/.txt 优先，拖拽用 Tauri 原生 drag-drop API |
| **知识浏览（树形目录 + 内容预览）** | 竞品（MaxKB 文档列表、Dify 数据集视图）均有组织浏览能力；仅有搜索没有浏览 = 不完整 | MEDIUM | 左侧树形按标签/来源组织，右侧 Markdown 渲染预览 |
| **知识删除（单条 + 批量）** | 基础 CRUD，无删除 = 垃圾数据堆积、用户失控 | LOW | 确认弹窗防误删，支持按标签/来源批量删除 |
| **向量检索（语义搜索）** | RAG 工具的定义性能力（Cherry Studio、AnythingLLM、Dify 全部具备），无向量检索不叫 RAG 工具 | MEDIUM | 本地 all-MiniLM-L6-v2（384维），ChromaDB 嵌入式 |
| **关键词检索（BM25/全文搜索）** | 纯向量检索在精确关键词匹配（如"PCR-003"、"星达铜业"）上表现差，竞品（Dify、RAGFlow、FastGPT）均提供混合检索 | MEDIUM | BM25 算法，基于 tantivy 或自实现关键词倒排索引 |
| **混合检索（向量 + 关键词融合）** | Dify、RAGFlow 标配，缺失 = 检索召回率明显低于竞品 | MEDIUM | RRFR 融合算法（k=60），SPEC 已定义参数 |
| **AI 问答（基于检索上下文）** | RAG 工具的最终价值输出，AnythingLLM、MaxKB、Cherry Studio 的核心功能 | MEDIUM | OpenAI Chat Completions API 兼容（v0.1），Anthropic 后续 |
| **API 配置（Key + Endpoint + Model）** | 用户自备 LLM 是本地工具的基本假设；竞品全部支持多提供商配置 | LOW | 本地 config.json 存储，测试连接按钮 |
| **本地数据存储（全离线）** | 用户选择本地工具的核心动机 = 数据隐私；AnythingLLM、Cherry Studio 均以此为卖点 | LOW | ~/.kingdee-kb/，ChromaDB SQLite，零网络依赖 |
| **跨平台桌面客户端（Windows x64 首发）** | 竞品（Cherry Studio 支持 Win/Mac/Linux，AnythingLLM 有桌面版）；单平台 = 排除部分用户 | HIGH | Tauri 2.x 天然跨平台，Win 首发因目标用户 Windows 为主 |

### Differentiators (Competitive Advantage)

Features that set KingdeeKB apart from generic RAG tools. Not required by all users, but valuable for the target audience.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| **递归分块策略（H2→段落→句子）** | 保留文档层级结构，ERP 案例文档（项目名→模块→问题）适合结构化检索；竞品多使用固定窗口分块（AnythingLLM、FastGPT），信息丢失严重 | MEDIUM | SPEC 已定义完整算法，标目大小 256-512 tokens，50 token 重叠 |
| **分块元数据提取（来源文件、章节路径、标签、行号）** | 检索结果可溯源（"星达铜业/采购/期货点价"），精准定位原文；竞品（Dify、Cherry Studio）元数据大多仅文件名级别 | MEDIUM | Rust struct 已定义（ChunkMetadata），标注来源是降低幻觉的关键 |
| **ERP 场景专用 System Prompt** | 竞品（通用 ChatGPT 式回答）不适配专业领域；预置"你是金蝶 ERP 实施顾问知识助手"提示词 + "无相关知识时明确说明"约束，减少幻觉 | LOW | SPEC 已定义模板，不可修改保证一致性 |
| **社区知识包导入（Git clone）** | 解决"冷启动"问题——新顾问没有自己的知识库；社区贡献者托管 GitHub，用户一键导入 | MEDIUM | v0.2 规划，需 Git 集成（Tauri sidecar 或 Rust git2） |
| **检索结果来源标注（文件名 + 章节路径 + 相关性得分）** | 竞品（RAGFlow 做得好，Cherry Studio/AnythingLLM 较弱）；ERP 顾问需要核实原始文档，来源标注 = 信任基础 | LOW | 上下文组装格式已定义（SPEC 5.5 节） |
| **标签自动推断（从文件名/章节路径提取）** | 减少手动打标签成本；从路径"制造业/期货点价.md"自动提取"制造业"、"期货点价"标签 | LOW | 解析引擎在入库时自动执行 |
| **轻量安装包（<150MB，不含 LLM）** | AnythingLLM 桌面版 ~300MB+，Cherry Studio ~200MB；Tauri 打包优势明显，embedding 模型按需下载 | MEDIUM | Tauri 2.x 编译体积小（~15MB base + assets） |
| **知识去重检测** | 顾问可能重复添加相同或相似案例；竞品（AnythingLLM 无此功能）会导致向量库冗余 | MEDIUM | 基于内容哈希或向量相似度（>0.95 阈值提示） |

### Anti-Features (Commonly Requested, Often Problematic)

Features that seem good but create problems for this product and audience.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| **云端知识库同步/备份** | "换个电脑也能用" | 违背"本地优先、数据隐私"核心定位；增加服务器成本、安全风险（API Key 泄露）；竞品（Dify 云版）已有，KingdeeKB 差异化在于本地 | v0.1 支持本地导入/导出备份文件（手动）；v0.3 可考虑 WebDAV 本地网络同步 |
| **内置 LLM 服务（代理 API 调用）** | "不想自己搞 API Key" | 成本不可控（LLM 调用费用）、法律风险（转售 API）、运维负担；一旦代理就被期待 7x24 可用 | 用户自备 API Key（SPEC 已确认）；提供配置指南文档降低门槛 |
| **多用户协作 / 团队空间** | "团队一起用" | 本地工具 + 多用户 = 并发写入冲突、权限管理复杂度爆炸；与本地优先架构冲突；竞品（Dify 企业版）需要服务端 | v0.1~v0.2 仅单人使用；远期可通过 Git 知识包共享间接实现 |
| **实时文档编辑 / Markdown 编辑器** | "直接在这里写文档" | 变成"又一个笔记软件"，偏离"知识检索+AI问答"核心定位；编辑器开发复杂度高（所见即所得、图片上传、导出等） | 聚焦"消费已有知识"；用户用现有编辑器（VS Code、Typora）写作，KingdeeKB 负责检索 |
| **支持所有文档格式（PDF/DOCX/PPT/Excel/扫描件）** | "什么都能导入" | 格式解析复杂度指数级增长；PDF→文本质量参差不齐；中文 OCR 准确率依赖外部服务；竞品（RAGFlow）PDF 解析依赖重型依赖（GPU） | v0.1 仅 .md/.txt；按需扩展（v0.3 DOCX/PDF）；格式不是核心差异化 |
| **对话历史云端存储** | "换个设备接着聊" | 与本地优先冲突；对话中可能包含 API Key 提示词注入风险 | 本地 SQLite 存储对话历史，导出/导入；本地搜索对话历史 |
| **内置 Prompt 市场 / 用户自定义 System Prompt** | "我想定制 AI 回答风格" | 可修改 System Prompt → 用户可能绕过"无相关知识时明确说明"约束 → 幻觉增加 → 口碑崩塌；竞品（Cherry Studio、Dify）普遍支持，但通用工具，KingdeeKB 是专业工具 | 固定 System Prompt（保证专业性和可靠性）；v0.3 可开放有限的自定义（追加领域描述，不替换核心约束） |
| **移动端 App（iOS/Android）** | "手机上也能查知识库" | 移动端 UI 适配、本地 ChromaDB 运行、embedding 模型在移动端性能差；维护两套代码库成本高 | 桌面端优先；WebView 技术栈为未来 PWA 留可能性 |

### Chinese-Specific Features

Features specifically important for Chinese-language knowledge management scenarios.

| Feature | Why Important | Complexity | Notes |
|---------|--------------|------------|-------|
| **中文分词优化（jieba 或类似）** | BM25 关键词检索对中文需分词（英文天然空格分隔），不分词 = 关键词检索无效 | MEDIUM | Rust 端集成 jieba-rs 或 tantivy 的 jieba tokenizer |
| **中文 Embedding 模型兼容** | all-MiniLM-L6-v2 英文训练为主，中文语义检索效果次优；为后续升级留接口 | LOW | v0.1 使用 all-MiniLM-L6-v2（成本最低），架构支持后续切换 bge-m3 等中文优化模型 |
| **中文 UI 界面** | 目标用户为国内金蝶顾问，英文界面 = 使用门槛 | LOW | React 前端全中文，i18n 架构预留但 v0.1 不实现多语言 |
| **中文模糊搜索容错** | 中文输入法常见同音字/形近字错误（"期货点价"→"期货典价"）；BM25 严格匹配会漏掉 | HIGH | 需要编辑距离或拼音索引；v0.1 向量检索可部分缓解，v0.2 可引入模糊匹配 |
| **中文知识包生态** | 金蝶社区知识包以中文为主；RAG 工具多为英文生态（HuggingFace 数据集） | LOW | 社区贡献（非产品功能），但需设计知识包格式规范（manifest.json + .md 文件） |

## Feature Dependencies

```
知识添加（粘贴/拖入）
    └──requires──> 解析引擎（.md/.txt 清洗）
                       └──requires──> 递归分块引擎
                                          ├──requires──> 向量化引擎（本地 embedding）
                                          │                   └──requires──> ChromaDB 存储
                                          └──requires──> BM25 索引构建

知识浏览（树形目录）
    └──depends_on──> 知识添加（先有数据才能浏览）
    └──enhances──> 知识删除（在浏览界面触发删除）

标签系统
    └──enhances──> 知识浏览（按标签筛选目录树）
    └──enhances──> 检索过滤（按标签缩小搜索范围）

向量检索 ──complements──> 关键词检索（BM25）
    └──both feed──> 混合检索（RRFR 融合）
                       └──requires──> AI 问答（检索上下文→LLM）
                                           └──requires──> API 配置（Key/Endpoint/Model）

知识去重
    └──enhances──> 知识添加（入库前检测重复）

Git 知识包导入（v0.2）
    └──depends_on──> 知识添加（导入本质是批量添加）
    └──depends_on──> 知识去重（避免重复入库）

中文分词（jieba）
    └──enhances──> 关键词检索（BM25 需要分词）

数据备份/导出
    └──depends_on──> ChromaDB 存储（导出向量库 + 源文件）
```

### Dependency Notes

- **AI 问答 requires 混合检索 + API 配置**：没有检索上下文和 LLM 连接，问答无法工作；这是核心价值链的终点
- **混合检索 requires 向量检索 AND 关键词检索**：两者独立实现，通过 RRFR 融合；可先实现一个再补另一个
- **标签系统 enhances 浏览和检索**：不是硬依赖（没标签也能用），但大幅提升体验
- **Git 知识包导入 is isolated for v0.2**：v0.1 所有数据手动添加，v0.2 新增导入能力，不影响 v0.1 核心链路

## MVP Definition

### Launch With (v0.1 — Windows x64)

Minimum viable product — what's needed to validate the concept with real ERP consultants.

- [x] **知识添加（粘贴文本 + 拖入 .md/.txt）** — 没有数据就没有一切
- [x] **知识浏览（树形目录 + 内容预览）** — 用户需要看到自己有什么知识
- [x] **知识删除（单条删除）** — 基础数据管理闭环
- [x] **向量检索（all-MiniLM-L6-v2 本地 embedding）** — RAG 的定义性能力
- [x] **关键词检索（BM25 + jieba 中文分词）** — 精确匹配场景必须（如项目代号、模块名）
- [x] **混合检索（RRFR 融合）** — 兼顾语义和关键词，提升召回率
- [x] **AI 问答（OpenAI API，基于检索上下文）** — 最终价值输出
- [x] **API 配置（Key + Endpoint + Model + 测试连接）** — 用户自备 LLM
- [x] **递归分块 + 元数据提取** — 保留文档结构，检索结果可溯源

### Add After Validation (v0.2)

Features to add once v0.1 is deployed and getting real feedback.

- [ ] **Anthropic API 兼容** — 目标用户中部分使用 Claude；v0.1 仅 OpenAI 协议
- [ ] **Git 知识包导入** — 社区知识包生态的基础设施；用户反馈"不知道怎么开始"时的解决方案
- [ ] **知识去重检测** — v0.1 手动添加量少时非关键，v0.2 批量导入时必需
- [ ] **批量删除（按标签/来源）** — 用户积累数据后的管理需求
- [ ] **检索过滤（按标签/时间范围/来源）** — 基础检索完成后提升精度
- [ ] **数据导出/备份** — 用户"我的数据怎么带走"的安全感需求
- [ ] **存储统计仪表板** — 让用户了解知识库规模和使用情况

### Future Consideration (v0.3+)

Features to defer until product-market fit is established with core ERP consultant audience.

- [ ] **.docx / .pdf 解析** — 格式解析复杂度高，v0.1~v0.2 聚焦 Markdown/纯文本验证核心链路
- [ ] **深色模式** — UI 美化，不影响核心价值验证
- [ ] **macOS / Linux 客户端** — 跨平台编译 + 测试 + 签名；Windows 顾问占绝对主流
- [ ] **中文模糊搜索容错（拼音/编辑距离）** — 技术复杂度高，向量检索已可部分缓解
- [ ] **有限 System Prompt 自定义** — 在保证"不编造"约束的前提下，允许用户追加领域描述
- [ ] **对话历史管理（搜索/删除/导出）** — 对话积累后的管理需求
- [ ] **本地 WebDAV 备份** — 局域网内多设备同步（非云端）

### Explicitly Out of Scope (Permanent)

Features that conflict with the product's core identity and will not be built.

- [ ] **云端知识库 / LLM 代理服务** — 违背本地优先、零服务器成本、数据隐私三大原则
- [ ] **团队协作 / 实时同步** — 本地工具定位不支持；社区知识包共享是异步方案
- [ ] **官方知识包服务器** — 开源社区自行托管，不建中心化服务
- [ ] **内置 LLM 模型（非 embedding）** — 用户自备 API Key，不代理不内置

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| 知识添加（粘贴+拖入） | HIGH | LOW | P1 |
| 知识浏览（树形+预览） | HIGH | MEDIUM | P1 |
| 向量检索 | HIGH | MEDIUM | P1 |
| 关键词检索（BM25+分词） | HIGH | MEDIUM | P1 |
| 混合检索（RRFR） | MEDIUM | LOW | P1 |
| AI 问答（OpenAI） | HIGH | MEDIUM | P1 |
| API 配置 | HIGH | LOW | P1 |
| 递归分块+元数据 | MEDIUM | MEDIUM | P1 |
| 知识删除 | MEDIUM | LOW | P1 |
| 标签自动推断 | MEDIUM | LOW | P1 |
| 知识去重检测 | MEDIUM | MEDIUM | P2 |
| Anthropic API 兼容 | MEDIUM | LOW | P2 |
| Git 知识包导入 | HIGH | MEDIUM | P2 |
| 检索过滤 | HIGH | LOW | P2 |
| 数据备份/导出 | MEDIUM | MEDIUM | P2 |
| 存储统计 | LOW | LOW | P2 |
| .docx/.pdf 解析 | MEDIUM | HIGH | P3 |
| 深色模式 | LOW | MEDIUM | P3 |
| macOS/Linux 客户端 | MEDIUM | HIGH | P3 |
| 中文模糊搜索 | MEDIUM | HIGH | P3 |

**Priority key:**
- P1: Must have for v0.1 launch
- P2: Should have, add in v0.2 when validated
- P3: Nice to have, v0.3+ future consideration

## Competitor Feature Analysis

| Feature | AnythingLLM | Cherry Studio | MaxKB | Dify | RAGFlow | KingdeeKB Approach |
|---------|-------------|---------------|-------|------|---------|-------------------|
| 文档格式支持 | 200+ 格式（PDF/Word/图片/音视频） | PDF/Word/PPT/TXT/MD | PDF/Word/PPT/Excel/MD+ | TXT/MD/PDF/HTML+ | PDF/Word/PPT/Excel/图片/扫描件 | **v0.1 仅 .md/.txt — 聚焦核心格式** |
| 部署方式 | Docker/桌面版/云 | **桌面零配置（双击安装）** | Docker（Web界面） | Docker/K8s/云托管 | Docker（多容器） | **桌面零配置（Tauri 安装包）** |
| 分块策略 | 固定窗口+语义边界 | 简单分块 | 自动分块 | 固定长度分块 | **智能布局分析+可视化调整** | **递归分块（H2→段落→句子）+元数据** |
| 检索方案 | 向量检索（ChromaDB） | 基础语义检索 | 向量+关键词 | 向量+关键词+混合 | **混合检索+多路召回+重排** | **向量+BM25+RRFR+标签过滤** |
| 多模型支持 | 本地+云端混合 | 30+模型聚合 | 主流模型+本地 | **数百模型自由切换** | 需外接LLM | **OpenAI(v0.1)+Anthropic(v0.2)** |
| 工作流编排 | 无 | 无 | 内置工作流引擎 | **低代码节点编排** | 无 | **不提供（聚焦检索+问答）** |
| 团队协作 | 精细化权限管理 | 无 | 企业版支持 | 企业版支持 | 企业级审计日志 | **v0.1 单人；v0.2+ 知识包共享** |
| 中文优化 | 基础支持 | 基础支持 | 中文界面 | 中文界面 | 中文界面+模型 | **jieba分词+中文UI+中文Embedding路线** |
| 数据隐私 | 可全本地 | 默认依赖在线服务 | 可本地部署 | 需私有化部署 | 可本地部署 | **全本地优先（零网络依赖设计）** |
| 包体积 | ~300MB+ | ~200MB | Docker 镜像 | Docker 镜像 | Docker 多容器 | **目标 <150MB（Tauri 优势）** |

### Competitive Positioning

1. **vs AnythingLLM / MaxKB / Dify**：它们定位为"通用知识库平台"，功能全但重（Docker/服务器架构）；KingdeeKB 定位为"ERP 顾问专用桌面工具"，轻量、专用、开箱即用
2. **vs Cherry Studio**：最接近的竞品——同样是桌面端、多模型、本地 RAG；但 Cherry Studio 是"通用 AI 聊天客户端"（300+ 助手），KingdeeKB 专注"ERP 知识管理"一个场景
3. **核心差异化**：递归分块保留文档结构 + ERP 专用 System Prompt + 社区知识包生态 + 全本地零配置

## Feature Philosophy

Based on research, the following principles guide feature decisions:

1. **"消费"优于"生产"**：KingdeeKB 是知识消费工具（检索+问答），不是知识生产工具（编辑器）。用户用现有工具写文档，KingdeeKB 让文档可检索
2. **"深"优于"广"**：在 .md/.txt 上做到极致的检索体验，好过在所有格式上做到"勉强能用"
3. **"专业"优于"通用"**：一个为 ERP 顾问调优的工具，好过一个"什么都能做、什么都不精"的工具
4. **"离线"是根基**：所有功能必须离线可用（embedding 本地、数据本地），LLM 调用是唯一的在线依赖（且用户可控）
5. **"简单"保护"信任"**：System Prompt 不可修改（保护回答质量），API Key 本地存储（保护用户隐私）

## Sources

- **竞品分析**：AnythingLLM (Mintplex Labs, MIT License)、Cherry Studio (开源桌面端)、MaxKB (1Panel 出品, GPLv3)、Dify (LangGenius, Apache 2.0)、RAGFlow (InfiniFlow, Apache 2.0)、FastGPT (Labring, Apache 2.0) — 基于 2025-2026 年 CSDN、掘金、知乎技术文章（MEDIUM 置信度）
- **竞品对比详情**：CSDN 博客《本地自建知识库工具全解析：5大主流方案对比》（2025-09）、《一文详解几种常见本地大模型个人知识库工具》（2025-04）、《本地知识库构建利器：Dify、Ragflow、MaxKB大比拼》（2025-05）
- **Cherry Studio 功能详情**：掘金文章《Cherry Studio搭建AI知识库》（2025-04）、Cherry Studio 官网 (cherry-ai.com)
- **中文 RAG 场景**：基于 jieba 分词生态、bge-m3 中文 embedding 模型研究、中国 RAG 知识库社区最佳实践
- **产品规格参考**：项目 SPEC.md（v0.1 草案）、PROJECT.md（核心定位与约束）

---
*Feature research for: KingdeeKB — 本地 RAG 知识管理桌面工具*
*Researched: 2026-05-23*
