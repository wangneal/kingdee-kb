---
name: project-sync
version: 8.0.0
category: core
phase: all
description: |
  团队文件 Git 同步工具。拉取/推送/解决冲突，中文交互。
    This skill should be used when the user wants to "同步项目" "拉取最新" "保存并同步" "配置同步" "解决冲突",
    or mentions similar keywords related to project-sync.
---

## 概述

管理团队文件协同，通过 Gitee 私有仓库实现多人项目交付物同步。

## 触发条件

当用户说出以下任意短语时激活：
- "同步项目" "拉取最新" "保存并同步" "配置同步" "解决冲突" "Gitee" "团队协同"

## 工作流

### Step 1: 状态检查

执行 `git status` 检查：
- 未提交的本地修改
- 未推送的提交
- 远程仓库的新更改
- 冲突文件列表

### Step 2: 同步操作

根据用户意图执行：
- "拉取最新" → `git pull --rebase`
- "保存并同步" → `git add -A && git commit && git push`
- "查看同步状态" → 显示本地/远程差异摘要

### Step 3: 冲突处理

若检测到冲突：
1. 列出冲突文件
2. 逐个提示冲突内容
3. 提供合并建议
4. 不自动覆盖，等待用户确认

## 反模式

- 强制推送 → 绝不执行 `git push --force`
- 自动覆盖冲突 → 冲突必须人工确认

## 上下文更新

- 读取：CLAUDE.md → ## 项目配置
- 写入：CLAUDE.md → ## 活动日志

