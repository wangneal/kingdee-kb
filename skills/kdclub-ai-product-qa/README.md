# 金蝶产品智能问答

金蝶产品使用问题智能问答助手，基于金蝶云社区智能服务接口，为用户解答金蝶 ERP 产品在日常使用过程中遇到的各类操作、配置、报错等问题。

## 功能简介

本技能通过调用金蝶云社区的智能搜索接口（`/aisapi/ai-search`），以 SSE 流式方式实时返回思考过程和回答内容，支持 Markdown 与 HTML 两种格式的原样输出，并支持多轮对话。

覆盖的产品线包括：金蝶AI星空企业版/标准版、金蝶AI套件、金蝶AI星瀚、金蝶AI星辰、金蝶AI苍穹、EAS Cloud、S-HR Cloud、精斗云-云会计、精斗云-云进销存。产品列表通过 `products.json` 配置文件管理，可灵活扩展。

典型使用场景涵盖财务总账操作（凭证录入、科目设置、结账反结账、报表生成）、供应链管理（采购销售单据、库存出入库、盘点调拨）、生产制造（BOM 维护、生产领料入库、成本核算、MRP 运算）、基础资料维护（物料、客户、供应商管理、组织架构、权限角色）以及系统管理（客户端安装下载、密码账户问题、系统参数配置）等。

## 目录结构

```
kdclub-ai-product-qa/
├── SKILL.md                          # 技能主说明文件（Agent 读取）
├── README.md                         # 本文档
├── products.json                     # 产品列表配置文件
└── scripts/
    └── cosmic_qa.py                  # 核心脚本：问答接口 + Token 管理
```

## 环境要求

### 运行环境

| 项目 | 要求 |
|------|------|
| 操作系统 | Windows / macOS / Linux |
| Python | 3.8 及以上 |
| 网络 | 需能访问 https://vip.kingdee.com |

### Python 依赖

本技能仅使用 Python 标准库，**无需安装任何第三方 pip 包**，也**无需安装任何外部技能依赖**。

### 认证方式

使用金蝶云社区 PAT Token 认证，通过 HTTP Header `Authorization: Bearer <token>` 传递。

**获取 Token 的方法：**
1. 访问 https://vip.kingdee.com 并登录
2. 点击右上角头像 → 个人主页 → 编辑资料
3. 找到「个人访问令牌」区域 → 新建令牌
4. 复制 token（格式如 `kdt_xxxxxxxx...`）

Token 通过脚本内置的 `--save-token` 命令保存到本地 `~/.kdclub/pat_token.json`，一次配置后续自动读取。

### 宿主工具

| 工具 | 安装路径 |
|------|---------|
| QoderWork | `~/.qoderwork/skills/kdclub-ai-product-qa/` |

## 安装步骤

1. 将 `kdclub-ai-product-qa` 整个目录复制到 `~/.qoderwork/skills/` 下。
2. 重启 QoderWork 使技能生效。

无需安装任何外部依赖。

## 脚本说明

**cosmic_qa.py** 是核心脚本，集成了 Token 管理和智能问答两大功能：

**Token 管理命令：**
- `--save-token "kdt_xxx..."` — 保存 Token 到本地文件
- `--check-token` — 检查本地 Token 状态

**问答命令：**
- `--question "问题" --product-id 1` — 调用问答接口
- `--list-products` — 列出所有可选产品

Token 加载优先级：`--token` 参数 → 环境变量 `KDCLOUD_PAT_TOKEN` → 本地文件 `~/.kdclub/pat_token.json`

## 支持的产品

产品列表通过 `products.json` 配置文件管理，当前支持：

| 产品名称 | productId |
|---------|-----------|
| 金蝶AI星空企业版/标准版 | 1 |
| 金蝶AI套件 | 93 |
| 金蝶AI星瀚 | 3 |
| 金蝶AI星辰 | 9 |
| 金蝶AI苍穹 | 87 |
| EAS Cloud | 11 |
| S-HR Cloud | 16 |
| 精斗云-云会计 | 15 |
| 精斗云-云进销存 | 98 |

如需增减产品，直接编辑 `products.json` 即可，无需修改代码。

## 版本信息

| 版本 | 日期 | 说明 |
|------|------|------|
| v1.0 | 2026-04-13 | 初始版本，支持流式问答、多轮对话、UTF-8 编码修复 |
| v1.1 | 2026-04-14 | 简化架构，改用 kdclub-login 管理身份；产品列表改为配置文件驱动 |
| v1.2 | 2026-04-17 | 优化环境变量访问逻辑，添加条件判断和详细注释 |
| v1.3 | 2026-04-30 | 认证方式升级：从 Cookie 登录改为 PAT Token 认证，适配 kdclub-login v2.0 |
| v2.0 | 2026-04-30 | 去除 kdclub-login 依赖，Token 管理内置于脚本；新增 --save-token / --check-token 命令；Token 持久化到本地文件，跨会话自动读取 |
