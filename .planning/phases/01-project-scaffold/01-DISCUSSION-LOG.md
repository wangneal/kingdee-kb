# Phase 1 Discussion Log

**Phase:** 01 — 项目脚手架与基础设施
**Date:** 2026-05-23
**Mode:** --auto (all decisions auto-selected)
**Areas discussed:** 11

## Decisions Log

| Area | Question | Selected | Rationale |
|------|----------|----------|-----------|
| Tauri 初始化 | create-tauri-app vs cargo tauri init | create-tauri-app (npm) | 官方推荐，一键生成 React+Vite+TS 脚手架 |
| 包管理器 | npm vs pnpm vs yarn | npm | Node.js 自带，Tauri 模板默认 |
| 构建工具 | Vite vs Webpack | Vite 6 | Tauri 2.x 官方推荐，极快 HMR |
| React 版本 | 18 vs 19 | React 19 | 2026 最新稳定版 |
| 路由 | react-router-dom vs TanStack Router | react-router-dom v7 | 最广泛使用，简单可靠 |
| 代码风格 | Biome vs ESLint+Prettier | Biome | 单一工具，快 10-50x |
| Splash Screen | Tauri 原生 vs React 组件 | Tauri 原生 splashscreen | 消除 WebView2 冷启动白屏（PITFALLS §4） |
| Keyring | tauri-plugin-keyring-store vs keyring-rs | tauri-plugin-keyring-store | Tauri 官方插件，跨平台统一 API |
| 目录结构 | 标准 Tauri vs 自定义 | 标准 Tauri 结构 | 遵循社区约定 |
| TypeScript | strict vs relaxed | strict | 从第一天减少类型安全问题 |
| TailwindCSS | v3 vs v4 | TailwindCSS 4 | 最新版，性能更好 |

## Deferred Ideas

None.
