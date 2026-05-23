# Research Summary: v0.2 智能文档生成

**Date:** 2026-05-23

## Key Technical Decisions

### 1. Docx 模板引擎: `docx-template` crate
- 纯 Rust，自动处理 OOXML split-run 问题
- `DocxTemplate::fill(&hashmap)` 替换 `{key}` 占位符
- 保留所有格式（仅修改文本节点）
- 复杂场景（表格循环/图片）fallback 到 python-docx sidecar

### 2. Xlsx 模板引擎: `umya-spreadsheet` crate
- **唯一支持"读→改→写"闭环**的 Rust crate
- 读取模板 → 替换单元格值 → 保留样式/公式/合并单元格
- 大文件可启用 `lazy_read()` 优化

### 3. 模板字段定义: YAML Sidecar
- 每个 .docx/.xlsx 模板配一个 `.schema.yaml`
- 定义: 字段名、类型、必填/可选、默认值、LLM 提示
- `fill_strategy`: `user` (手动) | `llm` (LLM填充) | `auto` (系统计算)

### 4. 文档生成架构: LLM → JSON → Template
- LLM 不直接生成 .docx，生成结构化 JSON
- JSON 经 JSON Schema 验证后传入模板引擎渲染
- `temperature=0.2` 确保确定性

### 5. Prompt 设计
- System: ERP 文档生成助手
- Input: YAML schema (字段定义) + 用户非结构化文本 + 已填字段
- Output: `{ fields: {...}, _warnings: [...], _confidence: {...} }`
