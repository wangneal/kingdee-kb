"""
读取 signals.jsonl，按 baseline / type 索引。
§3.2 定义了 schema，本模块仅做读取和索引，不验证字段完整性。
"""

import json
from pathlib import Path


def read_signals(project_root: Path) -> dict:
    """
    读取项目根或 00_项目管理/ 下的 .dashboard-signals.jsonl（或 signals.jsonl）。
    返回结构：
        {
            "events": [...],
            "by_baseline": {"scope": [...], ...},
            "by_type": {"change.submitted": [...], ...},
            "count": int
        }
    文件不存在或解析失败时返回空结构，永不抛异常。
    """
    empty = {
        "events": [],
        "by_baseline": {},
        "by_type": {},
        "count": 0,
    }

    candidates = [
        project_root / "00_项目管理" / ".dashboard-signals.jsonl",
        project_root / "00_项目管理" / "signals.jsonl",
        project_root / ".dashboard-signals.jsonl",
        project_root / "signals.jsonl",
    ]

    signals_path = None
    for cand in candidates:
        if cand.exists():
            signals_path = cand
            break

    if signals_path is None:
        return empty

    events = []
    try:
        with open(signals_path, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    event = json.loads(line)
                    # 基础校验：确保是 dict 且包含必要字段
                    if isinstance(event, dict) and "baseline" in event and "type" in event:
                        events.append(event)
                except json.JSONDecodeError:
                    continue
    except Exception:
        return empty

    by_baseline: dict = {}
    by_type: dict = {}

    for event in events:
        baseline = event.get("baseline", "unknown")
        event_type = event.get("type", "unknown")
        by_baseline.setdefault(baseline, []).append(event)
        by_type.setdefault(event_type, []).append(event)

    return {
        "events": events,
        "by_baseline": by_baseline,
        "by_type": by_type,
        "count": len(events),
    }
