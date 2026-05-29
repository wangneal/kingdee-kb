# Mermaid Diagram Patterns

Ready-to-use syntax patterns for ERP blueprint flow diagrams.

## Table of Contents

- [Flowchart Patterns](#flowchart-patterns)
- [State Diagram Patterns](#state-diagram-patterns)
- [Sequence Diagram Patterns](#sequence-diagram-patterns)
- [Best Practices](#best-practices)

---

## Flowchart Patterns

### Node Shapes

```
[节点名称]            — 矩形: 流程节点/系统模块
{判断条件?}           — 菱形: 条件分支/审批判断
((开始/结束))         — 圆形: 流程起止
([操作])              — 体育场形: 用户操作/手动步骤
[[子流程]]            — 子流程: 链接到另一个图
>结果]                — 非对称: 输出/结果
```

### Subgraphs for Grouping

```mermaid
graph TD
    subgraph 采购模块["采购管理模块"]
        采购申请[采购申请]
        采购审批[采购审批]
        采购下单[采购下单]
    end

    subgraph 财务模块["财务管理模块"]
        付款申请[付款申请]
        付款审批[付款审批]
        付款执行[付款执行]
    end

    采购下单 --> 付款申请
    付款申请 --> 付款审批
```

### Complete Approval Flow Example

```mermaid
graph TD
    Start((开始)) --> Submit[提交申请]
    Submit --> DeptCheck{部门负责人审批}
    DeptCheck -->|通过| AmountCheck{金额>10万?}
    DeptCheck -->|驳回| Submit
    AmountCheck -->|是| VPApprove[VP审批]
    AmountCheck -->|否| FinanceCheck{财务审批}
    VPApprove --> FinanceCheck
    FinanceCheck -->|通过| Done[流程结束]
    FinanceCheck -->|驳回| Submit

    classDef process fill:#e1f5fe,stroke:#0288d1,stroke-width:2px
    classDef decision fill:#fff9c4,stroke:#fbc02d,stroke-width:2px
    classDef start fill:#c8e6c9,stroke:#388e3c,stroke-width:2px
    class Submit,Done classDef process
    class DeptCheck,AmountCheck,FinanceCheck,VPApprove decision
    class Start start
```

### Decision Branches

```mermaid
graph TD
    Current[当前环节] --> Action([用户操作])
    Action --> Check{条件判断}
    Check -->|条件A| PathA[路径A]
    Check -->|条件B| PathB[路径B]
    Check -->|条件C| PathC[路径C]
```

---

## State Diagram Patterns

### Basic State Transitions

```mermaid
stateDiagram-v2
    [*] --> 待处理
    待处理 --> 处理中: 提交
    处理中 --> 已完成: 成功
    处理中 --> 异常: 失败
    异常 --> 待处理: 退回
    已完成 --> [*]
```

### Complex Business Object States

```mermaid
stateDiagram-v2
    [*] --> 草稿

    state 草稿 {
        [*] --> 编辑中
        编辑中 --> 待提交: 保存
    }

    待提交 --> 审批中: 提交

    state 审批中 {
        [*] --> 部门审批
        部门审批 --> 财务审批: 通过
        部门审批 --> 退回修改: 驳回
        财务审批 --> 终审: 通过
        财务审批 --> 退回修改: 驳回
    }

    终审 --> 已生效
    退回修改 --> 草稿
    已生效 --> [*]
```

### Order Lifecycle Example

```mermaid
stateDiagram-v2
    [*] --> 新建
    新建 --> 已确认: 确认订单
    已确认 --> 生产中: 下达生产
    生产中 --> 已完工: 完工入库
    已完工 --> 已发货: 发货
    已发货 --> 已签收: 客户签收
    已发货 --> 退货中: 退货
    已签收 --> 已完成: 结算
    退货中 --> 已退款: 退款完成
```

---

## Sequence Diagram Patterns

### Basic System Interaction

```mermaid
sequenceDiagram
    actor 用户
    participant ERP as ERP系统
    participant 外部系统 as 外部系统
    participant DB as 数据库

    用户->>ERP: 操作请求
    ERP->>外部系统: 接口调用
    外部系统-->>ERP: 返回数据
    ERP->>DB: 写入记录
    DB-->>ERP: 确认
    ERP-->>用户: 操作结果
```

### ERP Module Interaction Example

```mermaid
sequenceDiagram
    actor 采购员
    participant 采购 as 采购模块
    participant 库存 as 库存模块
    participant 财务 as 财务模块

    采购员->>采购: 创建采购订单
    采购->>库存: 查询库存状态
    库存-->>采购: 返回库存信息
    采购->>采购: 生成采购订单

    Note over 采购: 订单审批通过后

    采购->>库存: 通知预计入库
    库存-->>采购: 确认

    Note over 采购,库存: 收货入库时

    库存->>财务: 生成应付凭证
    财务-->>库存: 确认
```

### Error Handling Pattern

```mermaid
sequenceDiagram
    actor 用户
    participant 前端
    participant 后端
    participant 第三方

    用户->>前端: 操作
    前端->>前端: 显示加载中
    前端->>后端: 请求

    alt 成功
        后端-->>前端: 200 OK
        前端-->>用户: 显示结果
    else 客户端错误
        后端-->>前端: 400 {errors}
        前端-->>用户: 显示验证错误
    else 服务端错误
        后端-->>前端: 500
        前端-->>用户: 显示系统异常+重试
    else 第三方超时
        后端->>后端: 超时
        后端-->>前端: 504
        前端-->>用户: 显示第三方服务异常
    end
```

---

## Best Practices

### Diagram Size
- Max 15-20 nodes per diagram
- More complex → split into sub-flows with cross-references
- Use subgraphs to group related nodes (max 3-4 subgraphs)

### Splitting Complex Flows
When a flow exceeds 20 nodes:
1. Identify logical boundaries (by module, by role, by phase)
2. Create a high-level flow with `[[子流程]]` nodes
3. Create separate detailed diagrams for each sub-flow
4. Link with a note: `详见: 采购-to-be/flow.md`

### Consistent Styling
```mermaid
classDef process fill:#e1f5fe,stroke:#0288d1,stroke-width:2px
classDef decision fill:#fff9c4,stroke:#fbc02d,stroke-width:2px
classDef system fill:#e8e8e8,stroke:#999,stroke-width:2px
classDef start fill:#c8e6c9,stroke:#388e3c,stroke-width:2px
classDef error fill:#ffebee,stroke:#d32f2f,stroke-width:1px
```

### Naming Conventions
- Process nodes: descriptive Chinese names — `采购申请`, `财务审批`
- Decisions: question format — `金额>10万?`, `是否通过?`
- States: Chinese status names — `待处理`, `审批中`, `已完成`
- Edges: short Chinese labels — `通过`, `驳回`, `提交`

### File Naming
- kebab-case or Chinese: `as-is-flow.md`, `to-be-flow.md`
- By module: `purchase-flow.md`, `finance-flow.md`
