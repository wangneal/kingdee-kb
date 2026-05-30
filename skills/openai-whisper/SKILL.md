---
name: openai-whisper
version: 8.0.0
category: tool
phase: all
description: |
  语音转文字工具。本地Whisper模型转录，支持会议录音。
    This skill should be used when the user wants to "语音转文字" "录音转文本" "会议录音转录",
    or mentions similar keywords related to openai-whisper.
---

## 概述

本地语音转文字工具，基于 OpenAI Whisper 模型。

## 触发条件

当用户说出以下任意短语时激活：
- "语音转文字" "录音转文本" "会议录音转录" "whisper" "转写"

## 工作流

### Step 1: 环境检查

检查 whisper 是否已安装：
```bash
whisper --help
```

若未安装，提示：
```bash
brew install whisper  # macOS
pip install openai-whisper  # Python
```

### Step 2: 音频准备

支持格式：mp3, wav, m4a, mp4, webm, mpeg, mpga, m4a

### Step 3: 转录

执行转录：
```bash
whisper <音频文件> --language zh --model medium
```

### Step 4: 输出

生成带时间戳的文本文件。

## 反模式

- 不检查环境直接调用 → whisper 可能未安装
- 使用小模型处理中文 → 中文建议 medium 或 large 模型

## 上下文更新

- 无特定上下文更新

