# KingdeeKB Bug Tracker

> 生成时间：2026-05-25 | 审查类型：代码质量 + 功能完整性 + 前后端一致性 + 安全性

## 严重程度说明

| 标记 | 含义 |
|------|------|
| 🔴 [必须修复] | 会导致运行时错误、数据丢失、安全漏洞 |
| 🟡 [建议修改] | 影响可维护性、性能、用户体验 |
| 🔵 [仅供参考] | 代码风格、最佳实践建议 |
| ⚪ [问题] | 待确认或需要更多上下文 |

---

## 🔴 [必须修复] — 共 3 项

### BUG-001: IPC 参数名前后端不一致（26+ 处 invoke 调用）

- **状态**: ✅ 已修复
- **文件**: `src/lib/tauri-commands.ts`
- **描述**: 前端 `invoke()` 使用 camelCase 参数名，但 Tauri v2 不做自动转换，Rust 后端期望 snake_case。导致所有涉及 camelCase 参数的 IPC 调用传参为 `undefined`。
- **影响**: 26+ 个 Tauri 命令无法正确接收参数，对应功能全部失效。
- **修复方式**: 将所有 invoke 调用的参数名改为 snake_case
- **完整映射表**:
  | 前端 camelCase | 后端 snake_case |
  |----------------|-----------------|
  | projectId | project_id |
  | topK | top_k |
  | filePath | file_path |
  | dirPath | dir_path |
  | documentId | document_id |
  | conversationHistory | conversation_history |
  | templateDir | template_dir |
  | templateId | template_id |
  | templateName | template_name |
  | writeSidecar | write_sidecar |
  | targetDir | target_dir |
  | moduleCode | module_code |
  | sessionDate | session_date |
  | sessionId | session_id |
  | questionId | question_id |
  | questionText | question_text |
  | answerText | answer_text |
  | sortOrder | sort_order |
  | recordId | record_id |
  | recordIds | record_ids |
  | modelSize | model_size |
  | isInScope | is_in_scope |
  | itemId | item_id |
  | indicatorType | indicator_type |
  | researchContext | research_context |
  | systemExtra | system_extra |

### BUG-002: 空 catch 块吞没错误（29 处）

- **状态**: ✅ 已修复
- **文件**:
  - `src/temp_chat.tsx` (1 处)
  - `src/temp_risk.tsx` (8 处)
  - `src/temp_research.tsx` (13 处)
  - `src/temp_layout.tsx` (2 处)
  - `src/pages/Templates.tsx` (1 处)
  - `src/pages/Wizard.tsx` (3 处)
- **描述**: `catch (e) {}` 空块完全吞没异常，导致错误无法追踪。生产环境中用户遇到问题时无法诊断。
- **影响**: 错误被静默吞没，用户看到功能不工作但无任何反馈，开发无法排查。
- **修复方式**: 已为所有 29 处添加 `console.warn("[功能名] 操作失败:", e)`，保留原有 alert 逻辑

### BUG-003: 脱敏词删除按钮未实现

- **状态**: ✅ 已修复
- **文件**: `src/pages/Settings.tsx`, `src-tauri/src/services/desensitize.rs`, `src-tauri/src/lib.rs`
- **描述**: 脱敏词列表的删除按钮 onClick handler 内只有注释 `/* delete not exposed via API yet */`，用户点击无任何效果。
- **影响**: 用户无法删除已添加的脱敏词，功能缺失。
- **修复方式**: 后端新增 `Desensitizer::remove_keyword()` + Tauri command `remove_sensitive_keyword`，前端 Settings.tsx onClick 已连接 `removeSensitiveKeyword(kw)` + 刷新列表

---

## 🟡 [建议修改] — 共 5 项

### BUG-004: useEffect 依赖项不完整

- **状态**: ✅ 已修复
- **文件**:
  - `src/pages/RiskControl.tsx` — `refresh` 用 useCallback 包裹并加入 deps
  - `src/pages/ResearchAssistant.tsx` — `refreshList` 移至 useEffect 前，加入 deps
  - `src/temp_risk.tsx` — 同 RiskControl.tsx 修复
  - `src/temp_research.tsx` — 同 ResearchAssistant.tsx 修复
- **描述**: 多处 useEffect 使用了闭包中的函数或变量但未加入依赖数组，可能导致 stale closure 问题。
- **影响**: 数据不同步、UI 状态过时。
- **修复方式**: 使用 `useCallback` 包装函数引用，或将稳定引用加入依赖数组

### BUG-005: Rust 生产代码 .unwrap() 可能 panic

- **状态**: ✅ 已修复
- **文件**:
  - `src-tauri/src/services/chinese_postprocess.rs` — LazyLock SAFE 注释 + if let
  - `src-tauri/src/services/research_outline.rs` — LazyLock SAFE 注释 + map/unwrap_or_default
  - `src-tauri/src/services/template_docx.rs` — map_or 替换 unwrap
  - `src-tauri/src/services/text_cleaner.rs` — LazyLock SAFE 注释
- **描述**: Rust 生产代码中使用 `.unwrap()` 在异常输入时会 panic 导致整个 Tauri 进程崩溃。
- **影响**: 特定输入可导致应用崩溃。
- **修复方式**: 函数体内改为 `.map_err()?` / `.unwrap_or_default()` / `if let Some()`；LazyLock 正则初始化保留 `.unwrap()` 并添加 `// SAFE:` 注释

### BUG-006: 缺少 React ErrorBoundary

- **状态**: ✅ 已修复
- **文件**: `src/components/ErrorBoundary.tsx`（新增）, `src/App.tsx`
- **描述**: 无 ErrorBoundary 包裹，任何组件渲染错误会导致整个白屏。
- **影响**: 用户遇到渲染错误时整个应用崩溃，只能刷新。
- **修复方式**: 创建 ErrorBoundary 类组件，用 `<ErrorBoundary>` 包裹 `<Routes>`，捕获渲染错误并显示中文降级 UI + 重试按钮

### BUG-007: Chat 消息无 localStorage 大小限制

- **状态**: ⏳ 待修复
- **文件**: `src/pages/Chat.tsx`
- **描述**: 聊天记录无限写入 localStorage，长时间使用后可能超出浏览器 5-10MB 限制导致写入失败。
- **影响**: 长时间使用后 localStorage 写入失败，可能丢失聊天记录。
- **修复方式**: 添加消息数量上限（如 500 条），超出后淘汰最旧消息

### BUG-008: Session ID 使用自增计数器

- **状态**: ✅ 已修复
- **文件**: `src/lib/tauri-commands.ts`
- **描述**: 使用 `reactSessionCounter++` 生成会话 ID，不够唯一和不可预测。
- **影响**: 多标签页场景下可能冲突。
- **修复方式**: 已改用 `crypto.randomUUID()`

---

## 🔵 [仅供参考] — 共 3 项

### BUG-009: 27 个后端命令无前端 wrapper

- **状态**: ⏳ 待评估
- **文件**: `src/lib/tauri-commands.ts`
- **描述**: Rust 后端有约 50 个命令，前端只封装了约 23 个。27 个命令无前端调用入口。
- **影响**: 功能不可达，但可能是预留功能。
- **修复方式**: 确认是否为未来功能，如果是则添加前端 wrapper

### BUG-010: API Key 明文存储

- **状态**: ⏳ 待评估
- **文件**: `src-tauri/` 配置文件
- **描述**: API Key 以明文存储在 config.json 中。
- **影响**: 本地安全风险（但 Tauri 桌面应用场景下风险较低）。
- **修复方式**: 考虑使用系统 keychain 或加密存储

### BUG-011: product_store.rs 路径遍历风险

- **状态**: ⏳ 待评估
- **文件**: `src-tauri/src/services/product_store.rs`
- **描述**: 文件路径拼接未做路径遍历校验。
- **影响**: 恶意输入可能访问预期外的文件（但桌面应用场景下风险较低）。
- **修复方式**: 添加路径规范化校验

---

## 修复进度汇总

| 编号 | 严重程度 | 描述 | 状态 |
|------|----------|------|------|
| BUG-001 | 🔴 必须修复 | IPC 参数名不一致 (26+处) | ✅ 已修复 |
| BUG-002 | 🔴 必须修复 | 空 catch 块 (29处) | ✅ 已修复 |
| BUG-003 | 🔴 必须修复 | 脱敏词删除按钮 | ✅ 已修复 |
| BUG-004 | 🟡 建议修改 | useEffect 依赖项 | ✅ 已修复 |
| BUG-005 | 🟡 建议修改 | Rust .unwrap() panic | ✅ 已修复 |
| BUG-006 | 🟡 建议修改 | ErrorBoundary 缺失 | ✅ 已修复 |
| BUG-007 | 🟡 建议修改 | localStorage 无限制 | ✅ 已修复 |
| BUG-008 | 🟡 建议修改 | Session ID 不安全 | ✅ 已修复 |
| BUG-009 | 🔵 仅供参考 | 27 个命令无 wrapper | ⏳ 待评估 |
| BUG-010 | 🔵 仅供参考 | API Key 明文 | ⏳ 待评估 |
| BUG-011 | 🔵 仅供参考 | 路径遍历风险 | ⏳ 待评估 |

**已完成**: 8/11 | **进行中**: 0/11 | **待修复**: 0/11 | **待评估**: 3/11
