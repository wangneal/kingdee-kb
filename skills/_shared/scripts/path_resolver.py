#!/usr/bin/env python3
"""路径解析器 — 统一处理套件内路径

不依赖 platform.py，可独立使用。
"""
import os
from pathlib import Path


def get_suite_root() -> Path:
    """获取skill套件根目录"""
    # 优先级1: 环境变量
    env_root = os.environ.get("KINGDEE_SUITE_ROOT")
    if env_root:
        return Path(env_root).resolve()

    # 优先级2: 从当前文件推断（适用于 .claude/skills/_shared/scripts/ 下的调用）
    this_file = Path(__file__).resolve()
    # 向上搜索：找到包含 .claude/skills 目录的父目录
    for parent in this_file.parents:
        if (parent / ".claude" / "skills").exists():
            return parent
        if (parent / ".qoderwork" / "skills").exists():
            return parent / ".qoderwork"

    # 优先级3: 从工作目录推断
    cwd = Path.cwd()
    if (cwd / ".claude" / "skills").exists():
        return cwd
    if (cwd / ".qoderwork" / "skills").exists():
        return cwd / ".qoderwork"

    # Fallback: 当前目录
    return cwd


def get_skill_path(skill_name: str) -> Path:
    """获取指定skill的目录路径"""
    root = get_suite_root()
    if (root / ".claude" / "skills").exists():
        return root / ".claude" / "skills" / skill_name
    return root / "skills" / skill_name


def get_shared_path() -> Path:
    """获取_shared目录路径"""
    root = get_suite_root()
    if (root / ".claude" / "skills").exists():
        return root / ".claude" / "skills" / "_shared"
    return root / "skills" / "_shared"


def get_scripts_path() -> Path:
    """获取scripts目录路径"""
    return get_suite_root() / "scripts"


def get_project_mgmt_path() -> Path:
    """获取00_项目管理目录路径"""
    cwd = Path.cwd()
    mgmt = cwd / "00_项目管理"
    if mgmt.exists():
        return mgmt
    return cwd
