import { convertFileSrc, invoke } from "@tauri-apps/api/core"
import { open, save } from "@tauri-apps/plugin-dialog"
import { openPath } from "@tauri-apps/plugin-opener"
import {
  AlertCircle,
  BookOpen,
  Brain,
  ChevronDown,
  ChevronUp,
  Copy,
  Download,
  ExternalLink,
  File,
  FileArchive,
  FileCode,
  FileSpreadsheet,
  FileText,
  Image as ImageIcon,
  Loader2,
  Paperclip,
  RefreshCw,
  Send,
  Settings,
  StopCircle,
  Trash2,
  X,
  Zap,
} from "lucide-react"
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react"
import ReactMarkdown from "react-markdown"
import { useNavigate } from "react-router-dom"
import remarkGfm from "remark-gfm"
import { VerificationBadge } from "../components/VerificationBadge"
import {
  type AgentMessage,
  buildAgentHistory,
  DEFAULT_SLOT,
  type FileAttachment,
  type RAGSource,
  type ReActTrace,
  useAgent,
} from "../contexts/AgentContext"
import { useProject } from "../contexts/ProjectContext"
import { extractFilesFromDropEvent, extractFilesFromPasteEvent } from "../lib/clipboard-files"
import { listLLMProviders } from "../lib/skill-commands"
import type { LLMProviderConfig } from "../lib/skill-types"
import {
  type ClarificationPayload,
  type ClarificationQuestion,
  countTokens,
  isLLMConfigured,
  type QuestionOption,
  saveChatMemory,
} from "../lib/tauri-commands"

interface ChatAttachment {
  id: string
  path: string
  name: string
  kind: "document" | "image" | "unsupported"
  status: "ready" | "ingesting" | "parsed" | "ingested" | "error"
  documentId?: number
  extractedText?: string
  charCount?: number
  error?: string
  /** 预览用 data URL（临时文件可能无法通过 convertFileSrc 访问） */
  previewUrl?: string
}

function nextId(): string {
  return crypto.randomUUID()
}

const CHAT_STORAGE_KEY = "kingdee_kb_chat_history"
const MAX_STORED_MESSAGES = 100

const DOCUMENT_EXTENSIONS = new Set([
  "md",
  "txt",
  "text",
  "markdown",
  "html",
  "htm",
  "pdf",
  "doc",
  "docx",
  "xlsx",
  "xls",
])
const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "webp", "bmp", "gif"])

function loadChatHistory(storageKey: string): AgentMessage[] {
  try {
    const raw = localStorage.getItem(storageKey)
    if (!raw) return []
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return (parsed as AgentMessage[]).filter((m) => m.id && m.role)
  } catch {
    return []
  }
}

function saveChatHistory(storageKey: string, messages: AgentMessage[]) {
  try {
    const clean = messages.map((m) => ({ ...m, streaming: false }))
    const trimmed =
      clean.length > MAX_STORED_MESSAGES ? clean.slice(clean.length - MAX_STORED_MESSAGES) : clean
    localStorage.setItem(storageKey, JSON.stringify(trimmed))
  } catch {
    // 忽略
  }
}

function summarizeToolArgs(args: string): string {
  if (!args.trim()) return ""
  try {
    const parsed = JSON.parse(args) as Record<string, unknown>
    const parts: string[] = []
    for (const key of [
      "skill_name",
      "script",
      "action",
      "name_or_query",
      "template_id",
      "project_name",
    ]) {
      const value = parsed[key]
      if (typeof value === "string") parts.push(`${key}: ${value}`)
    }
    if (Array.isArray(parsed.args)) parts.push(`args: ${parsed.args.length} 项`)
    if (Array.isArray(parsed.input_files))
      parts.push(`input_files: ${parsed.input_files.length} 个文件`)
    return parts.join("\n") || `参数 ${args.length} 字符`
  } catch {
    return args.length > 240 ? `参数 ${args.length} 字符` : args
  }
}

function PlanTimeline({ trace }: { trace: ReActTrace }) {
  if (!trace.plan || trace.plan.length === 0) return null

  return (
    <div className="plan-timeline space-y-1 text-sm text-gray-600 mb-2">
      <div className="font-medium text-gray-700 mb-1 flex items-center gap-1">
        📋 执行计划 (
        {trace.currentStepIndex !== null
          ? Math.min(trace.currentStepIndex + 1, trace.plan.length)
          : 0}
        /{trace.plan.length})
      </div>
      {trace.plan.map((step, i) => {
        const result = trace.stepResults[i]
        const isCurrent = trace.currentStepIndex === i
        const isDone = result !== undefined
        const isFailed = result && !result.success

        return (
          <div
            key={step.id}
            className={`flex items-start gap-2 py-1 px-2 rounded ${
              isCurrent
                ? "bg-blue-50 border-l-2 border-blue-400"
                : isFailed
                  ? "bg-red-50 border-l-2 border-red-300"
                  : isDone
                    ? "bg-green-50 border-l-2 border-green-300"
                    : "bg-gray-50"
            }`}
          >
            <span className="flex-shrink-0 mt-0.5">
              {isFailed ? "❌" : isDone ? "✅" : isCurrent ? "🔄" : "⬜"}
            </span>
            <div className="flex-1 min-w-0">
              <div className={`truncate ${isCurrent ? "font-medium text-blue-700" : ""}`}>
                {step.id}. {step.description}
              </div>
              {isCurrent && step.tool && (
                <div className="text-xs text-gray-400">工具: {step.tool}</div>
              )}
              {result && (
                <div
                  className={`text-xs mt-1 ${result.success ? "text-green-600" : "text-red-500"}`}
                >
                  {result.success ? "完成" : result.result.slice(0, 100)}
                </div>
              )}
            </div>
          </div>
        )
      })}
      {trace.plannerTimeoutMessage && (
        <div className="text-xs text-amber-600 mt-2 p-2 bg-amber-50 rounded">
          ⚠️ {trace.plannerTimeoutMessage}
        </div>
      )}
      {trace.replanReason && (
        <div className="text-xs text-blue-600 mt-1 p-2 bg-blue-50 rounded">
          🔄 {trace.replanReason}
        </div>
      )}
    </div>
  )
}

export default function Chat() {
  const { currentProjectId } = useProject()
  const chatStorageKey = `${CHAT_STORAGE_KEY}:${currentProjectId ?? "none"}`
  const agent = useAgent()
  const navigate = useNavigate()
  const slot = agent.slots.get("chat") ?? DEFAULT_SLOT
  const { messages, loading, currentTrace } = slot

  const [input, setInput] = useState("")
  const [attachments, setAttachments] = useState<ChatAttachment[]>([])
  const attaching = false
  const [llmReady, setLlmReady] = useState<boolean | null>(null)
  const [providers, setProviders] = useState<LLMProviderConfig[]>([])
  const [selectedProviderId, setSelectedProviderId] = useState<string>("")
  const [selectedModelId, setSelectedModelId] = useState<string>("")
  const [dropdownOpen, setDropdownOpen] = useState(false)
  const [isDragging, setIsDragging] = useState(false)
  const [tokenUsage, setTokenUsage] = useState<{ used: number; total: number } | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const dropdownRef = useRef<HTMLDivElement>(null)
  const lastInputRef = useRef<{ text: string; attachments: ChatAttachment[] } | null>(null)

  // 是否处于自动滚动跟随状态
  const isAutoScrollingRef = useRef(true)

  // 处理滚动事件，判断用户是否手动往上滚动
  const handleScroll = useCallback(() => {
    const el = scrollRef.current
    if (!el) return

    // 计算距离底部的像素数
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight

    // 如果距离底部小于等于 100px，认为用户重新回到了底部，开启自动跟随
    // 否则认为用户在手动翻看历史，关闭自动跟随
    if (distanceFromBottom <= 100) {
      isAutoScrollingRef.current = true
    } else {
      isAutoScrollingRef.current = false
    }
  }, [])

  // 从 localStorage 加载当前项目的聊天历史到 slot
  const loadedStorageKeyRef = useRef<string | null>(null)
  useEffect(() => {
    if (loadedStorageKeyRef.current === chatStorageKey) return
    loadedStorageKeyRef.current = chatStorageKey
    const history = loadChatHistory(chatStorageKey)
    agent.updateMessages("chat", () => history)
  }, [agent, chatStorageKey])

  // 挂载时检查 LLM 配置
  useEffect(() => {
    isLLMConfigured()
      .then(setLlmReady)
      .catch(() => setLlmReady(false))
  }, [])

  // 挂载时加载 LLM 供应商
  useEffect(() => {
    listLLMProviders()
      .then((fetchedProviders) => {
        const safeProviders = Array.isArray(fetchedProviders) ? fetchedProviders : []
        setProviders(safeProviders)
        // 预选默认供应商和模型
        const defaultProvider = safeProviders.find((p) => p.is_default) || safeProviders[0]
        if (defaultProvider) {
          setSelectedProviderId(defaultProvider.id)
          const defaultModel =
            defaultProvider.models.find((m) => m.is_default) || defaultProvider.models[0]
          if (defaultModel) {
            setSelectedModelId(defaultModel.id)
          }
        }
      })
      .catch((err) => {
        console.warn("[Chat] Failed to load LLM providers:", err)
      })
  }, [])

  // 首次加载且消息不为空时，无条件强制置底一次；当清空消息时重置状态
  const isInitialScrollRef = useRef(true)
  useEffect(() => {
    if (messages.length === 0) {
      isInitialScrollRef.current = true
      return
    }

    const el = scrollRef.current
    if (!el) return

    if (isInitialScrollRef.current) {
      isInitialScrollRef.current = false
      isAutoScrollingRef.current = true
      el.scrollTop = el.scrollHeight
    }
  }, [messages.length])

  // 双重保险：当消息列表或推理轨迹变化时，如果处于自动滚动状态，则执行滚动置底
  useEffect(() => {
    const el = scrollRef.current
    if (!el) return

    if (isAutoScrollingRef.current) {
      el.scrollTop = el.scrollHeight
    }
  })

  // 使用 ResizeObserver 监听聊天内容容器的尺寸变化
  // 只要内容高度改变（如图片加载完成、Markdown 渲染展开等）且处于自动滚动状态，就强制置底
  useEffect(() => {
    const el = scrollRef.current
    if (!el) return

    const resizeObserver = new ResizeObserver(() => {
      if (isAutoScrollingRef.current) {
        el.scrollTop = el.scrollHeight
      }
    })

    // 监听包裹消息列表的子元素 div
    const child = el.firstElementChild
    if (child) {
      resizeObserver.observe(child)
    }

    return () => {
      resizeObserver.disconnect()
    }
  }, [])

  // 更新 token 计数（禁止在 streaming 中运行，2 秒节流）
  const countTokensRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(() => {
    if (loading || messages.length === 0) {
      if (messages.length === 0) setTokenUsage(null)
      if (countTokensRef.current) {
        clearTimeout(countTokensRef.current)
        countTokensRef.current = null
      }
      return
    }
    if (countTokensRef.current) return // 已有挂起的节流
    countTokensRef.current = setTimeout(() => {
      countTokensRef.current = null
      const allText = messages.map((m) => m.content || "").join("\n")
      countTokens(allText)
        .then((count) => {
          setTokenUsage({ used: count, total: 128000 })
        })
        .catch(() => {})
    }, 2000)
    return () => {
      if (countTokensRef.current) {
        clearTimeout(countTokensRef.current)
        countTokensRef.current = null
      }
    }
  }, [messages, loading])

  const handleSend = useCallback(async () => {
    const text = input.trim()
    if ((!text && attachments.length === 0) || loading || attaching) return

    // 发送前检查 LLM 是否已配置
    if (llmReady === false) {
      alert("尚未配置 AI 模型，请前往【设置 → AI 模型】添加 LLM 供应商")
      return
    }

    // 保留输入以便重试时恢复
    lastInputRef.current = { text: input, attachments: [...attachments] }

    const outboundText = text || "请分析附件"
    const visibleText = [text || "请分析附件", buildAttachmentDisplay(attachments)]
      .filter(Boolean)
      .join("\n\n")

    const attachmentInfos = attachments.map((a) => ({
      name: a.name,
      path: a.path,
      kind: a.kind,
    }))

    // 转为 FileAttachment 在消息气泡中显示
    const fileAttachments: FileAttachment[] = attachments.map((a) => ({
      id: a.id,
      path: a.path,
      name: a.name,
      kind: a.kind === "unsupported" ? "document" : a.kind,
    }))

    setInput("")
    setAttachments([])

    // 发送消息后，强制开启自动跟随，并立即执行一次滚动置底
    isAutoScrollingRef.current = true
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }

    const history = buildAgentHistory(messages)
    await agent.sendMessage("chat", outboundText, {
      displayText: visibleText,
      history,
      projectId: currentProjectId,
      providerId: selectedProviderId || undefined,
      modelId: selectedModelId || undefined,
      attachments: attachmentInfos,
      fileAttachments,
    })
  }, [
    input,
    attachments,
    loading,
    messages,
    agent,
    currentProjectId,
    selectedProviderId,
    selectedModelId,
    llmReady,
  ])

  // 重试最后失败的消息
  const handleRetry = useCallback(async () => {
    if (loading || !lastInputRef.current) return
    const { text, attachments: prevAttachments } = lastInputRef.current
    setInput(text)
    setAttachments(prevAttachments)
    // 短延迟等待状态更新，再触发发送
    setTimeout(() => {
      inputRef.current?.focus()
    }, 50)
  }, [loading])

  // 导航到设置页面
  const handleNavigateSettings = useCallback(
    (section?: string) => {
      navigate(section ? `/settings?section=${section}` : "/settings")
    },
    [navigate],
  )

  const handleAttach = useCallback(async () => {
    if (loading || attaching) return
    try {
      const selected = await open({
        multiple: true,
        directory: false,
        filters: [
          {
            name: "文档和图片",
            extensions: [
              "md",
              "txt",
              "pdf",
              "doc",
              "docx",
              "xlsx",
              "xls",
              "html",
              "htm",
              "png",
              "jpg",
              "jpeg",
              "webp",
              "bmp",
              "gif",
            ],
          },
        ],
      })
      if (!selected) return

      const paths = Array.isArray(selected) ? selected : [selected]
      const next = paths.map(createAttachment)
      setAttachments((prev) => [...prev, ...next])
    } catch (err) {
      agent.updateMessages("chat", (prev) => [
        ...prev,
        {
          id: nextId(),
          role: "assistant",
          content: `附件选择失败：${String(err)}`,
          error: true,
        },
      ])
    }
  }, [loading, agent])

  const removeAttachment = useCallback((id: string) => {
    setAttachments((prev) => prev.filter((a) => a.id !== id))
  }, [])

  const addFilesAsAttachments = useCallback(
    (files: import("../lib/clipboard-files").PastedFile[]) => {
      const newAttachments = files.map((f) => {
        const att = createAttachment(f.path)
        return f.previewUrl ? { ...att, previewUrl: f.previewUrl } : att
      })
      setAttachments((prev) => [...prev, ...newAttachments])
    },
    [],
  )

  const handlePaste = useCallback(
    async (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const files = await extractFilesFromPasteEvent(e.nativeEvent)
      if (files.length === 0) return
      e.preventDefault()
      addFilesAsAttachments(files)
    },
    [addFilesAsAttachments],
  )

  const dragCounterRef = useRef(0)
  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
  }, [])

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    dragCounterRef.current++
    setIsDragging(true)
  }, [])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    dragCounterRef.current--
    if (dragCounterRef.current === 0) {
      setIsDragging(false)
    }
  }, [])

  const handleDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault()
      e.stopPropagation()
      setIsDragging(false)
      dragCounterRef.current = 0

      const files = await extractFilesFromDropEvent(e.nativeEvent)
      if (files.length === 0) return
      addFilesAsAttachments(files)
    },
    [addFilesAsAttachments],
  )

  // 取消正在运行的代理流
  const handleCancel = useCallback(async () => {
    await agent.cancelSession("chat")
  }, [agent])

  // 回答待处理的澄清问题
  const handleClarify = useCallback(
    async (questionId: string, answer: string) => {
      await agent.answerClarification("chat", questionId, answer)
    },
    [agent],
  )

  // 取消待处理的澄清问题
  const handleRejectClarification = useCallback(
    async (questionId: string) => {
      await agent.rejectClarification("chat", questionId)
    },
    [agent],
  )

  // 仅在流完成后保存到本地存储
  const prevLoadingRef = useRef(loading)
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(() => {
    if (!prevLoadingRef.current && loading) {
      // 流开始 → 清除待处理的保存
      if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current)
    }
    if (prevLoadingRef.current && !loading) {
      // 流完成 → 延迟 500ms 落盘，避免频繁写入本地存储
      if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current)
      saveTimeoutRef.current = setTimeout(() => saveChatHistory(chatStorageKey, messages), 500)
    }
    prevLoadingRef.current = loading
    return () => {
      if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current)
    }
  }, [loading, messages, chatStorageKey])

  // 会话完成后保存聊天记忆 (loading transitions true → false)
  const prevLoadingRef2 = useRef(loading)
  useEffect(() => {
    if (prevLoadingRef2.current && !loading && messages.length > 0) {
      const lastMsg = messages[messages.length - 1]
      if (lastMsg.role === "assistant" && !lastMsg.error && !lastMsg.cancelled) {
        const conversation = messages
          .filter((m) => !m.error && !m.cancelled)
          .map((m) => ({ role: m.role, content: m.content }))
        saveChatMemory(conversation, currentProjectId).catch((e) =>
          console.warn("[Chat] Failed to save chat memory:", e),
        )
      }
    }
    prevLoadingRef2.current = loading
  }, [currentProjectId, loading, messages])

  const handleClear = useCallback(() => {
    agent.clearSlot("chat")
    localStorage.removeItem(chatStorageKey)
    isAutoScrollingRef.current = true
  }, [agent, chatStorageKey])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault()
        handleSend()
      }
    },
    [handleSend],
  )

  const selectedProvider = providers.find((p) => p.id === selectedProviderId)
  const selectedModel = selectedProvider?.models.find((m) => m.id === selectedModelId)

  // 构建所有供应商+模型组合的平面列表供下拉选择
  const modelOptions = providers.flatMap((p) =>
    p.models.map((m) => ({
      providerId: p.id,
      providerName: p.name,
      modelId: m.id,
      modelName: m.name,
      isMultimodal: m.is_multimodal,
      isDefault: p.is_default && m.is_default,
    })),
  )

  return (
    <div className="flex h-full flex-col">
      {/* 页头 */}
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
        <div className="flex items-center gap-2">
          <Brain className="h-5 w-5 text-amber-600" />
          <h1 className="text-base font-semibold text-neutral-800">AI 助手</h1>
          <span className="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] font-medium text-amber-700">
            Agent
          </span>
          <span className="text-xs text-neutral-400">
            {messages.filter((m) => m.role === "user").length} 轮对话
          </span>
          {tokenUsage && (
            <span className="ml-2 inline-flex items-center gap-1 rounded-full bg-neutral-100 px-2 py-0.5 text-[10px] text-neutral-500">
              {(tokenUsage.used / 1000).toFixed(1)}K / {(tokenUsage.total / 1000).toFixed(0)}K
              tokens
            </span>
          )}
        </div>
        <button
          type="button"
          onClick={handleClear}
          className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium text-neutral-500 hover:bg-neutral-100 transition-colors"
        >
          <Trash2 className="h-3.5 w-3.5" />
          清空对话
        </button>
      </div>

      {/* 消息列表 */}
      <div ref={scrollRef} onScroll={handleScroll} className="flex-1 overflow-y-auto px-6 py-4">
        <div className="space-y-4">
          {messages.length === 0 && !loading ? (
            <div className="flex flex-col items-center justify-center pt-20 text-center">
              <div className="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-amber-50">
                <Brain className="h-8 w-8 text-amber-300" />
              </div>
              <p className="text-sm font-medium text-neutral-500">输入问题开始对话</p>
              <p className="mt-1 text-xs text-neutral-400">
                Agent 可以搜索知识库、生成文档、分析风险
              </p>
              {llmReady === false && (
                <div className="mt-4 inline-flex items-center gap-1.5 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700">
                  <AlertCircle className="h-3.5 w-3.5" />
                  LLM 未配置，请先在设置中填写 API Key
                </div>
              )}
            </div>
          ) : (
            messages.map((msg) => (
              <MessageBubble
                key={msg.id}
                message={msg}
                onClarify={handleClarify}
                onRejectClarification={handleRejectClarification}
                onRetry={handleRetry}
                onNavigateSettings={handleNavigateSettings}
              />
            ))
          )}

          {/* ReAct 执行轨迹（加载时显示） */}
          {loading &&
            (currentTrace.thinking || currentTrace.toolCalls.length > 0 || currentTrace.plan) && (
              <div className="space-y-2 border-l-2 border-amber-200 pl-4">
                <PlanTimeline trace={currentTrace} />
                {currentTrace.thinking && (
                  <details className="text-xs text-amber-700 italic leading-relaxed" open>
                    <summary className="cursor-pointer text-amber-500 not-italic font-medium">
                      ?? 推理过程
                    </summary>
                    {currentTrace.thinking.length > 2000
                      ? `...${currentTrace.thinking.slice(-2000)}`
                      : currentTrace.thinking}
                  </details>
                )}
                {currentTrace.toolCalls.map((tc, i) => (
                  <div key={`${tc.name}-${tc.args}`}>
                    <details
                      className="rounded-lg border border-amber-200 bg-amber-50 text-xs"
                      open={i === currentTrace.toolCalls.length - 1}
                    >
                      <summary className="cursor-pointer px-3 py-2 font-medium text-amber-800">
                        🔧 {tc.name}
                      </summary>
                      {tc.args && (
                        <pre className="max-h-60 overflow-auto border-t border-amber-200 bg-white/70 px-3 py-2 font-mono text-[11px] leading-relaxed text-amber-950 whitespace-pre-wrap break-words">
                          {summarizeToolArgs(tc.args)}
                        </pre>
                      )}
                    </details>
                    {tc.result && (
                      <div className="rounded-b-lg border border-green-200 border-t-0 bg-green-50 px-3 py-2 text-xs text-green-700">
                        工具执行完成
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
        </div>
      </div>

      {/* 输入栏 */}
      <section
        aria-label="消息输入与附件拖放区"
        className={`relative border-t border-neutral-200 bg-white p-4 transition-colors ${
          isDragging ? "border-blue-400 bg-blue-50/50" : ""
        }`}
        onDragOver={handleDragOver}
        onDragEnter={handleDragEnter}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        {isDragging && (
          <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center">
            <span className="rounded-lg border-2 border-dashed border-blue-400 bg-blue-50/80 px-6 py-3 text-sm font-medium text-blue-600">
              松开以添加附件
            </span>
          </div>
        )}
        <div>
          {attachments.length > 0 && (
            <div className="mb-3 flex flex-wrap gap-2">
              {attachments.map((attachment) => (
                <AttachmentChip
                  key={attachment.id}
                  attachment={attachment}
                  onRemove={() => removeAttachment(attachment.id)}
                />
              ))}
            </div>
          )}
          <div className="flex items-end gap-2">
            <button
              type="button"
              onClick={handleAttach}
              disabled={loading || attaching}
              title="添加附件"
              className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border border-neutral-200 bg-white text-neutral-500 hover:bg-neutral-50 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {attaching ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Paperclip className="h-4 w-4" />
              )}
            </button>

            {/* 模型选择器 */}
            {providers.length > 0 && (
              <div ref={dropdownRef} className="relative shrink-0">
                <button
                  type="button"
                  onClick={() => setDropdownOpen(!dropdownOpen)}
                  disabled={loading}
                  className="flex h-10 items-center gap-1.5 rounded-lg border border-neutral-200 bg-white px-3 text-xs font-medium text-neutral-700 hover:bg-neutral-50 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  title="选择模型"
                >
                  <Brain className="h-3.5 w-3.5 text-amber-600" />
                  <span className="max-w-[160px] truncate">
                    {selectedProvider && selectedModel
                      ? `${selectedProvider.name} > ${selectedModel.name}`
                      : "选择模型"}
                  </span>
                  {selectedModel?.is_multimodal && (
                    <span className="rounded bg-blue-100 px-1 py-0.5 text-[9px] font-medium text-blue-700">
                      多模态
                    </span>
                  )}
                  <svg
                    className={`h-3 w-3 text-neutral-400 transition-transform ${dropdownOpen ? "rotate-180" : ""}`}
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                    aria-hidden="true"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M19 9l-7 7-7-7"
                    />
                  </svg>
                </button>

                {/* 自动路由提示 */}
                {attachments.some((a) => a.kind === "image") &&
                  selectedModel &&
                  selectedModel.is_multimodal !== true && (
                    <div className="mt-1 flex items-center gap-1 text-[10px] text-amber-600">
                      <Zap className="h-3 w-3" />
                      <span>图片附件将自动使用多模态模型</span>
                    </div>
                  )}

                {dropdownOpen && (
                  <div className="absolute bottom-full left-0 z-50 mb-1 w-72 rounded-lg border border-neutral-200 bg-white py-1 shadow-lg max-h-64 overflow-y-auto">
                    <div className="px-3 py-1.5 text-[10px] font-medium text-neutral-400 uppercase tracking-wider">
                      选择模型
                    </div>
                    {modelOptions.map((option) => (
                      <button
                        key={`${option.providerId}-${option.modelId}`}
                        type="button"
                        onClick={() => {
                          setSelectedProviderId(option.providerId)
                          setSelectedModelId(option.modelId)
                          setDropdownOpen(false)
                        }}
                        className={`flex w-full items-center justify-between px-3 py-2 text-left text-xs hover:bg-neutral-50 transition-colors ${
                          option.providerId === selectedProviderId &&
                          option.modelId === selectedModelId
                            ? "bg-amber-50 text-amber-700"
                            : "text-neutral-700"
                        }`}
                      >
                        <div className="flex flex-col gap-0.5 min-w-0">
                          <span className="font-medium truncate">{option.providerName}</span>
                          <span className="text-[10px] text-neutral-400 truncate">
                            {option.modelName}
                          </span>
                        </div>
                        <div className="flex items-center gap-1.5 shrink-0">
                          {option.isMultimodal && (
                            <span className="rounded bg-blue-100 px-1 py-0.5 text-[9px] font-medium text-blue-700">
                              多模态
                            </span>
                          )}
                          {option.isDefault && (
                            <span className="rounded bg-green-100 px-1 py-0.5 text-[9px] font-medium text-green-700">
                              默认
                            </span>
                          )}
                          {option.providerId === selectedProviderId &&
                            option.modelId === selectedModelId && (
                              <svg
                                className="h-3.5 w-3.5 text-amber-600"
                                fill="none"
                                viewBox="0 0 24 24"
                                stroke="currentColor"
                                aria-hidden="true"
                              >
                                <path
                                  strokeLinecap="round"
                                  strokeLinejoin="round"
                                  strokeWidth={2}
                                  d="M5 13l4 4L19 7"
                                />
                              </svg>
                            )}
                        </div>
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}

            <textarea
              ref={inputRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              onPaste={handlePaste}
              placeholder="输入问题，或先添加文档/图片附件..."
              rows={1}
              disabled={loading}
              className="flex-1 resize-none rounded-lg border border-neutral-200 bg-white px-4 py-2.5 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-amber-500 focus:ring-2 focus:ring-amber-500/20 disabled:opacity-50"
            />
            <button
              type="button"
              onClick={handleSend}
              disabled={loading || attaching || (!input.trim() && attachments.length === 0)}
              className="flex h-10 w-10 items-center justify-center rounded-lg bg-amber-600 text-white hover:bg-amber-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {loading ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Send className="h-4 w-4" />
              )}
            </button>
            {loading && (
              <button
                type="button"
                onClick={handleCancel}
                title="取消生成"
                className="flex h-10 w-10 items-center justify-center rounded-lg bg-red-500 text-white hover:bg-red-600 transition-colors"
              >
                <StopCircle className="h-4 w-4" />
              </button>
            )}
          </div>
        </div>
      </section>
    </div>
  )
}

function createAttachment(path: string): ChatAttachment {
  const name = path.split(/[\\/]/).pop() || path
  const ext = name.split(".").pop()?.toLowerCase() ?? ""
  const kind = DOCUMENT_EXTENSIONS.has(ext)
    ? "document"
    : IMAGE_EXTENSIONS.has(ext)
      ? "image"
      : "unsupported"
  return {
    id: nextId(),
    path,
    name,
    kind,
    status: kind === "unsupported" ? "error" : "ready",
    error: kind === "unsupported" ? "当前格式暂不支持内容解析。" : undefined,
  }
}

function buildAttachmentDisplay(attachments: ChatAttachment[]): string {
  if (attachments.length === 0) return ""
  return [
    "附件：",
    ...attachments.map((a) => {
      const status =
        a.status === "ingested" && a.documentId
          ? `已入库 #${a.documentId}`
          : a.status === "parsed"
            ? (a.error ?? "已解析")
            : a.kind === "image" && a.extractedText
              ? "已识别"
              : a.error
                ? a.error
                : a.status
      return `- ${a.name}（${status}）`
    }),
  ].join("\n")
}

function AttachmentChip({
  attachment,
  onRemove,
}: {
  attachment: ChatAttachment
  onRemove: () => void
}) {
  const preview =
    attachment.previewUrl ||
    (attachment.kind === "image" ? convertFileSrc(attachment.path) : undefined)
  const statusText =
    attachment.status === "ingesting"
      ? "入库中"
      : attachment.status === "parsed"
        ? "已解析"
        : attachment.status === "ingested"
          ? attachment.kind === "image"
            ? "图片"
            : "已入库"
          : attachment.status === "error"
            ? "失败"
            : "待入库"

  return (
    <div className="group flex max-w-[260px] items-center gap-2 rounded-lg border border-neutral-200 bg-neutral-50 px-2.5 py-2">
      {preview ? (
        <img src={preview} alt="" className="h-8 w-8 rounded object-cover" />
      ) : attachment.kind === "document" ? (
        <FileText className="h-4 w-4 shrink-0 text-amber-600" />
      ) : (
        <ImageIcon className="h-4 w-4 shrink-0 text-neutral-500" />
      )}
      <div className="min-w-0 flex-1">
        <div className="truncate text-xs font-medium text-neutral-700">{attachment.name}</div>
        <div
          className={`text-[10px] ${
            attachment.status === "error" || attachment.kind === "unsupported"
              ? "text-red-500"
              : "text-neutral-400"
          }`}
        >
          {statusText}
        </div>
      </div>
      {attachment.status === "ingesting" ? (
        <Loader2 className="h-3.5 w-3.5 animate-spin text-neutral-400" />
      ) : (
        <button
          type="button"
          onClick={onRemove}
          title="移除附件"
          className="rounded p-1 text-neutral-400 hover:bg-neutral-200 hover:text-neutral-700"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      )}
    </div>
  )
}

// ── 文件气泡组件 ──────────────────────────────────────────────

/** 根据文件扩展名获取图标 */
function getFileIcon(name: string) {
  const ext = name.split(".").pop()?.toLowerCase() ?? ""
  if (["xlsx", "xls", "csv"].includes(ext)) return FileSpreadsheet
  if (
    ["js", "ts", "py", "java", "rs", "go", "html", "css", "json", "xml", "yaml", "yml"].includes(
      ext,
    )
  )
    return FileCode
  if (["zip", "rar", "7z", "tar", "gz"].includes(ext)) return FileArchive
  if (["pdf"].includes(ext)) return FileText
  if (["doc", "docx", "md", "txt"].includes(ext)) return FileText
  return File
}

/** 格式化文件大小 */
function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** 判断是否为图片文件 */
function isImageFile(name: string): boolean {
  const ext = name.split(".").pop()?.toLowerCase() ?? ""
  return ["png", "jpg", "jpeg", "webp", "bmp", "gif", "svg"].includes(ext)
}

function FileBubble({ attachment }: { attachment: FileAttachment }) {
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null)
  const [previewOpen, setPreviewOpen] = useState(false)
  const [actionMessage, setActionMessage] = useState<{
    type: "success" | "error"
    text: string
  } | null>(null)
  const actionMessageTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isImage = isImageFile(attachment.name)
  const Icon = isImage ? ImageIcon : getFileIcon(attachment.name)
  const src = isImage && attachment.path ? convertFileSrc(attachment.path) : undefined

  const showActionMessage = useCallback((type: "success" | "error", text: string) => {
    if (actionMessageTimerRef.current) {
      clearTimeout(actionMessageTimerRef.current)
    }
    setActionMessage({ type, text })
    actionMessageTimerRef.current = setTimeout(() => setActionMessage(null), 2400)
  }, [])

  useEffect(() => {
    if (!previewOpen) return
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setPreviewOpen(false)
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [previewOpen])

  useEffect(() => {
    return () => {
      if (actionMessageTimerRef.current) {
        clearTimeout(actionMessageTimerRef.current)
      }
    }
  }, [])

  const handleClick = useCallback(() => {
    if (isImage) {
      setPreviewOpen(true)
    } else {
      openPath(attachment.path).catch((err) => showActionMessage("error", `打开文件失败：${err}`))
    }
  }, [isImage, attachment.path, showActionMessage])

  const handleCopyPath = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(attachment.path)
      showActionMessage("success", "路径已复制")
    } catch (err) {
      showActionMessage("error", `复制路径失败：${err}`)
    }
  }, [attachment.path, showActionMessage])

  const handleSaveAs = useCallback(async () => {
    try {
      const dest = await save({
        defaultPath: attachment.name,
        filters: [{ name: "All Files", extensions: ["*"] }],
      })
      if (dest) {
        await invoke("save_attachment_as", { source: attachment.path, dest })
        showActionMessage("success", `已保存到：${dest}`)
      }
    } catch (err) {
      showActionMessage("error", `另存失败：${err}`)
    }
  }, [attachment.path, attachment.name, showActionMessage])

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setContextMenu({ x: e.clientX, y: e.clientY })
  }, [])

  const closeContextMenu = useCallback(() => setContextMenu(null), [])

  return (
    <div className="relative">
      <button
        type="button"
        onClick={handleClick}
        onContextMenu={handleContextMenu}
        className="flex max-w-[280px] cursor-pointer items-center gap-3 rounded-lg border border-neutral-200 bg-neutral-50 px-3 py-2.5 text-left transition-colors hover:bg-neutral-100"
      >
        {src ? (
          <img src={src} alt="" className="h-12 w-12 rounded object-cover shrink-0" />
        ) : (
          <div className="flex h-12 w-12 items-center justify-center rounded bg-amber-50 shrink-0">
            <Icon className="h-5 w-5 text-amber-600" />
          </div>
        )}
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium text-neutral-800">{attachment.name}</div>
          {attachment.size != null && (
            <div className="text-xs text-neutral-400">{formatFileSize(attachment.size)}</div>
          )}
        </div>
        {isImage ? (
          <span className="text-[10px] text-amber-600 shrink-0">预览</span>
        ) : (
          <ExternalLink className="h-4 w-4 shrink-0 text-neutral-400" />
        )}
      </button>

      {actionMessage && (
        <div
          className={`mt-1 max-w-[280px] break-all rounded px-2 py-1 text-[10px] ${
            actionMessage.type === "error"
              ? "bg-red-50 text-red-600"
              : "bg-emerald-50 text-emerald-700"
          }`}
        >
          {actionMessage.text}
        </div>
      )}

      {/* 图片预览浮层 */}
      {previewOpen && src && (
        <div
          role="dialog"
          aria-modal="true"
          aria-label={attachment.name}
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/70"
          onClick={(event) => {
            if (event.target === event.currentTarget) setPreviewOpen(false)
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") setPreviewOpen(false)
          }}
          tabIndex={-1}
        >
          <button
            type="button"
            onClick={() => setPreviewOpen(false)}
            className="absolute right-4 top-4 z-10 rounded-full bg-black/50 p-2 text-white hover:bg-black/70"
          >
            <X className="h-5 w-5" />
          </button>
          <img
            src={src}
            alt={attachment.name}
            className="max-h-[90vh] max-w-[90vw] rounded object-contain"
          />
        </div>
      )}

      {/* 右键菜单 */}
      {contextMenu && (
        <>
          <button
            type="button"
            aria-label="关闭附件菜单"
            className="fixed inset-0 z-40 cursor-default"
            onClick={closeContextMenu}
          />
          <div
            className="fixed z-50 min-w-[140px] rounded-lg border border-neutral-200 bg-white py-1 shadow-lg"
            style={{ left: contextMenu.x, top: contextMenu.y }}
          >
            <button
              type="button"
              onClick={() => {
                handleClick()
                closeContextMenu()
              }}
              className="flex w-full items-center gap-2 px-3 py-1.5 text-xs text-neutral-700 hover:bg-neutral-50"
            >
              <ExternalLink className="h-3.5 w-3.5" />
              {isImage ? "预览" : "打开文件"}
            </button>
            <button
              type="button"
              onClick={() => {
                handleCopyPath()
                closeContextMenu()
              }}
              className="flex w-full items-center gap-2 px-3 py-1.5 text-xs text-neutral-700 hover:bg-neutral-50"
            >
              <Copy className="h-3.5 w-3.5" />
              复制路径
            </button>
            <button
              type="button"
              onClick={() => {
                handleSaveAs()
                closeContextMenu()
              }}
              className="flex w-full items-center gap-2 px-3 py-1.5 text-xs text-neutral-700 hover:bg-neutral-50"
            >
              <Download className="h-3.5 w-3.5" />
              另存为
            </button>
          </div>
        </>
      )}
    </div>
  )
}

/** 内联图片预览（支持点击放大） */
function ImagePreview({ src, alt }: { src: string; alt: string }) {
  const [open, setOpen] = useState(false)

  useEffect(() => {
    if (!open) return
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false)
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [open])

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="block rounded border border-neutral-200 hover:opacity-90 transition-opacity"
      >
        <img src={src} alt={alt} />
      </button>
      {open && (
        <div
          role="dialog"
          aria-modal="true"
          aria-label={alt || "图片预览"}
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/70"
          onClick={(event) => {
            if (event.target === event.currentTarget) setOpen(false)
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") setOpen(false)
          }}
          tabIndex={-1}
        >
          <button
            type="button"
            onClick={() => setOpen(false)}
            className="absolute right-4 top-4 z-10 rounded-full bg-black/50 p-2 text-white hover:bg-black/70"
          >
            <X className="h-5 w-5" />
          </button>
          <img src={src} alt={alt} className="max-h-[90vh] max-w-[90vw] rounded object-contain" />
        </div>
      )}
    </>
  )
}

// ── 消息气泡组件 ──────────────────────────────────────────────

const MessageBubble = memo(function MessageBubble({
  message,
  onClarify,
  onRejectClarification,
  onRetry,
  onNavigateSettings,
}: {
  message: AgentMessage
  onClarify: (questionId: string, answer: string) => void
  onRejectClarification: (questionId: string) => void
  onRetry: () => void
  onNavigateSettings: (section?: string) => void
}) {
  const isUser = message.role === "user"
  const clarification = message.clarification

  const isLLMError =
    message.error && /未配置|api.?key|llm|模型|unauthorized|401/i.test(message.content)
  const isTimeoutError = message.error && /超时|timeout/i.test(message.content)

  return (
    <div className={`flex ${isUser ? "justify-end" : "justify-start"}`}>
      <div className={`max-w-[80%] ${isUser ? "" : "w-full"}`}>
        {/* 头像 */}
        <div
          className={`mb-1 flex items-center gap-1.5 text-xs ${
            isUser ? "justify-end text-neutral-400" : "text-amber-600"
          }`}
        >
          <span className="flex h-5 w-5 items-center justify-center rounded-full bg-neutral-100 text-[10px]">
            {isUser ? "👤" : "🤖"}
          </span>
          <span className="font-medium">{isUser ? "你" : "AI 助手"}</span>
        </div>

        {/* 消息 */}
        <div
          className={`rounded-2xl px-4 py-3 text-sm leading-relaxed ${
            isUser
              ? "bg-amber-600 text-white rounded-tr-md"
              : message.error
                ? "bg-red-50 text-red-700 border border-red-200 rounded-tl-md"
                : "bg-white text-neutral-700 border border-neutral-200 rounded-tl-md shadow-sm"
          }`}
        >
          {isUser ? (
            <div className="whitespace-pre-wrap">{message.content}</div>
          ) : message.streaming ? (
            /* 流式阶段：纯文本渲染，避免 ReactMarkdown 重复解析的性能开销 */
            message.content ? (
              <div className="text-sm leading-relaxed whitespace-pre-wrap">
                {message.content.replace(/^\n+/, "")}
                <span className="ml-1 inline-block h-3.5 w-1.5 animate-pulse bg-amber-500 rounded-sm align-middle" />
              </div>
            ) : (
              <div className="flex items-center gap-2 text-sm text-neutral-500">
                <Loader2 className="h-4 w-4 shrink-0 animate-spin text-amber-500" />
                <span>{message.statusText || "正在准备回答..."}</span>
                <span className="flex gap-0.5" aria-hidden="true">
                  <span className="h-1 w-1 animate-pulse rounded-full bg-neutral-400" />
                  <span className="h-1 w-1 animate-pulse rounded-full bg-neutral-400 [animation-delay:150ms]" />
                  <span className="h-1 w-1 animate-pulse rounded-full bg-neutral-400 [animation-delay:300ms]" />
                </span>
              </div>
            )
          ) : (
            /* 完成后：Markdown 渲染（含本地图片自动转 Tauri URL） */
            <div className="prose prose-sm max-w-none prose-headings:text-neutral-800 prose-a:text-amber-600 prose-code:bg-neutral-100 prose-code:px-1 prose-code:rounded prose-pre:bg-neutral-900 prose-pre:text-neutral-100 [&_pre_code]:bg-transparent [&_pre_code]:text-inherit">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={{
                  img: ({ src, alt }) => {
                    if (!src) return null
                    // 本地文件路径 → Tauri asset URL
                    const imgSrc = src.startsWith("http") ? src : convertFileSrc(src)
                    return <ImagePreview src={imgSrc} alt={alt || ""} />
                  },
                }}
              >
                {message.content.replace(/^\n+/, "")}
              </ReactMarkdown>
            </div>
          )}

          {/* 验证报告 */}
          {message.verificationReport && !message.streaming && (
            <VerificationBadge report={message.verificationReport} />
          )}

          {/* 文件附件 */}
          {message.attachments && message.attachments.length > 0 && (
            <div className="mt-3 flex flex-wrap gap-2">
              {message.attachments.map((att) => (
                <FileBubble key={att.id} attachment={att} />
              ))}
            </div>
          )}

          {/* 错误操作按钮 */}
          {message.error && !message.streaming && (
            <div className="mt-3 flex flex-wrap gap-2 border-t border-red-200 pt-3">
              {(isLLMError || isTimeoutError) && (
                <button
                  type="button"
                  onClick={() => onNavigateSettings("llm")}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-red-300 bg-white px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-50 transition-colors"
                >
                  <Settings className="h-3 w-3" />
                  去设置
                </button>
              )}
              <button
                type="button"
                onClick={onRetry}
                className="inline-flex items-center gap-1.5 rounded-lg border border-red-300 bg-white px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-50 transition-colors"
              >
                <RefreshCw className="h-3 w-3" />
                恢复输入
              </button>
            </div>
          )}

          {/* RAG 来源 */}
          {!isUser && !message.error && message.sources && message.sources.length > 0 && (
            <SourcesDisplay sources={message.sources} />
          )}

          {/* 澄清问题 */}
          {clarification && !message.clarificationAnswered && (
            <ClarificationTabs
              key={clarification.question_id}
              clarification={clarification}
              onClarify={onClarify}
              onReject={onRejectClarification}
            />
          )}
        </div>
      </div>
    </div>
  )
})

/** 可折叠的 RAG 来源展示 */
function SourcesDisplay({ sources }: { sources: RAGSource[] }) {
  const [expanded, setExpanded] = useState(false)

  return (
    <div className="mt-3 border-t border-neutral-100 pt-3">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center gap-1.5 text-xs text-neutral-500 hover:text-neutral-700 transition-colors"
      >
        <BookOpen className="h-3 w-3" />
        <span className="font-medium">参考来源 ({sources.length})</span>
        {expanded ? (
          <ChevronUp className="h-3 w-3 ml-auto" />
        ) : (
          <ChevronDown className="h-3 w-3 ml-auto" />
        )}
      </button>
      {expanded && (
        <div className="mt-2 space-y-1.5">
          {sources.map((src) => (
            <div
              key={`${src.title}-${src.section_path ?? ""}-${src.content_snippet ?? ""}-${src.score}`}
              className="rounded-lg border border-neutral-100 bg-neutral-50 px-3 py-2"
            >
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs font-medium text-neutral-700 truncate">{src.title}</span>
                {src.score > 0 && (
                  <span className="shrink-0 rounded bg-amber-100 px-1.5 py-0.5 text-[10px] font-medium text-amber-700">
                    {(src.score * 100).toFixed(0)}%
                  </span>
                )}
              </div>
              {src.section_path && (
                <div className="mt-0.5 text-[10px] text-neutral-400 truncate">
                  {src.section_path}
                </div>
              )}
              {src.content_snippet && (
                <div className="mt-1 text-[11px] text-neutral-500 line-clamp-2 leading-relaxed">
                  {src.content_snippet}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function ClarificationTabs({
  clarification,
  onClarify,
  onReject,
}: {
  clarification: ClarificationPayload
  onClarify: (questionId: string, answer: string) => void
  onReject: (questionId: string) => void
}) {
  const questions = useMemo(
    () =>
      clarification.questions && clarification.questions.length > 0
        ? clarification.questions
        : [
            {
              prompt: clarification.prompt,
              header: clarification.header || "问题",
              mode: clarification.mode,
              options: clarification.options,
              multiple: clarification.multiple,
              custom: clarification.custom,
            },
          ],
    [
      clarification.questions,
      clarification.prompt,
      clarification.header,
      clarification.mode,
      clarification.options,
      clarification.multiple,
      clarification.custom,
    ],
  )
  const [active, setActive] = useState(0)
  const [answers, setAnswers] = useState<string[][]>(() => questions.map(() => []))
  const [customValues, setCustomValues] = useState<string[]>(() => questions.map(() => ""))
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([])
  const activeQuestion = questions[Math.min(active, questions.length - 1)]
  const tabBaseId = `clarification-${clarification.question_id.replace(/[^a-zA-Z0-9_-]/g, "-")}`

  const setQuestionAnswer = (index: number, value: string[]) => {
    setAnswers((prev) => prev.map((item, i) => (i === index ? value : item)))
  }

  const answeredCount = answers.filter((items) => items.length > 0).length
  const canSubmit = answeredCount === questions.length

  const submit = () => {
    if (!canSubmit) return
    onClarify(clarification.question_id, JSON.stringify(answers))
  }

  const activateTab = (index: number, focus = true) => {
    const next = (index + questions.length) % questions.length
    setActive(next)
    if (focus) {
      requestAnimationFrame(() => tabRefs.current[next]?.focus())
    }
  }

  const moveTab = (direction: -1 | 1) => {
    activateTab(active + direction)
  }

  return (
    <div className="mt-3 border-t border-neutral-100 pt-3">
      {questions.length > 1 && (
        <div
          role="tablist"
          aria-label="澄清问题"
          className="mb-3 flex gap-1 overflow-x-auto border-b border-neutral-100"
          onKeyDown={(event) => {
            if (event.key === "ArrowRight") {
              event.preventDefault()
              moveTab(1)
            } else if (event.key === "ArrowLeft") {
              event.preventDefault()
              moveTab(-1)
            } else if (event.key === "Home") {
              event.preventDefault()
              activateTab(0)
            } else if (event.key === "End") {
              event.preventDefault()
              activateTab(questions.length - 1)
            } else if (event.key === "Tab") {
              event.preventDefault()
              moveTab(event.shiftKey ? -1 : 1)
            }
          }}
        >
          {questions.map((question, index) => (
            <button
              key={`${question.header}-${question.prompt}`}
              ref={(element) => {
                tabRefs.current[index] = element
              }}
              type="button"
              role="tab"
              id={`${tabBaseId}-tab-${index}`}
              aria-selected={active === index}
              aria-controls={`${tabBaseId}-panel`}
              tabIndex={active === index ? 0 : -1}
              onClick={() => activateTab(index, false)}
              className={`shrink-0 border-b-2 px-3 py-2 text-xs font-medium transition-colors ${
                active === index
                  ? "border-amber-500 text-amber-700"
                  : answers[index]?.length
                    ? "border-transparent text-emerald-700 hover:text-amber-700"
                    : "border-transparent text-neutral-500 hover:text-neutral-700"
              }`}
            >
              {question.header || `问题 ${index + 1}`}
            </button>
          ))}
        </div>
      )}

      <div
        id={`${tabBaseId}-panel`}
        role="tabpanel"
        aria-labelledby={questions.length > 1 ? `${tabBaseId}-tab-${active}` : undefined}
        className="space-y-3"
      >
        <div>
          <div className="text-xs font-medium text-neutral-500">
            {questions.length > 1 ? `${active + 1}/${questions.length}` : "需要确认"}
          </div>
          <div className="mt-1 text-sm font-medium text-neutral-800">{activeQuestion.prompt}</div>
        </div>

        <QuestionAnswerInput
          question={activeQuestion}
          answer={answers[active] ?? []}
          customValue={customValues[active] ?? ""}
          onAnswer={(value) => setQuestionAnswer(active, value)}
          onCustomValue={(value) =>
            setCustomValues((prev) => prev.map((item, index) => (index === active ? value : item)))
          }
          onSubmit={submit}
        />

        <div className="flex items-center justify-between gap-3 pt-1">
          <span className="text-xs text-neutral-400">
            已回答 {answeredCount}/{questions.length}
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => onReject(clarification.question_id)}
              className="rounded-lg border border-neutral-200 bg-white px-3 py-1.5 text-xs font-medium text-neutral-600 transition-colors hover:border-neutral-300 hover:bg-neutral-50"
            >
              取消
            </button>
            <button
              type="button"
              onClick={submit}
              disabled={!canSubmit}
              className="rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-amber-700 disabled:opacity-50"
            >
              提交回答
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}

function QuestionAnswerInput({
  question,
  answer,
  customValue,
  onAnswer,
  onCustomValue,
  onSubmit,
}: {
  question: ClarificationQuestion
  answer: string[]
  customValue: string
  onAnswer: (answer: string[]) => void
  onCustomValue: (value: string) => void
  onSubmit: () => void
}) {
  const toggleOption = (option: QuestionOption) => {
    if (question.mode === "multi_choice" || question.multiple) {
      onAnswer(
        answer.includes(option.label)
          ? answer.filter((item) => item !== option.label)
          : [...answer, option.label],
      )
    } else {
      onAnswer([option.label])
    }
  }

  const commitCustom = () => {
    const value = customValue.trim()
    if (!value) return
    onAnswer(question.multiple ? [...answer.filter((item) => item !== value), value] : [value])
    onCustomValue("")
  }

  return (
    <div className="space-y-2">
      {question.options.length > 0 && (
        <div className="grid gap-2 sm:grid-cols-2">
          {question.options.map((option) => {
            const selected = answer.includes(option.label)
            return (
              <button
                key={option.label}
                type="button"
                onClick={() => toggleOption(option)}
                className={`min-h-[58px] rounded-lg border px-3 py-2 text-left text-xs transition-colors ${
                  selected
                    ? "border-amber-500 bg-amber-50 text-amber-800"
                    : "border-neutral-200 bg-white text-neutral-600 hover:border-amber-200"
                }`}
              >
                <span className="block font-medium">{option.label}</span>
                {option.description && (
                  <span className="mt-0.5 block leading-snug text-neutral-500">
                    {option.description}
                  </span>
                )}
              </button>
            )
          })}
        </div>
      )}

      {(question.custom || question.mode === "free_input") && (
        <div className="flex gap-2">
          <input
            type="text"
            value={customValue}
            onChange={(event) => onCustomValue(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault()
                if (customValue.trim()) {
                  commitCustom()
                } else {
                  onSubmit()
                }
              }
            }}
            placeholder="输入自定义回答..."
            className="min-w-0 flex-1 rounded-lg border border-neutral-200 bg-white px-3 py-1.5 text-xs outline-none focus:border-amber-500"
          />
          <button
            type="button"
            onClick={commitCustom}
            disabled={!customValue.trim()}
            className="rounded-lg border border-neutral-200 bg-white px-3 py-1.5 text-xs font-medium text-neutral-700 transition-colors hover:border-amber-200 disabled:opacity-50"
          >
            添加
          </button>
        </div>
      )}
    </div>
  )
}
