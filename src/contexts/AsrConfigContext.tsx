/**
 * ASR 状态的全局共享。
 *
 * 背景：原本 Settings.tsx 和 ResearchAssistant.tsx 各自维护本地 useState
 * 调 getAsrConfigStatus()，Settings 改 ASR 后 ResearchAssistant 不感知，
 * 用户切回录音界面时拿到的还是旧状态。
 *
 * 行为：mount 时拉取一次；调用 reload() 主动重新拉取；
 * saveAsrConfig() 内部会自动 reload（用户改 ASR 后另一页面立即看到）。
 */
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react"
import { type AsrConfigStatus, getAsrConfigStatus } from "../lib/tauri-commands"

interface AsrConfigContextValue {
  /** 后端 ASR 配置状态；加载中为 null */
  status: AsrConfigStatus | null
  /** 首次拉取是否完成 */
  loading: boolean
  /** 主动重新拉取（如保存 ASR 配置后） */
  reload: () => Promise<void>
}

const AsrConfigContext = createContext<AsrConfigContextValue | null>(null)

export function AsrConfigProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AsrConfigStatus | null>(null)
  const [loading, setLoading] = useState(true)

  const reload = useCallback(async () => {
    try {
      const value = await getAsrConfigStatus()
      setStatus(value)
    } catch {
      setStatus(null)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  return (
    <AsrConfigContext.Provider value={{ status, loading, reload }}>
      {children}
    </AsrConfigContext.Provider>
  )
}

export function useAsrConfig(): AsrConfigContextValue {
  const ctx = useContext(AsrConfigContext)
  if (!ctx) throw new Error("useAsrConfig must be used within AsrConfigProvider")
  return ctx
}
