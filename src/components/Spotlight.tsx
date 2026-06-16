import { Loader2, Search, Send, X } from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { useProject } from "@/contexts/ProjectContext"
import { agentChat, listenAgentEvents } from "@/lib/tauri-commands"

export default function Spotlight() {
  const { currentProjectId } = useProject()
  const [visible, setVisible] = useState(false)
  const [input, setInput] = useState("")
  const [loading, setLoading] = useState(false)
  const [result, setResult] = useState("")
  const resultRef = useRef("")
  const inputRef = useRef<HTMLInputElement>(null)
  const overlayRef = useRef<HTMLDivElement>(null)
  const spotSessionRef = useRef<string | null>(null)

  // 监听 Tauri 全局快捷键 Alt+Space 事件
  useEffect(() => {
    let unlisten: (() => void) | null = null

    ;(async () => {
      const { listen } = await import("@tauri-apps/api/event")
      unlisten = await listen("spotlight-toggle", () => {
        setVisible((v) => !v)
        setInput("")
        setResult("")
        resultRef.current = ""
      })
    })()

    // 保留本地 Escape 关闭处理
    const escHandler = (e: KeyboardEvent) => {
      if (e.key === "Escape" && visible) {
        setVisible(false)
      }
    }
    window.addEventListener("keydown", escHandler)

    return () => {
      if (unlisten) unlisten()
      window.removeEventListener("keydown", escHandler)
    }
  }, [visible])

  // 浮层显示时自动聚焦输入框
  useEffect(() => {
    if (visible && inputRef.current) {
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [visible])

  // 监听 ReAct 事件（按会话过滤）
  useEffect(() => {
    let cancelled = false
    const unlistenRef: { current: (() => void) | null } = { current: null }
    listenAgentEvents((event) => {
      // 同时支持 snake_case 和 camelCase（Tauri v2 可能转换）
      const eventSessionId = event.session_id || event.sessionId
      if (eventSessionId !== spotSessionRef.current) return
      if (event.type === "text_delta") {
        resultRef.current += event.content
        setResult(resultRef.current)
      }
      if (event.type === "error") {
        const message = event.message || "AI 请求失败"
        resultRef.current = message
        setResult(message)
      }
      if (event.type === "done" || event.type === "error") {
        setLoading(false)
        spotSessionRef.current = null
      }
    }).then((fn) => {
      unlistenRef.current = fn
      if (cancelled) {
        fn()
        return
      }
    })
    return () => {
      cancelled = true
      unlistenRef.current?.()
    }
  }, [])

  const handleSubmit = useCallback(async () => {
    const text = input.trim()
    if (!text || loading) return
    setLoading(true)
    setResult("")
    resultRef.current = ""
    try {
      const sid = `spot_${Date.now()}`
      spotSessionRef.current = sid
      await agentChat(text, sid, currentProjectId)
    } catch (error) {
      const message = `AI 请求失败：${String(error)}`
      resultRef.current = message
      setResult(message)
      setLoading(false)
      spotSessionRef.current = null
    }
  }, [currentProjectId, input, loading])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      handleSubmit()
    }
  }

  if (!visible) return null

  return (
    <div
      ref={overlayRef}
      role="dialog"
      tabIndex={-1}
      aria-modal="true"
      aria-label="全局搜索"
      className="fixed inset-0 z-[9999] flex items-start justify-center bg-black/30 pt-[15vh]"
      onClick={(e) => {
        if (e.target === overlayRef.current) setVisible(false)
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") setVisible(false)
      }}
    >
      <div className="w-full max-w-xl rounded-xl bg-white shadow-2xl border border-neutral-200 overflow-hidden">
        {/* 搜索栏 */}
        <div className="flex items-center gap-3 border-b border-neutral-100 px-4 py-3">
          <Search className="h-5 w-5 text-neutral-400 shrink-0" />
          <input
            ref={inputRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="向 AI 提问（搜索知识库、生成文档、分析风险）..."
            className="flex-1 text-sm text-neutral-700 placeholder-neutral-400 outline-none bg-transparent"
          />
          {loading ? (
            <Loader2 className="h-4 w-4 animate-spin text-amber-500 shrink-0" />
          ) : input.trim() ? (
            <button
              type="button"
              onClick={handleSubmit}
              className="p-1 text-amber-600 hover:text-amber-700"
            >
              <Send className="h-4 w-4" />
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => setVisible(false)}
            className="p-1 text-neutral-300 hover:text-neutral-500"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* 结果 */}
        {result && (
          <div className="max-h-60 overflow-y-auto px-4 py-3">
            <div className="text-xs leading-relaxed text-neutral-700 whitespace-pre-wrap">
              {result}
            </div>
          </div>
        )}

        {/* 提示 */}
        {!input && !result && (
          <div className="px-4 py-3 text-xs text-neutral-400 flex items-center gap-3">
            <span>Alt+Space 切换</span>
            <span>Enter 发送</span>
            <span>Esc 关闭</span>
          </div>
        )}
      </div>
    </div>
  )
}
