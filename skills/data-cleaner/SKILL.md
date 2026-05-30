---
name: data-cleaner
version: 8.0.0
category: tool
phase: all
description: |
  数据清洗工具。格式转换、去重、标准化、导入数据准备。
    This skill should be used when the user wants to "数据清洗" "格式转换" "去重" "数据导入",
    or mentions similar keywords related to data-cleaner.
---

## 概述

数据清洗工具，支持去重、格式转换、缺失值处理。

## 触发条件

当用户说出以下任意短语时激活：
- "数据清洗" "数据整理" "格式转换" "数据导入" "clean data" "deduplicate" "去重"

## 工作流

### Step 1: 数据分析

扫描数据源：
- 数据格式
- 数据量
- 字段类型
- 缺失情况

### Step 2: 去重处理

识别并处理重复数据：
- 完全匹配去重
- 模糊匹配去重
- 保留最新记录

### Step 3: 格式标准化

统一数据格式：
- 日期格式
- 数值精度
- 编码格式
- 字段命名

### Step 4: 缺失值处理

处理缺失数据：
- 填充默认值
- 标记为未知
- 删除不完整记录

### Step 5: 输出

生成清洗后的数据文件和清洗报告。

## 反模式

- 删除不告知 → 任何删除操作需用户确认
- 不记录清洗日志 → 必须记录每步处理的记录数

## 上下文更新

- 写入：04_构建阶段/ 数据清洗/

