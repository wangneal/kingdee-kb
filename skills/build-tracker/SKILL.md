---

name: build-tracker
version: 8.0.0
category: stage
phase: 构建
description: |
  构建阶段跟踪器。数据清洗导入、系统配置清单、构建进度跟踪。
    This skill should be used when the user wants to "构建阶段" "数据清洗导入" "系统配置清单" "build tracker",
    or mentions similar keywords related to build-tracker.
paths: "04_构建, build"
---

## 概述

跟踪金蝶 ERP 项目构建阶段进度，包括数据清洗导入、系统配置清单管理。

## 触发条件

当用户说出以下任意短语时激活：
- "构建阶段" "数据清洗导入" "系统配置清单" "build tracker"

## 工作流

### Step 0: 模板检查

检查构建阶段模板是否就绪。

### Step 1: 构建进度跟踪

维护构建任务清单：
- 系统配置项
- 数据导入项
- 二次开发项
- 集成接口项

### Step 2: 数据质量审计

调用 data-auditor Skill：
1. 检查数据完整性
2. 识别重复数据
3. 验证数据格式

### Step 3: 数据清洗

调用 data-cleaner Skill：
1. 去重处理
2. 格式标准化
3. 缺失值处理

### Step 4: 数据导入

生成导入模板，记录导入日志。

### Step 5: 系统配置清单

维护配置清单，记录每项配置状态。

## 参考文件

- `data-auditor` - 数据质量审计
- `data-cleaner` - 数据清洗

## 反模式

- 数据不清洗直接导入 → 脏数据会导致系统问题
- 配置清单不更新 → 无法追踪配置状态

## 上下文更新

- 读取：03_蓝图阶段/ 需求规格
- 写入：04_构建阶段/ 下各文件
- 写入：CLAUDE.md → ## 活动日志

