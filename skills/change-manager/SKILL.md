---

name: change-manager
version: 8.0.0
category: mgmt
phase: all
description: |
  变更管理工具。变更申请创建、审批跟踪、OOXML模板自动填充。
    This skill should be used when the user wants to "新增变更" "变更申请" "提变更" "需求变更",
    or mentions similar keywords related to change-manager.
paths: "变更, change"
---

## 概述

管理金蝶 ERP 项目变更申请，支持 OOXML 模板自动填充。

## 触发条件

当用户说出以下任意短语时激活：
- "新增变更" "变更申请" "提变更" "change request" "PCR" "需求变更"

## 工作流

### Step 1: 变更信息收集

收集变更申请信息：
- 变更标题
- 变更原因
- 影响范围（模块、流程、数据）
- 优先级
- 预估工作量

### Step 2: 影响评估

分析变更影响：
- 对现有功能的影响
- 对进度的影响
- 对成本的影响
- 对其他模块的关联影响

### Step 3: 生成变更申请单

使用 OOXML 模板填充变更申请单。

### Step 4: 审批流程

记录审批状态：
- 提交
- 评审中
- 已批准
- 已拒绝
- 已实施

## 反模式

- 变更不评估影响 → 必须分析影响范围
- 变更不跟踪状态 → 必须有完整的生命周期管理

## 上下文更新

- 写入：00_项目管理/变更管理/
- 写入：CLAUDE.md → ## 活动日志

