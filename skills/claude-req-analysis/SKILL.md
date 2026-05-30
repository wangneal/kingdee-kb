---
name: claude-req-analysis
version: 8.0.0
category: tool
phase: all
description: |
  客户需求分析工具。多格式文件解析，结构化需求文档生成。
    This skill should be used when the user wants to "需求分析" "客户资料分析" "需求梳理",
    or mentions similar keywords related to claude-req-analysis.
---

## 概述

客户资料系统性分析，将非结构化文档转化为结构化需求文档。

## 触发条件

当用户说出以下任意短语时激活：
- "需求分析" "客户资料分析" "需求文档" "需求梳理" "requirement analysis"

## 工作流

### Step 1: 文档收集

收集客户提供的各类文档：
- 招标文件
- 需求说明书
- 会议纪要
- 邮件往来

### Step 2: 信息提取

从非结构化文本提取结构化信息：
- 功能需求
- 非功能需求
- 约束条件
- 假设条件

### Step 3: 需求分类

按维度分类：
- 业务需求
- 用户需求
- 系统需求
- 接口需求

### Step 4: 生成需求文档

按模板生成结构化需求文档。

## 参考文件

- `references/doc-template.md` - 文档模板
- `references/parse-scripts.md` - 解析脚本

## 反模式

- 只摘抄不分析 → 需求分析要提炼和结构化
- 忽略非功能需求 → 性能、安全等同样重要

## 上下文更新

- 写入：02_调研阶段/ 需求分析/

