import { Brain, Loader2, Mic, Network, Square } from "lucide-react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import type { AgentSlot } from "@/contexts/AgentContext"
import type { OutlineNode } from "@/lib/outline-commands"
import type { AsrProviderInfo, AudioInputDeviceInfo, WhisperStatus } from "@/lib/tauri-commands"

// ── Props 接口 ──────────────────────────────────────────────────────────────

export interface CopilotPanelProps {
  // Tab
  copilotTab: "assistant" | "transcript"
  setCopilotTab: (tab: "assistant" | "transcript") => void

  // 录音
  recording: boolean
  asrProviders: AsrProviderInfo[]
  selectedAsrProvider: string
  setSelectedAsrProvider: (provider: string) => void
  whisperStatus: WhisperStatus | null
  audioInputDevices: AudioInputDeviceInfo[]
  selectedAudioDeviceId: string
  setSelectedAudioDeviceId: (deviceId: string) => void
  liveTranscript: string
  liveTranscribing: boolean
  autoPromptEnabled: boolean
  setAutoPromptEnabled: (enabled: boolean) => void
  llmConfigured: boolean
  llmReviewEnabled: boolean
  setLlmReviewEnabled: (enabled: boolean) => void
  loadingWhisper: boolean
  recordingStarting: boolean
  reviewingTranscript: boolean

  // AI 提词
  activeTab: "qa" | "outline" | "mindmap"
  aiLoading: boolean
  newQuestion: string
  setNewQuestion: (question: string) => void
  newAnswer: string
  setNewAnswer: (answer: string) => void

  // Agent slot & 报告上下文
  slot: AgentSlot
  reportContextRef: React.MutableRefObject<boolean>
  lastReportPromptRef: React.MutableRefObject<string>

  // 回调函数
  handleAIAssist: (questionOverride?: string) => void
  handleStartRecording: () => void
  handleStopRecording: () => void
  handleAddRecord: () => void
  handleSaveReport: () => void
  handleRetryReport: () => void
  resetReportContext: () => void

  // Agent 控制
  cancelSession: (slotId: string) => Promise<void>

  // 大纲
  outlineNodes: OutlineNode[]
  outlineSelectedNodeId: number | null

  // Toast
  toast: {
    success: (msg: string) => void
    warning: (msg: string) => void
    error: (msg: string) => void
  }

  // 插入到编辑器光标处
  onInsertToCursor: (text: string) => void
}

// ── 组件 ────────────────────────────────────────────────────────────────────

export default function CopilotPanel(props: CopilotPanelProps) {
  const {
    copilotTab,
    setCopilotTab,
    recording,
    asrProviders,
    selectedAsrProvider,
    setSelectedAsrProvider,
    whisperStatus,
    audioInputDevices,
    selectedAudioDeviceId,
    setSelectedAudioDeviceId,
    liveTranscript,
    liveTranscribing,
    autoPromptEnabled,
    setAutoPromptEnabled,
    llmConfigured,
    llmReviewEnabled,
    setLlmReviewEnabled,
    loadingWhisper,
    recordingStarting,
    reviewingTranscript,
    activeTab,
    aiLoading,
    newQuestion,
    setNewQuestion,
    newAnswer,
    setNewAnswer,
    slot,
    reportContextRef,
    lastReportPromptRef,
    handleAIAssist,
    handleStartRecording,
    handleStopRecording,
    handleAddRecord,
    handleSaveReport,
    handleRetryReport,
    resetReportContext,
    cancelSession,
    outlineNodes,
    outlineSelectedNodeId,
    toast,
    onInsertToCursor,
  } = props

  const selectedNode = outlineNodes.find((n) => n.id === outlineSelectedNodeId)

  return (
    <div className="flex w-80 shrink-0 flex-col border-l border-neutral-200 bg-neutral-50 p-4 min-h-0 overflow-hidden gap-3">
      {/* Tab 切换 */}
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
          {recording && <span className="h-1.5 w-1.5 rounded-full bg-green-500" />}
        </button>
      </div>

      {/* ── 语音转写面板 ── */}
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
                本地离线语音识别，无需网络，支持中文/英文。需要先下载模型（约
                80MB）。首次使用时自动下载。
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
            <div className="flex items-center justify-between">
              <span className="text-[10px] font-semibold text-neutral-500">腾讯会议</span>
            </div>
            <p className="mt-1.5 text-[10px] text-neutral-400">
              会议同步与纪要生成已迁移至会议管理页面。
            </p>
            <a
              href="/meetings"
              className="mt-1.5 flex items-center gap-1 rounded border border-neutral-200 px-2 py-1.5 text-[10px] font-medium text-[#1A6BD8] hover:bg-blue-50 transition-colors"
            >
              <Network className="h-3 w-3" />
              前往会议管理
            </a>
          </div>
          {activeTab === "outline" && (
            <p className="text-[10px] text-neutral-400 mt-1.5 text-center">
              录音结束将自动插入中栏编辑器光标位置
            </p>
          )}
        </div>
      </div>

      {/* ── AI 提词与辅助面板 ── */}
      <div
        className={copilotTab === "assistant" ? "flex min-h-0 flex-1 flex-col space-y-3" : "hidden"}
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
                  当前节点: <span className="text-[#1A6BD8] font-bold">{selectedNode.content}</span>
                </p>
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => {
                      const question = `针对大纲节点\u201c${selectedNode.content}\u201d，我应该向客户调研哪些核心业务问题？请从系统配置、业务规则、蓝图规划这几个维度列出具体的提问大纲和要点。`
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
                      const question = `针对大纲节点\u201c${selectedNode.content}\u201d，如果在金蝶云·星空中我应当如何进行功能配置？是使用标准功能配置（如工作流、单据转换）还是需要二次开发？请给出配置路径。`
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
            {aiLoading && (
              <button
                type="button"
                onClick={() => void cancelSession("research")}
                className="flex items-center justify-center gap-1.5 rounded-lg border border-red-300 px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 transition-colors"
              >
                <Square className="h-3.5 w-3.5" />
                停止
              </button>
            )}
          </div>
        </div>

        {/* AI 流式结果渲染区 */}
        {newAnswer &&
          (() => {
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
                {slot.currentTrace?.thinking && (
                  <details className="text-xs text-amber-700 italic leading-relaxed">
                    <summary className="cursor-pointer text-amber-500 not-italic font-medium">
                      💭 推理过程
                    </summary>
                    {slot.currentTrace.thinking.length > 2000
                      ? `...${slot.currentTrace.thinking.slice(-2000)}`
                      : slot.currentTrace.thinking}
                  </details>
                )}
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
                  {!isReport &&
                    (activeTab === "outline" ? (
                      <button
                        type="button"
                        onClick={() => {
                          onInsertToCursor(newAnswer)
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
