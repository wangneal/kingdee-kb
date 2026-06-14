/**
 * 知识编译开关的全局共享状态。
 *
 * 背景：原本 Import.tsx、Settings.tsx 和 useImport.ts 各自维护一份本地 useState，
 * mount-only 读取后端配置，切换页面时显示旧值（跨页不同步）。
 * 现统一收敛到本 Context，任一页面切换开关后所有页面立即看到新值。
 */
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react"
import { getKbCompilationEnabled, setKbCompilationEnabled } from "@/lib/tauri-commands"

interface KbCompilationContextValue {
  /** 知识编译开关是否启用 */
  enabled: boolean
  /** 配置是否正在从后端加载（加载完成前 enabled 可能不准确） */
  loading: boolean
  /** 是否正在持久化切换（用于禁用 checkbox） */
  saving: boolean
  /** 切换开关：同步更新本地 state 并持久化到后端 */
  setEnabled: (next: boolean) => Promise<void>
}

const KbCompilationContext = createContext<KbCompilationContextValue | null>(null)

export function KbCompilationProvider({ children }: { children: ReactNode }) {
  const [enabled, setEnabledState] = useState(false)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const loadedRef = useRef(false)

  // 首次挂载时从后端读取一次，作为整个应用的权威值
  useEffect(() => {
    let cancelled = false
    getKbCompilationEnabled()
      .then((value) => {
        if (cancelled) return
        setEnabledState(value)
        loadedRef.current = true
      })
      .catch(() => {
        if (cancelled) return
        // 读取失败按关闭处理，但不阻塞功能
        setEnabledState(false)
        loadedRef.current = true
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const setEnabled = useCallback(async (next: boolean) => {
    setSaving(true)
    try {
      await setKbCompilationEnabled(next)
      // 持久化成功后才更新本地 state，保证 UI 与后端一致
      setEnabledState(next)
    } finally {
      setSaving(false)
    }
  }, [])

  return (
    <KbCompilationContext.Provider value={{ enabled, loading, saving, setEnabled }}>
      {children}
    </KbCompilationContext.Provider>
  )
}

export function useKbCompilation(): KbCompilationContextValue {
  const ctx = useContext(KbCompilationContext)
  if (!ctx) throw new Error("useKbCompilation must be used within KbCompilationProvider")
  return ctx
}

/**
 * useImport 等"必须等待配置加载完成才允许操作"的场景使用的辅助 hook。
 * 返回配置是否已加载（loading 取反）。
 */
export function useKbCompilationLoaded(): boolean {
  const { loading } = useKbCompilation()
  return !loading
}
