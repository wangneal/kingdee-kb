# 失败知识编译重试实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 在知识库页面提供“重编译失败项”入口，只重试当前项目中原始资料存在但知识编译未成功写入 `ingest_cache` 的条目。

**架构：** 后端新增 `recompile_failed_kb_sources(project_id)` Tauri 命令，扫描 `raw_sources` 与 `ingest_cache` 的差集，读取原始文件并复用 `process_with_kb_compilation`。前端在 `/browse` 知识库页面添加按钮，调用命令后刷新 Wiki 列表并展示结果。

**技术栈：** Tauri v2、Rust、React 19、TypeScript、Playwright、SQLite。

---

## 文件结构

- 修改：`src-tauri/src/commands/kb_compilation.rs`，新增结果类型和重编译命令。
- 修改：`src-tauri/src/lib.rs`，注册 `recompile_failed_kb_sources`。
- 修改：`src/lib/tauri-commands.ts`，新增 TS 类型和命令封装。
- 修改：`src/pages/Browse.tsx`，添加按钮、状态和结果提示。
- 修改：`e2e/mocks/tauri-mock.ts`，添加默认 mock 返回值。
- 修改：`e2e/browse.spec.ts`，添加按钮调用和刷新断言。

## 任务 1：前端红灯测试

**文件：**
- 测试：`e2e/browse.spec.ts`
- 参考：`e2e/mocks/tauri-mock.ts`

- [ ] **步骤 1：编写失败的 E2E 测试**

在 `Browse page` describe 中添加测试：

```ts
test("should recompile failed kb sources and refresh wiki list", async ({ page }) => {
  await mockTauriApis(page, {
    responses: {
      recompile_failed_kb_sources: {
        retried: 1,
        succeeded: 1,
        failed: [],
      },
    },
    sequences: {
      list_wiki_pages: [
        [],
        [{ id: 20, slug: "retry-doc", title: "重编译文档", page_type: "summary" }],
      ],
    },
  })
  await page.addInitScript(() => {
    localStorage.setItem("kingdee_kb_active_project", "1")
  })

  await page.goto("/browse")
  await page.getByRole("button", { name: "重编译失败项" }).click()

  await expect.poll(async () => {
    const calls = await page.evaluate(() => Reflect.get(globalThis, "__TAURI_MOCK_CALLS__"))
    const recompileCalls = Reflect.get(calls, "recompile_failed_kb_sources") as
      | Record<string, unknown>[]
      | undefined
    return recompileCalls?.at(-1)?.projectId
  }).toBe(1)
  await expect(page.getByText("重编译完成：成功 1/1 项")).toBeVisible()
  await expect(page.getByRole("button", { name: "重编译文档 summary" })).toBeVisible()
})
```

- [ ] **步骤 2：运行测试验证失败**

运行：`pnpm playwright test e2e/browse.spec.ts`

预期：FAIL，找不到“重编译失败项”按钮。

## 任务 2：后端命令实现

**文件：**
- 修改：`src-tauri/src/commands/kb_compilation.rs`
- 修改：`src-tauri/src/lib.rs`

- [ ] **步骤 1：新增返回类型和命令**

在 `kb_compilation.rs` 中添加：

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecompileFailedSourceError {
    pub source_id: i64,
    pub title: String,
    pub error: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RecompileFailedSourcesResult {
    pub retried: usize,
    pub succeeded: usize,
    pub failed: Vec<RecompileFailedSourceError>,
}
```

命令行为：

```rust
#[tauri::command]
pub async fn recompile_failed_kb_sources(
    state: tauri::State<'_, AppState>,
    project_id: i64,
) -> Result<RecompileFailedSourcesResult, String> {
    // 1. 读取 raw_sources.list_by_project(project_id)
    // 2. 读取 ingest_cache.list_by_project(project_id)
    // 3. 过滤没有匹配 (identity, sha256) 缓存的 source
    // 4. 对每个 source 读取 storage_path 文本
    // 5. 调用 process_with_kb_compilation(..., true, ...)
    // 6. 成功计数，失败加入 failed 列表
}
```

- [ ] **步骤 2：注册 Tauri 命令**

在 `src-tauri/src/lib.rs` 的 KB Compilation Config 区域加入：

```rust
commands::kb_compilation::recompile_failed_kb_sources,
```

- [ ] **步骤 3：运行 Rust 检查**

运行：`cargo check`

预期：PASS。

## 任务 3：前端封装和 UI 实现

**文件：**
- 修改：`src/lib/tauri-commands.ts`
- 修改：`src/pages/Browse.tsx`
- 修改：`e2e/mocks/tauri-mock.ts`

- [ ] **步骤 1：新增命令封装**

在 `tauri-commands.ts` 添加：

```ts
export interface RecompileFailedSourceError {
  source_id: number
  title: string
  error: string
}

export interface RecompileFailedSourcesResult {
  retried: number
  succeeded: number
  failed: RecompileFailedSourceError[]
}

export async function recompileFailedKbSources(
  projectId: number,
): Promise<RecompileFailedSourcesResult> {
  return invoke("recompile_failed_kb_sources", { projectId })
}
```

- [ ] **步骤 2：新增 Browse 按钮**

在 `Browse.tsx` 中添加状态：

```ts
const [recompiling, setRecompiling] = useState(false)
const [recompileMessage, setRecompileMessage] = useState<string | null>(null)
```

添加处理函数：

```ts
const handleRecompileFailed = useCallback(async () => {
  if (currentProjectId == null) return
  setRecompiling(true)
  setRecompileMessage(null)
  try {
    const result = await recompileFailedKbSources(currentProjectId)
    setRecompileMessage(
      result.failed.length > 0
        ? `重编译完成：成功 ${result.succeeded}/${result.retried} 项，失败 ${result.failed.length} 项`
        : `重编译完成：成功 ${result.succeeded}/${result.retried} 项`,
    )
    await refreshWikiPages()
  } catch (error) {
    setRecompileMessage(`重编译失败：${error instanceof Error ? error.message : String(error)}`)
  } finally {
    setRecompiling(false)
  }
}, [currentProjectId, refreshWikiPages])
```

在标题区域添加按钮，按钮禁用条件为 `currentProjectId == null || recompiling`。

- [ ] **步骤 3：补 mock**

在 `e2e/mocks/tauri-mock.ts` 的 `MOCK_RESPONSES` 添加：

```ts
recompile_failed_kb_sources: { retried: 0, succeeded: 0, failed: [] },
```

- [ ] **步骤 4：运行 E2E 验证绿灯**

运行：`pnpm playwright test e2e/browse.spec.ts`

预期：PASS。

## 任务 4：全量相关验证

**文件：**
- 修改文件全体

- [ ] **步骤 1：TypeScript 类型检查**

运行：`pnpm typecheck`

预期：PASS。

- [ ] **步骤 2：相关 E2E**

运行：`pnpm playwright test e2e/browse.spec.ts`

预期：PASS。

- [ ] **步骤 3：Rust 编译检查**

运行：`cargo check`，工作目录 `src-tauri`

预期：PASS。

- [ ] **步骤 4：定向格式/静态检查**

运行：

```bash
pnpm exec biome check --formatter-enabled=false --assist-enabled=false src/pages/Browse.tsx src/lib/tauri-commands.ts e2e/browse.spec.ts e2e/mocks/tauri-mock.ts
rustfmt --edition 2024 --check src/commands/kb_compilation.rs
```

预期：PASS。

---

## 自检

- 规格覆盖：仅处理“失败项”，不覆盖已有成功缓存，不清理缓存，不删除数据。
- 类型一致性：前端 `projectId` 顶层参数映射到 Rust `project_id`；返回结构使用 snake_case 字段，与 Tauri 嵌套 serde 保持一致。
- 范围控制：不新增强制重编译全部项目、不覆盖 Wiki 已审批内容。
