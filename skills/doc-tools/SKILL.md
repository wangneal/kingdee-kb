---
name: doc-tools
version: 8.0.0
category: tool
phase: all
description: |
  文档操作工具集。OOXML编辑、docx模板填充、HTML→PDF转换。
    This skill should be used when the user wants to "填充模板" "编辑docx" "生成PDF" "文档转换",
    or mentions similar keywords related to doc-tools.
---

## 概述

底层文档操作工具，提供 OOXML 模板编辑和 HTML→PDF 转换能力。

## 触发条件

当用户说出以下任意短语时激活：
- "填充模板" "编辑docx" "生成PDF" "OOXML" "文档转换"

## 工作流

### Step 1: 模板识别

识别模板类型：
- Word (.docx)
- Excel (.xlsx)
- PowerPoint (.pptx)

### Step 2: 数据填充

OOXML 模板编辑：
1. 解析模板占位符
2. 映射数据字段
3. 填充内容
4. 保存为新文件

### Step 3: 格式转换

HTML→PDF 转换：
1. 渲染 HTML 内容
2. 应用打印样式
3. 生成 PDF 文件

## 反模式

- 直接修改原模板 → 应生成新文件
- 不保留原格式 → 填充时需保持模板样式

## 上下文更新

- 无特定上下文更新

