/**
 * AppError 全局 Provider — 监听后端 Tauri 错误，弹"配置 API Key"对话框。
 *
 * ## 工作流程
 *
 * 1. 业务代码在 catch 块里发现 LLM_INVALID_KEY，调
 *    `useAppError().showLlmKeyError(payload)` 把错误推到全局队列。
 * 2. Provider 维护一个 pending 错误队列，**同时只展示一个对话框**
 *    （避免错误风暴时弹十几个对话框把用户吓跑）。
 * 3. 用户点"去设置"→ 关闭对话框 + 跳转到 /settings。
 * 4. 用户点"稍后再说"→ 关闭对话框，错误继续保留在队列中（不再重弹，
 *    避免阻塞用户当前操作）。
 *
 * ## 边界
 *
 * - **不**接管普通 toast 错误，那些仍走 `useToast`。
 * - **不**自动 retry，避免静默重试造成雪崩。
 * - **不**在用户已经看到过一次 401 后继续弹；去重逻辑见 [`showLlmKeyError`]
 *   内的 `lastShownAt` 节流。
 */

import { AlertTriangle, Key, Settings, X } from "lucide-react"
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from "react"
import { useNavigate } from "react-router-dom"
import type { AppErrorPayload } from "../lib/app-error"

interface AppErrorContextValue {
  /**
   * 推送一个 LLM API Key 错误。
   *
   * 重复错误（同 provider_id 在 5 秒内）会被节流，避免多个并发失败
   * 导致用户被弹 N 次对话框。
   */
  showLlmKeyError: (payload: AppErrorPayload) => void
  /** 当前是否正在显示 API Key 对话框 */
  hasPendingLlmKeyError: boolean
}

const AppErrorContext = createContext<AppErrorContextValue | null>(null)

export function useAppError(): AppErrorContextValue {
  const ctx = useContext(AppErrorContext)
  if (!ctx) throw new Error("useAppError must be used within AppErrorProvider")
  return ctx
}

const LLM_KEY_DIALOG_THROTTLE_MS = 5000

export function AppErrorProvider({ children }: { children: ReactNode }) {
  const [currentError, setCurrentError] = useState<AppErrorPayload | null>(null)
  const lastShownAtRef = useRef<{ providerId: string; at: number } | null>(null)
  const queueRef = useRef<AppErrorPayload[]>([])
  const navigate = useNavigate()

  const showLlmKeyError = useCallback(
    (payload: AppErrorPayload) => {
      if (payload.code !== "LLM_INVALID_KEY") return

      const now = Date.now()
      const last = lastShownAtRef.current
      // 同 provider 5 秒内不重复弹
      if (
        last &&
        last.providerId === (payload.provider_id ?? "") &&
        now - last.at < LLM_KEY_DIALOG_THROTTLE_MS
      ) {
        return
      }

      if (currentError) {
        // 已有对话框在展示 → 排队
        const providerKey = payload.provider_id ?? ""
        const duplicateInQueue = queueRef.current.some((e) => (e.provider_id ?? "") === providerKey)
        if (!duplicateInQueue) {
          queueRef.current.push(payload)
        }
        return
      }

      lastShownAtRef.current = { providerId: payload.provider_id ?? "", at: now }
      setCurrentError(payload)
    },
    [currentError],
  )

  const dismiss = useCallback(() => {
    setCurrentError(null)
    // 显示完一个后，弹队列里下一个（如果有）
    const next = queueRef.current.shift()
    if (next) {
      // 跳过 throttle：队列里的都是用户没看过的
      lastShownAtRef.current = null
      setCurrentError(next)
    }
  }, [])

  const goToSettings = useCallback(() => {
    setCurrentError(null)
    queueRef.current = []
    navigate("/settings")
  }, [navigate])

  const value = useMemo<AppErrorContextValue>(
    () => ({
      showLlmKeyError,
      hasPendingLlmKeyError: currentError !== null,
    }),
    [showLlmKeyError, currentError],
  )

  return (
    <AppErrorContext.Provider value={value}>
      {children}
      {currentError && (
        <ApiKeyConfigDialog
          error={currentError}
          onDismiss={dismiss}
          onGoToSettings={goToSettings}
        />
      )}
    </AppErrorContext.Provider>
  )
}

// ── 对话框组件 ──────────────────────────────────────────────────────

interface ApiKeyConfigDialogProps {
  error: AppErrorPayload
  onDismiss: () => void
  onGoToSettings: () => void
}

function ApiKeyConfigDialog({ error, onDismiss, onGoToSettings }: ApiKeyConfigDialogProps) {
  // provider_id 缺失时仍展示（用户进设置页自查哪个 Key 失效了）
  const hasProvider = Boolean(error.provider_id)

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      data-testid="llm-key-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="llm-key-dialog-title"
    >
      <div className="relative w-full max-w-md rounded-lg border border-amber-200 bg-white p-6 shadow-xl">
        <button
          type="button"
          onClick={onDismiss}
          className="absolute right-3 top-3 rounded p-1 text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          aria-label="关闭"
        >
          <X className="h-4 w-4" />
        </button>

        <div className="mb-4 flex items-start gap-3">
          <div className="rounded-full bg-amber-100 p-2 text-amber-600">
            <Key className="h-5 w-5" />
          </div>
          <div className="flex-1">
            <h2
              id="llm-key-dialog-title"
              className="flex items-center gap-2 text-base font-semibold text-neutral-900"
            >
              <AlertTriangle className="h-4 w-4 text-amber-500" />
              LLM API Key 失效
            </h2>
            <p className="mt-2 text-sm leading-relaxed text-neutral-600">
              {hasProvider
                ? `供应商「${error.provider_id}」的 API Key 已失效或被吊销。`
                : "默认 LLM 供应商的 API Key 已失效或被吊销。"}
              请到设置页更换 Key 后重试。
            </p>
          </div>
        </div>

        {error.message && (
          <details className="mb-4 rounded border border-neutral-200 bg-neutral-50 px-3 py-2 text-xs text-neutral-600">
            <summary className="cursor-pointer font-medium text-neutral-700">详细信息</summary>
            <p className="mt-1 whitespace-pre-wrap break-words font-mono text-[11px]">
              {error.message}
            </p>
          </details>
        )}

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onDismiss}
            className="rounded-lg border border-neutral-200 bg-white px-3 py-1.5 text-sm font-medium text-neutral-700 hover:bg-neutral-50"
          >
            稍后再说
          </button>
          <button
            type="button"
            onClick={onGoToSettings}
            className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-3 py-1.5 text-sm font-medium text-white hover:bg-[#1559b8]"
          >
            <Settings className="h-3.5 w-3.5" />
            去设置
          </button>
        </div>
      </div>
    </div>
  )
}
