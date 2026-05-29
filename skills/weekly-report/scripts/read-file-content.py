#!/usr/bin/env python3
"""Deprecated wrapper — 请使用 scripts/extract-content.py

v6.1.0: 本脚本已迁移到套件级共享工具 .claude/scripts/extract-content.py。
此wrapper保留向后兼容性，转发所有参数到新脚本。
"""
import subprocess
import sys
from pathlib import Path

# 定位 extract-content.py
script_dir = Path(__file__).resolve().parent  # weekly-report/scripts/
# scripts/ → weekly-report/ → skills/ → .claude/
suite_root = script_dir.parent.parent.parent
new_script = suite_root / "scripts" / "extract-content.py"

if not new_script.exists():
    print("❌ 共享脚本不存在: scripts/extract-content.py", file=sys.stderr)
    print("   请确认套件安装完整。", file=sys.stderr)
    sys.exit(1)

print("[deprecated] read-file-content.py → 请使用 scripts/extract-content.py", file=sys.stderr)

# 转发所有参数到新脚本
result = subprocess.run([sys.executable, str(new_script)] + sys.argv[1:])
sys.exit(result.returncode)
