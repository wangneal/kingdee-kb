---
name: doc-sanitizer
version: 8.0.0
category: tool
phase: all
description: |
  文档脱敏工具。识别并清理敏感信息（人名/电话/地址/金额等）。
    This skill should be used when the user wants to "脱敏" "数据脱敏" "敏感信息" "sanitize",
    or mentions similar keywords related to doc-sanitizer.
---

## 概述

客户文档敏感信息脱敏工具，自动识别并替换敏感数据。

## 触发条件

当用户说出以下任意短语时激活：
- "脱敏" "数据脱敏" "敏感信息" "sanitize" "mask" "anonymize"

## 工作流

### Step 1: 敏感信息识别

扫描文档，识别敏感信息：
- 身份证号
- 手机号
- 银行卡号
- 姓名
- 地址
- 公司名称

### Step 2: 脱敏策略

选择脱敏方式：
- 掩码：保留部分字符（如 138****5678）
- 替换：用占位符替换（如 [手机号]）
- 删除：直接移除

### Step 3: 执行脱敏

按策略执行脱敏处理。

### Step 4: 验证

检查脱敏结果，确保无遗漏。

## 反模式

- 脱敏不完整 → 必须覆盖所有敏感字段
- 过度脱敏影响理解 → 保留必要的上下文信息

## 上下文更新

- 无特定上下文更新

