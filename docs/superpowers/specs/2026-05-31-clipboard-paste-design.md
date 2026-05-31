# AI 对话剪贴板文件粘贴 + 拖拽增强

日期：2026-05-31
状态：待审查

## 背景

KingdeeKB 是 Tauri v2 + React + TypeScript 桌面应用，AI 对话模块（Chat.tsx）已有完整附件系统：通过回形针按钮调用系统文件选择器添加文档/图片附件，经 `extractFileText` / `processImage` 解析后注入对话上下文。

当前唯一附件入口是文件选择对话框，不支持：
- 截图后 Ctrl+V 粘贴为图片附件
- 从文件管理器复制文件后 Ctrl+V 粘贴为文档附件
- 从文件管理器拖拽文件到输入区域

用户需求：截图粘贴 + 复制文件粘贴，两者都要。

## 方案选择

经对比三个方案（纯前端 Web API、Tauri 剪贴板插件、Web API + 拖拽增强），选定 **方案 C：纯前端 Web API + 拖拽增强**。

理由：
1. 截图粘贴用浏览器原生 `onPaste` 即可解决，零新 Rust 依赖
2. 复制文件粘贴在 Windows WebView2 中 `clipboardData.files` 可获取 File 对象
3. 拖拽是自然延伸，Import.tsx 有参考模式
4. 只需新增 `@tauri-apps/plugin-fs`（写临时文件），这是 Tauri 官方插件
5. 完全复用现有附件管线，无需改 Rust 后端

## 架构

### 核心原则

不改动现有附件状态机和后端处理管线，仅在输入层增加两个新入口（粘贴、拖拽），将新入口产生的文件转换为现有 `ChatAttachment` 对象，进入已有流程。

### 数据流

```
粘贴/拖拽事件 → 新增 handlePaste / handleDrop 处理器
  → 获取 File 对象或文件路径
  → 图片 Blob: 写入临时文件 (plugin-fs) → createAttachment(tempPath)
  → 本地文件路径: 直接 createAttachment(path)
  → 附件进入现有 attachments state
  → 发送时走 prepareAttachmentsForSend() 管线
```

### 不改动的部分

- `ChatAttachment` 接口（L41-51）
- `createAttachment()` 函数（L600-619）
- `prepareAttachmentsForSend()` 函数（L621-725）
- `buildAttachmentPrompt()` / `buildAttachmentDisplay()` 函数
- `AttachmentChip` 组件
- Rust 后端命令（`extract_file_text`、`ingest_file`、`process_image`）

## 功能规格

### 第一层：截图/图片粘贴

**触发**：textarea 上 Ctrl+V，`clipboardData.files` 包含 `image/*` 类型文件。

**处理流程**：
1. `textarea.onPaste` 回调拦截 `ClipboardEvent`
2. 从 `e.clipboardData.files` 过滤出 `type.startsWith('image/')` 的 File 对象
3. 用 `File.arrayBuffer()` 读取图片二进制数据
4. 用 `@tauri-apps/plugin-fs` 写入临时目录：
   - 先调用 `mkdir` 确保临时子目录存在：`BaseDirectory.Temp` 下的 `kingdee-kb`
   - 再调用 `writeFile()` 写入文件
   - 文件名：`paste-{timestamp}-{random}.{ext}`（ext 由 MIME 类型推断：png/jpg/webp/gif）
5. 构造临时文件路径字符串，调用现有 `createAttachment(tempFilePath)`
6. 将新附件追加到 `attachments` state

**MIME → 扩展名映射**：
- `image/png` → `.png`
- `image/jpeg` → `.jpg`
- `image/webp` → `.webp`
- `image/gif` → `.gif`
- `image/bmp` → `.bmp`
- 其他 → `.png`（默认）

### 第二层：复制文件粘贴

**触发**：textarea 上 Ctrl+V，`clipboardData.files` 包含非图片文件。

**处理流程**：
1. 从 `e.clipboardData.files` 过滤出非 image 类型的 File 对象
2. 检查 File 对象是否有 `path` 属性（注意：`path` 是 Tauri WebView2 特有属性，标准浏览器 File API 不包含此属性；在 WebView2 中，从文件管理器拖入或粘贴的本地文件 File 对象会附加 `path` 字段指向本地绝对路径）
3. **有 path**：直接用 `file.path` 调用 `createAttachment(path)`，无需写临时文件
4. **无 path**：读取 `file.arrayBuffer()` → 写入临时目录 → `createAttachment(tempPath)`
5. 将新附件追加到 `attachments` state

**扩展名判断**：从 `file.name` 取扩展名，若 `file.name` 为空则从 MIME 推断。

### 第三层：拖拽文件

**触发**：用户将文件从资源管理器拖入聊天输入区域。

**处理流程**：
1. 整个底部输入区域 div（包含附件按钮、模型选择器、textarea）添加 `onDragOver`：`e.preventDefault()` + 设置 `isDragging` state 为 true
2. 添加 `onDragLeave`：设置 `isDragging` state 为 false
3. 添加 `onDrop`：
   - 从 `e.dataTransfer.files` 获取 File 对象
   - 同复制文件粘贴逻辑：优先用 `file.path`，无 path 则写临时文件
   - 调用 `createAttachment(path)` → 追加到 `attachments`
   - 重置 `isDragging` state

**UI 反馈**：
- `isDragging === true` 时：
  - 输入区域边框变为蓝色虚线（`border-blue-400 border-dashed`）
  - 显示提示文字："松开以添加附件"
  - 背景 `bg-blue-50/50`
- `isDragging === false`：恢复原样式

### 交互细节

1. **粘贴无文件时**：`clipboardData.files` 为空或仅含文本 → 不拦截，让浏览器正常粘贴文本到 textarea
2. **粘贴含文件时**：文件被提取为附件，**不将文件名或路径文本粘贴到输入框**，仅添加为附件
3. **unsupported 类型**：走现有 `createAttachment` 逻辑，`kind = "unsupported"`，`status = "error"`，显示错误提示
4. **临时文件清理**：无需主动清理，操作系统会清理临时目录；附件发送后状态变为 `parsed/ingested`，path 仅用于处理
5. **并发粘贴**：连续多次 Ctrl+V 粘贴，每次追加到现有 `attachments` 数组

## 依赖变更

| 变更项 | 说明 |
|--------|------|
| 新增 npm 依赖 | `@tauri-apps/plugin-fs` |
| Cargo.toml | `tauri-plugin-fs = "2"` |
| capabilities/default.json | 添加 `fs:allow-write-file`、`fs:allow-write-file-with-options`（限制 Temp 目录） |
| Rust 代码变更 | 无（`tauri-plugin-fs` 插件自带 Tauri 命令） |

## 文件变更清单

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `src/pages/Chat.tsx` | 修改 | 添加 `handlePaste`、`handleDrop`、`isDragging` state，textarea 事件绑定和拖拽样式 |
| `src-tauri/Cargo.toml` | 修改 | 添加 `tauri-plugin-fs` 依赖 |
| `src-tauri/capabilities/default.json` | 修改 | 添加 fs 写权限 |
| `src/lib/tauri-commands.ts` | 不改 | — |
| `src/contexts/AgentContext.tsx` | 不改 | — |
| Rust 后端所有 .rs 文件 | 不改 | — |

## 验收标准

1. 截图后 Ctrl+V → 图片出现在附件区域，状态从 ready → ingesting → parsed
2. 文件管理器复制 docx 文件后 Ctrl+V → 文件出现在附件区域，正常解析
3. 文件管理器复制 png 文件后 Ctrl+V → 图片出现在附件区域，OCR 正常
4. 拖拽文件到输入区域 → 出现蓝色虚线高亮反馈 → 松开后文件变为附件
5. 粘贴纯文本（无文件） → 文本正常粘贴到输入框，不创建附件
6. 粘贴不支持的文件类型 → 附件显示 unsupported 状态和错误提示
7. 已有回形针按钮选文件功能不受影响
8. 不支持的格式（如 .exe、.zip） → createAttachment 正确标记为 unsupported

## 错误处理

1. **临时文件写入失败**：catch 错误，创建 `ChatAttachment` 对象 `status = "error"`，`error` 字段显示 "粘贴文件保存失败：{错误信息}"
2. **clipboardData.files 为 null**：某些旧版 WebView2 可能不支持，安全返回不做操作
3. **file.arrayBuffer() 失败**：catch 错误，同上创建 error 附件
4. **file.path 为空字符串**：视为无路径，走写入临时文件降级路径

## 范围排除

- 不实现剪贴板文本内容的智能识别（如粘贴 URL 自动下载）
- 不实现拖拽目录（仅支持文件）
- 不修改 Rust 后端的任何文件处理逻辑
- 不添加 `@tauri-apps/plugin-clipboard-manager`（纯 Web API 处理剪贴板）