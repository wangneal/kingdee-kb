# Roadmap: KingdeeKB v0.2 — 智能文档生成

**Milestone:** v0.2 — 基于实施方法论模板的标准化文档生成
**Granularity:** Coarse（6 阶段）
**Created:** 2026-05-23
**Requirements:** 28 v0.2 requirements → 100% mapped

---

## Phases

- [ ] **Phase 9: 模板解析引擎** — 解析 .docx/.xlsx 模板字段 + YAML sidecar 元数据
- [ ] **Phase 10: 文档生成核心** — LLM JSON 填充 + docx-template + umya-spreadsheet 渲染
- [ ] **Phase 11: 智能补全** — 知识库检索辅助填充 + 信息追问
- [ ] **Phase 12: 产物管理后端** — 产物存储/历史/导出
- [ ] **Phase 13: 向导式生成前端** — 模板选择 + 分步填写 + 补全建议 UI
- [ ] **Phase 14: 产物管理前端 + 打磨** — 产物预览/编辑/导出界面

---

## Phase Details

### Phase 9: 模板解析引擎

**Goal**: 解析 85 个实施方法论模板，提取字段占位符，建立 YAML sidecar 元数据。

**Requirements**: TMPL-01~05

**Success Criteria**:
1. 能扫描模板目录，按 8 阶段分类展示
2. 解析 .docx 模板中 `{field_name}` 占位符
3. 解析 .xlsx 模板中单元格占位符
4. YAML sidecar 定义字段类型/必填/fill_strategy
5. 用户可上传自定义模板

**Depends on**: —（v0.1 基础设施已就绪）

---

### Phase 10: 文档生成核心

**Goal**: LLM 填充 + 模板渲染，输出 .docx/.xlsx 产物。

**Requirements**: DGEN-01~07

**Success Criteria**:
1. 用户选择模板后进入向导式流程
2. LLM 根据用户输入生成 JSON 字段值
3. docx-template 渲染 .docx，umya-spreadsheet 渲染 .xlsx
4. 缺失必填字段时提示用户
5. 产物保留模板样式

**Depends on**: Phase 9

---

### Phase 11: 智能补全

**Goal**: 知识库检索辅助 + 信息追问。

**Requirements**: DGEN-05, DLVR-01~07

**Success Criteria**:
1. 自动从 v0.1 知识库检索相关历史内容
2. 检索内容作为 LLM 上下文提升填充质量
3. 用户可手动选择知识库条目辅助填充
4. 7 种关键产物端到端可用

**Depends on**: Phase 9, v0.1 knowledge base

---

### Phase 12: 产物管理后端

**Goal**: 产物持久化存储、历史管理、导出。

**Requirements**: PROD-01~04

**Success Criteria**:
1. 产物按项目/时间存储到 ~/.kingdee-kb/products/
2. 产物历史列表 API
3. 产物重新生成（修改输入后重新填充）
4. 产物导出到指定目录

**Depends on**: Phase 10

---

### Phase 13: 向导式生成前端

**Goal**: 模板选择 + 分步填写 + 智能补全 UI。

**Requirements**: FUI-01~04

**Success Criteria**:
1. 模板选择界面（按 8 阶段分类卡片）
2. 向导式填写流程（步骤指示器）
3. LLM 字段自动填充 + 用户确认
4. 知识库补全建议展示
5. 产物预览界面

**Depends on**: Phase 10, Phase 11

---

### Phase 14: 产物管理前端 + 打磨

**Goal**: 产物历史管理界面 + 最终打磨。

**Requirements**: FUI-05, PROD-01~04 (frontend)

**Success Criteria**:
1. 产物历史列表（按时间/项目筛选）
2. 产物预览与编辑
3. 产物导出按钮
4. 整体 UX 打磨

**Depends on**: Phase 12, Phase 13

---

## Progress

| Phase | Status | Completed |
|-------|--------|-----------|
| 9 | Not started | — |
| 10 | Not started | — |
| 11 | Not started | — |
| 12 | Not started | — |
| 13 | Not started | — |
| 14 | Not started | — |
