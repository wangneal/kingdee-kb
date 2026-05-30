---
name: data-auditor
version: 8.0.0
category: tool
phase: all
description: |
  数据质量审计工具。完整性检查、一致性校验、质量评分。
    This skill should be used when the user wants to "数据质量" "数据检查" "数据审计",
    or mentions similar keywords related to data-auditor.
---

## 概述

数据质量审计工具，检查数据完整性、一致性、准确性。

## 触发条件

当用户说出以下任意短语时激活：
- "数据质量" "数据检查" "数据审计" "quality check" "data audit"

## 工作流

### Step 1: 审计规则定义

定义检查规则：
- 完整性：必填字段不为空
- 一致性：跨表数据一致
- 准确性：值在合理范围
- 唯一性：主键不重复

### Step 2: 执行审计

按规则逐项检查。

### Step 3: 问题分类

将问题分类：
- 严重：必须修复
- 警告：建议修复
- 信息：仅供参考

### Step 4: 生成审计报告

输出审计报告，包含问题清单和修复建议。

## 参考文件

- `references/anomaly-rules.md` - 异常规则
- `references/health-score.md` - 健康评分

## 反模式

- 只检查不给建议 → 每个问题需有修复建议
- 规则不全面 → 需覆盖完整性、一致性、准确性、唯一性

## 上下文更新

- 写入：04_构建阶段/ 数据审计/

