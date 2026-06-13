// Tauri 命令结构化错误：code 必须与后端 error.rs 的 AppError::code() 保持一致。
export interface AppErrorPayload {
  code: string
  message: string
  /** 仅 LLM_INVALID_KEY 携带：定位到具体供应商 */
  provider_id?: string
}

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

export function formatAppError(error: unknown, fallback = "未知错误"): string {
  const parsed = parseAppError(error)
  if (parsed) return parsed.message
  if (error instanceof Error) return error.message || fallback
  if (typeof error === "string") return error
  return fallback
}
