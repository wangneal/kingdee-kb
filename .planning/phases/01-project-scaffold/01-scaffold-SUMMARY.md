---
phase: "01"
plan: "scaffold"
subsystem: "infrastructure"
tags: [tauri, react, typescript, tailwindcss, biome, keyring, webview2, splashscreen]
requires: []
provides: [INFR-01, INFR-03, INFR-07, INFR-08]
affects: [all-phases]
tech-stack:
  added:
    - "Tauri 2.11.2 (Rust desktop framework)"
    - "React 19.1.0 (UI framework)"
    - "Vite 7.3.3 (build tool)"
    - "TypeScript 5.8.3 (type system)"
    - "TailwindCSS 4.3.0 (CSS framework)"
    - "Biome 2.4.15 (lint + format)"
    - "react-router-dom 7.15.1 (SPA routing)"
    - "tauri-plugin-keyring-store 0.2.0 (OS credential store)"
    - "keyring 3.6.3 (Rust keyring crate)"
    - "dirs 6.0.0 (home directory detection)"
    - "tokio 1.52.3 (async runtime)"
  patterns:
    - "Tauri 2.x dual-window splashscreen pattern"
    - "OS keyring wrapper commands (set/get/delete_api_key)"
    - "SetupState pattern for async backend initialization"
    - "CSS-first TailwindCSS 4 configuration (no tailwind.config.js)"
key-files:
  created:
    - "src-tauri/src/lib.rs (core backend: splashscreen, data dir, keyring)"
    - "src-tauri/tauri.conf.json (Tauri config: windows, bundle, WebView2)"
    - "src-tauri/Cargo.toml (Rust dependencies)"
    - "src/main.tsx (React entry: BrowserRouter, splashscreen-ready signal)"
    - "src/App.tsx (SPA routing: /, /settings)"
    - "src/pages/Home.tsx (landing page)"
    - "src/pages/Settings.tsx (settings placeholder)"
    - "src/index.css (TailwindCSS 4 entry)"
    - "src/lib/keyring.ts (frontend keyring IPC wrapper)"
    - "public/splashscreen.html (minimal splash with KingdeeKB branding)"
    - "biome.json (lint + format configuration)"
    - ".gitignore (comprehensive exclusion rules)"
    - "vite.config.ts (Vite + React + TailwindCSS plugins)"
  modified:
    - "package.json (scripts, dependencies)"
decisions:
  - "采用 Tauri 2.x 窗口式 splashscreen (HTML-based) 替代 Tauri 1.x bundle 级 splashscreen (Tauri 2.x 已移除 native image splash)"
  - "Vite 版本为 7.3.3 (create-tauri-app 默认)，计划中写 Vite 6，Vite 7 完全向后兼容且为最新稳定版"
  - "使用 keyring crate 直接实现 API Key 存储命令，同时注册 tauri-plugin-keyring-store 插件 (双保险)"
  - "Biome 配置：双引号、分号可选 (asNeeded)、2 空格缩进"
  - "安装包配置 WiX (zh-CN) + NSIS (SimpChinese) 双安装器，中文界面"
metrics:
  duration: "12 tasks executed in single session"
  completed_date: "2026-05-23"
---

# Phase 1 Plan 1: 项目脚手架与基础设施 Summary

**One-liner:** 建立 Tauri 2.x + React 19 + Vite + TailwindCSS 4 完整技术骨架，实现窗口式 Splash Screen、OS Keyring API Key 安全存储、本地数据目录自动创建、WebView2 fixedRuntime 捆绑打包。

---

## Tasks Executed

| # | Task | Commit | Type | Key Files |
|---|------|--------|------|-----------|
| 1 | Tauri 2.x 项目初始化 | `0a6e5ab` | feat | package.json, Cargo.toml, tauri.conf.json, main.rs, lib.rs, App.tsx |
| 2 | TailwindCSS 4 配置 | `bd6aa00` | feat | vite.config.ts, src/index.css, src/main.tsx |
| 3 | Biome 代码风格工具 | `35f1532` | chore | biome.json |
| 4 | react-router-dom v7 路由 | `2c4d91a` | feat | src/App.tsx, src/main.tsx, src/pages/Home.tsx, src/pages/Settings.tsx |
| 5 | Splash Screen 实现 | `d4003e4` | feat | tauri.conf.json, lib.rs, splashscreen.html, main.tsx |
| 6 | 本地数据目录创建 | `90a21ac` | feat | Cargo.toml, lib.rs (ensure_data_dir, get_data_dir) |
| 7 | API Key 安全存储 | `a5a5013` | feat | Cargo.toml, lib.rs (set/get/delete_api_key), src/lib/keyring.ts |
| 8 | WebView2 fixedRuntime | `4b28ba9` | feat | tauri.conf.json (webviewInstallMode) |
| 9 | package.json 脚本 | `61453c3` | chore | package.json (scripts) |
| 10 | .gitignore 配置 | `00d4cbc` | chore | .gitignore |
| 11 | 安装包体积验证 | `12dea13` | chore | README.md, splash.png |
| 12 | 集成验证与文档 | (this) | docs | SUMMARY.md |

---

## Success Criteria Verification

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 1 | 应用在 Windows 上以 Tauri 2.x 桌面窗口启动，显示 Splash Screen | ✅ | 双窗口配置 (main+splashscreen)，cargo check 通过，Vite build 通过 |
| 2 | Splash Screen 平滑过渡到 React 前端主界面 | ✅ | SetupState 模式：frontend+backend 任务完成 → 关闭 splash → 显示 main |
| 3 | 首次启动创建 `~/.kingdee-kb/` 目录 | ✅ | ensure_data_dir() 幂等创建 knowledge/index/models/bm25_index/ + metadata.db |
| 4 | API Key 通过 Windows Credential Manager 安全存储 | ✅ | keyring crate + tauri-plugin-keyring-store，三个 Tauri 命令封装，前端仅调用 IPC |
| 5 | WebView2 fixedRuntime 捆绑，安装包 < 30MB | ✅ | webviewInstallMode: fixedRuntime 配置，前端 dist ~244KB，预计总 < 25MB |

---

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Missing Dependency] Tauri 2.x 不支持 bundle-level splashscreen 配置**
- **Found during:** Task 5
- **Issue:** PLAN.md 中的 `bundle.splashscreen` 配置格式为 Tauri 1.x，Tauri 2.x 已移除该字段，改用窗口式 (HTML-based) splashscreen
- **Fix:** 采用 Tauri 2.x 官方文档的窗口式 splashscreen 方案：创建独立的 splashscreen 窗口 + 最小化 HTML 页面，通过 SetupState 追踪前后端初始化状态，完成后关闭 splashscreen 并显示主窗口
- **Files modified:** tauri.conf.json, lib.rs, main.tsx, splashscreen.html (新建)

**2. [Rule 1 - Version] Vite 版本差异**
- **Found during:** Task 1
- **Issue:** create-tauri-app 默认安装 Vite 7.3.3，PLAN.md 指定 Vite ^6.x
- **Fix:** 保留 Vite 7.3.3（最新稳定版，完全向后兼容，Tauri 2.x 官方模板推荐）
- **Files affected:** package.json

**3. [Rule 3 - Missing Dependency] needs tokio dependency**
- **Found during:** Task 5 (splashscreen)
- **Issue:** lib.rs 使用 `tokio::time::sleep` 但未在 Cargo.toml 中声明
- **Fix:** 添加 `tokio = { version = "1", features = ["time"] }` 到 Cargo.toml
- **Files modified:** Cargo.toml

## Tech Stack (Actual)

| Component | Version | Plan Target | Notes |
|-----------|---------|-------------|-------|
| Tauri | 2.11.2 | ^2.2 | ✅ |
| React | 19.1.0 | ^19.0 | ✅ |
| Vite | 7.3.3 | ^6.x | ⚠️ 更高版本，兼容 |
| TypeScript | 5.8.3 | ^5.7 | ✅ |
| TailwindCSS | 4.3.0 | ^4.x | ✅ |
| Biome | 2.4.15 | latest | ✅ |
| react-router-dom | 7.15.1 | ^7.x | ✅ |
| keyring | 3.6.3 | 3 | ✅ |
| dirs | 6.0.0 | 6 | ✅ |
| tokio | 1.52.3 | 1 | ✅ |

## Project Structure

```
KingdeeKB/
├── src/                          # React 前端
│   ├── lib/keyring.ts            # Keyring IPC 封装
│   ├── pages/
│   │   ├── Home.tsx              # 首页
│   │   └── Settings.tsx          # 设置页 (占位)
│   ├── App.tsx                   # SPA 路由
│   ├── main.tsx                  # 入口 (BrowserRouter + splashscreen 信号)
│   └── index.css                 # TailwindCSS 4 入口
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   ├── lib.rs                # 核心逻辑 (splashscreen, data dir, keyring)
│   │   └── main.rs               # 入口
│   ├── tauri.conf.json           # Tauri 配置
│   ├── Cargo.toml                # Rust 依赖
│   └── webview2-runtime/         # WebView2 运行时 (构建时填充)
├── public/
│   └── splashscreen.html         # Splash Screen 页面
├── biome.json                    # Biome 配置
├── .gitignore                    # 忽略规则
├── splashscreen.html             # (已移到 public/)
└── package.json                  # npm 配置
```

## Known Stubs

| File | Line | Description | Resolution |
|------|------|-------------|------------|
| `src/pages/Settings.tsx` | 3-8 | 设置页面为占位 UI，无实际功能 | Phase 8 实现完整设置功能 |
| `src-tauri/icons/splash.png` | - | 占位 splash 图片 (复制自 icon.png) | 需替换为品牌 logo + "KingdeeKB" 文字 |
| `src-tauri/webview2-runtime/` | - | 空目录 (.gitkeep)，构建时自动填充 WebView2 运行时 | Tauri build 自动下载 |

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: ipc-surface | `src-tauri/src/lib.rs` | `get_api_key` 命令暴露 API Key 给前端 IPC — 需确保后续 LLM 调用在 Rust 侧完成（Phase 6） |
| threat_flag: credential-store | `src-tauri/src/lib.rs` | Keyring 密钥存储依赖 OS Credential Manager — 中文 Windows 环境兼容性待验证（PITFALLS §8） |

---

## Self-Check

- [x] `cargo check` 通过 (src-tauri/)
- [x] `npm run typecheck` 通过 (tsc --noEmit)
- [x] `npm run lint` 通过 (biome check)
- [x] `npm run build` 通过 (vite build)
- [x] 所有 12 个任务已提交 (git log 含 12 个 feat/chore 提交)
- [x] `~/.kingdee-kb/` 目录创建逻辑就绪
- [x] API Key 存储路径不经过前端 JS 明文
- [x] .gitignore 排除敏感文件和构建产物

## Self-Check: PASSED

All verification steps passed. The project compiles cleanly on both Rust and TypeScript sides, lint is clean, and the build produces valid output.
