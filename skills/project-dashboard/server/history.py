"""看板健康度历史 — F1 趋势线（v6.0）

每次生成看板时把当前健康度快照追加到 00_项目管理/.dashboard-history.jsonl，
趋势线据此绘制多周走势。append-only，每天只保留最后一条，软上限 120 行。
"""
from __future__ import annotations
import json
from datetime import date
from pathlib import Path
from typing import Any

_FILENAME = ".dashboard-history.jsonl"
_SOFT_CAP = 120


def _path(project_root: Path) -> Path:
    return Path(project_root) / "00_项目管理" / _FILENAME


def record(project_root: Path, today: str, health: dict[str, Any]) -> None:
    """追加一条健康度快照。同一天重复运行则覆盖当天记录。失败静默。"""
    try:
        p = _path(project_root)
        p.parent.mkdir(parents=True, exist_ok=True)
        rows: list[dict] = []
        if p.exists():
            for line in p.read_text(encoding="utf-8").splitlines():
                line = line.strip()
                if not line:
                    continue
                try:
                    rows.append(json.loads(line))
                except Exception:
                    continue
        day = (today or date.today().isoformat())[:10]
        entry = {
            "date": day,
            "score": int(health.get("score", 0)),
            "subscores": health.get("subscores", {}),
        }
        # 覆盖当天，否则追加
        rows = [r for r in rows if r.get("date") != day]
        rows.append(entry)
        rows.sort(key=lambda r: r.get("date", ""))
        if len(rows) > _SOFT_CAP:
            rows = rows[-_SOFT_CAP:]
        with open(p, "w", encoding="utf-8") as f:
            for r in rows:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")
    except Exception as e:
        print(f"⚠ 历史记录写入失败（不影响看板生成）: {e}")


def read_trend(project_root: Path, limit: int = 8) -> dict[str, Any]:
    """读取最近 limit 条历史，返回趋势数据。无文件/不足时优雅降级。"""
    empty = {"count": 0, "dates": [], "health": [],
             "baselines": {k: [] for k in ("schedule", "cost", "quality", "risk", "scope", "comm")}}
    try:
        p = _path(project_root)
        if not p.exists():
            return empty
        rows: list[dict] = []
        for line in p.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except Exception:
                continue
        rows.sort(key=lambda r: r.get("date", ""))
        rows = rows[-limit:]
        keys = ("schedule", "cost", "quality", "risk", "scope", "comm")
        return {
            "count": len(rows),
            "dates": [r.get("date", "") for r in rows],
            "health": [int(r.get("score", 0)) for r in rows],
            "baselines": {k: [int(r.get("subscores", {}).get(k, 0)) for r in rows] for k in keys},
        }
    except Exception:
        return empty
