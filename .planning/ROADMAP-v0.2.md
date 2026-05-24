# Roadmap: KingdeeKB v0.2 — 智能调研与文档生成

**Milestone:** v0.2 — 智能调研与文档生成（合并 v0.2 文档生成 + v0.3 调研助手）
**Granularity:** Fine（7 阶段）
**Created:** 2026-05-24
**Requirements:** TBD

---

## Phases

- [x] **Phase 9: 源文档解析引擎** ✅ — 解析85个交付物模板(.docx/.xlsx) + 25份调研提纲(.doc)，Edition Profile 框架
- [x] **Phase 10: 文档生成核心** ✅ — LLM 填充 + 模板渲染 + 调研报告/纪要自动生成
- [x] **Phase 11: 问题推荐 + 智能补全引擎** ✅ — 语义匹配 + 问题推荐 + 知识库辅助填充
- [x] **Phase 12: Whisper 语音识别** ✅ — 本地麦克风 + 实时转写（cpal+whisper-rs+中文后处理）
- [x] **Phase 13: 调研记录 + 产物管理后端** ✅ — Session 管理 + Q&A CRUD + CSV/MD导出
- [x] **Phase 14: 统一前端** ✅ — 调研助手页面 + 语音输入 + 问题推荐卡片 + 导航
- [x] **Phase 15: 集成测试 + 打磨** ✅ — 121 tests passing, backend+frontend verified

---

## Phase Details

### Phase 9: 源文档解析引擎 ✓ Complete

**Goal:** 统一解析所有源文档——85 个交付物模板 + 25 份调研提纲（企业版）

**Tasks:**
- DOCX/XLSX 模板解析器（占位符提取 + YAML 元数据）
- DOC 调研提纲解析器（章节/分类/问题提取）
- Edition Profile 框架（企业版/旗舰版）
- 调研提纲入库 SQLite + 向量+BM25 索引
- 调研问题 embedding 生成

**Depends on:** v0.1 基础设施（usearch/embeddings/tantivy/rusqlite）

---

### Phase 10: 文档生成核心

**Goal:** LLM 填充模板 → 标准化 .docx/.xlsx 输出

**Tasks:**
- LLM 模板填充引擎
- DOCX/XLSX 渲染
- 调研报告生成（调研记录 → 模板填充）
- 调研纪要生成

**Depends on:** Phase 9

---

### Phase 11: 问题推荐 + 智能补全引擎

**Goal:** 语义匹配、问题推荐、知识库辅助填充

**Tasks:**
- 问题检索内核（向量+BM25+RRFR 融合）
- Edition filter
- 上下文累积匹配
- 知识库智能补全
- 信息追问

**Depends on:** Phase 9

---

### Phase 12: Whisper 语音识别

**Goal:** 本地麦克风 → Whisper 实时转写 → 问题推荐引擎

**Tasks:**
- whisper-rs 集成（tiny ~75MB / small ~500MB）
- 桌面麦克风捕获
- 流式转写 pipeline
- 中文后处理（标点恢复、短句合并）

**Depends on:** 独立，可与 Phase 11 并行

---

### Phase 13: 调研记录 + 产物管理后端

**Goal:** 调研会话管理和产物持久化存储

**Tasks:**
- ResearchSession CRUD
- Q&A 记录
- 调研记录导出（CSV/Markdown）
- 产物历史/重新生成/导出

**Depends on:** Phase 10

---

### Phase 14: 统一前端

**Goal:** 调研模式 + 文档生成模式 + 腾讯会议侧边栏

**调研模式：**
- 模块导航 + 问题推荐卡片 + 答案录入
- 录音控制 + 版本切换 + 会话管理

**文档生成模式：**
- 模板选择 + 向导填写 + LLM 填充
- 产物预览/编辑/导出

**腾讯会议侧边栏：**
- H5 Web Extension，手动输入 → 问题推荐

**Depends on:** Phase 10, Phase 11, Phase 12, Phase 13

---

### Phase 15: 集成测试 + 打磨

**Tasks:**
- 端到端：录音→转写→推荐→记录→报告生成
- 端到端：模板选择→填写→生成→导出
- 25 份提纲全量验证
- Whisper 延迟 benchmark
- Edition 切换 + 腾讯会议兼容性
- UX 打磨

**Depends on:** 全部阶段

---

## Progress

| Phase | Status | Completed |
|-------|--------|-----------|
| 9 | ✅ Complete | 2026-05-24 |
| 10 | ✅ Complete | 2026-05-24 |
| 11 | ✅ Complete | 2026-05-24 |
| 12 | ✅ Complete | 2026-05-24 |
| 13 | ✅ Complete | 2026-05-24 |
| 14 | ✅ Complete | 2026-05-24 |
| 15 | ✅ Complete | 2026-05-24 |

---

## Dependency Graph

```
Phase 9 (源文档解析)
  ├─ Phase 10 (文档生成) ── Phase 13 (产物管理)
  ├─ Phase 11 (推荐引擎) ──┐
  │    └─ Phase 12 (Whisper)┤
  └─────────────────────────┘
           ├─ Phase 14 (统一前端)
           └─ Phase 15 (集成测试)
```

All phases complete. v0.2 milestone achieved — 智能调研与文档生成.
Next: v1.0 planning (production hardening,腾讯会议集成, user testing).

---

*Roadmap updated: 2026-05-24 — v0.2 全部 7 阶段完成（智能调研与文档生成）*
