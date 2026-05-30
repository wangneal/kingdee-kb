---
name: humanizer
version: 8.0.0
category: tool
phase: all
description: |
  AI文案去味工具。检测24种AI写作模式，重写为自然人类风格。
    This skill should be used when the user wants to "人性化" "去AI味" "自然化" "改写",
    or mentions similar keywords related to humanizer.
---

## 概述

AI 文案去味工具，检测并修正 24 种 AI 写作痕迹，让文档读起来更自然。

## 触发条件

当用户说出以下任意短语时激活：
- "人性化" "去AI味" "humanize" "自然化" "改写"

## 工作流

### Step 1: 文本分析

扫描输入文本，检测 24 种 AI 模式：
- 意义膨胀（Significance inflation）
- 名人堆砌（Notability name-dropping）
- 表面分析（Superficial -ing analyses）
- 推销语言（Promotional language）
- AI 词汇（AI vocabulary）
- 情感回避（Copula avoidance）
- 否定并列（Negative parallelisms）
- 三连法则（Rule of three）
- 同义词循环（Synonym cycling）
- 破折号滥用（Em dash overuse）

### Step 2: 修正建议

为每个检测到的问题提供修正建议。

### Step 3: 重写

根据建议重写文本，保持原意但更自然。

### Step 4: 质量检查

验证重写后的文本是否还有 AI 痕迹。

## 参考文件

- 24 种 AI 检测模式（基于 Wikipedia Signs of AI writing）

## 反模式

- 过度修正改变原意 → 保持原意是前提
- 只替换词汇不改结构 → 结构问题更严重

## 上下文更新

- 无特定上下文更新

