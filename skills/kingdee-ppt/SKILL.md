---
name: kingdee-ppt
version: 8.0.0
category: tool
phase: all
description: |
  金蝶官方风格PPT生成器。29种版式、双风格体系、7种思维模型自动识别。
    This skill should be used when the user wants to "做PPT" "汇报材料" "演示文稿" "PPTX",
    or mentions similar keywords related to kingdee-ppt.
---

## 概述

生成金蝶官方风格幻灯片，支持 PPTX 和 HTML 两种格式，含 29 种版式和 7 种思维模型。

## 触发条件

当用户说出以下任意短语时激活：
- "做PPT" "汇报材料" "演示文稿" "生成幻灯片" "PPTX" "HTML幻灯片"

## 工作流

### Step 1: 格式选择

询问用户输出格式：
- HTML 交互式演示（支持动效）
- PPTX 可编辑文件

### Step 2: 内容分析

1. 分析输入内容结构
2. 自动匹配思维模型（金字塔/SWOT/PDCA/黄金圈/5W1H/SCQA/IPD五看）
3. 推荐版式组合

### Step 3: 大纲确认

输出每页大纲及推荐版式，等待用户确认。

### Step 4: 内容生成

逐页生成内容脚本（含排版指令）。

### Step 5: 输出

- HTML：生成可交互的网页演示
- PPTX：生成可编辑的 PowerPoint 文件

## 参考文件

- 20+ 设计规范文件（layout-*.md, html-*.md）
- `references/anti-ai-slop.md` - 设计反模式
- `references/brand-colors.md` - 品牌色规范

## 反模式

- 使用红色 #E8210A → 严格禁止
- 版式选择不当 → 应根据内容结构自动匹配

## 上下文更新

- 无特定上下文更新

