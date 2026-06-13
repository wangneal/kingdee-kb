# 顾问工作台

> 面向金蝶 ERP 实施顾问的 AI 辅助知识库桌面应用
> 软件名：顾问工作台 / KingdeeKB（内部代号）

## 核心功能

- 知识库文档导入：支持文本、文件、文件夹及视频转写等多种导入方式
- 混合搜索：向量检索 + BM25 + RRF 融合排序，精准定位知识片段
- AI 对话：基于 ReAct Agent 的多轮对话，支持附件上传与流式输出
- 调研助手：结构化问答记录、语音输入、一键蓝图提炼
- 文档生成：7 种内置模板（调研报告、蓝图、会议纪要等），AI 智能填充
- 风险把控舱：需求蔓延检测、项目健康度评估、防身话术生成
- 知识编译：LLM 自动生成 Wiki 候选页面，构建结构化知识体系
- 技能系统：25+ 内置技能，支持自定义 SKILL.md 扩展

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri v2 (Rust + WebView) |
| 前端 | React 19 + TypeScript + Tailwind CSS v4 |
| 构建工具 | Vite + pnpm |
| 向量模型 | fastembed (BGE-Small-ZH) |
| 全文搜索 | tantivy |
| Agent 引擎 | rig-core (Rust) |
| 数据存储 | SQLite |
| LLM 供应商 | OpenAI / Anthropic / DeepSeek / Ollama |

## 快速开始

**前置条件**：Node.js 20+、Rust stable、pnpm

```bash
# 安装依赖
pnpm install

# 开发模式启动
pnpm tauri dev

# 构建生产包
pnpm tauri build
```

## 项目结构

```
KingdeeKB/
├── src/                    # 前端 (React + TypeScript)
│   ├── components/         # UI 组件
│   ├── contexts/           # React Context 状态管理
│   ├── lib/                # 工具函数与 Tauri IPC 封装
│   └── pages/              # 页面：Chat / Settings / Skills / RiskControl
├── src-tauri/              # 后端 (Rust)
│   ├── src/commands/       # Tauri 命令层
│   ├── src/services/       # 核心服务：Agent、LLM、搜索、技能
│   └── resources/          # 系统提示词等资源文件
├── skills/                 # 用户安装的技能
├── docs/                   # 项目文档
└── models/                 # 本地嵌入模型缓存
```

## 文档

- [架构文档](docs/ARCHITECTURE.md) - 系统架构、核心流程、技术细节
- [使用指南](docs/USER-GUIDE.md) - 功能说明、操作指引、常见问题

## 许可证

MIT License
