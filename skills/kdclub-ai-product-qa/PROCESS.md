# 金蝶产品智能问答 - 处理流程

> 本文档描述 skill 的完整处理流程，供开发和调试参考。
> 当前版本：v1.3（PAT Token 认证）

---

## 一、整体流程概览

```
┌─────────────────────────────────────────────────────────────────────┐
│                         用户输入问题                                  │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ 步骤1：身份验证（kdclub-login）                                       │
│ ┌─────────────────────────────────────────────────────────────────┐ │
│ │ 1. 检查环境变量 KDCLOUD_PAT_TOKEN                                │ │
│ │ 2. 检查 token 文件 ~/.kdclub/token_vip_kingdee_com.json          │ │
│ │ 3. 有效 → 继续    无效 → 引导用户配置 token                      │ │
│ └─────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ 步骤2：产品选择（检查会话状态）                                        │
│ ┌─────────────────────────────────────────────────────────────────┐ │
│ │ selected_product_id 已设置?                                      │ │
│ │   ├─ 是 → 跳过选择，直接使用已选产品                              │ │
│ │   └─ 否 → 获取产品列表 → 展示给用户选择 → 保存选择               │ │
│ └─────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ 步骤3：流式调用智能问答接口                                           │
│ ┌─────────────────────────────────────────────────────────────────┐ │
│ │ 请求：GET /aisapi/ai-search                                      │ │
│ │ Header: Authorization: Bearer <token>                            │ │
│ │ 参数：question, productId, sessionId, channel_level...           │ │
│ └───────────────────────────┬─────────────────────────────────────┘ │
│                             │                                       │
│                             ▼                                       │
│ ┌─────────────────────────────────────────────────────────────────┐ │
│ │ SSE 流式响应（JSON Lines）                                       │ │
│ │ ├── {"type": "start"}                                           │ │
│ │ ├── {"type": "think", ...}      ← 思考过程（可折叠展示）        │ │
│ │ ├── {"type": "answer", ...}     ← 回答内容（原样展示）          │ │
│ │ └── {"type": "end", ...}        ← 结束标记（保存 sessionId）    │ │
│ └─────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                          展示答案给用户                               │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 二、会话状态

skill 需要维护以下 3 个会话状态：

| 状态项 | 类型 | 初始值 | 说明 |
|--------|------|--------|------|
| `selected_product_id` | int / null | null | 用户选择的产品ID |
| `selected_product_name` | string / null | null | 用户选择的产品名称 |
| `session_id` | string / null | null | 多轮对话会话ID |

**状态生命周期：**
- 首次提问：产品选择后设置 `selected_product_id` 和 `selected_product_name`
- API 返回后：从 `end` 消息中提取并保存 `session_id`
- 追问：使用已有的 `selected_product_id` 和 `session_id`
- 更换产品：重置 `selected_product_id` = null, `session_id` = null

---

## 三、详细步骤分解

### 步骤1：身份验证（kdclub-login）

**目的**：确保用户拥有有效的 PAT Token 用于调用接口。

**执行脚本：**
```bash
python <kdclub_login_dir>/scripts/login.py
```

**kdclub-login 内部处理流程：**

```
检查环境变量 KDCLOUD_PAT_TOKEN
    │
    ├── 存在且有效 → 返回成功
    │
    └── 不存在或无效
            │
            ▼
    检查 token 文件 ~/.kdclub/token_vip_kingdee_com.json
            │
            ├── 存在且有效 → 返回成功
            │
            └── 不存在或无效
                    │
                    ▼
            提示用户输入 PAT Token
            （首次配置时）
```

**Token 获取方式：**
1. 访问 https://vip.kingdee.com
2. 登录账号 → 右上角头像 → 个人主页 → 编辑资料
3. PAT TOKEN 区域 → 新建令牌
4. 复制 token（格式：`kdt_xxxxxxxx...`）
5. 设置环境变量：`export KDCLOUD_PAT_TOKEN=your_token`

**注意：** PAT Token 认证无需浏览器交互，重复调用不会弹出浏览器。

---

### 步骤2：产品选择

**目的**：确定用户要咨询的金蝶产品。

**判断逻辑：**

```
selected_product_id 是否为 null?
    │
    ├── 否（已设置）
    │       │
    │       └── 直接使用该 product_id，跳过本步骤
    │
    └── 是（未设置）
            │
            ▼
    执行：python scripts/cosmic_qa.py --list-products
            │
            ▼
    返回产品列表 JSON：
    [
        {"productId": 3, "name": "金蝶AI星瀚"},
        {"productId": 93, "name": "金蝶AI套件"},
        {"productId": 1, "name": "金蝶AI星空企业版/标准版"},
        ...
    ]
            │
            ▼
    展示给用户并等待选择
            │
            ▼
    保存选择到会话状态：
    selected_product_id = 用户选择的 productId
    selected_product_name = 对应的产品名称
```

**触发更换产品的用户指令：**
- "更换产品" / "切换产品" / "我要问其他产品"

**更换产品时的处理：**
1. `selected_product_id` = null
2. `session_id` = null
3. 重新执行产品选择流程

---

### 步骤3：流式调用接口

**目的**：调用金蝶云社区智能服务接口获取答案。

**执行脚本：**
```bash
python scripts/cosmic_qa.py \
  --question "用户问题" \
  --product-id <selected_product_id> \
  --session-id <session_id>
```

**cosmic_qa.py 内部处理流程：**

```
1. 加载 PAT Token
   ├── 环境变量 KDCLOUD_PAT_TOKEN（优先）
   ├── ~/.kdclub/token_vip_kingdee_com.json
   └── 都找不到 → 返回错误

2. 构建请求
   URL: https://vip.kingdee.com/aisapi/ai-search
   Method: GET
   Headers:
     - Authorization: Bearer <token>
     - Accept: text/event-stream
   Query 参数:
     - scene=1
     - searchText=用户问题
     - productId=选择的产品ID
     - useDeepThink=true/false
     - productLineId=35
     - channel_level=Agent Skill
     - sessionId=多轮对话ID（如有）

3. 发送 SSE 请求，接收流式响应

4. 逐行解析并输出 JSON Lines
```

**流式输出格式：**

| 输出顺序 | type | 内容说明 | Agent 处理 |
|---------|------|----------|-----------|
| 1 | `start` | 开始标记 | 内部处理，不展示 |
| 2~N | `think` | 思考过程 | 可折叠展示 |
| 2~N | `answer` | 回答内容片段 | **原样展示给用户** |
| N+1 | `end` | 结束标记 + 完整信息 | 提取 sessionId 保存 |

**`end` 消息结构：**
```json
{
  "type": "end",
  "sessionId": "833052315488370176",
  "fullAnswer": "<p>完整HTML或Markdown内容...</p>",
  "answerFormat": "html",
  "thinkContent": "完整思考过程...",
  "step": "generateAnswer",
  "searchSources": [
    {
      "entityId": "360921589350938368",
      "entityType": "Knowledge",
      "title": "文档标题",
      "url": "https://vip.kingdee.com/knowledge/360921589350938368"
    }
  ]
}
```

**展示要求：**
1. `thinkContent` → 可折叠展示（如"思考过程..."）
2. `fullAnswer` → **原样展示，不改一字**
3. `answerFormat` → 根据值选择渲染方式（html / markdown）
4. `searchSources` → 在 `fullAnswer` 之后展示「参考来源」版块，按顺序列出所有引用文档的标题和链接

---

### 步骤4：多轮对话

**目的**：处理用户的追问，保持对话上下文。

**追问时的流程：**

```
用户继续提问
    │
    ├── 步骤1：执行 kdclub-login（确保 token 有效）
    │
    ├── 步骤2：检查产品（selected_product_id 已设置 → 直接使用）
    │
    └── 步骤3：调用接口（携带 session_id）
            │
            └── 接口返回的答案会基于前文上下文
```

**关键：** 追问时必须传入之前保存的 `session_id`，否则接口会当作新对话处理。

---

## 四、场景流程图

### 场景A：首次提问（完整流程）

```
用户：总账怎么初始化？
  │
  ▼
[步骤1] 执行 kdclub-login
  │
  ├── token 有效
  │       │
  │       ▼
  [步骤2] selected_product_id = null?
  │       │
  │       └── 是 → 展示产品列表
  │               │
  │               └── 用户选择：1. 金蝶AI星空企业版/标准版
  │                       │
  │                       ▼
  │               保存：selected_product_id = 1
  │
  [步骤3] 调用接口
  │       │
  │       └── 返回答案
  │               │
  │               └── 保存：session_id = xxx
  │
  ▼
展示答案给用户
```

### 场景B：同产品追问

```
用户：凭证怎么录入？
  │
  ▼
[步骤1] 执行 kdclub-login → token 有效
  │
[步骤2] selected_product_id = 1（已设置）→ 直接使用
  │
[步骤3] 调用接口（携带 session_id = xxx）
  │
  ▼
展示答案（基于前文上下文）
```

### 场景C：更换产品

```
用户：我要换产品
  │
  ▼
重置状态：
  selected_product_id = null
  session_id = null
  │
  ▼
[步骤2] 展示产品列表
  │
  └── 用户选择：3. 金蝶AI星瀚
          │
          ▼
  保存：selected_product_id = 3
  │
[步骤3] 调用接口（不携带 session_id）
  │
  ▼
展示答案（新对话上下文）
```

### 场景D：Token 失效

```
用户：总账怎么结账？
  │
  ▼
[步骤1] 执行 kdclub-login → token 有效
  │
[步骤2] 检查产品 → 已选(1)
  │
[步骤3] 调用接口
  │
  └── 返回：{"type": "error", "errorCode": "UNAUTHORIZED"}
          │
          ▼
  [必须] 重新执行 kdclub-login 配置有效 token
          │
          ▼
  [步骤3] 重新调用接口
          │
          ▼
  展示答案
```

---

## 五、错误处理流程

### 错误类型与处理

| 错误场景 | 错误码/提示 | 处理方式 |
|---------|------------|---------|
| **Token 未找到** | 脚本返回错误 | 提示安装 kdclub-login 并配置 PAT Token |
| **Token 失效** | `errorCode: UNAUTHORIZED` | **必须**重新执行 kdclub-login，**严禁**自行回答 |
| **网络异常** | 连接失败 | 提示检查网络后重试 |
| **缺少参数** | 脚本返回错误 | 检查调用参数是否完整 |

### ⚠️ 极其重要的处理原则

当 API 调用失败时：
1. **绝对禁止**使用自身知识回答用户问题
2. **必须**明确告知用户 API 调用失败
3. **只能**提供解决建议（重新配置 token、检查网络等）

---

## 六、接口调用时序图

```
用户          Agent           cosmic_qa.py        kdclub-login        金蝶云社区
 │              │                  │                   │                  │
 │── 提问 ─────>│                  │                   │                  │
 │              │── 1.检查token ──>│                   │                  │
 │              │                  │── 读取token文件 ─>│                  │
 │              │                  │<─ 返回token ──────│                  │
 │              │<─ token有效 ─────│                   │                  │
 │              │                  │                   │                  │
 │              │── 2.检查产品 ───>│                   │                  │
 │              │<─ 已选/未选 ─────│                   │                  │
 │              │                  │                   │                  │
 │<─ 选择产品 ──│（如未选）        │                   │                  │
 │── 选择 ─────>│                  │                   │                  │
 │              │── 3.调用接口 ───>│                   │                  │
 │              │                  │────────────────────┴─────────────────>│
 │              │                  │<─ SSE流式响应 ─────────────────────────│
 │              │<─ 实时输出 ──────│                   │                  │
 │              │                  │                   │                  │
 │<─ 展示答案 ──│                  │                   │                  │
 │              │                  │                   │                  │
 │── 追问 ─────>│                  │                   │                  │
 │              │── 4.多轮对话 ───>│（携带sessionId）  │                  │
 │              │                  │────────────────────┴─────────────────>│
 │              │                  │<─ SSE流式响应 ─────────────────────────│
 │<─ 展示答案 ──│                  │                   │                  │
```

---

## 七、关键文件说明

| 文件 | 作用 | 调用关系 |
|------|------|---------|
| `SKILL.md` | Agent 读取的技能说明 | 被 Agent 解析 |
| `scripts/cosmic_qa.py` | 核心问答脚本 | 被 Agent 调用 |
| `products.json` | 产品列表配置 | 被 cosmic_qa.py 读取 |
| `~/.kdclub/token_vip_kingdee_com.json` | Token 存储文件 | 被 cosmic_qa.py 读取 |
| `kdclub-login/scripts/login.py` | Token 管理脚本 | 被 Agent 调用 |

---

## 八、版本变更记录

| 版本 | 日期 | 认证方式 | 关键变更 |
|------|------|---------|---------|
| v1.0 | 2026-04-13 | Cookie | 初始版本 |
| v1.1 | 2026-04-14 | Cookie | 简化架构，配置文件驱动 |
| v1.2 | 2026-04-17 | Cookie | 安全修复，优化环境变量访问 |
| **v1.3** | **2026-04-30** | **PAT Token** | **认证方式升级，Authorization: Bearer** |
