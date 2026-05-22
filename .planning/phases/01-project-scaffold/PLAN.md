# Phase 1: 项目脚手架与基础设施 — 执行计划

**Created:** 2026-05-23
**Status:** Ready for execution
**Dependencies:** None (first phase)

---

## Overview

本阶段建立 KingdeeKB 桌面应用的完整技术骨架：Tauri 2.x + React 19 + Vite 6 + TailwindCSS 4 + Biome。完成后，应用能在 Windows 上成功启动，显示 Splash Screen，具备安全的 API Key 存储框架和本地数据目录结构。

**成功标准（来自 ROADMAP.md）：**
1. 应用在 Windows 上以 Tauri 2.x 桌面窗口成功启动，启动过程显示 Splash Screen
2. Splash Screen 平滑过渡到 React 前端主界面（消除 WebView2 冷启动白屏）
3. 首次启动自动创建 `~/.kingdee-kb/` 目录结构（knowledge/、index/、models/、bm25_index/、metadata.db）
4. API Key 通过 Windows Credential Manager 安全存储与读取，不落盘为明文 JSON
5. WebView2 fixedRuntime 捆绑打包，安装包体积 < 30MB（不含业务依赖）

---

## Task 1: Tauri 2.x 项目初始化

**Goal:** 使用 `create-tauri-app` 创建标准 Tauri 2.x + React + Vite + TypeScript 项目骨架。

**Steps:**
1. 在项目根目录运行 `npm create tauri-app@latest`，选择 React + TypeScript 模板
2. 确认生成的目录结构：`src-tauri/`（Rust 后端）、`src/`（React 前端）
3. 验证 `src-tauri/Cargo.toml` 中 `tauri` 依赖版本为 `^2.2`
4. 验证 `src-tauri/tauri.conf.json` 基础配置正确
5. 运行 `npm install` 安装前端依赖
6. 运行 `cd src-tauri && cargo check` 验证 Rust 后端编译通过

**Files to create/modify:**
- `package.json`（根目录）
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `src-tauri/src/main.rs`
- `src-tauri/src/lib.rs`
- `src/main.tsx`
- `src/App.tsx`
- `vite.config.ts`
- `tsconfig.json`

**Verification:**
- [ ] `npm run tauri dev` 能启动 Tauri 窗口（可能显示默认页面）
- [ ] `src-tauri/Cargo.toml` 中 tauri 版本 `^2.2`
- [ ] 目录结构符合标准 Tauri 布局

**Dependencies:** None

---

## Task 2: React 19 + Vite 6 + TailwindCSS 4 配置

**Goal:** 确保前端技术栈版本正确，TailwindCSS 4 配置就绪。

**Steps:**
1. 检查 `package.json` 中 React 版本，确保为 `^19.0`
2. 检查 Vite 版本，确保为 `^6.x`
3. 安装 TailwindCSS 4：`npm install tailwindcss @tailwindcss/vite`
4. 在 `vite.config.ts` 中添加 `@tailwindcss/vite` 插件
5. 在 `src/index.css`（或主 CSS 文件）顶部添加 `@import "tailwindcss";`
6. 验证 TailwindCSS 4 的 CSS-first 配置方式（无需 `tailwind.config.js`）
7. 确认 `tsconfig.json` 中 `strict: true` 已启用

**Files to modify:**
- `package.json`（添加 tailwindcss 依赖）
- `vite.config.ts`（添加 TailwindCSS 插件）
- `src/index.css`（添加 TailwindCSS import）
- `tsconfig.json`（确认 strict 模式）

**Verification:**
- [ ] `npm run dev` 启动 Vite 开发服务器无报错
- [ ] TailwindCSS 类名（如 `bg-blue-500`）在页面上生效
- [ ] TypeScript strict 模式已启用

**Dependencies:** Task 1

---

## Task 3: Biome 代码风格工具配置

**Goal:** 安装并配置 Biome 作为统一的格式化和 Lint 工具。

**Steps:**
1. 安装 Biome：`npm install -D @biomejs/biome`
2. 运行 `npx @biomejs/biome init` 生成 `biome.json`
3. 配置 `biome.json`：
   - `formatter.indentStyle`: "space"
   - `formatter.indentWidth`: 2
   - `linter.enabled`: true
   - `javascript.formatter.quoteStyle`: "double"
   - `javascript.formatter.semicolons`: "asNeeded"
4. 在 `package.json` 中添加脚本：
   - `"lint": "biome check src/"`
   - `"lint:fix": "biome check --fix src/"`
   - `"format": "biome format --write src/"`
5. 运行 `npx biome check src/` 验证配置

**Files to create/modify:**
- `biome.json`
- `package.json`（添加 lint/format 脚本）

**Verification:**
- [ ] `npm run lint` 无报错
- [ ] `npm run format` 能格式化代码
- [ ] Biome 配置与项目代码风格一致

**Dependencies:** Task 1

---

## Task 4: react-router-dom v7 路由配置

**Goal:** 配置 react-router-dom v7 作为 SPA 路由方案。

**Steps:**
1. 安装 react-router-dom：`npm install react-router-dom`
2. 在 `src/main.tsx` 中配置 `BrowserRouter`
3. 创建基础路由结构：
   - `/` → 首页（`src/pages/Home.tsx`）
   - `/settings` → 设置页（`src/pages/Settings.tsx`，后续阶段使用）
4. 创建 `src/pages/` 目录和基础页面组件
5. 在 `src/App.tsx` 中使用 `<Routes>` 和 `<Route>` 配置路由

**Files to create/modify:**
- `src/main.tsx`（添加 BrowserRouter）
- `src/App.tsx`（配置路由）
- `src/pages/Home.tsx`（首页组件）
- `src/pages/Settings.tsx`（设置页占位）

**Verification:**
- [ ] 应用启动后显示首页
- [ ] 路由切换正常工作（如手动导航到 `/settings`）
- [ ] TypeScript 无类型错误

**Dependencies:** Task 2

---

## Task 5: Splash Screen 实现

**Goal:** 实现 Tauri 原生 Splash Screen，消除 WebView2 冷启动白屏。

**Steps:**
1. 准备 Splash Screen 图片：`src-tauri/icons/splash.png`（品牌 logo + "KingdeeKB" 文字）
2. 在 `src-tauri/tauri.conf.json` 中配置 Splash Screen：
   ```json
   {
     "app": {
       "windows": [
         {
           "title": "KingdeeKB",
           "width": 800,
           "height": 600,
           "visible": false
         }
       ],
       "withGlobalTauri": false
     },
     "bundle": {
       "splashscreen": {
         "image": "icons/splash.png",
         "width": 400,
         "height": 300
       }
     }
   }
   ```
3. 在 `src-tauri/src/lib.rs` 中添加 Splash Screen 关闭逻辑：
   - 使用 `tauri::async_runtime::spawn` 异步等待前端就绪
   - 调用 `splashscreen.close()` 和 `window.show()`
4. 在前端 `src/main.tsx` 中，应用挂载完成后调用 `emit('splash-end')`
5. 配置主窗口 `visible: false`，Splash Screen 自动显示

**Files to create/modify:**
- `src-tauri/icons/splash.png`（Splash Screen 图片）
- `src-tauri/tauri.conf.json`（Splash Screen 配置）
- `src-tauri/src/lib.rs`（Splash Screen 关闭逻辑）
- `src/main.tsx`（发送 splash-end 事件）

**Verification:**
- [ ] 应用启动时显示 Splash Screen（品牌 logo）
- [ ] Splash Screen 平滑过渡到 React 主界面
- [ ] 无白屏闪烁现象

**Dependencies:** Task 1

---

## Task 6: 本地数据目录创建（~/.kingdee-kb/）

**Goal:** 首次启动时在 Rust 端自动创建 `~/.kingdee-kb/` 目录结构。

**Steps:**
1. 在 `src-tauri/src/lib.rs` 中添加目录创建逻辑：
   ```rust
   use std::fs;
   use std::path::PathBuf;
   
   fn ensure_data_dir() -> Result<PathBuf, String> {
       let home = dirs::home_dir().ok_or("Cannot find home directory")?;
       let data_dir = home.join(".kingdee-kb");
       
       let subdirs = ["knowledge", "index", "models", "bm25_index"];
       for subdir in subdirs {
           fs::create_dir_all(data_dir.join(subdir))
               .map_err(|e| format!("Failed to create {}: {}", subdir, e))?;
       }
       
       // 创建 metadata.db 空文件（后续阶段使用 SQLite）
       let db_path = data_dir.join("metadata.db");
       if !db_path.exists() {
           fs::File::create(&db_path)
               .map_err(|e| format!("Failed to create metadata.db: {}", e))?;
       }
       
       Ok(data_dir)
   }
   ```
2. 在 `main()` 或 `lib.rs` 的 `run()` 函数开头调用 `ensure_data_dir()`
3. 将 `dirs` crate 添加到 `src-tauri/Cargo.toml` 依赖
4. 添加 Tauri 命令 `get_data_dir` 供前端查询数据目录路径

**Files to modify:**
- `src-tauri/Cargo.toml`（添加 dirs 依赖）
- `src-tauri/src/lib.rs`（添加目录创建逻辑和 Tauri 命令）

**Verification:**
- [ ] 首次启动后 `~/.kingdee-kb/` 目录存在
- [ ] 子目录 knowledge/、index/、models/、bm25_index/ 已创建
- [ ] metadata.db 文件已创建
- [ ] 重复启动不会报错（幂等性）

**Dependencies:** Task 1

---

## Task 7: API Key 安全存储（Keyring 插件）

**Goal:** 配置 tauri-plugin-keyring-store，实现 API Key 通过 Windows Credential Manager 安全存储。

**Steps:**
1. 在 `src-tauri/Cargo.toml` 中添加依赖：
   ```toml
   [dependencies]
   tauri-plugin-keyring-store = "2"
   ```
2. 在 `src-tauri/src/lib.rs` 中注册插件：
   ```rust
   tauri::Builder::default()
       .plugin(tauri_plugin_keyring_store::init())
       // ...
   ```
3. 创建 Tauri 命令封装 keyring 操作：
   - `set_api_key(service: &str, key: &str) -> Result<(), String>`
   - `get_api_key(service: &str) -> Result<Option<String>, String>`
   - `delete_api_key(service: &str) -> Result<(), String>`
4. 在前端创建 `src/lib/keyring.ts` 封装 IPC 调用
5. 添加基础测试：存储 → 读取 → 删除 API Key

**Files to modify:**
- `src-tauri/Cargo.toml`（添加 keyring 插件依赖）
- `src-tauri/src/lib.rs`（注册插件、添加 Tauri 命令）
- `src/lib/keyring.ts`（前端封装）

**Verification:**
- [ ] `set_api_key("test", "sk-xxx")` 成功存储
- [ ] `get_api_key("test")` 返回 `"sk-xxx"`
- [ ] `delete_api_key("test")` 成功删除
- [ ] API Key 未以明文 JSON 形式存储在磁盘上
- [ ] Windows Credential Manager 中可见存储的凭据

**Dependencies:** Task 1

---

## Task 8: WebView2 fixedRuntime 捆绑配置

**Goal:** 配置 WebView2 fixedRuntime 捆绑，确保安装包包含 WebView2 运行时。

**Steps:**
1. 在 `src-tauri/tauri.conf.json` 中配置 WebView2 捆绑：
   ```json
   {
     "bundle": {
       "windows": {
         "webviewInstallMode": {
           "type": "fixedRuntime",
           "path": "./webview2-runtime"
         }
       }
     }
   }
   ```
2. 下载 WebView2 fixedRuntime 二进制文件（约 8MB）到 `src-tauri/webview2-runtime/`
3. 在 `.gitignore` 中添加 `src-tauri/webview2-runtime/`（二进制文件不入版本控制）
4. 配置构建脚本自动下载 WebView2 运行时

**Files to modify:**
- `src-tauri/tauri.conf.json`（WebView2 捆绑配置）
- `src-tauri/webview2-runtime/`（运行时二进制）
- `.gitignore`（添加 webview2-runtime 忽略）

**Verification:**
- [ ] `npm run tauri build` 成功生成安装包
- [ ] 安装包包含 WebView2 运行时
- [ ] 在无 WebView2 的 Windows 机器上能正常启动

**Dependencies:** Task 1

---

## Task 9: package.json 脚本配置

**Goal:** 配置完整的开发和构建脚本。

**Steps:**
1. 在 `package.json` 中配置脚本：
   ```json
   {
     "scripts": {
       "dev": "vite",
       "build": "tsc && vite build",
       "preview": "vite preview",
       "tauri": "tauri",
       "tauri:dev": "tauri dev",
       "tauri:build": "tauri build",
       "lint": "biome check src/",
       "lint:fix": "biome check --fix src/",
       "format": "biome format --write src/",
       "typecheck": "tsc --noEmit"
     }
   }
   ```
2. 验证所有脚本可执行

**Files to modify:**
- `package.json`

**Verification:**
- [ ] `npm run dev` 启动 Vite 开发服务器
- [ ] `npm run build` 成功构建前端
- [ ] `npm run tauri:dev` 启动 Tauri 开发模式
- [ ] `npm run lint` 运行 Biome 检查
- [ ] `npm run typecheck` 运行 TypeScript 类型检查

**Dependencies:** Task 1, Task 3

---

## Task 10: .gitignore 配置

**Goal:** 配置完整的 .gitignore，排除构建产物和敏感文件。

**Steps:**
1. 创建/更新 `.gitignore`：
   ```gitignore
   # Dependencies
   node_modules/
   
   # Build outputs
   dist/
   src-tauri/target/
   
   # WebView2 runtime (binary, not in version control)
   src-tauri/webview2-runtime/
   
   # IDE
   .vscode/
   .idea/
   *.swp
   *.swo
   
   # OS
   .DS_Store
   Thumbs.db
   
   # Environment
   .env
   .env.local
   .env.*.local
   
   # Logs
   *.log
   npm-debug.log*
   
   # TypeScript
   *.tsbuildinfo
   
   # Rust
   **/*.rs.bk
   
   # Local data (user-specific)
   .kingdee-kb/
   ```

**Files to create/modify:**
- `.gitignore`

**Verification:**
- [ ] `git status` 不显示应忽略的文件
- [ ] 敏感文件（.env、API Key）未被跟踪

**Dependencies:** None

---

## Task 11: 安装包体积验证

**Goal:** 验证安装包体积 < 30MB（不含业务依赖）。

**Steps:**
1. 运行 `npm run tauri build` 生成 Windows 安装包
2. 检查生成的 `.msi` 或 `.exe` 安装包大小
3. 确认体积 < 30MB（不含 bge-small-zh-v1.5 模型和业务依赖）
4. 如果超限，分析体积来源并优化：
   - 检查 WebView2 运行时大小
   - 检查 Rust 二进制大小（`cargo build --release`）
   - 检查前端打包大小

**Files:** 无新增（验证任务）

**Verification:**
- [ ] 安装包体积 < 30MB
- [ ] 安装包包含所有必要组件
- [ ] 安装后应用能正常启动

**Dependencies:** Task 8

---

## Task 12: 集成验证与文档

**Goal:** 端到端验证所有功能，更新项目文档。

**Steps:**
1. 完整测试启动流程：
   - Splash Screen 显示 → 平滑过渡 → React 主界面
   - 首次启动创建 `~/.kingdee-kb/` 目录
   - API Key 存储/读取/删除
2. 运行所有 lint 和类型检查：
   - `npm run lint`
   - `npm run typecheck`
3. 更新 `.planning/phases/01-project-scaffold/` 文档：
   - 标记完成状态
   - 记录实际版本号和配置
4. 创建 `src-tauri/icons/splash.png`（品牌 logo）

**Files to create/modify:**
- `src-tauri/icons/splash.png`（品牌图片）
- `.planning/phases/01-project-scaffold/` 文档更新

**Verification:**
- [ ] 所有成功标准逐一验证通过
- [ ] 代码通过 lint 和类型检查
- [ ] 文档已更新

**Dependencies:** Task 1-11

---

## Execution Order

```
Task 1 (Tauri 初始化)
  ├── Task 2 (React + Vite + TailwindCSS)
  │   └── Task 4 (react-router-dom)
  ├── Task 3 (Biome)
  ├── Task 5 (Splash Screen)
  ├── Task 6 (数据目录)
  ├── Task 7 (Keyring)
  └── Task 8 (WebView2 捆绑)
       └── Task 11 (体积验证)

Task 10 (.gitignore) — 可并行
Task 9 (package.json 脚本) — 依赖 Task 1, Task 3
Task 12 (集成验证) — 依赖所有任务
```

**并行执行机会：**
- Task 2、Task 3、Task 5、Task 6、Task 7、Task 8 可在 Task 1 完成后并行执行
- Task 10 可在任何时候执行

---

## Risk Mitigation

| 风险 | 缓解措施 |
|------|----------|
| WebView2 fixedRuntime 体积超限 | 使用 `fixedRuntime` 而非 `bootstrapper`，固定版本约 8MB |
| Splash Screen 白屏闪烁 | 严格遵循 PITFALLS.md §4.1：主窗口 `visible: false`，Splash Screen 原生渲染 |
| Keyring Windows 中文环境兼容 | 测试 Windows 中文 locale 下 Credential Manager 行为（PITFALLS.md §8） |
| TailwindCSS 4 配置变更 | 使用 CSS-first 配置（`@import "tailwindcss"`），无需 `tailwind.config.js` |
| React 19 新特性兼容 | 使用标准客户端渲染，不引入 Server Components |
