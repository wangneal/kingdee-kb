# signals.jsonl 数据契约

> 位置：`00_项目管理/.dashboard-signals.jsonl`
> 性质：append-only 事件流，UTF-8，每行一个 JSON 对象
> 版本：v3.0（只读定义，v4.0 强制各 Skill 写入）

---

## 1. 格式

```jsonl
{"ts":"2026-05-17T14:30:00+08:00","baseline":"risk","type":"risk.created","id":"R-007","payload":{"severity":"high","title":"需求边界争议","owner":"张三"}}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `ts` | ISO8601 string | 事件发生时区时间 |
| `baseline` | enum | `scope` `schedule` `quality` `risk` `cost` `deliverable` `stakeholder` |
| `type` | dot-notation string | `<domain>.<action>` |
| `id` | string | 实体全局唯一标识 |
| `payload` | object | 各 baseline 自定义字段（见下表） |

---

## 2. v3.0 必须支持的 event_type

### scope（范围基线）

| type | payload 关键字段 | 写入时机 |
|------|-----------------|----------|
| `change.submitted` | `pcr_id`, `impact_days`, `submitter`, `submitted_at` | 变更申请提交后 |
| `change.approved` | `pcr_id`, `approved_at` | 变更审批通过后 |

### schedule（进度基线）

| type | payload 关键字段 | 写入时机 |
|------|-----------------|----------|
| `milestone.shifted` | `milestone_id`, `old_date`, `new_date` | 里程碑日期调整时 |
| `phase.completed` | `phase_id`, `completed_at` | 阶段标记完成时 |

### quality（质量基线）

| type | payload 关键字段 | 写入时机 |
|------|-----------------|----------|
| `defect.created` | `defect_id`, `level` (P0/P1/P2/P3), `module` | 缺陷登记时 |
| `defect.closed` | `defect_id`, `closed_at` | 缺陷关闭时 |

### risk（风险基线）

| type | payload 关键字段 | 写入时机 |
|------|-----------------|----------|
| `risk.created` | `risk_id`, `severity`, `title`, `owner` | 风险识别时 |
| `risk.updated` | `risk_id`, `updated_at`, `status` | 风险状态变更时 |

### cost（成本基线）

| type | payload 关键字段 | 写入时机 |
|------|-----------------|----------|
| `timesheet.logged` | `role`, `hours`, `date` | 顾问填写人天后 |

### deliverable（交付物基线）

| type | payload 关键字段 | 写入时机 |
|------|-----------------|----------|
| `deliverable.completed` | `phase_id`, `name`, `signed_off` (bool) | 交付物签字确认后 |

### stakeholder（干系人基线）

| type | payload 关键字段 | 写入时机 |
|------|-----------------|----------|
| `meeting.held` | `meeting_id`, `participants[]`, `summary` | 会议纪要归档后 |

---

## 3. 写入指南（v3.0 推荐，v4.0 强制）

### Python 示例

```python
import json, datetime, pathlib

SIGNALS_PATH = pathlib.Path("00_项目管理/.dashboard-signals.jsonl")

def emit(event):
    line = json.dumps(event, ensure_ascii=False, default=str)
    with open(SIGNALS_PATH, "a", encoding="utf-8") as f:
        f.write(line + "\n")

# 示例：记录风险创建
emit({
    "ts": datetime.datetime.now().isoformat(),
    "baseline": "risk",
    "type": "risk.created",
    "id": "R-007",
    "payload": {
        "severity": "high",
        "title": "接口延迟导致蓝图确认延期",
        "owner": "李四"
    }
})
```

### 各 Skill 写入职责

| Skill | 应写入事件 |
|-------|-----------|
| `change-manager` | `change.submitted`, `change.approved` |
| `risk-manager` | `risk.created`, `risk.updated` |
| `test-manager` | `defect.created`, `defect.closed` |
| `weekly-report` | `timesheet.logged`（可选） |
| `stakeholder-comms` | `meeting.held` |
| `kickoff-pack` ~ `acceptance-pack` | `phase.completed`, `deliverable.completed` |
| `project-dashboard` | 不写入，只读取 |

---

## 4. 降级行为

- 文件不存在 → dashboard 不报错，相关指标显示"暂无数据"
- 某行 JSON 损坏 → 跳过该行，继续解析后续行
- `ts` 无时区 → 按本地时区解析
