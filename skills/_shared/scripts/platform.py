#!/usr/bin/env python3
"""平台环境检测模块 — 双平台兼容

检测逻辑（按可靠性排序）：
1. 环境变量（最可靠，无性能开销）
2. 父进程名检测（轻量，比 ps aux 快）
3. 安装路径特征（静态推断）
4. 保守 fallback
"""
import os
import sys
import subprocess
from pathlib import Path


def detect_platform() -> str:
    """
    检测当前运行平台。
    返回: "qoderwork" | "claude-code"
    """
    # 优先级1: 环境变量（最可靠，零开销）
    if os.environ.get("QODERWORK") == "1":
        return "qoderwork"

    # 优先级1b: Claude Code 环境变量（CC启动时设置）
    if os.environ.get("CLAUDE_CODE_CLI") == "1":
        return "claude-code"

    # 优先级2: 安装路径特征（比进程检测更可靠，不受后台进程干扰）
    skill_path = Path(__file__).resolve()
    skill_str = str(skill_path)
    if ".qoderwork" in skill_str:
        return "qoderwork"
    if ".claude" in skill_str:
        return "claude-code"

    # 优先级3: 父进程名检测（轻量，替代 ps aux 全进程扫描）
    # 注意：pgrep 匹配范围较宽，可能误匹配后台进程，故排在路径检测之后
    try:
        # macOS/Linux: 用 pgrep 检测 QoderWork 进程
        result = subprocess.run(
            ["pgrep", "-l", "qoderwork"],
            capture_output=True, text=True, timeout=2
        )
        if result.returncode == 0 and "qoderwork" in result.stdout.lower():
            return "qoderwork"
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass

    # 优先级4: 保守fallback（默认claude-code，因为该套件起源于CC）
    return "claude-code"


def is_qoderwork() -> bool:
    return detect_platform() == "qoderwork"


def is_claude_code() -> bool:
    return detect_platform() == "claude-code"
