# Requirements: KingdeeKB

**Defined:** 2026-05-23
**Core Value:** 让金蝶ERP实施顾问能快速检索历史案例并基于检索结果进行 AI 辅助问答

## v1 Requirements

Requirements for initial release (v0.1 MVP). Each maps to roadmap phases.

### Knowledge Management (KNOW)

- [ ] **KNOW-01**: 用户可以通过粘贴文本到输入框添加单条知识
- [ ] **KNOW-02**: 用户可以通过拖拽 .md/.txt 文件到应用窗口批量导入知识
- [ ] **KNOW-03**: 用户可以通过点击按钮选择文件夹批量扫描导入 .md/.txt 文件
- [ ] **KNOW-04**: 系统自动解析文件名作为标题，按 `#` 层级提取章节结构
- [ ] **KNOW-05**: 系统自动提取标签（从文件名、章节路径推断），支持去重检测
- [ ] **KNOW-06**: 用户可以通过左侧树形目录按标签/来源浏览知识
- [ ] **KNOW-07**: 用户可以在右侧预览区查看知识完整内容
- [ ] **KNOW-08**: 用户可以对知识进行编辑（修改标题、标签、内容）
- [ ] **KNOW-09**: 用户可以删除单条知识
- [ ] **KNOW-10**: 用户可以通过标签/来源进行筛选查看

### Search & Retrieval (SRCH)

- [ ] **SRCH-01**: 系统支持基于本地 embedding（bge-small-zh-v1.5）的向量语义检索
- [ ] **SRCH-02**: 系统支持基于 BM25（jieba 中文分词）的关键词检索
- [ ] **SRCH-03**: 系统支持混合检索（向量+BM25 结果经 RRFR 融合排序）
- [ ] **SRCH-04**: 用户可以通过顶部搜索框输入自然语言查询（如「客户要做期货点价怎么处理」）
- [ ] **SRCH-05**: 检索结果展示相关知识片段，高亮命中关键词
- [ ] **SRCH-06**: 检索结果标注来源（文件名、章节路径），显示相关性得分
- [ ] **SRCH-07**: 用户可以通过标签过滤检索范围
- [ ] **SRCH-08**: 检索结果支持项目级隔离（不跨项目混合知识）

### AI Q&A (AIQA)

- [ ] **AIQA-01**: 用户可以打开对话模式，输入问题获得 AI 回答
- [ ] **AIQA-02**: AI 基于检索到的知识库上下文生成回答（RAG 模式）
- [ ] **AIQA-03**: AI 回答中标注参考来源（文件名 + 章节）
- [ ] **AIQA-04**: 支持追问（多轮对话上下文保持）
- [ ] **AIQA-05**: 当知识库中无相关内容时，AI 明确说明而非编造答案
- [ ] **AIQA-06**: 支持 OpenAI Chat Completions API 协议（含流式响应）

### Application Infrastructure (INFR)

- [ ] **INFR-01**: 应用以 Tauri 2.x 桌面客户端方式运行（Windows x64 首发）
- [ ] **INFR-02**: 用户可以在设置中填写自己的 OpenAI API Key
- [ ] **INFR-03**: API Key 存储在系统凭据管理器（Windows Credential Manager），非明文 config.json
- [ ] **INFR-04**: 用户可以在设置中自定义 API Endpoint 和 Model
- [ ] **INFR-05**: 用户可以通过测试按钮验证 API 连接是否可用
- [ ] **INFR-06**: 本地 embedding 模型（bge-small-zh-v1.5）首次启动自动下载，显示下载进度
- [ ] **INFR-07**: 所有数据本地存储在 `~/.kingdee-kb/`，包括向量库和知识文件
- [ ] **INFR-08**: 应用启动时显示 Splash Screen（避免 WebView2 冷启动白屏）
- [ ] **INFR-09**: 用户可以查看当前存储空间占用
- [ ] **INFR-10**: 支持递归分块（H2→段落→句子），使用中文感知分隔符（。！？，）
- [ ] **INFR-11**: 系统实现增量入库（内容哈希去重，避免重复向量化）

## v2 Requirements

Deferred to v0.2. Tracked but not in current roadmap.

### Knowledge Management v2

- **KNOW-11**: 用户通过填入 Git 仓库 URL 导入社区知识包
- **KNOW-12**: 系统自动执行 `git clone` 到 `~/.kingdee-kb/packages/` 并扫描新文件入库
- **KNOW-13**: 知识包可导出为标准格式，支持分享给其他用户

### AI Q&A v2

- **AIQA-07**: 支持 Anthropic Messages API 协议

### Application v2

- **INFR-12**: 跨平台支持（macOS）
- **INFR-13**: 跨平台支持（Linux）
- **INFR-14**: 知识备份/导出功能
- **INFR-15**: 检索结果支持时间范围过滤

## v3 Requirements (Future)

Deferred to v0.3+.

- **KNOW-14**: 支持 .docx 文件解析入库
- **KNOW-15**: 支持 .pdf 文件解析入库
- **INFR-16**: 深色模式
- **INFR-17**: 导入知识库备份

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| 云端知识库/同步 | 核心定位是本地优先，所有数据不上传任何服务器 |
| 内置 LLM 服务/代理调用 | 用户自备 API Key，不代理调用，零服务器成本 |
| 官方知识包服务器 | 知识包由社区自行托管（GitHub 等），不建官方服务器 |
| 多用户/团队协作 | v0.1 仅限单人使用，团队功能远期规划 |
| 实时 Markdown 编辑器 | 应用定位是知识消费而非创作，轻量编辑即可 |
| 全格式文件支持 | v0.1 仅 .md/.txt，降低 MVP 复杂度 |
| 对话历史云端存储 | 本地存储已足够，云端增加复杂度 |
| 移动端应用 | 桌面优先，移动端远期规划 |
| Prompt 模板市场 | 核心是 ERP 垂直领域，不需要通用模板 |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| KNOW-01 | Phase 3 | Completed |
| KNOW-02 | Phase 3 | Completed |
| KNOW-03 | Phase 3 | Completed |
| KNOW-04 | Phase 3 | Completed |
| KNOW-05 | Phase 3 | Completed |
| KNOW-06 | Phase 7 | Pending |
| KNOW-07 | Phase 7 | Pending |
| KNOW-08 | Phase 7 | Pending |
| KNOW-09 | Phase 7 | Pending |
| KNOW-10 | Phase 7 | Pending |
| SRCH-01 | Phase 2 | Completed |
| SRCH-02 | Phase 4 | Completed |
| SRCH-03 | Phase 5 | Completed |
| SRCH-04 | Phase 7 | Pending |
| SRCH-05 | Phase 7 | Pending |
| SRCH-06 | Phase 7 | Pending |
| SRCH-07 | Phase 7 | Pending |
| SRCH-08 | Phase 5 | Completed |
| AIQA-01 | Phase 8 | Pending |
| AIQA-02 | Phase 6 | Pending |
| AIQA-03 | Phase 6 | Pending |
| AIQA-04 | Phase 8 | Pending |
| AIQA-05 | Phase 6 | Pending |
| AIQA-06 | Phase 6 | Pending |
| INFR-01 | Phase 1 | Completed |
| INFR-02 | Phase 8 | Pending |
| INFR-03 | Phase 1 | Completed |
| INFR-04 | Phase 8 | Pending |
| INFR-05 | Phase 8 | Pending |
| INFR-06 | Phase 2 | Completed |
| INFR-07 | Phase 1 | Completed |
| INFR-08 | Phase 1 | Completed |
| INFR-09 | Phase 8 | Pending |
| INFR-10 | Phase 3 | Completed |
| INFR-11 | Phase 3 | Completed |

**Coverage:**
- v1 requirements: 35 total
- Mapped to phases: 35
- Unmapped: 0 ✓

**Phase → Requirements mapping (summary):**

| Phase | Count | Requirements |
|-------|-------|--------------|
| Phase 1 — 脚手架与基础设施 | 4 | INFR-01, INFR-03, INFR-07, INFR-08 |
| Phase 2 — 嵌入与向量存储 | 2 | SRCH-01, INFR-06 |
| Phase 3 — 入库流水线 | 7 | KNOW-01~05, INFR-10, INFR-11 |
| Phase 4 — BM25 全文检索 | 1 | SRCH-02 |
| Phase 5 — 混合检索引擎 | 2 | SRCH-03, SRCH-08 |
| Phase 6 — LLM 集成 | 4 | AIQA-02, AIQA-03, AIQA-05, AIQA-06 |
| Phase 7 — 知识管理与检索前端 | 9 | KNOW-06~10, SRCH-04~07 |
| Phase 8 — AI 对话与设置前端 | 6 | AIQA-01, AIQA-04, INFR-02, INFR-04, INFR-05, INFR-09 |

---
*Requirements defined: 2026-05-23*
*Last updated: 2026-05-23 — Phase 1-5 requirements marked Completed*
