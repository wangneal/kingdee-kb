import { invoke } from "@tauri-apps/api/core"
import { save } from "@tauri-apps/plugin-dialog"
import {
  AlertCircle,
  BookOpen,
  Brain,
  ChevronLeft,
  ClipboardList,
  Download,
  Edit3,
  FileText,
  ListTodo,
  Loader2,
  MessageSquare,
  Mic,
  Network,
  Plus,
  Square,
  Trash2,
} from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import ReactMarkdown from "react-markdown"
import { useParams } from "react-router-dom"
import remarkGfm from "remark-gfm"
import MarkdownEditor from "../components/outliner/MarkdownEditor"
import MindmapView from "../components/outliner/MindmapView"
import OutlineTree from "../components/outliner/OutlineTree"
import { useToast } from "../components/Toast"
import { DEFAULT_SLOT, useAgent } from "../contexts/AgentContext"
import { useOutline } from "../contexts/OutlineContext"
import { useProject } from "../contexts/ProjectContext"
import {
  type AsrConfigStatus,
  type AsrProviderInfo,
  type AudioInputDeviceInfo,
  addQARecord,
  createResearchSession,
  deleteQARecord,
  deleteResearchSession,
  exportSessionCsv,
  exportSessionMarkdown,
  fetchInvestigationRecipe,
  fetchTencentMeetingTranscript,
  getAsrConfigStatus,
  getResearchSession,
  getTencentMeetingConfigStatus,
  getWhisperStatus,
  isLLMConfigured,
  listAsrProviders,
  listAudioInputDevices,
  listResearchSessions,
  loadWhisperModel,
  type ResearchSession,
  reviewTranscriptionText,
  type SessionDetail,
  startWhisperRecording,
  stopWhisperRecording,
  transcribeWhisperRecordingChunk,
  updateQARecord,
  type WhisperStatus,
} from "../lib/tauri-commands"

export default function ResearchAssistant() {
  const { currentProjectId } = useProject()
  const { sessionId: urlSessionId } = useParams<{ sessionId: string }>()
  const [mode, setMode] = useState<"list" | "detail" | "new">("list")
  const [sessions, setSessions] = useState<ResearchSession[]>([])
  const [detail, setDetail] = useState<SessionDetail | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // 挂载时加载会话列表
  const refreshList = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const list = await listResearchSessions(currentProjectId)
      setSessions(list)
    } catch (err) {
      setError(String(err))
    } finally {
      setLoading(false)
    }
  }, [currentProjectId])

  useEffect(() => {
    refreshList()
  }, [refreshList])

  const openSession = useCallback(async (id: number) => {
    setLoading(true)
    setError(null)
    try {
      const d = await getResearchSession(id)
      setDetail(d)
      setMode("detail")
    } catch (err) {
      setError(String(err))
    } finally {
      setLoading(false)
    }
  }, [])

  // URL 直接访问大纲视图时自动打开会话
  useEffect(() => {
    if (urlSessionId && sessions.length > 0 && mode === "list") {
      const id = Number(urlSessionId)
      if (!Number.isNaN(id)) {
        openSession(id)
      }
    }
  }, [urlSessionId, sessions, mode, openSession])

  const handleDelete = useCallback(
    async (id: number) => {
      if (!confirm("确认删除此调研会话？所有记录将被永久删除。")) return
      try {
        await deleteResearchSession(id)
        refreshList()
      } catch (err) {
        setError(String(err))
      }
    },
    [refreshList],
  )

  // ── 列表视图 ──
  if (mode === "list") {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
          <div className="flex items-center gap-2">
            <ClipboardList className="h-5 w-5 text-[#1A6BD8]" />
            <h1 className="text-base font-semibold text-neutral-800">调研助手</h1>
            <span className="text-xs text-neutral-400">{sessions.length} 个会话</span>
          </div>
          <button
            type="button"
            onClick={() => setMode("new")}
            className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#1558B0] transition-colors"
          >
            <Plus className="h-3.5 w-3.5" />
            新建调研
          </button>
        </div>

        {error && (
          <div className="mx-6 mt-3 flex items-center gap-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-red-600">
            <AlertCircle className="h-3.5 w-3.5" />
            {error}
          </div>
        )}

        <div className="flex-1 overflow-y-auto p-6">
          {loading ? (
            <div className="flex items-center justify-center pt-20">
              <Loader2 className="h-5 w-5 animate-spin text-neutral-400" />
            </div>
          ) : sessions.length === 0 ? (
            <div className="flex flex-col items-center justify-center pt-20 text-center">
              <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-neutral-100">
                <ClipboardList className="h-8 w-8 text-neutral-300" />
              </div>
              <p className="text-sm font-medium text-neutral-500">暂无调研会话</p>
              <p className="mt-1 text-xs text-neutral-400">点击"新建调研"开始</p>
            </div>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {sessions.map((s) => (
                <div
                  key={s.id}
                  className="group rounded-lg border border-neutral-200 bg-white p-4 hover:border-[#1A6BD8]/30 hover:shadow-sm transition-all"
                >
                  <div className="mb-2 flex items-start justify-between">
                    <h3 className="text-sm font-medium text-neutral-800 line-clamp-1">{s.title}</h3>
                    <div className="flex shrink-0 items-center gap-1">
                      <button
                        type="button"
                        onClick={() => openSession(s.id)}
                        className="rounded px-2 py-1 text-[10px] text-[#1A6BD8] opacity-0 transition-all hover:bg-blue-50 group-hover:opacity-100"
                      >
                        打开
                      </button>
                      <button
                        type="button"
                        onClick={() => handleDelete(s.id)}
                        className="rounded p-1 text-neutral-300 opacity-0 group-hover:opacity-100 hover:text-red-500 transition-all"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-1.5 text-[10px] text-neutral-500">
                    <span className="rounded bg-neutral-100 px-1.5 py-0.5">
                      {s.edition === "enterprise" ? "企业版" : "旗舰版"}
                    </span>
                    {s.module_code && (
                      <span className="rounded bg-blue-50 px-1.5 py-0.5 text-blue-600">
                        {s.module_code}
                      </span>
                    )}
                    {s.status === "completed" && (
                      <span className="rounded bg-green-100 px-1.5 py-0.5 text-green-700">
                        已完成
                      </span>
                    )}
                  </div>
                  <p className="mt-2 text-xs text-neutral-400">
                    {s.interviewee || "未填受访人"} · {s.session_date || "未填日期"}
                  </p>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    )
  }

  // ── 新建会话表单 ──
  if (mode === "new") {
    return (
      <NewSessionForm
        onCreated={(id) => {
          refreshList()
          openSession(id)
        }}
        onCancel={() => setMode("list")}
      />
    )
  }

  // ── 会话详情 ──
  if (mode === "detail" && detail) {
    return (
      <SessionDetailView
        detail={detail}
        onBack={() => {
          setMode("list")
          setDetail(null)
          refreshList()
        }}
        onUpdated={() => {
          if (detail) {
            getResearchSession(detail.session.id)
              .then(setDetail)
              .catch((err) => setError(String(err)))
          }
        }}
        initialTab="outline"
      />
    )
  }

  return null
}

// ── 新建会话表单 ───────────────────────────────────────────────────────────

function NewSessionForm({
  onCreated,
  onCancel,
}: {
  onCreated: (id: number) => void
  onCancel: () => void
}) {
  const [title, setTitle] = useState("")
  const [edition, setEdition] = useState("enterprise")
  const [moduleCode, setModuleCode] = useState("")
  const [interviewee, setInterviewee] = useState("")
  const [sessionDate, setSessionDate] = useState(new Date().toISOString().slice(0, 10))
  const [saving, setSaving] = useState(false)
  const toast = useToast()
  const { currentProjectId } = useProject()

  const handleSubmit = async () => {
    if (!title.trim()) return
    setSaving(true)
    try {
      const id = await createResearchSession(
        title.trim(),
        edition,
        moduleCode.trim(),
        interviewee.trim(),
        sessionDate,
        currentProjectId,
      )
      onCreated(id)
    } catch (err) {
      toast.error(String(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-14 items-center gap-2 border-b border-neutral-200 px-6">
        <button
          type="button"
          onClick={onCancel}
          className="flex items-center gap-1 text-xs text-neutral-500 hover:text-neutral-700 transition-colors"
        >
          <ChevronLeft className="h-4 w-4" />
          返回
        </button>
        <span className="text-sm text-neutral-300">|</span>
        <h1 className="text-base font-semibold text-neutral-800">新建调研会话</h1>
      </div>
      <div className="flex-1 overflow-y-auto p-6">
        <div className="space-y-4">
          <div>
            <label
              htmlFor="research-title"
              className="mb-1 block text-xs font-medium text-neutral-600"
            >
              会话标题 *
            </label>
            <input
              id="research-title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="如：BOS 基础平台调研"
              className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20"
            />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label
                htmlFor="research-edition"
                className="mb-1 block text-xs font-medium text-neutral-600"
              >
                版本
              </label>
              <select
                id="research-edition"
                value={edition}
                onChange={(e) => setEdition(e.target.value)}
                className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8]"
              >
                <option value="enterprise">企业版</option>
                <option value="flagship">旗舰版</option>
              </select>
            </div>
            <div>
              <label
                htmlFor="research-module"
                className="mb-1 block text-xs font-medium text-neutral-600"
              >
                调研模块
              </label>
              <input
                id="research-module"
                value={moduleCode}
                onChange={(e) => setModuleCode(e.target.value)}
                placeholder="如：采购、销售、库存、财务..."
                className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20"
              />
            </div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label
                htmlFor="research-interviewee"
                className="mb-1 block text-xs font-medium text-neutral-600"
              >
                受访人
              </label>
              <input
                id="research-interviewee"
                value={interviewee}
                onChange={(e) => setInterviewee(e.target.value)}
                placeholder="姓名"
                className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20"
              />
            </div>
            <div>
              <label
                htmlFor="research-date"
                className="mb-1 block text-xs font-medium text-neutral-600"
              >
                调研日期
              </label>
              <input
                id="research-date"
                type="date"
                value={sessionDate}
                onChange={(e) => setSessionDate(e.target.value)}
                className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8]"
              />
            </div>
          </div>
          <div className="flex justify-end gap-2 pt-2">
            <button
              type="button"
              onClick={onCancel}
              className="rounded-lg border border-neutral-200 px-4 py-2 text-xs font-medium text-neutral-600 hover:bg-neutral-50 transition-colors"
            >
              取消
            </button>
            <button
              type="button"
              onClick={handleSubmit}
              disabled={saving || !title.trim()}
              className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-xs font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
            >
              {saving ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Plus className="h-3.5 w-3.5" />
              )}
              创建
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}

// ── 会话详情视图 ───────────────────────────────────────────────────────────

function SessionDetailView({
  detail,
  onBack,
  onUpdated,
  initialTab = "qa",
}: {
  detail: SessionDetail
  onBack: () => void
  onUpdated: () => void
  initialTab?: "qa" | "outline" | "mindmap"
}) {
  const { session, records } = detail
  const agent = useAgent()
  const slot = agent.slots.get("research") ?? DEFAULT_SLOT
  const aiLoading = slot.loading
  const { currentProjectId } = useProject()
  const [recording, setRecording] = useState(false)
  const [recordingStarting, setRecordingStarting] = useState(false)
  const [whisperStatus, setWhisperStatus] = useState<WhisperStatus | null>(null)
  const [loadingWhisper, setLoadingWhisper] = useState(false)
  const [asrProviders, setAsrProviders] = useState<AsrProviderInfo[]>([])
  const [selectedAsrProvider, setSelectedAsrProvider] = useState<string>("whisper")
  const [audioInputDevices, setAudioInputDevices] = useState<AudioInputDeviceInfo[]>([])
  const [selectedAudioDeviceId, setSelectedAudioDeviceId] = useState("")
  const [liveTranscript, setLiveTranscript] = useState("")
  const [liveTranscribing, setLiveTranscribing] = useState(false)
  const [llmConfigured, setLlmConfigured] = useState(false)
  const [llmReviewEnabled, setLlmReviewEnabled] = useState(true)
  const [autoPromptEnabled, setAutoPromptEnabled] = useState(true)
  const [reviewingTranscript, setReviewingTranscript] = useState(false)
  const [tencentMeetingConfigured, setTencentMeetingConfigured] = useState(false)
  const [tencentMeetingCode, setTencentMeetingCode] = useState("")
  const [tencentMeetingRecordFileId, setTencentMeetingRecordFileId] = useState("")
  const [tencentMeetingSyncing, setTencentMeetingSyncing] = useState(false)
  const [tencentMeetingLoading, setTencentMeetingLoading] = useState(false)
  const [_asrConfigStatus, setAsrConfigStatus] = useState<AsrConfigStatus | null>(null)
  const [newQuestion, setNewQuestion] = useState("")
  const [newAnswer, setNewAnswer] = useState("")
  const [editingRecord, setEditingRecord] = useState<number | null>(null)
  const [editAnswer, setEditAnswer] = useState("")
  const [activeTab, setActiveTab] = useState<"qa" | "outline" | "mindmap">(initialTab)
  const [copilotTab, setCopilotTab] = useState<"assistant" | "transcript">("assistant")
  const toast = useToast()
  const liveSampleRef = useRef(0)
  const liveTranscribingRef = useRef(false)
  const lastAutoPromptTranscriptRef = useRef("")
  const autoPromptTimerRef = useRef<number | null>(null)
  // 缓存最近一次"生成调研报告"的 prompt，供失败重试用
  const lastReportPromptRef = useRef<string>("")
  // 标记最近一次报告生成时的 Q&A 快照（id+answer+notes 拼接 hash），用于重试时漂移检测
  const lastReportQaHashRef = useRef<string>("")
  // 标记当前 AI 响应是否处于"调研报告"上下文（横幅、重试按钮根据这个开关显示）
  const reportContextRef = useRef<boolean>(false)

  // 大纲上下文与文本插入触发器
  const outline = useOutline()
  const [insertTextTrigger, setInsertTextTrigger] = useState<{
    text: string
    timestamp: number
  } | null>(null)

  // 切换到大纲视图或脑图视图时加载大纲
  useEffect(() => {
    if (activeTab === "outline" || activeTab === "mindmap") {
      outline.loadOutline(session.id)
    }
  }, [activeTab, session.id, outline.loadOutline])

  // 加载调研报告配方（与后端 prompts::RECIPE_INVESTIGATION 同源）
  useEffect(() => {
    fetchInvestigationRecipe()
      .then(setReportRecipe)
      .catch((err) => {
        console.error("加载调研报告配方失败:", err)
        setReportRecipe("")
      })
  }, [])

  useEffect(() => {
    getWhisperStatus()
      .then(setWhisperStatus)
      .catch((err) => {
        console.error("获取 Whisper 状态失败:", err)
      })
    listAsrProviders()
      .then((providers) => {
        // 单一明确语义：后端只返回腾讯云这一种在线 ASR，前端无需再过滤
        setAsrProviders(providers || [])
      })
      .catch(console.error)
    listAudioInputDevices()
      .then((devices) => {
        setAudioInputDevices(devices)
        const defaultDevice = devices.find((device) => device.is_default)
        if (defaultDevice) setSelectedAudioDeviceId(defaultDevice.id)
      })
      .catch(() => setAudioInputDevices([]))
    isLLMConfigured()
      .then((configured) => {
        setLlmConfigured(configured)
        setLlmReviewEnabled(configured)
        setAutoPromptEnabled(configured)
      })
      .catch(() => {
        setLlmConfigured(false)
        setLlmReviewEnabled(false)
        setAutoPromptEnabled(false)
      })
    getTencentMeetingConfigStatus()
      .then((status) => setTencentMeetingConfigured(status.configured))
      .catch(() => setTencentMeetingConfigured(false))
    getAsrConfigStatus().then(setAsrConfigStatus).catch(console.error)
  }, [])

  useEffect(() => {
    if (!recording || selectedAsrProvider !== "whisper") return
    const timer = window.setInterval(async () => {
      if (liveTranscribingRef.current) return
      liveTranscribingRef.current = true
      setLiveTranscribing(true)
      try {
        const result = await transcribeWhisperRecordingChunk(liveSampleRef.current)
        if (result.sample_count > liveSampleRef.current) {
          liveSampleRef.current = result.sample_count
        }
        if (result.text.trim()) {
          setLiveTranscript((prev) => {
            const nextText = result.text.trim()
            if (!prev.trim()) return nextText
            return `${prev.trim()}\n${nextText}`
          })
        }
      } catch (err) {
        console.error("实时转写失败:", err)
      } finally {
        liveTranscribingRef.current = false
        setLiveTranscribing(false)
      }
    }, 1800)
    return () => window.clearInterval(timer)
  }, [recording, selectedAsrProvider])

  // 流式输出时同步 AI 回答
  useEffect(() => {
    if (slot.loading) {
      const last = slot.messages[slot.messages.length - 1]
      if (last?.role === "assistant") {
        setNewAnswer(last.content)
      }
    }
  }, [slot.loading, slot.messages])

  const handleAIAssist = async (questionOverride?: string) => {
    const question = (questionOverride ?? newQuestion).trim()
    if (!question || aiLoading) return
    setCopilotTab("assistant")
    agent.clearSlot("research")
    setNewAnswer("")
    resetReportContext()
    const context = `当前调研：${session.title}（${session.edition}/${session.module_code}）\n已有记录：${records.map((r) => `Q: ${r.question_text}`).join("\n")}`
    const prompt = `请回答以下调研问题，基于知识库中的金蝶ERP实施经验。回答要具体、可操作，包含系统配置路径或单据类型；不确定的写[待确认]。\n\n问题：${question}\n\n背景：${context}`
    await agent.sendMessage("research", prompt, { projectId: currentProjectId })
  }

  useEffect(() => {
    const transcriptStreaming = recording || tencentMeetingSyncing
    if (!transcriptStreaming || activeTab !== "outline" || !autoPromptEnabled || !llmConfigured) {
      return
    }

    const transcript = liveTranscript.trim()
    if (transcript.length < 12) return

    const lastTranscript = lastAutoPromptTranscriptRef.current
    if (transcript === lastTranscript) return
    if (lastTranscript && transcript.length - lastTranscript.length < 8) return

    if (autoPromptTimerRef.current) {
      window.clearTimeout(autoPromptTimerRef.current)
    }

    autoPromptTimerRef.current = window.setTimeout(async () => {
      if (aiLoading) return
      const selectedNode = outline.nodes.find((node) => node.id === outline.selectedNodeId)
      const nodeContext = selectedNode
        ? `当前大纲节点：${selectedNode.content}`
        : "当前未选中大纲节点"
      const existingQuestions = records.map((record) => `Q: ${record.question_text}`).join("\n")
      const promptTitle = selectedNode ? `实时提词：${selectedNode.content}` : "实时提词"
      const prompt = [
        "你是金蝶 ERP 实施调研的实时提词助手。",
        "请根据当前客户口述转写，给实施顾问下一步追问建议。",
        "只输出 3 到 5 条可直接开口问客户的问题或核对点，优先围绕业务规则、系统配置、异常场景、主数据、审批流、单据流转。",
        "不要总结客户已经说过的话；不确定的内容写[待确认]；不要编造客户没有提到的事实。",
        "",
        `调研会话：${session.title}（${session.edition}/${session.module_code}）`,
        nodeContext,
        existingQuestions ? `已有调研问题：\n${existingQuestions}` : "已有调研问题：无",
        "",
        `实时转写草稿：\n${transcript}`,
      ].join("\n")

      lastAutoPromptTranscriptRef.current = transcript
      setNewQuestion(promptTitle)
      setCopilotTab("assistant")
      agent.clearSlot("research")
      setNewAnswer("")
      try {
        await agent.sendMessage("research", prompt, {
          projectId: currentProjectId,
          displayText: promptTitle,
        })
      } catch (err) {
        console.error("实时提词失败:", err)
      }
    }, 1600)

    return () => {
      if (autoPromptTimerRef.current) {
        window.clearTimeout(autoPromptTimerRef.current)
        autoPromptTimerRef.current = null
      }
    }
  }, [
    activeTab,
    aiLoading,
    autoPromptEnabled,
    currentProjectId,
    liveTranscript,
    llmConfigured,
    outline.nodes,
    outline.selectedNodeId,
    records,
    recording,
    session.edition,
    session.module_code,
    session.title,
    tencentMeetingSyncing,
    agent.clearSlot,
    agent.sendMessage,
  ])

  const handleStartRecording = async () => {
    setRecordingStarting(true)
    setCopilotTab("transcript")
    setLiveTranscript("")
    liveSampleRef.current = 0
    lastAutoPromptTranscriptRef.current = ""
    // 非 Whisper 模式（如腾讯 ASR）不需要加载本地模型
    if (selectedAsrProvider !== "whisper") {
      try {
        await startWhisperRecording(selectedAudioDeviceId || undefined)
        setRecording(true)
      } catch (err) {
        toast.error(`启动录音失败: ${String(err)}`)
      } finally {
        setRecordingStarting(false)
      }
      return
    }

    if (!whisperStatus?.model_loaded || whisperStatus.model_size !== "base") {
      setLoadingWhisper(true)
      try {
        await loadWhisperModel("base")
        const status = await getWhisperStatus()
        setWhisperStatus(status)
      } catch (err) {
        toast.error(`加载语音模型失败: ${String(err)}`)
        setLoadingWhisper(false)
        setRecordingStarting(false)
        return
      }
      setLoadingWhisper(false)
    }
    try {
      await startWhisperRecording(selectedAudioDeviceId || undefined)
      setRecording(true)
    } catch (err) {
      toast.error(`启动录音失败: ${String(err)}`)
    } finally {
      setRecordingStarting(false)
    }
  }

  const commitTranscriptText = async (text: string) => {
    const finalText = text.trim()
    if (!finalText) {
      toast.warning("未检测到可识别语音文本")
      return
    }

    let outputText = finalText
    if (llmConfigured && llmReviewEnabled) {
      setReviewingTranscript(true)
      try {
        const reviewedText = await reviewTranscriptionText(finalText)
        outputText = reviewedText.trim() || finalText
      } catch (err) {
        toast.warning(`LLM 校订失败，已使用原始转写: ${String(err)}`)
      } finally {
        setReviewingTranscript(false)
      }
    }

    if (activeTab === "outline") {
      setInsertTextTrigger({ text: outputText, timestamp: Date.now() })
      toast.success("转写内容已插入光标所在位置")
    } else {
      setNewQuestion(outputText)
    }
    setLiveTranscript("")
    liveSampleRef.current = 0
    lastAutoPromptTranscriptRef.current = ""
  }

  const handleStopRecording = async () => {
    try {
      const result =
        selectedAsrProvider === "tencent"
          ? await stopWhisperRecording("tencent")
          : await stopWhisperRecording()
      setRecording(false)
      const draftText = liveTranscript.trim()
      const stopText = result.text?.trim() || ""
      const finalText = [draftText, stopText].filter(Boolean).join("\n").trim()
      if (!finalText) {
        toast.warning("未检测到可识别语音，请确认已对着麦克风说话并适当延长录音时间")
        return
      }

      await commitTranscriptText(finalText)
    } catch (err) {
      setRecording(false)
      setReviewingTranscript(false)
      toast.error(`停止录音失败: ${String(err)}`)
    }
  }

  const syncTencentMeetingTranscript = useCallback(async () => {
    const meetingCode = tencentMeetingCode.trim()
    const recordFileId = tencentMeetingRecordFileId.trim()
    if (!meetingCode && !recordFileId) {
      toast.warning("请填写腾讯会议号或录制文件 ID")
      return ""
    }

    setTencentMeetingLoading(true)
    try {
      const result = await fetchTencentMeetingTranscript({
        meetingCode: meetingCode || undefined,
        recordFileId: recordFileId || undefined,
        includeMinutes: false,
      })
      if (result.record_file_id && !recordFileId) {
        setTencentMeetingRecordFileId(result.record_file_id)
      }
      const transcript = result.transcript.trim()
      if (transcript) {
        setLiveTranscript(transcript)
      }
      return transcript
    } catch (err) {
      toast.error(`腾讯会议转写同步失败: ${String(err)}`)
      return ""
    } finally {
      setTencentMeetingLoading(false)
    }
  }, [tencentMeetingCode, tencentMeetingRecordFileId, toast])

  useEffect(() => {
    if (!tencentMeetingSyncing) return
    const timer = window.setInterval(() => {
      void syncTencentMeetingTranscript()
    }, 6000)
    return () => window.clearInterval(timer)
  }, [tencentMeetingSyncing, syncTencentMeetingTranscript])

  const handleStartTencentMeetingSync = async () => {
    if (!tencentMeetingConfigured) {
      toast.warning("请先在设置中配置腾讯会议 MCP Token")
      return
    }
    setCopilotTab("transcript")
    setLiveTranscript("")
    lastAutoPromptTranscriptRef.current = ""
    const transcript = await syncTencentMeetingTranscript()
    if (transcript) {
      setTencentMeetingSyncing(true)
    }
  }

  const handleStopTencentMeetingSync = async () => {
    setTencentMeetingSyncing(false)
    const transcript = (await syncTencentMeetingTranscript()) || liveTranscript
    await commitTranscriptText(transcript)
  }

  // 渲染 AI Copilot 工具箱侧边栏
  const renderCopilotPanel = () => {
    const selectedNode = outline.nodes.find((n) => n.id === outline.selectedNodeId)

    return (
      <div className="flex w-80 shrink-0 flex-col border-l border-neutral-200 bg-neutral-50 p-4 min-h-0 overflow-hidden gap-3">
        <div className="grid shrink-0 grid-cols-2 gap-1 rounded-lg border border-neutral-200 bg-white p-1">
          <button
            type="button"
            onClick={() => setCopilotTab("assistant")}
            className={`flex min-h-8 items-center justify-center gap-1.5 rounded-md px-2 text-xs font-medium transition-colors ${
              copilotTab === "assistant"
                ? "bg-amber-50 text-amber-700"
                : "text-neutral-500 hover:bg-neutral-50"
            }`}
          >
            <Brain className="h-3.5 w-3.5" />
            AI 提词
            {newAnswer && <span className="h-1.5 w-1.5 rounded-full bg-amber-500" />}
          </button>
          <button
            type="button"
            onClick={() => setCopilotTab("transcript")}
            className={`flex min-h-8 items-center justify-center gap-1.5 rounded-md px-2 text-xs font-medium transition-colors ${
              copilotTab === "transcript"
                ? "bg-[#1A6BD8]/10 text-[#1A6BD8]"
                : "text-neutral-500 hover:bg-neutral-50"
            }`}
          >
            <Mic className="h-3.5 w-3.5" />
            语音转写
            {(recording || tencentMeetingSyncing) && (
              <span className="h-1.5 w-1.5 rounded-full bg-green-500" />
            )}
          </button>
        </div>

        <div
          className={copilotTab === "transcript" ? "min-h-0 flex-1 overflow-y-auto pr-1" : "hidden"}
        >
          {/* 语音录音 */}
          <div className="rounded-lg border border-neutral-200 bg-white p-4">
            <div className="mb-2 flex items-center gap-2">
              <Mic className="h-4 w-4 text-[#1A6BD8]" />
              <span className="text-xs font-semibold text-neutral-700">语音录音控制</span>
            </div>
            {/* ASR 供应商选择 */}
            <div className="mb-2">
              <select
                value={selectedAsrProvider}
                onChange={(e) => setSelectedAsrProvider(e.target.value)}
                className="w-full rounded border border-neutral-200 px-2 py-1 text-[10px] text-neutral-600 outline-none focus:border-[#1A6BD8]"
              >
                {/* Whisper 是内置默认项，list_asr_providers 不再包含它 */}
                <option value="whisper">
                  本地 Whisper {whisperStatus?.model_loaded ? "✓" : "⚠"}
                </option>
                {asrProviders.map((p) => (
                  <option key={p.kind} value={p.kind}>
                    {p.name}
                  </option>
                ))}
              </select>
              {/* 供应商描述说明 */}
              {selectedAsrProvider === "whisper" ? (
                <p className="text-[10px] text-neutral-400 mt-0.5">
                  本地离线语音识别，无需网络，支持中文/英文。需要先下载模型（约 80MB）。首次使用时自动下载。
                </p>
              ) : (
                asrProviders.find((p) => p.kind === selectedAsrProvider) && (
                  <p className="text-[10px] text-neutral-400 mt-0.5">
                    {asrProviders.find((p) => p.kind === selectedAsrProvider)?.description}
                  </p>
                )
              )}
            </div>
            {whisperStatus && !whisperStatus.model_loaded && selectedAsrProvider === "whisper" && (
              <span className="text-[10px] text-amber-600">（模型未加载）</span>
            )}
            <div className="mb-2">
              <select
                value={selectedAudioDeviceId}
                onChange={(e) => setSelectedAudioDeviceId(e.target.value)}
                disabled={recording || recordingStarting}
                className="w-full rounded border border-neutral-200 px-2 py-1 text-[10px] text-neutral-600 outline-none focus:border-[#1A6BD8] disabled:opacity-60"
              >
                <option value="">自动选择麦克风</option>
                {audioInputDevices.map((device) => (
                  <option key={device.id} value={device.id}>
                    {device.name}
                    {device.host ? ` - ${device.host}` : ""}
                    {device.is_default ? "（系统默认）" : ""}
                  </option>
                ))}
              </select>
              {audioInputDevices.length === 0 && (
                <p className="mt-0.5 text-[10px] text-red-500">
                  未枚举到麦克风设备，请检查系统输入设备和应用权限。
                </p>
              )}
            </div>
            <div className="mb-2 grid grid-cols-2 gap-2 text-[10px] text-neutral-600">
              <label className="flex items-center gap-1.5">
                <input
                  type="checkbox"
                  checked={autoPromptEnabled}
                  onChange={(event) => setAutoPromptEnabled(event.target.checked)}
                  disabled={!llmConfigured}
                  className="h-3 w-3 accent-[#1A6BD8]"
                />
                LLM 实时提词
              </label>
              <label className="flex items-center gap-1.5">
                <input
                  type="checkbox"
                  checked={llmReviewEnabled}
                  onChange={(event) => setLlmReviewEnabled(event.target.checked)}
                  disabled={!llmConfigured}
                  className="h-3 w-3 accent-[#1A6BD8]"
                />
                LLM 校订
              </label>
            </div>
            {!llmConfigured && (
              <p className="mb-2 text-[10px] text-amber-600">
                当前未配置 LLM，实时提词和校订已停用。
              </p>
            )}
            {loadingWhisper || recordingStarting || reviewingTranscript ? (
              <div className="flex items-center gap-2 text-xs text-neutral-500">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {loadingWhisper
                  ? "加载语音模型..."
                  : reviewingTranscript
                    ? "LLM 校订中..."
                    : "检测麦克风..."}
              </div>
            ) : recording ? (
              <button
                type="button"
                onClick={handleStopRecording}
                className="flex w-full items-center justify-center gap-2 rounded-lg bg-red-500 px-3 py-2 text-xs font-medium text-white hover:bg-red-600 transition-colors"
              >
                <Square className="h-3.5 w-3.5" />
                停止录音
              </button>
            ) : (
              <button
                type="button"
                onClick={handleStartRecording}
                className="flex w-full items-center justify-center gap-2 rounded-lg border border-[#1A6BD8] px-3 py-2 text-xs font-medium text-[#1A6BD8] hover:bg-[#1A6BD8]/5 transition-colors"
              >
                <Mic className="h-3.5 w-3.5" />
                开始录音
              </button>
            )}
            {recording && (
              <div className="mt-2 rounded border border-neutral-100 bg-neutral-50 p-2">
                <div className="mb-1 flex items-center justify-between text-[10px] text-neutral-400">
                  <span>实时转写草稿</span>
                  {liveTranscribing && <span>转写中...</span>}
                </div>
                <p className="max-h-24 overflow-y-auto whitespace-pre-wrap text-[11px] leading-relaxed text-neutral-600">
                  {liveTranscript || "开始说话后会自动出现文本"}
                </p>
              </div>
            )}
            <div className="mt-3 border-t border-neutral-100 pt-3">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-[10px] font-semibold text-neutral-500">腾讯会议线上转写</span>
                {tencentMeetingSyncing && (
                  <span className="text-[10px] text-green-600">同步中</span>
                )}
              </div>
              <div className="space-y-2">
                <input
                  value={tencentMeetingCode}
                  onChange={(event) => setTencentMeetingCode(event.target.value)}
                  disabled={tencentMeetingSyncing}
                  placeholder="会议号"
                  className="w-full rounded border border-neutral-200 px-2 py-1 text-[10px] text-neutral-600 outline-none focus:border-[#1A6BD8] disabled:opacity-60"
                />
                <input
                  value={tencentMeetingRecordFileId}
                  onChange={(event) => setTencentMeetingRecordFileId(event.target.value)}
                  disabled={tencentMeetingSyncing}
                  placeholder="录制文件 ID"
                  className="w-full rounded border border-neutral-200 px-2 py-1 text-[10px] text-neutral-600 outline-none focus:border-[#1A6BD8] disabled:opacity-60"
                />
                {tencentMeetingSyncing ? (
                  <button
                    type="button"
                    onClick={() => void handleStopTencentMeetingSync()}
                    disabled={tencentMeetingLoading || reviewingTranscript}
                    className="flex w-full items-center justify-center gap-2 rounded-lg bg-red-500 px-3 py-2 text-xs font-medium text-white hover:bg-red-600 disabled:opacity-50 transition-colors"
                  >
                    {tencentMeetingLoading || reviewingTranscript ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Square className="h-3.5 w-3.5" />
                    )}
                    停止线上同步
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => void handleStartTencentMeetingSync()}
                    disabled={recording || tencentMeetingLoading || !tencentMeetingConfigured}
                    className="flex w-full items-center justify-center gap-2 rounded-lg border border-emerald-600 px-3 py-2 text-xs font-medium text-emerald-700 hover:bg-emerald-50 disabled:opacity-50 transition-colors"
                  >
                    {tencentMeetingLoading ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Network className="h-3.5 w-3.5" />
                    )}
                    开始线上同步
                  </button>
                )}
                {!tencentMeetingConfigured && (
                  <p className="text-[10px] text-amber-600">请先在设置中配置腾讯会议 MCP Token。</p>
                )}
              </div>
            </div>
            {tencentMeetingSyncing && (
              <div className="mt-2 rounded border border-neutral-100 bg-neutral-50 p-2">
                <div className="mb-1 flex items-center justify-between text-[10px] text-neutral-400">
                  <span>线上转写草稿</span>
                  {tencentMeetingLoading && <span>同步中...</span>}
                </div>
                <p className="max-h-24 overflow-y-auto whitespace-pre-wrap text-[11px] leading-relaxed text-neutral-600">
                  {liveTranscript || "等待腾讯会议转写生成"}
                </p>
              </div>
            )}
            {activeTab === "outline" && (
              <p className="text-[10px] text-neutral-400 mt-1.5 text-center">
                录音结束将自动插入中栏编辑器光标位置
              </p>
            )}
          </div>
        </div>

        {/* AI 提词与辅助 */}
        <div
          className={
            copilotTab === "assistant" ? "flex min-h-0 flex-1 flex-col space-y-3" : "hidden"
          }
        >
          <div className="flex items-center gap-2">
            <Brain className="h-4 w-4 text-amber-600" />
            <span className="text-xs font-semibold text-neutral-700">AI 智能提词器</span>
          </div>

          {/* 提词自动触发快捷按钮 */}
          {activeTab === "outline" && (
            <div className="space-y-2">
              {selectedNode ? (
                <div className="rounded-lg border border-blue-100 bg-blue-50/50 p-3 text-xs space-y-2">
                  <p className="font-semibold text-neutral-700">
                    当前节点:{" "}
                    <span className="text-[#1A6BD8] font-bold">{selectedNode.content}</span>
                  </p>
                  <div className="flex gap-2">
                    <button
                      type="button"
                      onClick={() => {
                        const question = `针对大纲节点“${selectedNode.content}”，我应该向客户调研哪些核心业务问题？请从系统配置、业务规则、蓝图规划这几个维度列出具体的提问大纲和要点。`
                        setNewQuestion(question)
                        void handleAIAssist(question)
                      }}
                      disabled={aiLoading}
                      className="flex-1 rounded bg-[#1A6BD8]/10 px-2 py-1.5 text-[10px] font-medium text-[#1A6BD8] hover:bg-[#1A6BD8]/20 transition-colors text-center"
                    >
                      💡 获取调研提词
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        const question = `针对大纲节点“${selectedNode.content}”，如果在金蝶云·星空中我应当如何进行功能配置？是使用标准功能配置（如工作流、单据转换）还是需要二次开发？请给出配置路径。`
                        setNewQuestion(question)
                        void handleAIAssist(question)
                      }}
                      disabled={aiLoading}
                      className="flex-1 rounded bg-amber-50 px-2 py-1.5 text-[10px] font-medium text-amber-700 border border-amber-200 hover:bg-amber-100 transition-colors text-center"
                    >
                      🤖 获取金蝶方案
                    </button>
                  </div>
                </div>
              ) : (
                <div className="rounded-lg border border-dashed border-neutral-200 bg-white p-3 text-center text-[11px] text-neutral-400 italic">
                  请在大纲树中选中节点以激活智能提词
                </div>
              )}
            </div>
          )}

          {/* AI 自由提问输入 */}
          <div className="space-y-2">
            <textarea
              value={newQuestion}
              onChange={(e) => setNewQuestion(e.target.value)}
              placeholder="输入或语音录入问题进行 AI 辅助..."
              rows={3}
              className="w-full resize-none rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20"
            />
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => void handleAIAssist()}
                disabled={!newQuestion.trim() || aiLoading}
                className="flex-1 flex items-center justify-center gap-1.5 rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50 transition-colors"
              >
                {aiLoading ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Brain className="h-3.5 w-3.5" />
                )}
                {aiLoading ? "AI 检索中..." : "AI 辅助提词"}
              </button>
            </div>
          </div>

          {/* AI 流式结果渲染区 */}
          {newAnswer && (() => {
            const lastMsg = slot.messages[slot.messages.length - 1]
            const isReport = reportContextRef.current
            const isError = lastMsg?.role === "assistant" && lastMsg.error === true
            return (
            <div className="rounded-lg border border-neutral-200 bg-white p-3 space-y-2 flex min-h-0 flex-1 flex-col overflow-hidden">
              <div className="flex items-center justify-between">
                <span className="text-[10px] font-bold text-neutral-400 uppercase">
                  {isReport ? "调研报告（AI 生成，请审阅）" : "AI 助手响应"}
                </span>
                {isReport && !aiLoading && !isError && (
                  <span className="rounded bg-blue-50 px-1.5 py-0.5 text-[10px] text-blue-600">
                    📋 4 段结构
                  </span>
                )}
              </div>
              {isReport && isError && (
                <div className="rounded border border-red-200 bg-red-50 px-2 py-1.5 text-[10px] text-red-700 flex items-center justify-between">
                  <span>⚠️ 生成失败，可点击重试沿用原 prompt 再次发起</span>
                  <button
                    type="button"
                    onClick={handleRetryReport}
                    disabled={aiLoading || !lastReportPromptRef.current}
                    className="rounded border border-red-300 bg-white px-2 py-0.5 text-[10px] font-medium text-red-700 hover:bg-red-100 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    重试
                  </button>
                </div>
              )}
              <div className="flex-1 overflow-y-auto text-xs prose prose-sm leading-relaxed max-w-none text-neutral-700 bg-neutral-50 p-2 rounded border border-neutral-100">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{newAnswer}</ReactMarkdown>
              </div>
              <div className="flex gap-1.5 pt-1 border-t border-neutral-100">
                {!isReport && (activeTab === "outline" ? (
                  <button
                    type="button"
                    onClick={() => {
                      setInsertTextTrigger({ text: `\n\n${newAnswer}`, timestamp: Date.now() })
                      toast.success("内容已插入到编辑器当前光标处")
                    }}
                    className="flex-1 text-center bg-[#1A6BD8] text-white rounded py-1 text-[10px] font-medium hover:bg-[#1558B0] transition-colors"
                  >
                    插入中栏光标
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={handleAddRecord}
                    disabled={!newQuestion.trim()}
                    className="flex-1 text-center bg-[#1A6BD8] text-white rounded py-1 text-[10px] font-medium hover:bg-[#1558B0] transition-colors"
                  >
                    保存为问答记录
                  </button>
                ))}
                {isReport && newAnswer.trim() && !isError && (
                  <button
                    type="button"
                    onClick={handleSaveReport}
                    className="flex-1 text-center bg-[#1A6BD8] text-white rounded py-1 text-[10px] font-medium hover:bg-[#1558B0] transition-colors"
                  >
                    保存报告
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => {
                    navigator.clipboard
                      .writeText(newAnswer)
                      .then(() => toast.success("已复制到剪贴板"))
                      .catch((err) => toast.error(`复制失败: ${String(err)}`))
                  }}
                  className="rounded border border-neutral-200 px-2 py-1 text-[10px] text-neutral-600 hover:bg-neutral-50 transition-colors"
                >
                  复制
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setNewAnswer("")
                    resetReportContext()
                  }}
                  className="rounded border border-neutral-200 px-2 py-1 text-[10px] text-red-500 hover:bg-red-50 transition-colors hover:border-red-200"
                >
                  清空
                </button>
              </div>
            </div>
            )
          })()}
        </div>
      </div>
    )
  }

  const handleAddRecord = async () => {
    if (!newQuestion.trim()) return
    try {
      await addQARecord(session.id, null, newQuestion.trim(), newAnswer.trim(), "", records.length)
      setNewQuestion("")
      setNewAnswer("")
      resetReportContext()
      onUpdated()
    } catch (err) {
      toast.error(String(err))
    }
  }

  const handleSaveReport = async () => {
    if (!newAnswer.trim()) return
    try {
      const dest = await save({
        defaultPath: `调研报告_${session.title}.md`,
        filters: [{ name: "Markdown", extensions: ["md"] }],
      })
      if (dest) {
        const savedPath = await invoke<string>("export_report", { content: newAnswer, filePath: dest })
        toast.success(`已保存到：${savedPath}`)
      }
    } catch (err) {
      toast.error(`保存调研报告失败: ${String(err)}`)
    }
  }

  const handleUpdateRecord = async (recordId: number) => {
    try {
      await updateQARecord(recordId, editAnswer, "")
      setEditingRecord(null)
      onUpdated()
    } catch (err) {
      toast.error(String(err))
    }
  }

  const handleDeleteRecord = async (recordId: number) => {
    if (!confirm("确认删除此记录？")) return
    try {
      await deleteQARecord(recordId)
      onUpdated()
    } catch (err) {
      toast.error(String(err))
    }
  }

  const handleExportCsv = async () => {
    try {
      const csv = await exportSessionCsv(session.id)
      const dest = await save({
        defaultPath: `调研记录_${session.title}_${session.session_date}.csv`,
        filters: [{ name: "CSV", extensions: ["csv"] }],
      })
      if (dest) await invoke("export_report", { content: csv, filePath: dest })
    } catch (err) {
      toast.error(String(err))
    }
  }

  const handleExportMd = async () => {
    try {
      const md = await exportSessionMarkdown(session.id)
      const dest = await save({
        defaultPath: `调研记录_${session.title}_${session.session_date}.md`,
        filters: [{ name: "Markdown", extensions: ["md"] }],
      })
      if (dest) await invoke("export_report", { content: md, filePath: dest })
    } catch (err) {
      toast.error(String(err))
    }
  }

  // 调研报告配方：后端 prompts::RECIPE_INVESTIGATION 同源，启动时拉取避免漂移
  const [reportRecipe, setReportRecipe] = useState<string>("")

  const buildReportPrompt = (): string => {
    const qaText = records
      .map(
        (r, i) =>
          `Q${i + 1}: ${r.question_text}\nA: ${r.answer_text}${r.notes ? `\n备注: ${r.notes}` : ""}`,
      )
      .join("\n\n")
    return [
      `${reportRecipe}`,
      "",
      "请先用 use-skill 加载 survey-assistant 技能（action=load, name_or_query=survey-assistant），按其 Step 4「生成调研报告」指引，结合下方调研记录输出。",
      "",
      `调研会话：${session.title}（${session.edition}/${session.module_code}）`,
      `访谈对象：${session.interviewee || "未填写"}`,
      `访谈日期：${session.session_date || "未填写"}`,
      "",
      "【调研记录】",
      qaText,
    ].join("\n")
  }

  const sendReportPrompt = async (prompt: string): Promise<boolean> => {
    reportContextRef.current = true
    try {
      agent.clearSlot("research")
      setCopilotTab("assistant")
      await agent.sendMessage("research", prompt, { projectId: currentProjectId })
      return true
    } catch (err) {
      toast.error(`生成调研报告失败: ${String(err)}`)
      return false
    }
  }

  // 计算当前 records 的指纹 hash（id+answer+notes），用于重试时漂移检测
  const computeQaHash = (): string => {
    const concat = records
      .map((r) => `${r.id}|${r.answer_text}|${r.notes}`)
      .join("\u0001")
    // djb2 字符串 hash，不要求密码学安全
    let h = 5381
    for (let i = 0; i < concat.length; i++) {
      h = ((h * 33) ^ concat.charCodeAt(i)) >>> 0
    }
    return h.toString(16)
  }

  const handleGenerateReport = async () => {
    if (aiLoading) return
    if (records.length === 0) {
      toast.warning("暂无调研记录，无法生成报告")
      return
    }
    const prompt = buildReportPrompt()
    lastReportPromptRef.current = prompt
    lastReportQaHashRef.current = computeQaHash()
    await sendReportPrompt(prompt)
  }

  const handleRetryReport = async () => {
    if (aiLoading) return
    if (!lastReportPromptRef.current) return
    if (records.length === 0) {
      // 记录中途被删光，prompt 中的 Q&A 已失效，禁用重试
      toast.warning("调研记录已清空，无法重试")
      return
    }
    // 漂移检测：当前 records 指纹与上次生成时不一致 → 提示
    const currentHash = computeQaHash()
    if (lastReportQaHashRef.current && lastReportQaHashRef.current !== currentHash) {
      toast.warning("调研记录已变更，重试将沿用原 prompt（旧 Q&A 快照）")
    }
    await sendReportPrompt(lastReportPromptRef.current)
  }

  // 切换 tab、用户发新提问、清空报告时，重置报告上下文
  const resetReportContext = () => {
    reportContextRef.current = false
    lastReportPromptRef.current = ""
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* 页头 */}
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onBack}
            className="flex items-center gap-1 text-xs text-neutral-500 hover:text-neutral-700 transition-colors"
          >
            <ChevronLeft className="h-4 w-4" />
            返回
          </button>
          <span className="text-sm text-neutral-300">|</span>
          <BookOpen className="h-5 w-5 text-[#1A6BD8]" />
          <h1 className="text-base font-semibold text-neutral-800">{session.title}</h1>
          <span className="rounded bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">
            {session.edition}/{session.module_code}
          </span>
          {session.status === "completed" && (
            <span className="rounded bg-green-100 px-1.5 py-0.5 text-[10px] text-green-700">
              已完成
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={handleExportCsv}
            className="flex items-center gap-1 rounded-lg border border-neutral-200 px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50 transition-colors"
          >
            <Download className="h-3.5 w-3.5" /> CSV
          </button>
          <button
            type="button"
            onClick={handleExportMd}
            className="flex items-center gap-1 rounded-lg border border-neutral-200 px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50 transition-colors"
          >
            <FileText className="h-3.5 w-3.5" /> MD
          </button>
          <button
            type="button"
            onClick={handleGenerateReport}
            disabled={aiLoading || records.length === 0}
            title={records.length === 0 ? "暂无调研记录" : "基于当前调研记录生成 4 段结构报告"}
            className="flex items-center gap-1 rounded-lg border border-blue-200 px-3 py-1.5 text-xs text-blue-600 hover:bg-blue-50 disabled:cursor-not-allowed disabled:opacity-50 transition-colors"
          >
            <FileText className="h-3.5 w-3.5" /> 生成调研报告
          </button>
        </div>
      </div>

      {/* 标签导航 */}
      <div className="flex border-b border-neutral-200 px-6">
        <button
          type="button"
          onClick={() => setActiveTab("outline")}
          className={`flex items-center gap-1.5 border-b-2 px-4 py-2.5 text-xs font-medium transition-colors ${
            activeTab === "outline"
              ? "border-[#1A6BD8] text-[#1A6BD8]"
              : "border-transparent text-neutral-500 hover:text-neutral-700"
          }`}
        >
          <ListTodo className="h-3.5 w-3.5" />
          大纲视图
        </button>
        <button
          type="button"
          onClick={() => setActiveTab("mindmap")}
          className={`flex items-center gap-1.5 border-b-2 px-4 py-2.5 text-xs font-medium transition-colors ${
            activeTab === "mindmap"
              ? "border-[#1A6BD8] text-[#1A6BD8]"
              : "border-transparent text-neutral-500 hover:text-neutral-700"
          }`}
        >
          <Network className="h-3.5 w-3.5" />
          脑图视图
        </button>
        <button
          type="button"
          onClick={() => setActiveTab("qa")}
          className={`flex items-center gap-1.5 border-b-2 px-4 py-2.5 text-xs font-medium transition-colors ${
            activeTab === "qa"
              ? "border-[#1A6BD8] text-[#1A6BD8]"
              : "border-transparent text-neutral-500 hover:text-neutral-700"
          }`}
        >
          <MessageSquare className="h-3.5 w-3.5" />
          问答记录
        </button>
      </div>

      {/* 内容 */}
      {activeTab === "qa" ? (
        <div className="flex flex-1 overflow-hidden">
          {/* 问答记录 */}
          <div className="flex-1 overflow-y-auto p-6">
            {records.length === 0 ? (
              <div className="flex flex-col items-center justify-center pt-16 text-center">
                <MessageSquare className="mb-3 h-10 w-10 text-neutral-200" />
                <p className="text-sm text-neutral-400">暂无记录，使用录音或手动添加问题</p>
              </div>
            ) : (
              <div className="space-y-3">
                {records.map((r, i) => (
                  <div key={r.id} className="rounded-lg border border-neutral-200 bg-white p-4">
                    <div className="mb-2 flex items-start justify-between">
                      <span className="text-xs font-medium text-[#1A6BD8]">Q{i + 1}</span>
                      <div className="flex gap-1">
                        <button
                          type="button"
                          onClick={() => {
                            setEditingRecord(r.id)
                            setEditAnswer(r.answer_text)
                          }}
                          className="rounded p-1 text-neutral-300 hover:text-[#1A6BD8] transition-colors"
                        >
                          <Edit3 className="h-3.5 w-3.5" />
                        </button>
                        <button
                          type="button"
                          onClick={() => handleDeleteRecord(r.id)}
                          className="rounded p-1 text-neutral-300 hover:text-red-500 transition-colors"
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    </div>
                    <p className="mb-2 text-sm font-medium text-neutral-800">{r.question_text}</p>
                    {editingRecord === r.id ? (
                      <div className="space-y-2">
                        <textarea
                          value={editAnswer}
                          onChange={(e) => setEditAnswer(e.target.value)}
                          rows={2}
                          className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-[#1A6BD8]"
                        />
                        <div className="flex gap-2">
                          <button
                            type="button"
                            onClick={() => handleUpdateRecord(r.id)}
                            className="rounded bg-[#1A6BD8] px-3 py-1 text-xs text-white hover:bg-[#1558B0]"
                          >
                            保存
                          </button>
                          <button
                            type="button"
                            onClick={() => setEditingRecord(null)}
                            className="rounded border border-neutral-200 px-3 py-1 text-xs text-neutral-600 hover:bg-neutral-50"
                          >
                            取消
                          </button>
                        </div>
                      </div>
                    ) : (
                      <p className="text-xs leading-relaxed text-neutral-600">
                        {r.answer_text || (
                          <span className="italic text-neutral-300">未填写回答</span>
                        )}
                      </p>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      ) : activeTab === "outline" ? (
        /* 大纲视图：左树只读 + 中栏 Markdown 编辑 + 右栏 AI Copilot */
        <div className="flex flex-1 overflow-hidden">
          {/* 大纲树（左侧，只读） */}
          <div className="w-72 shrink-0 overflow-y-auto border-r border-neutral-200 bg-white p-2">
            <OutlineTree sessionId={session.id} />
          </div>

          {/* Markdown 编辑区（中栏） */}
          <div className="flex flex-1 flex-col overflow-hidden">
            <div className="flex-1 overflow-hidden">
              <MarkdownEditor sessionId={session.id} insertTextTrigger={insertTextTrigger} />
            </div>
          </div>

          {/* 右侧 Copilot 面板 */}
          {renderCopilotPanel()}
        </div>
      ) : (
        /* 脑图视图 */
        <div className="relative flex-1 overflow-hidden">
          <MindmapView sessionId={session.id} />
        </div>
      )}
    </div>
  )
}
