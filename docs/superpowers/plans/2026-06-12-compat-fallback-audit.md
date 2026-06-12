# 回退 / 兼容 / 迁移逻辑审查报告

**日期**：2026-06-12
**审查范围**：`src-tauri/src` 全部 `.rs` 文件
**审查依据**：`AGENTS.md` 的"重写与兼容原则"——项目尚未发布，禁止保留旧实现、回退、双协议、根据内容猜测协议。

## 严重违规（按规则必须删除）

### 1. 显式"向后兼容"标注
- `src-tauri/src/services/llm_service.rs:13`
  ```rust
  // Re-export for backward compatibility with external callers
  use crate::services::agent_timeout::{ ... };
  pub use crate::services::token::truncate_to_tokens;
  ```
  **违反**：明确写"backward compatibility"注释。`truncate_to_tokens` 应当从原始模块导入，不要 re-export。

### 2. 旧版数据库迁移链路（4 个 store 全部中招）

| 文件 | 违规函数 | 行号 |
|---|---|---|
| `services/raw_source.rs` | `migrate_legacy_table_in_tx` | 153-195 |
| `services/raw_source.rs` | `backfill_project_id_in_tx`（带 legacy 分支） | 120-147 |
| `services/wiki_page.rs` | `migrate_legacy_table` | 185-236 |
| `services/wiki_page.rs` | `backfill_project_id`（带 legacy 分支） | 238-261 |
| `services/wiki_page.rs` | `ensure_column` | 163-183（仅为兼容添加列用） |
| `services/ingest_cache.rs` | `migrate_legacy_table` | 120-153 |
| `services/ingest_cache.rs` | `backfill_project_id`（带 legacy 分支） | 155-178 |
| `services/metadata.rs` | 兼容已有数据库注释 + 补列逻辑 | 252 + `ensure_column` 系列 |
| `services/project_store.rs` | `import_legacy_projects` | 198-219 |
| `services/project_store.rs` | `list_projects` 中的 4 处 `table_has_column` 分支 | 266-290 |

**违规**：这些函数 / 分支全部基于"`project` 列（旧字段名）→ `project_id` 列（新字段名）"的迁移假设；项目尚未发布，旧 schema 不可能在生产中存在。所有迁移代码应**整段删除**，仅保留"按当前 schema CREATE TABLE IF NOT EXISTS"。

### 3. 协议 / 字段自动猜测
- `services/llm_providers.rs:282-290, 421, 443-444, 1830, 1862` —— `FALLBACK_FREE_MODEL`
  ```rust
  // 错误处理策略：网络/解析失败时返回空 Vec，调用方会自动 fallback 到 FALLBACK_FREE_MODEL
  const FALLBACK_FREE_MODEL: &str = "minimax-m2.5-free";
  ```
  **违反**：当网络失败时"猜测"一个兜底模型名，违反"禁止根据内容猜测协议"原则。应当：拉取失败就**报错**或**返回空配置让用户手动添加**，不要硬塞兜底。

- `services/llm_providers.rs:975` —— 多模态探测 Base64 失败时"尝试公网图片 URL 探测 (fallback)"
  **边界 case**：这是探测多种输入格式（Base64 vs URL），可视为**输入识别**而非配置兼容。视情况保留。

- `services/llm_providers.rs:1222` —— "获取所有多模态候选模型（按优先级排序，用于自动回退）"
  **边界 case**：多供应商多模态自动回退是**业务策略**（用户没配多模态时降级），不是配置兼容。视情况保留。

- `services/document_analysis.rs:150, 798, 829, 834, 840, 929` —— `fallback_rust`（LLM 文档分析失败时回退到 Rust 简单实现）
  **边界 case**：优雅降级，业务容错，不属于"配置兼容"。可保留。

- `services/wikilink_parser.rs:23` —— "传空切片时跳过验证（向后兼容：未传项目 slugs 时也能提取）"
  **违反**：注释明确写"向后兼容"，应当要求调用方必传 slugs。改为强制参数即可。

## 误报 / 可保留

- `services/embedding.rs:222` —— "Fallback: ~/.cache (Linux/macOS) or %USERPROFILE%\.cache"
  → 这是**多平台路径查找**，不是配置兼容。

- `services/llm_providers.rs:171, 173` —— "OpenAI 兼容 / Anthropic 兼容"
  → 这是**协议标准**（多个 LLM 厂商遵循同一协议），不是双协议猜测。

- `services/llm_service.rs:538, 1282, 1331, 1977, 2263-2264` —— `fallback_response`（LLM 不可用时回退到纯检索结果）
  → 业务级优雅降级，可保留。

- `services/ingestion_pipeline.rs:391` —— "证书兼容"
  → TLS 证书兼容（relaxed TLS），是**网络层兼容**，可保留。

## 修复优先级

1. **P0 立即修**（注释里直接写"backward compatibility"）：
   - [llm_service.rs:13] 删 `pub use crate::services::token::truncate_to_tokens;` 改成显式 `use`
   - [wikilink_parser.rs:23] 移除空切片跳过逻辑

2. **P1 本次清**（4 个 store 的迁移函数 + 4 处 list_projects 分支）：
   - 删除 `migrate_legacy_*` 函数及其调用点
   - 删除 `backfill_project_id` 内的 legacy 分支
   - 删除 `list_projects` 内 4 处 `table_has_column` 分支
   - 删除相关测试 `migrates_legacy_*` / `imports_legacy_*`
   - 删除 `has_column` 辅助函数（无其他使用方时）

3. **P2 评估**：
   - `FALLBACK_FREE_MODEL` 兜底策略：建议改成"网络失败时不写默认配置，记日志，提示用户手动添加"，而不是用兜底模型填空。
   - 多模态自动回退策略：保留，但需在代码注释里**明确**这是业务级容错而非兼容旧实现。

## 风险提示

- 删除迁移代码后，**已经创建过旧 schema 数据的开发机**会因 `project_id NOT NULL` 约束插入失败而启动崩溃。
  → 缓解方案：要么把本地 dev 数据删库重建（项目尚未发布，符合规则），要么保留迁移代码但只跑一次。
  → 决策建议：直接删，符合"项目尚未发布"原则。
