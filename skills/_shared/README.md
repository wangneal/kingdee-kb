# _shared — 跨skill共享资源

## 目录结构

```
_shared/
├── README.md                 # 本文件
├── signals-writer.md          # 项目事件流写入规范（已存在）
├── template-check.md          # 模板检查规范（已存在）
├── deliverable-scan.md        # 交付物扫描规范（已存在）
├── platform-detector.md       # P0-1 平台检测规范
├── platform-adapter.md        # P0-2 平台适配器规范
├── safe-file-ops.md           # P1-1 文件安全操作规范
├── scripts/                   # 共享Python脚本
│   ├── __init__.py            # 使scripts成为Python package
│   ├── platform.py            # 平台检测模块
│   ├── platform_adapter.py    # 平台抽象适配器
│   └── path_resolver.py       # 路径解析器
└── references/                # 共享参考文档
    ├── phase-activities.md    # 7阶段典型活动列表
    ├── report-templates.md    # 周报Markdown模板
    ├── version-rules.md       # 脱敏规则与双版本差异定义
    ├── tailoring-matrix.md    # 裁剪方案对照表
    ├── folder-structure.md    # 三级文件夹结构蓝图
    ├── git-setup.md           # Git/Gitee团队协作配置
    └── scheduled-inquiries.md # 主动问询定时任务配置
```

## 引用约定

- skill内Python脚本：`from _shared.scripts.platform import detect_platform`
- skill内SKILL.md：`../_shared/signals-writer.md`
- 外部工具：`.claude/skills/_shared/scripts/`

## 导入方式

```python
import sys
from pathlib import Path

_shared_scripts = Path(__file__).resolve().parent.parent / "_shared" / "scripts"
if str(_shared_scripts) not in sys.path:
    sys.path.insert(0, str(_shared_scripts))

from platform import detect_platform
from platform_adapter import get_adapter
from path_resolver import get_suite_root
```
