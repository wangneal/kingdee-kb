# signals.jsonl 统一写入规范

> 本文件是所有 Skill 写入项目事件流的**唯一约定来源**。
> risk-manager / change-manager / test-manager / stakeholder-comms / weekly-report
> 在各自动作完成后引用本文件写入 signal，驱动 project-dashboard 的异常检测与协同中心。

---

## 1. 文件位置与性质

- **路径**：`{项目根}/00_项目管理/.dashboard-signals.jsonl`
- **性质**：append-only 事件流，UTF-8，每行一个 JSON 对象
- **不读旧内容、不重写、不去重**：每次只在文件末尾追加一行
- 看板读取端按 `ts` 取同实体最新状态，写入端无需关心重复

---

## 2. 行格式

```json
{"ts":"ISO8601","baseline":"...","type":"<domain>.<action>","id":"<实体ID>","payload":{...}}
```

| 字段 | 说明 |
|------|------|
| `ts` | 事件时间，带时区的 ISO8601，如 `2026-05-20T14:30:00+08:00` |
| `baseline` | `scope` `schedule` `quality` `risk` `cost` `deliverable` `stakeholder` |
| `type` | `<domain>.<action>`，见 §4 |
| `id` | 实体唯一标识（风险号/变更号/缺陷号/会议号/阶段号） |
| `payload` | 各事件自定义字段，见 §4 |

---

## 3. 写入方法

在 Skill 流程对应步骤执行以下 Python（项目根目录下运行）：

```python
import json, datetime, pathlib

def emit_signal(project_root, baseline, type_, id_, payload):
    """向 .dashboard-signals.jsonl 追加一条事件。失败静默，不中断主流程。"""
    try:
        path = pathlib.Path(project_root) / "00_项目管理" / ".dashboard-signals.jsonl"
        path.parent.mkdir(parents=True, exist_ok=True)
        event = {
            "ts": datetime.datetime.now().astimezone().isoformat(timespec="seconds"),
            "baseline": baseline, "type": type_, "id": id_, "payload": payload,
        }
        with open(path, "a", encoding="utf-8") as f:
            f.write(json.dumps(event, ensure_ascii=False, default=str) + "\n")
    except Exception as e:
        print(f"⚠ signal 写入失败（不影响主流程）: {e}")
```

**容错原则**（全局原则 #5）：写入失败仅记 warning，绝不中断 Skill 主流程。

---

## 4. 各 Skill 写入的事件

| Skill | type | baseline | payload 关键字段 | 写入时机 |
|-------|------|----------|-----------------|----------|
| risk-manager | `risk.created` | risk | `severity, title, owner` | 新增风险后 |
| risk-manager | `risk.updated` | risk | `updated_at, status` | 风险状态/内容更新后 |
| change-manager | `change.submitted` | scope | `impact_days, submitter, submitted_at` | 变更申请提交后 |
| change-manager | `change.approved` | scope | `approved_at` | 变更审批通过后 |
| test-manager | `defect.created` | quality | `level (P0/P1/P2/P3), module` | 缺陷登记后 |
| test-manager | `defect.closed` | quality | `closed_at` | 缺陷关闭后 |
| stakeholder-comms | `meeting.held` | stakeholder | `participants[], summary` | 会议纪要归档后 |
| 7 个阶段 Skill | `deliverable.completed` | deliverable | `phase_id, name, signed_off(bool)` | 产出某项交付物后 |

> `id` 取值：risk→风险号(R-007)，change→变更号(PCR-003)，defect→缺陷号(D-012)，
> meeting→会议号或日期串，deliverable→`{phase_id}/{交付物名}`。
> `deliverable.completed` 的 `signed_off`：交付物产出时填 `false`；客户签字确认后再写一条 `true`。

---

## 5. 不在 v5.0 范围

`milestone.shifted` `timesheet.logged` `phase.completed` 暂无对应 Skill 写入。
其中阶段完成状态看板直接读 CLAUDE.md，无需 signal。看板对缺失事件按既有降级逻辑显示「暂无数据」，不报错。
