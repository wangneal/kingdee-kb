/**
 * Tauri 命令结构化错误（与后端 `AppError` 对应）。
 *
 * 后端在 IPC 失败时把 `AppError` 序列化为：
 * ```json
 * { "code": "LLM_INVALID_KEY", "message": "...", "provider_id": "openai" }
 * ```
 *
 * 前端在 catch 块里调 `parseAppError(error)` 拿到这个结构，
 * 根据 `code` 决定走默认 toast、API Key 对话框或其他专门处理。
 *
 * ## 错误码（必须与后端 `error.rs` 的 `code()` 保持一致）
 *
 * - `LLM_INVALID_KEY` — LLM API Key 失效/过期/被吊销。前端弹"配置 API Key"对话框
 *   而不是普通 toast。
 * - 其他 — 普通错误，前端走默认 toast。
 */
export interface AppErrorPayload {
  code: string
  message: string
  /** 仅 `LLM_INVALID_KEY` 携带：定位到具体供应商 */
  provider_id?: string
}

/**
 * Tauri invoke 在 reject 时把后端序列化的对象作为 error 抛出。
 *
 * 因为后端是结构化对象（不是字符串），TS 这里只能用 unknown 收。
 */
export function parseAppError(error: unknown): AppErrorPayload | null {
  if (!error || typeof error !== "object") return null
  const e = error as Record<string, unknown>
  if (typeof e.code !== "string" || typeof e.message !== "string") return null
  return {
    code: e.code,
    message: e.message,
    provider_id: typeof e.provider_id === "string" ? e.provider_id : undefined,
  }
}

/**
 * 判断是否是 LLM API Key 失效错误。
 *
 * 用于在 catch 块里：
 * ```ts
 * try {
 *   await invoke(...)
 * } catch (err) {
 *   if (isLlmInvalidKeyError(err)) {
 *     appErrorContext.showLlmKeyError(parseAppError(err)!)
 *     return
 *   }
 *   throw err
 * }
 * ```
 */
export function isLlmInvalidKeyError(error: unknown): boolean {
  return parseAppError(error)?.code === "LLM_INVALID_KEY"
}

/**
 * 把任意 Tauri 错误转成可读字符串。
 *
 * 优先级：结构化 message > 错误对象自身的 toString > 兜底文案。
 * 不要直接 `error.toString()`，因为后端结构化错误可能 toString 成 "[object Object]"。
 */
export function formatAppError(error: unknown, fallback = "未知错误"): string {
  const parsed = parseAppError(error)
  if (parsed) return parsed.message
  if (error instanceof Error) return error.message || fallback
  if (typeof error === "string") return error
  return fallback
}

/**
 * 把 Tauri 错误"分类"成两种语义：
 * - `'llm_key'` — 后端返回 `LLM_INVALID_KEY`，调用方应触发 API Key 对话框
 * - `'other'` — 普通错误，调用方走默认 toast
 *
 * 用于业务 catch 块里：
 * ```ts
 * try {
 *   await invoke(...)
 * } catch (err) {
 *   const kind = classifyAppError(err)
 *   if (kind === "llm_key") {
 *     appError.showLlmKeyError(parseAppError(err)!)
 *     return
 *   }
 *   toast.error(formatAppError(err))
 * }
 * ```
 */
export function classifyAppError(error: unknown): "llm_key" | "other" {
  return isLlmInvalidKeyError(error) ? "llm_key" : "other"
}
