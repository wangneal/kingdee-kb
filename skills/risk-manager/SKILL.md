---

name: risk-manager
version: 8.0.0
category: mgmt
phase: all
description: |
  风险管理工具。风险识别、7Keys评估、xlsx级联跟踪。
    This skill should be used when the user wants to "记录风险" "识别风险" "风险评估" "风险管理",
    or mentions similar keywords related to risk-manager.
paths: "风险, risk"
---

## 概述

管理金蝶 ERP 项目风险，支持 7Keys 评估和 xlsx 级联跟踪。

## 触发条件

当用户说出以下任意短语时激活：
- "记录风险" "识别风险" "风险评估" "风险管理" "risk" "添加风险"

## 工作流

### Step 1: 风险识别

引导用户识别风险：
- 技术风险
- 进度风险
- 资源风险
- 范围风险
- 外部风险

### Step 2: 7Keys 评估

对每个风险进行 7 维度评估：
1. 发生概率
2. 影响程度
3. 可检测性
4. 应对成本
5. 残余风险
6. 风险责任人
7. 应对策略

### Step 3: 应对策略制定

选择应对策略：
- 规避：消除风险源
- 转移：转嫁给第三方
- 缓解：降低概率或影响
- 接受：制定应急预案

### Step 4: 跟踪与更新

维护风险登记册，定期更新风险状态。

## 反模式

- 风险只识别不跟踪 → 必须有责任人和应对策略
- 评估维度不完整 → 7Keys 缺一不可

## 上下文更新

- 写入：00_项目管理/风险管理/
- 写入：CLAUDE.md → ## 活动日志

