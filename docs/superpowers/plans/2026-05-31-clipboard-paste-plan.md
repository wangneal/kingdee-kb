# 实现记录：AI 对话剪贴板文件粘贴 + 拖拽增强

日期：2026-05-31
关联规格：docs/superpowers/specs/2026-05-31-clipboard-paste-design.md
状态：**已完成**

## 概览

在 Chat.tsx 输入区域添加 onPaste 和 onDrop 事件处理，复用现有附件管线。仅前端变更 + Tauri plugin-fs 配置，不改 Rust 业务代码。

## 变更文件

| 文件 | 变更 | 说明 |
|------|------|------|
| `src/lib/clipboard-files.ts` | 新建 | 粘贴/拖拽文件提取，92 行 |
| `src/pages/Chat.tsx` | 修改 | +80 行：handlePaste、handleDrop、拖拽 UI |
| `src-tauri/Cargo.toml` | 修改 | 添加 `tauri-plugin-fs = "2"` |
| `src-tauri/Cargo.lock` | 修改 | Cargo 自动更新 |
| `src-tauri/src/lib.rs` | 修改 | 注册 `.plugin(tauri_plugin_fs::init())` |
| `src-tauri/capabilities/default.json` | 修改 | 添加 fs 权限：mkdir、write-file、exists、write-text-file |
| `package.json` | 修改 | 添加 `@tauri-apps/plugin-fs` |
| `package-lock.json` | 修改 | npm 自动更新 |

## 实现要点

### clipboard-files.ts

- `extractFilesFromPasteEvent(e)` / `extractFilesFromDropEvent(e)` 两个导出函数，共享 `extractFileList` + `processFile` 逻辑
- Tauri WebView2 的 File 对象带 `.path` 属性 → 直接用，不走临时文件
- 无 `.path`（截图 blob）→ 通过 plugin-fs 写 `$TEMP/kingdee-kb/paste-{ts}-{rand}.{ext}`
- 扩展名：image/* 查 MIME_TO_EXT 表，其他从 file.name 取

### Chat.tsx

- `addFilesAsAttachments(files, errorPrefix)` — 共享函数，粘贴和拖拽共用，避免重复
- `handlePaste` — textarea onPaste，无文件时 return 让浏览器正常粘贴文本
- `handleDrop` — 输入区域 div onDrop，isDragging 状态管理拖拽高亮
- 拖拽 UI：蓝色虚线边框 + 半透明 overlay "松开以添加附件"

### 未改动

- `ChatAttachment` 接口、`createAttachment()`、`prepareAttachmentsForSend()` — 全部复用
- Rust 后端所有 `.rs` 业务逻辑

## 待手动验证

| # | 场景 | 预期 |
|---|------|------|
| 1 | 截图后 Ctrl+V | 图片出现在附件区域 |
| 2 | 纯文本 Ctrl+V | 正常粘贴文本，不创建附件 |
| 3 | 复制 .docx 后 Ctrl+V | 文件出现在附件区域 |
| 4 | 复制 .png 后 Ctrl+V | 图片出现在附件区域 |
| 5 | 拖拽文件到输入区域 | 蓝色高亮 → 松开后变为附件 |
| 6 | 拖出区域 | 高亮消失，无附件 |
| 7 | 回形针按钮 | 功能不受影响 |
| 8 | 不支持的文件类型 | 显示 error 状态 |
| 9 | 连续多次 Ctrl+V | 每次追加附件 |

## 构建

`npm run build` + `cargo check` 均通过，无新增错误。
