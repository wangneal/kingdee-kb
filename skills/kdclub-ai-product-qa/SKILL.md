---
name: kdclub-ai-product-qa
version: 8.0.0
category: tool
phase: all
description: |
  金蝶产品智能问答。SSE流式调用金蝶云社区知识库API，支持9个产品线。
    This skill should be used when the user wants to "怎么做XX" "星空" "苍穹" "产品问答",
    or mentions similar keywords related to kdclub-ai-product-qa.
---

## 概述

金蝶产品智能问答，调用金蝶云社区官方知识库 API，回答产品操作问题。

## 触发条件

当用户说出以下任意短语时激活：
- "怎么做XX" "如何设置" "报错" "星空" "星瀚" "星辰" "苍穹" "EAS" "精斗云" "账无忧" "产品问答"

## 工作流

### Step 1: 问题解析

识别用户问题：
- 产品名称（星空/星瀚/星辰/苍穹等）
- 功能模块
- 操作场景

### Step 2: 知识库查询

调用金蝶云社区 API：
1. 构造查询关键词
2. 调用知识库搜索接口
3. 获取相关文档列表

### Step 3: 答案生成

基于搜索结果：
1. 提取关键信息
2. 结构化整理
3. 生成操作步骤

### Step 4: 补充说明

若知识库无结果：
- 提供通用排查思路
- 建议联系对应产品支持

## 参考文件

- `references/PROCESS.md` - 处理流程
- `references/products.json` - 产品配置
- `references/cosmic_qa.py` - API 调用脚本

## 反模式

- 编造答案 → 知识库无结果时应明确告知
- 不区分产品 → 不同产品功能差异大

## 上下文更新

- 无特定上下文更新

