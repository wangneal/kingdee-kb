# 异常检测规则（v3.0 五条基础规则）

> 实现位置：`server/anomaly_detector.py`
> 输入：聚合后的数据快照 `snapshot`
> 输出：`List[Anomaly]`，按 `severity` 降序，Top 3 进入"今日聚焦"

---

## Anomaly 数据结构

```python
@dataclass
class Anomaly:
    id: str           # 自动生成，如 ANO-001
    rule_id: str      # 见下表
    severity: str     # high / medium / low
    title: str        # 人类可读标题，如"风险 R-007 已停滞 5 天"
    detail: str       # 补充详情
    action_label: str # 按钮文案，如"立即处理"
    action_cmd: str   # 一键复制命令，如"/risk-manager 更新 R-007"
    baseline: str     # 关联基线
```

---

## 规则 R-STALL：风险停滞

**触发条件**：风险 `updated_at` 距今 > 3 天

```python
# 伪代码
for risk in snapshot["risks"]:
    if risk["status"] == "open":
        days_stalled = (today - risk["updated_at"]).days
        if days_stalled > 3:
            yield Anomaly(
                rule_id="R-STALL",
                severity="high",
                title=f"风险 {risk['id']} 已停滞 {days_stalled} 天",
                detail=f"{risk['title']}，最后更新 {risk['updated_at']:%Y-%m-%d}",
                action_label="立即处理",
                action_cmd=f"/risk-manager 更新 {risk['id']}",
                baseline="risk"
            )
```

---

## 规则 C-PENDING：变更待审批

**触发条件**：变更 `submitted` 状态 > 2 天未审批

```python
for change in snapshot["changes"]:
    if change["status"] == "submitted":
        days_pending = (today - change["submitted_at"]).days
        if days_pending > 2:
            yield Anomaly(
                rule_id="C-PENDING",
                severity="high",
                title=f"变更 {change['pcr_id']} 待审批 {days_pending} 天",
                detail=f"影响 {change.get('impact_days', '?')} 人天，提交人 {change['submitter']}",
                action_label="打开审批",
                action_cmd=f"/change-manager 审批 {change['pcr_id']}",
                baseline="scope"
            )
```

---

## 规则 D-DIVERGE：缺陷发散

**触发条件**：近 3 天缺陷新增数 > 关闭数

```python
recent = [d for d in snapshot["defects"]
          if (today - d["created_at"]).days <= 3]
closed = [d for d in recent if d.get("closed_at")]
if len(recent) - len(closed) > 0:
    yield Anomaly(
        rule_id="D-DIVERGE",
        severity="high",
        title=f"近 3 天缺陷新增 {len(recent)} 条，仅关闭 {len(closed)} 条",
        detail="缺陷收敛异常，建议排查测试环境稳定性",
        action_label="查看缺陷收敛",
        action_cmd="/test-manager 查看缺陷收敛",
        baseline="quality"
    )
```

---

## 规则 M-NEAR：里程碑临近

**触发条件**：里程碑距今 ≤ 7 天且状态为 `pending`

```python
for ms in snapshot["milestones"]:
    if ms["status"] == "pending":
        days_to = (ms["planned_date"] - today).days
        if 0 <= days_to <= 7:
            yield Anomaly(
                rule_id="M-NEAR",
                severity="medium",
                title=f"里程碑「{ms['name']}」T-{days_to}d",
                detail=f"计划 {ms['planned_date']:%Y-%m-%d}，尚未完成",
                action_label="起草周报",
                action_cmd="/weekly-report 起草本周周报",
                baseline="schedule"
            )
```

---

## 规则 W-LATE：周报漏期

**触发条件**：按周计算应有期数 > 实际期数

```python
import math
start = snapshot["project_start_date"]
weeks_elapsed = math.ceil((today - start).days / 7)
actual_reports = len(snapshot["weekly_reports"])
if actual_reports < weeks_elapsed:
    missing = weeks_elapsed - actual_reports
    yield Anomaly(
        rule_id="W-LATE",
        severity="medium",
        title=f"周报漏期 {missing} 期",
        detail=f"项目已进行 {weeks_elapsed} 周，仅生成 {actual_reports} 期周报",
        action_label="生成上周周报",
        action_cmd="/weekly-report 生成上周周报",
        baseline="stakeholder"
    )
```

---

## 排序与截断

1. 按 `severity` 排序：`high` > `medium` > `low`
2. 同 severity 按时间逆序（最新优先）
3. 取前 3 条进入"今日聚焦"
4. 不足 3 条时，剩余位置显示"今天无紧急事项 ✓"
