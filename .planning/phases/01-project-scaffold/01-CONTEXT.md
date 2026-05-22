# Phase 1: 项目脚手架与基础设施 — Context

**Gathered:** 2026-05-23
**Status:** Ready for planning
**Mode:** Auto-generated (--auto)

<domain>
## Phase Boundary

**Goal**: 应用能成功启动并具备基础框架——Tauri 2.x 桌面窗口、Splash Screen 消除冷启动白屏、本地数据目录就绪、API Key 安全存储框架就绪。

**Requirements**: INFR-01, INFR-03, INFR-07, INFR-08

**Success Criteria**:
1. 应用在 Windows 上以 Tauri 2.x 桌面窗口成功启动，启动过程显示 Splash Screen
2. Splash Screen 平滑过渡到 React 前端主界面（消除 WebView2 冷启动白屏）
3. 首次启动自动创建 `~/.kingdee-kb/` 目录结构（knowledge/、index/、models/、bm25_index/、metadata.db）
4. API Key 通过 Windows Credential Manager 安全存储与读取，不落盘为明文 JSON
5. WebView2 fixedRuntime 捆绑打包，安装包体积 < 30MB（不含业务依赖）

**Out of scope for this phase**: embedding、vector store、ingestion pipeline、BM25、search、LLM integration、knowledge management UI
</domain>

<decisions>
## Implementation Decisions

### the agent's Discretion
All implementation choices are at the agent's discretion — discuss phase was run in --auto mode. Use ROADMAP phase goal, success criteria, research findings, and codebase conventions to guide decisions.

### Auto-Selected Decisions (--auto mode)

[auto] **Tauri 初始化方式** — Q: "create-tauri-app (npm) vs cargo tauri init?" → Selected: `create-tauri-app` (npm) — 官方推荐方式，一键生成 React + Vite + TypeScript 脚手架，比纯 cargo 方式更快上手

[auto] **包管理器** — Q: "npm vs pnpm vs yarn?" → Selected: npm — Node.js 自带，Tauri 官方模板默认，零额外安装依赖

[auto] **前端构建工具** — Q: "Vite vs Webpack?" → Selected: Vite 6 — Tauri 2.x 官方推荐，极快的 HMR 和构建速度，对桌面应用的前端开发体验最优

[auto] **React 版本** — Q: "React 18 vs React 19?" → Selected: React 19 — 2026 年最新稳定版，React Server Components 等新特性成熟可用（尽管桌面应用主要用客户端渲染）

[auto] **路由方案** — Q: "react-router-dom vs TanStack Router?" → Selected: react-router-dom v7 — 最广泛使用的 React 路由库，简单可靠的 SPA 路由，TypeScript 支持完善

[auto] **代码风格工具** — Q: "Biome vs ESLint + Prettier?" → Selected: Biome — 单一工具同时处理格式化和 Lint，比 ESLint+Prettier 组合快 10-50x，原生支持 TypeScript/JSX

[auto] **Splash Screen 实现** — Q: "Tauri 原生 splashscreen vs React 组件?" → Selected: Tauri 原生 splashscreen（`tauri::splashscreen`）— 在 WebView 初始化前由 Rust 原生渲染，彻底消除冷启动白屏，PITFALLS.md §4 明确推荐

[auto] **Keyring 库** — Q: "tauri-plugin-keyring-store vs keyring-rs?" → Selected: `tauri-plugin-keyring-store` — Tauri 官方插件，对 Windows Credential Manager / macOS Keychain / Linux Secret Service 提供统一 API，PITFALLS.md §8 推荐方案

[auto] **目录结构** — Q: "标准 Tauri 结构 vs 自定义?" → Selected: 标准 Tauri 结构（`src-tauri/` Rust 后端 + `src/` React 前端），遵循 Tauri 社区约定

[auto] **TypeScript 严格模式** — Q: "strict vs relaxed?" → Selected: strict — 从第一天就启用，减少后续阶段的类型安全问题

[auto] **TailwindCSS 版本** — Q: "TailwindCSS 3 vs 4?" → Selected: TailwindCSS 4 — 2026 年最新版，性能更好，CSS-first 配置
</decisions>

<code_context>
## Existing Code

Greenfield project — no existing code. All code will be created from scratch.

**Reference patterns**:
- Tauri 2.x + React + Vite + TypeScript 标准脚手架（`create-tauri-app` 生成）
- 参考 `.planning/research/STACK.md` 获取完整技术栈版本号
- 参考 `.planning/research/ARCHITECTURE.md` §2 获取 Tauri 三层架构设计
- 参考 `.planning/research/PITFALLS.md` §4（Tauri 构建陷阱）和 §8（数据隐私陷阱）
</code_context>

<specifics>
## Specific Ideas

- 安装包体积目标 < 30MB（不含 bge-small-zh-v1.5 模型和 ChromaDB/业务依赖）
- Splash Screen 显示品牌 logo + "KingdeeKB" 文字 + 加载进度
- `~/.kingdee-kb/` 目录在首次启动的 Rust 端（非前端）创建
- API Key 配置通过设置页面输入，但存储在后端 Rust 代码调用 keyring API
</specifics>

<canonical_refs>
## Canonical References

- `.planning/PROJECT.md` — 项目上下文与核心定位
- `.planning/REQUIREMENTS.md` — v1 需求定义（INFR-01, INFR-03, INFR-07, INFR-08）
- `.planning/ROADMAP.md` — Phase 1 成功标准
- `.planning/research/STACK.md` — 完整技术栈推荐（含版本号）
- `.planning/research/ARCHITECTURE.md` — §2 Tauri 三层架构
- `.planning/research/PITFALLS.md` — §4 Tauri 构建陷阱、§8 数据隐私陷阱
- `.planning/research/SUMMARY.md` — 研究综合摘要
</canonical_refs>

<deferred>
## Deferred Ideas

None — first phase, all decisions fall within scope.
</deferred>
