---

name: kickoff-pack
version: 8.0.0
category: stage
phase: 启动
description: |
  启动阶段文档包。生成启动会PPT、任命书、干系人通讯录、总体计划。
    This skill should be used when the user wants to "启动阶段" "启动会" "启动会PPT" "任命书" "kickoff",
    or mentions similar keywords related to kickoff-pack.
paths: "01_启动, kickoff"
---

## 概述

生成金蝶 ERP 项目启动阶段全套文档：启动会 PPT、项目任命书、干系人通讯录、总体计划。

## 触发条件

当用户说出以下任意短语时激活：
- "启动阶段" "启动会" "启动会PPT" "任命书" "kickoff"

## 工作流

### Step 0: 模板检查

执行：`python .claude/scripts/download_templates.py <项目目录> --check`
- ✅ 已就绪 → 继续
- ⬜ 需下载 → 自动下载
- ❌ 失败 → 降级为纯 Markdown

### Step 1: 信息收集

从 CLAUDE.md 读取项目配置，补充：
- 客户方项目经理
- 金蝶方项目经理
- 项目组成员名单
- 关键干系人列表

### Step 2: 生成启动会 PPT

调用 kingdee-ppt Skill，生成金蝶官方风格启动会 PPT：
- 项目背景
- 实施方法论
- 团队介绍
- 总体计划
- 沟通机制

### Step 3: 生成配套文档

- 项目任命书
- 干系人通讯录
- 项目总体计划（甘特图）

### Step 4: 交付确认

列出生成的文件清单，等待用户确认。

## 反模式

- 跳过模板检查 → 模板不可用时应降级而非报错
- 信息不全就生成 → 务必先收集完整项目信息

## 上下文更新

- 读取：CLAUDE.md → ## 项目配置
- 写入：01_启动阶段/ 下各文件
- 写入：CLAUDE.md → ## 活动日志

