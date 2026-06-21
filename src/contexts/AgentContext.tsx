import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react"
import { formatAppError, parseAppError } from "@/lib/app-error"
import {
  copySlot,
  createEventBuffer,
  createEventHandlerMap,
  resolveSlotId,
} from "@/lib/event-buffer"
import {
  type AttachmentInfo,
  agentChat,
  answerQuestion,
  type ChatMessage,
  type ClarificationPayload,
  type PlanStep,
  cancelAgentStream,
  listenAgentEvents,
  rejectQuestion,
  runVerification,
} from "@/lib/tauri-commands"
import { useAppError } from "./AppErrorContext"

// ── 导出类型 ──────────────────────────────────────────────────────────

export interface RAGSource {
  title: string
  section_path?: string
  content_snippet?: string
  score: number
}

/** 验证报告（与 backend VerificationReport 对应） */
export interface VerificationReport {
  level: "Confirmed" | "NeedsReview" | "Suspected" | "Failed"
  overall_confidence: number
  checks: {
    check_name: string
    passed: boolean
    confidence: number
    detail: string
    evidence: string[]
  }[]
  suggested_labels: string[]
}

export interface AgentMessage {
  id: string
  role: "user" | "assistant"
  content: string
  streaming?: boolean
  /** 流式响应首包到达前展示的当前处理状态 */
  statusText?: string
  error?: boolean
  cancelled?: boolean
  hiddenContext?: string
  clarification?: ClarificationPayload
  clarificationAnswered?: boolean
  sources?: RAGSource[]
  /** 文件附件（用户发送的文件、Agent 生成的文件） */
  attachments?: FileAttachment[]
  /** 验证层报告 */
  verificationReport?: VerificationReport
}

/** 文件附件类型 */
export interface FileAttachment {
  id: string
  path: string
  name: string
  kind: "document" | "image" | "generated"
  size?: number
  mimeType?: string
}

export interface ReActTrace {
  thinking: string
  toolCalls: { name: string; args: string; result: string }[]
  plan: PlanStep[] | null
  currentStepIndex: number | null
  totalSteps: number
  stepResults: Record<number, { result: string; success: boolean }>
  replanReason: string | null
  plannerTimeoutMessage: string | null
}

export interface AgentSlot {
  messages: AgentMessage[]
  loading: boolean
  currentTrace: ReActTrace
  sessionId: string | null
}

export function createDefaultSlot(): AgentSlot {
  return {
    messages: [],
    loading: false,
    currentTrace: {
      thinking: "",
      toolCalls: [],
      plan: null,
      currentStepIndex: null,
      totalSteps: 0,
      stepResults: {},
      replanReason: null,
      plannerTimeoutMessage: null,
    },
    sessionId: null,
  }
}

/** @deprecated Use createDefaultSlot() instead to avoid shared mutable state */
export const DEFAULT_SLOT: AgentSlot = createDefaultSlot()

export interface SendMessageOptions {
  projectId?: number | null
  providerId?: string
  modelId?: string
  history?: ChatMessage[]
  /** 覆盖用户消息气泡展示文本，默认使用发送文本 */
  displayText?: string
  attachments?: AttachmentInfo[]
  /** 文件附件（用于在消息中显示） */
  fileAttachments?: FileAttachment[]
}

export interface AgentContextValue {
  /** Reactive map of all agent slots (keyed by slot ID). */
  slots: ReadonlyMap<string, AgentSlot>

  /** Send a message in a slot. Creates user + placeholder assistant messages and calls agentChat. */
  sendMessage: (slotId: string, text: string, options?: SendMessageOptions) => Promise<void>

  /** Answer a clarification question for a slot. */
  answerClarification: (slotId: string, questionId: string, answer: string) => Promise<void>

  /** Reject a clarification question for a slot. */
  rejectClarification: (slotId: string, questionId: string) => Promise<void>

  /** Cancel the active agent stream for a slot. */
  cancelSession: (slotId: string) => Promise<void>

  /** Clear all messages and reset a slot. */
  clearSlot: (slotId: string) => void

  /** Update messages in a slot using a functional updater. */
  updateMessages: (slotId: string, updater: (prev: AgentMessage[]) => AgentMessage[]) => void
}

// ── 辅助函数 ─────────────────────────────────────────────────────────────────

function nextId(): string {
  return crypto.randomUUID()
}

function summarizeToolResult(toolName: string, result: string): string {
  if (!result.trim()) return ""
  const important = result
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) =>
      /输出目录|沙箱目录|生成|文件|路径|output|sandbox|\.pptx|\.docx|\.xlsx|\.html|\.pdf/i.test(
        line,
      ),
    )
    .join("\n")
  const text = important || result
  const clipped = text.length > 2000 ? `${text.slice(0, 2000)}\n...[truncated]` : text
  return `工具 ${toolName} 结果摘要:\n${clipped}`
}

/** Try to extract RAG sources from a search-knowledge tool result */
function extractSourcesFromToolResult(toolName: string, result: string): RAGSource[] | undefined {
  const name = (toolName || "").toLowerCase().replace(/[-_\s]/g, "")
  if (
    !name.includes("searchknowledge") &&
    !name.includes("hybridsearch") &&
    !name.includes("knowledge")
  ) {
    return undefined
  }

  const sources: RAGSource[] = []

  // 解析格式: 【1】title (相关度: 0.xxx, 来源: xxx)\ncontent
  const regex = /【\d+】(.+?)\s*\(相关度:\s*([\d.]+),\s*来源:\s*(.+?)\)\n([\s\S]*?)(?=【\d+】|$)/g
  let match: RegExpExecArray | null = regex.exec(result)

  while (match !== null) {
    sources.push({
      title: match[1].trim(),
      section_path: match[3].trim(),
      content_snippet: match[4].trim().slice(0, 200),
      score: parseFloat(match[2]) || 0,
    })
    match = regex.exec(result)
  }

  // 如果上面的格式没匹配，尝试简单的格式
  if (sources.length === 0) {
    const simpleRegex = /【\d+】(.+?)\s*\(相关度:\s*([\d.]+)\)/g
    match = simpleRegex.exec(result)
    while (match !== null) {
      sources.push({
        title: match[1].trim(),
        score: parseFloat(match[2]) || 0,
      })
      match = simpleRegex.exec(result)
    }
  }

  return sources.length > 0 ? sources.slice(0, 8) : undefined
}

/** 从工具结果中提取生成的文件路径 */
function extractFilesFromToolResult(_toolName: string, result: string): FileAttachment[] {
  const files: FileAttachment[] = []
  const extPattern =
    /\.(pptx?|docx?|xlsx?|pdf|html?|md|txt|csv|png|jpg|jpeg|gif|svg|json|xml|yaml|zip|mp4|webm)\b/i

  // 匹配"输出/生成/文件: /path/to/file.ext" 模式
  const pathRegex =
    /(?:输出目录[:：\s]*|生成[:：\s]*|文件[:：\s]*|output[:：\s]*|sandbox[:：\s]*)(.+?\.\w+)/gi
  let match: RegExpExecArray | null = pathRegex.exec(result)
  while (match !== null) {
    const path = match[1].trim()
    if (extPattern.test(path)) {
      const name = path.split(/[\\/]/).pop() || path
      const isImage = /\.(png|jpg|jpeg|gif|svg|webp|bmp)$/i.test(path)
      files.push({
        id: crypto.randomUUID(),
        path,
        name,
        kind: isImage ? "image" : "generated",
      })
    }
    match = pathRegex.exec(result)
  }

  // 匹配 file:///path 或 /absolute/path 模式
  if (files.length === 0) {
    const absPathRegex = /(?:file:\/\/\/|output\s*(?:dir|directory)?[\s:]*)(\/[^\s,;]+\.\w+)/gi
    match = absPathRegex.exec(result)
    while (match !== null) {
      const path = match[1]
      const name = path.split(/[\\/]/).pop() || path
      const isImage = /\.(png|jpg|jpeg|gif|svg|webp|bmp)$/i.test(path)
      if (!files.some((f) => f.path === path)) {
        files.push({
          id: crypto.randomUUID(),
          path,
          name,
          kind: isImage ? "image" : "generated",
        })
      }
      match = absPathRegex.exec(result)
    }
  }

  return files.slice(0, 5)
}

/** Build conversation history for multi-turn agent context. */
export function buildAgentHistory(messages: AgentMessage[]): ChatMessage[] {
  return messages
    .filter((m) => !m.streaming && !m.error && !m.cancelled && !m.clarification)
    .map((m) => {
      const hidden = m.hiddenContext ? `\n\n【上一轮工具上下文】\n${m.hiddenContext}` : ""
      return { role: m.role, content: `${m.content}${hidden}`.trim() }
    })
    .filter((m) => m.content.length > 0)
    .slice(-12)
}

// ── 上下文 ─────────────────────────────────────────────────────────────────

const AgentContext = createContext<AgentContextValue | null>(null)

export function useAgent(): AgentContextValue {
  const ctx = useContext(AgentContext)
  if (!ctx) throw new Error("useAgent must be used within AgentProvider")
  return ctx
}

/**
 * Apply buffered text/thinking entries to slot state.
 * Called by the rAF flush when non-streaming events arrive.
 */
function applyBufferToSlots(
  entries: Map<string, { text: string; thinking: string }>,
  cancelledSlots: { current: Set<string> },
  updateSlots: (updater: (prev: Map<string, SlotInternal>) => Map<string, SlotInternal>) => void,
  nextId: () => string,
) {
  if (entries.size === 0) return

  updateSlots((prev) => {
    const next = new Map(prev)
    for (const [sid, buf] of entries) {
      if (cancelledSlots.current.has(sid)) continue
      const internal = next.get(sid)
      if (!internal) continue

      const slot: AgentSlot = {
        ...internal.slot,
        currentTrace: { ...internal.slot.currentTrace },
        messages: [...internal.slot.messages],
      }

      if (buf.text) {
        const last = slot.messages[slot.messages.length - 1]
        if (last && last.role === "assistant" && last.streaming) {
          slot.messages[slot.messages.length - 1] = {
            ...last,
            content: last.content + buf.text,
            statusText: undefined,
          }
        } else {
          slot.messages = [
            ...slot.messages,
            { id: nextId(), role: "assistant", content: buf.text, streaming: true },
          ]
        }
      }

      if (buf.thinking) {
        slot.currentTrace = {
          ...slot.currentTrace,
          thinking: slot.currentTrace.thinking + buf.thinking,
        }
        const last = slot.messages[slot.messages.length - 1]
        if (last && last.role === "assistant" && last.streaming && !last.content) {
          slot.messages[slot.messages.length - 1] = {
            ...last,
            statusText: "正在思考并组织回答...",
          }
        }
      }

      next.set(sid, { slot, latestToolName: internal.latestToolName })
    }
    return next
  })
}

// ── 内部类型 ──────────────────────────────────────────────────────────

interface SlotInternal {
  slot: AgentSlot
  latestToolName: string
}

/** 单个 slot 最多保留的消息数，超限时丢弃最早的非活跃消息 */
const MAX_MESSAGES_PER_SLOT = 200

/** 超出上限时裁剪最早的消息，保留最近的 N 条 */
function trimMessages(msgs: AgentMessage[]): AgentMessage[] {
  if (msgs.length <= MAX_MESSAGES_PER_SLOT) return msgs
  const overflow = msgs.length - MAX_MESSAGES_PER_SLOT
  return msgs.slice(overflow)
}

function formatClarificationAnswer(answer: string): string {
  try {
    const parsed = JSON.parse(answer) as unknown
    if (
      Array.isArray(parsed) &&
      parsed.every(
        (items) => Array.isArray(items) && items.every((item) => typeof item === "string"),
      )
    ) {
      return parsed
        .map((items, index) => {
          const text = items.length > 0 ? items.join("、") : "未回答"
          return parsed.length > 1 ? `${index + 1}. ${text}` : text
        })
        .join("\n")
    }
  } catch {
    // 非 JSON 回答直接展示原文
  }
  return answer
}

// ── 提供者 ────────────────────────────────────────────────────────────────

export function AgentProvider({ children }: { children: ReactNode }) {
  const [slots, setSlots] = useState<Map<string, SlotInternal>>(new Map())
  const sessionToSlot = useRef<Map<string, string>>(new Map())
  const cancelledSlots = useRef<Set<string>>(new Set())
  const latestSlots = useRef(slots)
  latestSlots.current = slots
  // 错误事件 → LLM API Key 对话框的桥接
  // 用 ref 持有最新回调，避免在 dialog 开/关时重订阅 listenAgentEvents
  const { showLlmKeyError } = useAppError()
  const showLlmKeyErrorRef = useRef(showLlmKeyError)
  showLlmKeyErrorRef.current = showLlmKeyError

  // 更新 slots 辅助函数（同步保持 ref）
  const updateSlots = useCallback(
    (updater: (prev: Map<string, SlotInternal>) => Map<string, SlotInternal>) => {
      setSlots((prev) => {
        const next = updater(prev)
        latestSlots.current = next
        return next
      })
    },
    [],
  )

  // ── 全局 ReAct 事件监听 ─────────────────────────────────────────
  useEffect(() => {
    let unsub: (() => void) | null = null
    let cancelled = false

    // Create the rAF-based event buffer and handler map (once per effect)
    const eventBuf = createEventBuffer()
    const handlerMap = createEventHandlerMap({
      nextId,
      extractSources: extractSourcesFromToolResult,
      summarizeTool: summarizeToolResult,
      extractFiles: extractFilesFromToolResult,
      showLlmKeyErrorRef,
    })

    listenAgentEvents((event) => {
      // Early-return guard: resolve slot id and check cancellation
      const slotId = resolveSlotId(event, sessionToSlot, cancelledSlots)
      if (!slotId) return

      // Streaming events → accumulate in buffer, schedule rAF flush
      if (event.type === "text_delta") {
        const buf = eventBuf.buffer.get(slotId) ?? { text: "", thinking: "" }
        buf.text += event.content
        eventBuf.buffer.set(slotId, buf)
        eventBuf.schedule()
        return
      }

      if (event.type === "thinking") {
        const buf = eventBuf.buffer.get(slotId) ?? { text: "", thinking: "" }
        buf.thinking += event.content
        eventBuf.buffer.set(slotId, buf)
        eventBuf.schedule()
        return
      }

      // Non-streaming → flush buffer first, then dispatch
      if (eventBuf.rafScheduled) {
        eventBuf.flush((entries) => {
          applyBufferToSlots(entries, cancelledSlots, updateSlots, nextId)
        })
      }

      // Dispatch through typed handler map

      updateSlots((prev) => {
        const next = new Map(prev)
        const internal = next.get(slotId)
        if (!internal) return prev

        const slot = copySlot(internal.slot)
        const result = handlerMap(slot, event, internal.latestToolName)
        next.set(slotId, { slot: result.slot, latestToolName: result.toolName })
        return next
      })

      // done → clean up session mapping + kick off verification
      if (event.type === "done") {
        const eid = event.session_id || event.sessionId || ""
        sessionToSlot.current.delete(eid)

        // Verification: run on the last assistant message
        updateSlots((prev) => {
          const int = prev.get(slotId)
          if (!int) return prev
          const lastMsg = int.slot.messages[int.slot.messages.length - 1]
          if (lastMsg && lastMsg.role === "assistant" && lastMsg.content) {
            runVerification(lastMsg.content, "chat", eid)
              .then((res) => {
                updateSlots((p) => {
                  const n = new Map(p)
                  const ii = n.get(slotId)
                  if (!ii) return p
                  n.set(slotId, {
                    ...ii,
                    slot: {
                      ...ii.slot,
                      messages: ii.slot.messages.map((m) =>
                        m.id === lastMsg.id ? { ...m, verificationReport: res.report } : m,
                      ),
                    },
                  })
                  return n
                })
              })
              .catch(() => {})
          }
          return prev
        })
      }

      // error → clean up session mapping
      if (event.type === "error") {
        sessionToSlot.current.delete(event.session_id || event.sessionId || "")
        // Also set sessionId to null on the slot
        updateSlots((prev) => {
          const next = new Map(prev)
          const internal = next.get(slotId)
          if (!internal) return prev
          next.set(slotId, {
            ...internal,
            slot: { ...internal.slot, sessionId: null },
          })
          return next
        })
      }

      // clarification → clean up session mapping
      if (event.type === "clarification") {
        sessionToSlot.current.delete(event.session_id || event.sessionId || "")
      }
    }).then((unsubFn) => {
      if (cancelled) {
        unsubFn()
      } else {
        unsub = unsubFn
      }
    })
    return () => {
      cancelled = true
      eventBuf.dispose()
      unsub?.()
    }
  }, [updateSlots])

  // ── 操作 ────────────────────────────────────────────────────────────

  const sendMessage = useCallback(
    async (slotId: string, text: string, options?: SendMessageOptions) => {
      cancelledSlots.current.delete(slotId)
      const sid = nextId()
      const displayText = options?.displayText ?? text
      const userMsg: AgentMessage = {
        id: nextId(),
        role: "user",
        content: displayText,
        attachments: options?.fileAttachments,
      }
      const assistantMsg: AgentMessage = {
        id: nextId(),
        role: "assistant",
        content: "",
        streaming: true,
        statusText: "正在理解你的问题...",
      }

      sessionToSlot.current.set(sid, slotId)

      updateSlots((prev) => {
        const next = new Map(prev)
        const internal = next.get(slotId) ?? { slot: createDefaultSlot(), latestToolName: "" }
        next.set(slotId, {
          slot: {
            messages: trimMessages([...internal.slot.messages, userMsg, assistantMsg]),
            loading: true,
            currentTrace: {
              thinking: "",
              toolCalls: [],
              plan: null,
              currentStepIndex: null,
              totalSteps: 0,
              stepResults: {},
              replanReason: null,
              plannerTimeoutMessage: null,
            },
            sessionId: sid,
          },
          latestToolName: "",
        })
        return next
      })

      try {
        await agentChat(
          text,
          sid,
          options?.projectId,
          options?.history,
          options?.providerId,
          options?.modelId,
          options?.attachments,
        )
      } catch (err) {
        // agent_chat 在 reject 时可能直接抛出结构化 LLM_INVALID_KEY 错误
        // （例如 provider_id 错误 / 模型未配置），需要走到 AppErrorProvider
        const parsed = parseAppError(err)
        if (parsed?.code === "LLM_INVALID_KEY") {
          showLlmKeyErrorRef.current(parsed)
        }
        const errorMsg = formatAppError(err, "请求失败")
        updateSlots((prev) => {
          const next = new Map(prev)
          const internal = next.get(slotId)
          if (!internal) return prev
          const msgs = internal.slot.messages.map((m) =>
            m.id === assistantMsg.id
              ? {
                  ...m,
                  content: `请求失败：${errorMsg}`,
                  streaming: false,
                  statusText: undefined,
                  error: true,
                }
              : m,
          )
          next.set(slotId, {
            ...internal,
            slot: { ...internal.slot, messages: msgs, loading: false },
          })
          return next
        })
      }
    },
    [updateSlots],
  )

  const answerClarification = useCallback(
    async (slotId: string, questionId: string, answer: string) => {
      // 1. 标记为已回答
      updateSlots((prev) => {
        const next = new Map(prev)
        const internal = next.get(slotId)
        if (!internal) return prev
        const msgs = internal.slot.messages.map((m) =>
          m.clarification?.question_id === questionId ? { ...m, clarificationAnswered: true } : m,
        )
        next.set(slotId, { ...internal, slot: { ...internal.slot, messages: msgs } })
        return next
      })

      // 2. 添加答案 + 占位消息
      const answerMsg: AgentMessage = {
        id: nextId(),
        role: "user",
        content: formatClarificationAnswer(answer),
      }
      const assistantMsg: AgentMessage = {
        id: nextId(),
        role: "assistant",
        content: "",
        streaming: true,
        statusText: "正在处理你的补充信息...",
      }

      updateSlots((prev) => {
        const next = new Map(prev)
        const internal = next.get(slotId)
        if (!internal) return prev
        next.set(slotId, {
          ...internal,
          slot: {
            ...internal.slot,
            messages: trimMessages([...internal.slot.messages, answerMsg, assistantMsg]),
            loading: true,
            currentTrace: {
              thinking: "",
              toolCalls: [],
              plan: null,
              currentStepIndex: null,
              totalSteps: 0,
              stepResults: {},
              replanReason: null,
              plannerTimeoutMessage: null,
            },
          },
        })
        return next
      })

      // 3. 发送到后端
      try {
        const sessionId = latestSlots.current.get(slotId)?.slot.sessionId ?? null
        await answerQuestion(questionId, answer, sessionId)
      } catch (err) {
        console.error("[AgentContext] Failed to answer question:", err)
        updateSlots((prev) => {
          const next = new Map(prev)
          const internal = next.get(slotId)
          if (!internal) return prev
          next.set(slotId, { ...internal, slot: { ...internal.slot, loading: false } })
          return next
        })
      }
    },
    [updateSlots],
  )

  const rejectClarification = useCallback(
    async (slotId: string, questionId: string) => {
      updateSlots((prev) => {
        const next = new Map(prev)
        const internal = next.get(slotId)
        if (!internal) return prev
        const msgs = internal.slot.messages.map((m) =>
          m.clarification?.question_id === questionId ? { ...m, clarificationAnswered: true } : m,
        )
        const answerMsg: AgentMessage = {
          id: nextId(),
          role: "user",
          content: "已取消回答该澄清问题。",
        }
        const assistantMsg: AgentMessage = {
          id: nextId(),
          role: "assistant",
          content: "",
          streaming: true,
          statusText: "正在继续处理...",
        }
        next.set(slotId, {
          ...internal,
          slot: {
            ...internal.slot,
            messages: trimMessages([...msgs, answerMsg, assistantMsg]),
            loading: true,
            currentTrace: {
              thinking: "",
              toolCalls: [],
              plan: null,
              currentStepIndex: null,
              totalSteps: 0,
              stepResults: {},
              replanReason: null,
              plannerTimeoutMessage: null,
            },
          },
        })
        return next
      })

      try {
        const sessionId = latestSlots.current.get(slotId)?.slot.sessionId ?? null
        await rejectQuestion(questionId, sessionId)
      } catch (err) {
        console.error("[AgentContext] Failed to reject question:", err)
        updateSlots((prev) => {
          const next = new Map(prev)
          const internal = next.get(slotId)
          if (!internal) return prev
          next.set(slotId, { ...internal, slot: { ...internal.slot, loading: false } })
          return next
        })
      }
    },
    [updateSlots],
  )

  const cancelSession = useCallback(
    async (slotId: string) => {
      const internal = latestSlots.current.get(slotId)
      if (!internal?.slot.sessionId) return
      const sessionId = internal.slot.sessionId
      cancelledSlots.current.add(slotId)
      sessionToSlot.current.delete(sessionId)
      updateSlots((prev) => {
        const next = new Map(prev)
        const current = next.get(slotId)
        if (!current) return prev
        next.set(slotId, {
          ...current,
          slot: {
            ...current.slot,
            loading: false,
            sessionId: null,
            messages: current.slot.messages.map((m) =>
              m.streaming
                ? {
                    ...m,
                    content: m.content || "已取消生成。",
                    streaming: false,
                    statusText: undefined,
                    cancelled: true,
                  }
                : m,
            ),
            currentTrace: {
              thinking: "",
              toolCalls: [],
              plan: null,
              currentStepIndex: null,
              totalSteps: 0,
              stepResults: {},
              replanReason: null,
              plannerTimeoutMessage: null,
            },
          },
        })
        return next
      })
      try {
        await cancelAgentStream(sessionId)
      } catch (err) {
        console.warn("[AgentContext] Failed to cancel:", err)
      }
    },
    [updateSlots],
  )

  const clearSlot = useCallback(
    (slotId: string) => {
      cancelledSlots.current.delete(slotId)
      updateSlots((prev) => {
        const next = new Map(prev)
        const internal = next.get(slotId)
        if (internal?.slot.sessionId) {
          sessionToSlot.current.delete(internal.slot.sessionId)
        }
        next.delete(slotId)
        return next
      })
    },
    [updateSlots],
  )

  const updateMessages = useCallback(
    (slotId: string, updater: (prev: AgentMessage[]) => AgentMessage[]) => {
      updateSlots((prev) => {
        const next = new Map(prev)
        const internal = next.get(slotId) ?? { slot: createDefaultSlot(), latestToolName: "" }
        next.set(slotId, {
          ...internal,
          slot: { ...internal.slot, messages: updater(internal.slot.messages) },
        })
        return next
      })
    },
    [updateSlots],
  )

  // ── 上下文值 ──────────────────────────────────────────────────────

  // 从内部状态派生出响应式的 AgentSlot 只读映射 Map
  const reactiveSlots: ReadonlyMap<string, AgentSlot> = new Map(
    Array.from(slots.entries()).map(([k, v]) => [k, v.slot]),
  )

  const ctx: AgentContextValue = {
    slots: reactiveSlots,
    sendMessage,
    answerClarification,
    rejectClarification,
    cancelSession,
    clearSlot,
    updateMessages,
  }

  return <AgentContext.Provider value={ctx}>{children}</AgentContext.Provider>
}
