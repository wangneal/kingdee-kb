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

## 使用手册

### 1. 下载与安装
从 GitHub [Releases](https://github.com/wangneal/kingdee-kb/releases) 页面下载对应系统的安装包：
- **Windows**：`.msi` 安装包 或 `-portable.zip` 绿色免安装版（解压即用）。
- **macOS**：`.dmg`（Intel 芯片选择 `x64` 版，Apple Silicon 芯片选择 `ARM` 版）。
- **Linux**：`.AppImage` 或 `.deb` 包。

### 2. 初始配置 (LLM API & 嵌入模型)
1. 启动应用后，点击左下角 **设置 (齿轮图标)**。
2. **LLM 服务商**：配置 OpenAI 兼容接口，推荐使用 **DeepSeek**（极速且性价比高）或本地 **Ollama**。填入 API 地址、密钥及模型名称（如 `deepseek-chat`），点击“测试连接”。
3. **嵌入模型 (Embedding)**：应用在首次启动时会自动静默加载本地 `BGE-Small-ZH` 向量模型。如本地缺失，可在设置中点击“下载模型”进行在线获取。

### 3. 核心功能操作指南

* 📁 **项目切换与隔离**：应用以“项目”为组织单元，所有数据（知识库、调研记录、风险控制等）均进行项目隔离。首次使用请前往 **项目管理** 新建您的客户项目，并在左侧栏顶部下拉菜单进行项目切换。
* 📖 **知识库与智能检索**：支持拖拽 PDF、Word、Excel、Visio、Markdown、甚至音视频文件一键导入。检索框支持**全文检索 + 向量检索**的混合搜索，精准定位原始条款。
* 🎙️ **调研助手 & 蓝图生成**：创建调研模块，支持**录音转文字**（需在设置中配置本地 Whisper 或腾讯云/讯飞语音 API）。记录问答后，点击“**一键蓝图提炼**”可自动提炼出现状 (As-Is)、目标 (To-Be) 与差距分析 (Gap)，并提供思维导图可视化呈现。
* 🛡️ **风险把控舱 & 防身话术**：
  * **需求蔓延检测**：导入合同后，将后续新增需求一键与合同条款进行比对，防范范围失控。
  * **防身话术生成**：针对项目拖延、验收推诿等场景，自动生成得体且不失立场的沟通话术。
* 🤝 **腾讯会议 MCP 集成**：在设置中绑定腾讯会议 AI Token。支持在软件内预约会议、自动拉取已结束会议的**音频转写和 AI 智能纪要**，快速整理待办。

## 开发者指南

**前置条件**：Node.js 20+、Rust stable、pnpm

```bash
# 安装依赖
pnpm install

# 开发模式启动
pnpm tauri dev

# 构建生产包
pnpm tauri build

# 代码检查
cargo clippy --all-targets -- -D warnings  # Rust
pnpm lint                                  # TypeScript
pnpm typecheck                             # 类型检查
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
│   ├── src/services/       # 核心服务：按领域模块化（agent, knowledge, project, risk, skill, media, security, common 等）
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
