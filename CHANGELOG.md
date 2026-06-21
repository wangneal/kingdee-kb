# Changelog

本文件记录每次版本更新的主要变更。格式基于 [Conventional Commits](https://www.conventionalcommits.org/)。

## [0.1.28] - 2026-06-21

### 🔒 安全修复

- **ASR 密钥迁移到系统钥匙串**：腾讯云 ASR 的 `secret_id` / `secret_key` 不再以明文 JSON 存储在磁盘，改用 `keyring` 写入操作系统加密存储（Windows DPAPI / macOS Keychain），与 LLM API Key 的存储方式保持一致
- **移除 Anthropic 浏览器直连头**：`anthropic-dangerous-direct-browser-access` 请求头不再硬编码，避免潜在的安全风险

### ♻️ 代码质量

- **路由懒加载**：所有页面组件改为 `React.lazy()` 按需加载，首屏只加载当前路由所需的代码，配合 `<Suspense>` 显示加载态
- **移除死代码**：删除 `Chat.tsx` 中硬编码为 `false` 的 `attaching` 变量及其在按钮 `disabled` 条件中的引用
- **移除模块级 dead_code 抑制**：`app_state.rs` 不再使用 `#![allow(dead_code)]` 全局抑制警告，改为按需标注
- **修复文档注释乱码**：`llm_service.rs` 模块级文档注释从 mojibake 恢复为正确的中文

### 📝 文档

- **新增 CHANGELOG**：建立版本变更记录，按 Conventional Commits 分类
- **README 更新**：开发者指南补充 `cargo clippy`、`pnpm lint`、`pnpm typecheck` 检查命令

## [0.1.27] - 2026-06-17

### ✨ 新功能

- Agent 会话取消机制（AtomicBool 级联取消）
- rAF 事件缓冲调度，减少流式渲染的 re-render 频率
- 死循环保护（DOOM_LOOP_THRESHOLD）

### 🐛 Bug 修复

- 修复共享可变 DEFAULT_SLOT 单例问题
- 修复 rAF buffer 数据丢失和 latestToolName 过期问题

### ♻️ 重构

- 大规模代码质量重构：消除重复、降低复杂度、提升可维护性
- 修复 39 个 TypeScript 编译错误
- 移除 AI 风格冗余注释和死代码
