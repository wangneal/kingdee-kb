# KingdeeKB 项目代码规范

本文档记录项目中发现的代码质量问题和规范约束，用于避免重复犯错。

---

## 1. Rust 后端规范

### 1.1 结构体组织（🔴 必须修复）

**问题**: 同一个结构体分散在多个 `impl` 块中。

```rust
// ❌ 错误：ModelManager 的方法分散在两个 impl 块（embedding.rs:252 和 750）
impl ModelManager { /* 方法组 1 */ }
// ... 500 行其他代码 ...
impl ModelManager { /* 方法组 2 */ }

// ✅ 正确：所有方法放在同一个 impl 块中，按功能分组用注释分隔
impl ModelManager {
    // ── 构造与配置 ──
    pub fn new(...) -> Self { ... }
    pub fn embedding_config(&self) -> ... { ... }

    // ── 初始化 ──
    pub fn init(&mut self) -> ... { ... }
    fn try_init_with_mirrors(...) -> ... { ... }

    // ── 缓存管理 ──
    fn load_user_defined_from_cache(...) -> ... { ... }
    fn has_cached_model_files(...) -> bool { ... }
}
```

**来源**: `src-tauri/src/services/embedding.rs` 第 252 行和第 750 行有两个 `impl ModelManager` 块。

### 1.2 日志规范（🔴 必须修复）

**问题**: 使用 `eprintln!` 做日志输出，无法分级、无法过滤。

```rust
// ❌ 错误
eprintln!("[ModelManager] Loading model...");

// ✅ 正确：使用 tracing 宏
tracing::info!("Loading model from {:?}", path);
tracing::warn!("Mirror {} failed: {}", i, e);
tracing::error!("All mirrors exhausted for {:?}", model);
```

**规则**:
- 引入 `tracing` crate（已在 `Cargo.toml` 中）
- 使用 `tracing::info!` / `warn!` / `error!` / `debug!`
- 日志标签用模块路径，不用硬编码 `[ModelManager]`

### 1.3 魔法数字（🟡 建议改进）

**问题**: 硬编码的数字缺少语义化常量。

```rust
// ❌ 错误
let pct = if total >= 95_000_000 { 99u32 } else { ... };
self.cached_dimension = 512; // default to BGE dimension

// ✅ 正确：提取为命名常量
const EXPECTED_BGE_MODEL_BYTES: u64 = 95_000_000;
const DEFAULT_BGE_DIMENSION: usize = 512;
const DEFAULT_MINILM_DIMENSION: usize = 384;
const DEFAULT_M3_DIMENSION: usize = 1024;
```

### 1.4 锁的使用（🟡 建议改进）

**问题**: 同一函数中多次获取同一 Mutex 的锁，存在 TOCTOU 竞态。

```rust
// ❌ 错误：两次 lock 之间状态可能变化
let result = { let mut mm = state.model_manager.lock()?; mm.init() };
let model = { let mut mm = state.model_manager.lock()?; mm.take_model() };

// ✅ 正确：单次 lock 完成所有操作
let model = {
    let mut mm = state.model_manager.lock()?;
    mm.init()?;
    mm.take_model().ok_or("No model returned")?
};
```

**来源**: `src-tauri/src/commands/embedding.rs` 第 33-56 行。

### 1.5 空函数占位（🔴 必须修复）

**问题**: 存在空函数或未实现的占位函数。

```rust
// ❌ 错误：永远返回 Err 的占位函数
pub fn init_from_local(&mut self, ...) -> Result<(), String> {
    let _ = (onnx_path, tokenizer_path, config_path);
    Err("Local model loading not yet implemented".to_string())
}

// ✅ 正确：要么实现，要么用 todo!() 标记，要么删除
pub fn init_from_local(&mut self, ...) -> Result<(), String> {
    todo!("implement local model loading from user-specified paths")
}
```

**来源**: `src-tauri/src/services/embedding.rs` 第 906-917 行。

### 1.6 错误处理（🟡 建议改进）

**问题**: 混合使用 `unwrap_or_default()` 和 `Result` 传播。

```rust
// ❌ 错误：静默吞掉错误
api_key: config.api_key.clone().unwrap_or_default(),

// ✅ 正确：明确处理缺失值
api_key: config.api_key.clone()
    .ok_or("API key is required for remote embedding provider")?,
```

**规则**: 配置字段缺失时，要么提供有意义的默认值（有注释说明），要么返回明确的错误。

---

## 2. TypeScript/React 前端规范

### 2.1 组件大小（🔴 必须修复）

**问题**: 单个组件文件过大，`Settings.tsx` 超过 1300 行。

```tsx
// ❌ 错误：Settings.tsx 包含 6 个独立的 UI 卡片 + 多个 helper 组件 + 所有逻辑
export default function Settings() { /* 600+ 行 JSX */ }
function DatabaseBackupCard() { /* 100 行 */ }
function FieldRow() { /* 20 行 */ }
function StatCard() { /* 20 行 */ }

// ✅ 正确：拆分为独立文件
// src/pages/settings/Settings.tsx        — 主布局
// src/pages/settings/LLMConfigCard.tsx   — LLM 配置卡片
// src/pages/settings/EmbeddingCard.tsx   — Embedding 卡片
// src/pages/settings/StorageStatsCard.tsx
// src/pages/settings/DesensitizeCard.tsx
// src/pages/settings/ASRConfigCard.tsx
// src/pages/settings/DatabaseBackupCard.tsx
```

**规则**: 单个组件文件不超过 300 行。超过时拆分为子组件文件。

### 2.2 状态管理（🟡 建议改进）

**问题**: 单个组件中使用过多 `useState`，导致状态分散。

```tsx
// ❌ 错误：Settings.tsx 有 20+ 个 useState
const [config, setConfig] = useState<LLMConfig>(DEFAULT_CONFIG);
const [showLocalPresets, setShowLocalPresets] = useState(false);
const [stats, setStats] = useState<KnowledgeStats | null>(null);
const [configured, setConfigured] = useState(false);
const [saving, setSaving] = useState(false);
const [testing, setTesting] = useState(false);
const [testResult, setTestResult] = useState(null);
const [saveMsg, setSaveMsg] = useState(null);
const [loading, setLoading] = useState(true);
const [showApiKey, setShowApiKey] = useState(false);
// ... 还有 10+ 个

// ✅ 正确：用 useReducer 或拆分为独立组件，每个组件管理自己的状态
const [llmState, llmDispatch] = useReducer(llmReducer, initialLLMState);
const [embeddingState, embeddingDispatch] = useReducer(embeddingReducer, initialEmbeddingState);
```

**规则**: 单个组件的 `useState` 不超过 8 个。超过时用 `useReducer` 或拆分组件。

### 2.3 错误处理（🔴 必须修复）

**问题**: 使用 `alert()` 和 `console.warn()` 处理错误。

```tsx
// ❌ 错误
catch (e) { alert(String(e)); }
console.warn("[Chat] Failed to save chat memory:", e);

// ✅ 正确：使用统一的 toast/notification 系统
catch (e) { showErrorToast(`添加敏感词失败：${String(e)}`); }
// 或使用组件内的错误状态展示
```

**规则**:
- 禁止使用 `alert()` — 阻塞 UI 且不可定制
- 使用组件内状态展示错误（如已有的 `saveMsg` 模式）
- 生产代码中禁止 `console.warn`/`console.error`（用统一日志或去掉）

### 2.4 全局可变状态（🟡 建议改进）

**问题**: 使用模块级 `let` 变量作为全局计数器。

```tsx
// ❌ 错误：全局可变状态，SSR 不安全
let msgIdCounter = 0;
function nextId(): string {
  return `msg_${++msgIdCounter}_${Date.now()}`;
}

// ✅ 正确：用 useRef 或 crypto.randomUUID()
function nextId(): string {
  return crypto.randomUUID();
}
```

**来源**: `src/pages/Chat.tsx` 第 58-61 行。

### 2.5 localStorage 敏感数据（🟡 建议改进）

**问题**: API Key 明文存储在 localStorage 中。

```tsx
// ❌ 错误：API Key 明文存储
localStorage.setItem(EMBEDDING_PROVIDER_STORAGE_KEY, JSON.stringify({
  api_key: embeddingProviderConfig.api_key, // 明文！
}));

// ✅ 正确：
// 1. API Key 应通过 Tauri 的 secure storage 或 Rust 后端存储
// 2. 前端只保存不含敏感信息的配置
localStorage.setItem(EMBEDDING_PROVIDER_STORAGE_KEY, JSON.stringify({
  provider: embeddingProviderConfig.provider,
  base_url: embeddingProviderConfig.base_url,
  model_name: embeddingProviderConfig.model_name,
  // api_key 不存 localStorage
}));
```

**规则**: API Key、Secret 等敏感信息禁止存入 localStorage。通过 Tauri 命令存储到后端。

### 2.6 类型断言（🟡 建议改进）

**问题**: 使用 `as any` 或不安全的类型断言。

```tsx
// ❌ 错误
const eventSessionId = event.session_id || (event as any).sessionId;

// ✅ 正确：在类型定义中包含两种命名
interface ReActEvent {
  session_id?: string;
  sessionId?: string;  // Tauri v2 camelCase 兼容
  // ...
}
```

**来源**: `src/pages/Chat.tsx` 第 133 行。

### 2.7 内联函数（🟡 建议改进）

**问题**: JSX 中定义内联函数导致不必要的重渲染。

```tsx
// ❌ 错误：每次渲染都创建新函数
<button onClick={async () => {
  if (!keywordInput.trim()) return;
  await addSensitiveKeyword(keywordInput.trim());
}}>添加</button>

// ✅ 正确：提取为 useCallback
const handleAddKeyword = useCallback(async () => {
  if (!keywordInput.trim()) return;
  await addSensitiveKeyword(keywordInput.trim());
  setKeywordInput("");
  setKeywords(await listSensitiveKeywords());
}, [keywordInput]);

<button onClick={handleAddKeyword}>添加</button>
```

---

## 3. 通用规范

### 3.1 文件命名

| 类型 | 规范 | 示例 |
|------|------|------|
| Rust 模块 | `snake_case.rs` | `embedding.rs`, `vector_index.rs` |
| React 组件 | `PascalCase.tsx` | `Settings.tsx`, `Chat.tsx` |
| 工具函数 | `camelCase.ts` | `tauri-commands.ts` |
| 测试文件 | 与源文件同名 | `embedding.rs` (tests 模块) / `home.spec.ts` |

### 3.2 注释语言

- Rust 代码注释：英文（doc comments 用 `///`）
- 前端代码注释：英文（JSDoc 用 `/** */`）
- 用户可见的 UI 文案：中文
- 错误消息：英文（面向开发者）+ 中文（面向用户）

### 3.3 错误处理原则

1. **Rust**: 使用 `Result<T, String>` 作为 Tauri 命令返回值，内部可用自定义 Error enum
2. **前端**: 使用 try/catch + 组件内状态展示错误，禁止 `alert()`
3. **日志**: Rust 用 `tracing`，前端用统一的 error boundary 或 toast

### 3.4 依赖管理

- 新增 Rust crate 前先检查 `Cargo.toml` 是否已有类似功能
- 新增 npm 包前先检查是否可以用现有依赖实现
- 禁止引入仅用于一两个小功能的大型依赖

---

## 4. 待修复清单

| # | 文件 | 行号 | 严重度 | 问题 |
|---|------|------|--------|------|
| 1 | `embedding.rs` | 252, 750 | 🔴 | 两个 `impl ModelManager` 块应合并 |
| 2 | `embedding.rs` | 全文 | 🔴 | `eprintln!` 应替换为 `tracing` |
| 3 | `embedding.rs` | 906-917 | 🔴 | 空函数 `init_from_local` 应删除或实现 |
| 4 | `Settings.tsx` | 全文 | 🔴 | 1300 行组件应拆分 |
| 5 | `Settings.tsx` | 1016, 1032 | 🔴 | `alert()` 应替换为状态展示 |
| 6 | `Chat.tsx` | 133 | 🟡 | `as any` 类型断言应修复类型定义 |
| 7 | `Chat.tsx` | 58-61 | 🟡 | 全局计数器应用 `crypto.randomUUID()` |
| 8 | `embedding.rs` | 138, 1003 | 🟡 | 魔法数字应提取为常量 |
| 9 | `commands/embedding.rs` | 33-56 | 🟡 | 多次 lock 应合并为单次 |
| 10 | `Settings.tsx` | 359 | 🟡 | API Key 不应存入 localStorage |

---

*最后更新: 2026-05-28*
*审查人: Sisyphus (AI Agent)*
