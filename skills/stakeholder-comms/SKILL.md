---
name: stakeholder-comms
version: 8.0.0
category: stage
phase: 项目管理
description: |
  干系人沟通工具。会议录音转录、结构化纪要生成、会议记录管理。
    This skill should be used when the user wants to "会议纪要" "会议记录" "录音转纪要" "stakeholder",
    or mentions similar keywords related to stakeholder-comms.
---

## 概述

管理项目干系人沟通，支持会议录音转纪要、会议记录整理。

## 触发条件

当用户说出以下任意短语时激活：
- "会议纪要" "会议记录" "录音转纪要" "stakeholder" "meeting minutes"

## 工作流

### Step 1: 音频转录

若提供录音文件：
1. 调用 openai-whisper 转录
2. 生成带时间戳的文本

### Step 2: 结构化提取

从转录文本提取：
- 会议主题
- 参会人员
- 讨论要点
- 决议事项
- 待办事项（含责任人和截止日期）

### Step 3: 生成会议纪要

按标准模板生成会议纪要。

### Step 4: 待办跟踪

将待办事项同步到项目活动日志。

## 反模式

- 只记录不提取决议 → 会议纪要核心是决议和待办
- 待办没有责任人 → 每项待办必须有明确责任人

## 上下文更新

- 写入：00_项目管理/会议纪要/
- 写入：CLAUDE.md → ## 活动日志

