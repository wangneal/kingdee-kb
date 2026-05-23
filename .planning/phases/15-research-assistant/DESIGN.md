# KingdeeKB v0.2 — 智能调研与文档生成（合并版）

**Milestone:** v0.2 — 智能调研与文档生成
**Created:** 2026-05-24
**Status:** Draft

---

## 1. 产品价值

让金蝶实施顾问能：

1. **调研辅助** — 调研会议上实时获得问题推荐、自动记录讨论内容
2. **文档生成** — 基于实施方法论 V10.0 模板自动生成标准化交付物
3. **打通闭环** — 调研产出直接导入文档模板，生成调研报告/纪要

### 核心流程

```
客户会议 → 问题提示板 → 顾问提问 + 记录答案 → 调研记录库
                ↑                                    │
          (语音转写/手动输入)                          ▼
                                             自动生成调研报告/纪要
                                                     │
                                                     ▼
                                             其他交付物模板
                                             (蓝图/PCR/验收单...)
```

---

## 2. 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│              KingdeeKB v0.2 — 智能调研与文档生成               │
│                                                             │
│  ┌───────────┐  ┌──────────┐  ┌─────────────────────┐       │
│  │ 模板解析器  │  │ Edition  │  │  问题推荐引擎          │       │
│  │ (85模板+25 │  │ Manager │  │  (语义匹配 + 排序)     │       │
│  │  调研提纲) │  └────┬─────┘  └─────────┬───────────┘       │
│  └─────┬─────┘       │                  │                   │
│        │              │                  │                   │
│        ▼              ▼                  ▼                   │
│  ┌──────────────────────────────────────────────────┐       │
│  │              统一知识库索引                         │       │
│  │  模板字段库 + 调研问题库 + Edition metadata        │       │
│  │  usearch + BM25 + SQLite                          │       │
│  └──────────────────────────────────────────────────┘       │
│          ▲                 ▲                  ▲             │
│          │                 │                  │             │
│  ┌───────┴────┐  ┌────────┴────────┐  ┌─────┴────────┐    │
│  │ 本地麦克风  │  │ 腾讯会议侧边栏    │  │ 手动输入      │    │
│  │ + Whisper  │  │ (Web Extension) │  │ (Text Input) │    │
│  └──────┬─────┘  └────────────────┘  └──────────────┘    │
│         │                                                  │
│         ▼                                                  │
│  ┌──────────────────────────────────────────────────┐       │
│  │         调研记录库 (ResearchSession)                 │       │
│  │  每次调研 = 一个 Session                             │       │
│  └──────────────────────┬───────────────────────────┘       │
│                         │                                    │
│                         ▼                                    │
│  ┌──────────────────────────────────────────────────┐       │
│  │  文档生成引擎                                      │       │
│  │  模板: 调研报告/纪要/蓝图/PCR/上线单/验收单...      │       │
│  │  填充: LLM + 知识库检索 + 用户输入                  │       │
│  │  输出: .docx / .xlsx                               │       │
│  └──────────────────────────────────────────────────┘       │
│                                                             │
│  ┌──────────────────────────────────────────────────┐       │
│  │  UI 层 — 双模式                                    │       │
│  │  调研模式: 桌面侧边栏 + 腾讯会议侧边栏               │       │
│  │  生成模式: 向导式模板选择 + 分步填写 + 产物预览      │       │
│  └──────────────────────────────────────────────────┘       │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. 数据模型

### Edition Profile（版本设计）

```rust
struct EditionConfig {
    current: Edition,
    available: Vec<Edition>,
}

enum Edition {
    Enterprise,  // 企业版（25 份调研提纲，优先实现）
    Flagship,    // 旗舰版（架构预留）
}
```

### 调研提纲

```rust
struct ResearchOutline {
    edition: Edition,
    module_code: String,     // e.g. "ECW2107"
    module_name: String,     // e.g. "总账"
    cloud_type: String,      // e.g. "财务"
    doc_file: String,
    sections: Vec<Section>,
}

struct Section {
    name: String,
    categories: Vec<Category>,
}

struct Category {
    name: String,
    questions: Vec<Question>,
}

struct Question {
    edition: Edition,
    outline_id: String,
    module_code: String,
    module_name: String,
    section: String,
    category: String,
    question_text: String,
    order: i32,
    embedding: Vec<f32>,
}
```

### 调研记录

```rust
struct ResearchSession {
    id: String,
    edition: Edition,
    title: String,
    client: String,
    participants: Vec<String>,
    start_time: DateTime,
    end_time: Option<DateTime>,
    status: SessionStatus,
    notes: String,
}

struct ResearchQA {
    id: String,
    session_id: String,
    question_id: Option<String>,
    question_text: String,
    answer_text: String,
    source: QASource,
    asked_at: DateTime,
    is_recommended: bool,
}
```

### 存储映射

| 数据 | 存储 | 说明 |
|---|---|---|
| 模板元数据 | SQLite | template_metadata 表 |
| 调研提纲 | SQLite | research_outlines / research_questions |
| 调研记录 | SQLite | research_sessions / research_qa |
| 产物 | 文件系统 | ~/.kingdee-kb/products/ |
| 向量索引 | usearch | 问题 + 知识库共享索引 |
| 全文索引 | tantivy BM25 | 问题 + 知识库共享索引 |
| 版本配置 | SQLite app_config | edition 切换 |

---

## 4. 阶段划分（合并版）

### Phase 9: 源文档解析引擎

**合并自:** 模板解析（原 P9）+ 调研提纲解析（原 P15）

**Goal:** 统一解析所有源文档——85 个交付物模板 + 25 份调研提纲

**Tasks:**
- [ ] DOCX 模板解析器：提取占位符 `{field_name}`，结构化 YAML 元数据
- [ ] XLSX 模板解析器：提取单元格占位符
- [ ] DOC 调研提纲解析器：提取章节/分类/问题（winapi COM）
- [ ] Edition Profile 框架：EditionConfig + 企业版/旗舰版
- [ ] 25 份调研提纲入库 SQLite + 构建向量+BM25 索引
- [ ] 调研问题 embedding 生成（512-dim）
- [ ] 单元测试：解析正确性、索引完整性

**Depends on:** v0.1 基础设施（usearch/embeddings/tantivy/rusqlite）

---

### Phase 10: 文档生成核心

**合并自:** 文档生成（原 P10）+ 报告生成（原 P18）

**Goal:** LLM 填充模板 + 模板渲染 + 调研报告自动生成

**Tasks:**
- [ ] LLM 模板填充引擎：用户输入/知识库检索 → JSON 字段值
- [ ] DOCX 渲染：docx-template 填充占位符
- [ ] XLSX 渲染：umya-spreadsheet 填充单元格
- [ ] 调研报告生成：调研记录 → `06调研报告_模板.docx` 填充
- [ ] 调研纪要生成：单次会议记录 → `03/04调研纪要_模板.docx` 填充
- [ ] 缺失必填字段时提示用户

**Depends on:** Phase 9; DOCX 能力基于 v0.1 现有 `docx_filler.rs` + `template_schema.rs`

---

### Phase 11: 问题推荐 + 智能补全引擎

**合并自:** 智能补全（原 P11）+ 问题推荐（原 P16）

**Goal:** 语义匹配、问题推荐、知识库辅助填充

**Tasks:**
- [ ] 问题检索内核：文本 → embedding → usearch → BM25 → RRFR 融合
- [ ] Edition filter：自动按当前版本过滤
- [ ] 上下文累积：多轮输入拼接，提升匹配精度
- [ ] 去重与优先级：已问降权，高频提权
- [ ] 知识库智能补全：从 v0.1 知识库检索相关内容辅助模板填充
- [ ] 信息追问：LLM 判断缺失关键信息时主动提问
- [ ] Tauri commands（`recommend_questions`, `smart_complete`, `get_modules`）

**Depends on:** Phase 9

---

### Phase 12: Whisper 语音识别

**来自:** 原 P17

**Goal:** 本地麦克风 → Whisper 实时转写 → 文本输出到推荐引擎

**Tasks:**
- [ ] Whisper 模型集成（whisper-rs，Rust 原生绑定）
- [ ] 模型管理：首次自动下载 tiny（~75MB），设置可切换 small（~500MB）
- [ ] 桌面麦克风捕获：Web Audio API (MediaRecorder)
- [ ] 流式转写 pipeline：5-10s 滑动窗口 → 推理 → 推送
- [ ] 中文优化：标点恢复、短句合并、过滤重复
- [ ] Tauri commands（`start_recording`, `stop_recording`, `on_transcription`）

**Depends on:** 独立，可与 Phase 11 并行

---

### Phase 13: 调研记录 + 产物管理后端

**合并自:** 产物管理（原 P12）+ 调研记录（原 P18 记录部分）

**Goal:** 调研会话管理和产物存储

**Tasks:**
- [ ] ResearchSession CRUD：创建/结束/查询调研会话
- [ ] Q&A 记录：从推荐引擎接收已问问题，手动录入答案
- [ ] 调研记录导出：CSV / Markdown 格式
- [ ] 产物历史 API：按项目/时间筛选
- [ ] 产物重新生成：修改输入后重新填充
- [ ] 产物导出到指定目录

**Depends on:** Phase 10

---

### Phase 14: 统一前端 — 提示板 + 向导生成 + 腾讯会议

**合并自:** 前端（原 P13/P14）+ 提示板 UI（原 P19）

**Goal:** 调研模式和文档生成模式共享同一前端框架

**调研模式：**
- 左侧模块/分类树形导航
- 中间推荐问题卡片列表（实时更新）
- 右侧问题详情 + 答案录入
- 录音控制（开始/停止）
- 版本切换控件（企业版/旗舰版）
- 调研会话管理

**文档生成模式：**
- 模板选择界面（按 8 阶段分类卡片）
- 向导式填写（步骤指示器）
- LLM 字段自动填充 + 用户确认
- 知识库补全建议展示
- 产物预览/编辑/导出

**腾讯会议侧边栏（Web Extension）：**
- H5 网页扩展应用
- 手动输入关键词 → 问题推荐
- 与桌面端通过本地 HTTP API 通信

**Depends on:** Phase 10, Phase 11, Phase 12, Phase 13

---

### Phase 15: 集成测试 + 打磨

**合并自:** 打磨（原 P14）+ 集成测试（原 P20）

**Tasks:**
- [ ] 端到端：录音→转写→推荐→记录→报告生成
- [ ] 端到端：模板选择→填写→生成→导出
- [ ] 25 份提纲全量解析验证
- [ ] Whisper 推理延迟 benchmark（目标：tiny < 实时）
- [ ] Edition 切换流程测试
- [ ] 腾讯会议扩展兼容性测试
- [ ] UX 打磨 + 错误处理

**Depends on:** 全部阶段

---

## 5. 依赖关系

```
Phase 9 (源文档解析) — 无依赖，最先启动
  ├─ Phase 10 (文档生成核心) — 依赖 Phase 9
  │    └─ Phase 13 (产物管理后端) — 依赖 Phase 10
  ├─ Phase 11 (推荐引擎) — 依赖 Phase 9
  │    ├─ Phase 12 (Whisper) — 独立，可与 Phase 11 并行
  │    └─ Phase 14 (统一 UI) — 依赖 Phase 10/11/12/13
  └─ Phase 15 (集成测试) — 依赖全部
```

**并行机会：**
- Phase 11 与 Phase 12 可并行
- Phase 12 与 Phase 13 可并行

---

## 6. 关键技术决策

| 决策 | 方案 | 理由 |
|---|---|---|
| STT | Whisper 本地模型（whisper-rs） | 零费用、离线、隐私 |
| Whisper 模型 | 默认 tiny（~75MB），可选 small（~500MB） | tiny CPU 实时运行 |
| 腾讯会议集成 | 网页扩展 H5 + 本地 HTTP API | 官方支持，无需 API key |
| 版本架构 | 同一索引 + edition filter | 避免多索引维护 |
| 文档生成 | LLM 填充 + docx-template 渲染 | v0.1 已有基础实现 |
| 音频捕获 | Web Audio API (MediaRecorder) | 零外部依赖 |

---

## 7. 企业版 vs 旗舰版

| 维度 | 企业版 | 旗舰版 |
|---|---|---|
| 调研提纲 | 25 份（财务12+供应链4+制造6） | 暂无，架构预留 |
| Edition ID | enterprise | flagship |
| 实现 | Phase 9 优先实现 | 框架准备，提纲就绪后导入 |

---

## 8. 风险与缓解

| 风险 | 缓解 |
|---|---|
| Whisper CPU 推理延迟 > 实时 | tiny 模型 + 滑动窗口，先上非实时模式兜底 |
| 腾讯会议 Web Extension 权限限制 | 纯文本输入兜底 |
| DOC 解析中文编码 | 先验证 3 份再批量 |
| 25+85 文档量大 | 后台进度展示，分批索引 |

---

*Design document for KingdeeKB v0.2 — 智能调研与文档生成（合并版）*
*2026-05-24*
