---
name: skill-updater
version: 8.0.0
category: core
phase: all
description: |
  套件版本更新工具。检查 Hub 版本，下载整包替换，保留项目数据。
    This skill should be used when the user wants to "检查更新" "更新套件" "重装套件",
    or mentions similar keywords related to skill-updater.
---

## 概述

金蝶实施 Skill 套件的整包版本更新机制。

## 触发条件

当用户说出以下任意短语时激活：
- "检查更新" "更新套件" "重装套件" "安装套件"

## 工作流

### Step 1: 版本查询

提示用户到 Kingdee Skill Hub 确认最新版本号。

### Step 2: 下载与备份

1. 备份当前 `.claude/skills/` 目录
2. 从 Hub 下载新版本整包
3. 解压到临时目录

### Step 3: 替换与验证

1. 替换 `.claude/skills/` 内容（保留用户项目数据）
2. 更新套件元信息（版本号、下载地址）
3. 展示变更摘要

## 反模式

- 增量更新单个 Skill → 本机制是整包替换，不支持单 Skill 增量
- 不备份直接替换 → 必须先备份

## 上下文更新

- 写入：CLAUDE.md → ## 套件元信息（版本号）
- 写入：CLAUDE.md → ## 活动日志

