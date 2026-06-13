import {
  AlertCircle,
  Calendar,
  CalendarPlus,
  ChevronRight,
  Clock,
  ExternalLink,
  FileText,
  Loader2,
  Mic,
  RefreshCw,
  Search,
  Sparkles,
  X,
} from "lucide-react"
import { useCallback, useEffect, useState } from "react"
import { useToast } from "../components/Toast"
import { useProject } from "../contexts/ProjectContext"
import { PRODUCT_NAME } from "../lib/constants"
import { formatAppError } from "../lib/app-error"
import {
  cancelTencentMeeting,
  convertTencentMeetingTimestamp,
  fetchTencentMeetingTranscript,
  getTencentMeeting,
  getTencentMeetingConfigStatus,
  listTencentUserEndedMeetings,
  listTencentUserMeetings,
  scheduleTencentMeeting,
} from "../lib/tauri-commands"

interface MeetingItem {
  meeting_id?: string
  meeting_code?: string
  subject?: string
  start_time?: string
  end_time?: string
  status?: string
  host?: { nick_name?: string; user_id?: string }
  [key: string]: unknown
}

type Tab = "upcoming" | "history"

function extractMeetings(payload: unknown): MeetingItem[] {
  if (!payload) return []
  const visit = (value: unknown, depth: number): MeetingItem | null => {
    if (depth > 5 || !value || typeof value !== "object") return null
    const obj = value as Record<string, unknown>
    if (
      (typeof obj.meeting_id === "string" || typeof obj.meeting_code === "string") &&
      (typeof obj.subject === "string" || typeof obj.start_time === "string")
    ) {
      return obj as MeetingItem
    }
    if (Array.isArray(obj.meeting_info_list)) {
      for (const child of obj.meeting_info_list) {
        const found = visit(child, depth + 1)
        if (found) return found
      }
    }
    if (Array.isArray(obj.meeting_list)) {
      for (const child of obj.meeting_list) {
        const found = visit(child, depth + 1)
        if (found) return found
      }
    }
    if (Array.isArray(obj)) {
      for (const child of obj) {
        const found = visit(child, depth + 1)
        if (found) return found
      }
    }
    if (Array.isArray(obj.meetings)) {
      for (const child of obj.meetings) {
        const found = visit(child, depth + 1)
        if (found) return found
      }
    }
    return null
  }
  const root = (payload as Record<string, unknown>)?.result ?? payload
  const container = (root as Record<string, unknown>)?.meeting_info_list
  if (Array.isArray(container)) {
    return container.filter((item): item is MeetingItem => !!item)
  }
  const list = (root as Record<string, unknown>)?.meeting_list
  if (Array.isArray(list)) {
    return list.filter((item): item is MeetingItem => !!item)
  }
  return []
}

function formatDateTime(iso?: string): string {
  if (!iso) return "—"
  const cleaned = iso.replace("T", " ").replace(/[+-]\d{2}:\d{2}$/, "").replace(/\.\d+$/, "")
  return cleaned
}

function sortByStart(items: MeetingItem[], dir: "asc" | "desc"): MeetingItem[] {
  return [...items].sort((a, b) => {
    const ta = Date.parse(a.start_time ?? "") || 0
    const tb = Date.parse(b.start_time ?? "") || 0
    return dir === "asc" ? ta - tb : tb - ta
  })
}

export default function Meetings() {
  const { currentProjectId } = useProject()
  const toast = useToast()
  void currentProjectId

  const [configured, setConfigured] = useState<boolean | null>(null)
  const [tab, setTab] = useState<Tab>("upcoming")
  const [upcoming, setUpcoming] = useState<MeetingItem[]>([])
  const [history, setHistory] = useState<MeetingItem[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const [showCreate, setShowCreate] = useState(false)
  const [createSubject, setCreateSubject] = useState("")
  const [createStart, setCreateStart] = useState("")
  const [createEnd, setCreateEnd] = useState("")
  const [createBusy, setCreateBusy] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)

  const [activeMeeting, setActiveMeeting] = useState<MeetingItem | null>(null)
  const [activeDetail, setActiveDetail] = useState<unknown>(null)
  const [transcriptBusy, setTranscriptBusy] = useState(false)
  const [transcriptResult, setTranscriptResult] = useState<{
    transcript: string
    minutes?: string | null
  } | null>(null)

  const refreshConfig = useCallback(async () => {
    try {
      const status = await getTencentMeetingConfigStatus()
      setConfigured(status.configured)
    } catch (err) {
      setConfigured(false)
      setError(formatAppError(err))
    }
  }, [])

  const refreshList = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [up, ended] = await Promise.all([
        listTencentUserMeetings({}),
        listTencentUserEndedMeetings({}),
      ])
      setUpcoming(sortByStart(extractMeetings(up), "asc"))
      setHistory(sortByStart(extractMeetings(ended), "desc"))
    } catch (err) {
      setError(formatAppError(err))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refreshConfig()
  }, [refreshConfig])

  useEffect(() => {
    if (configured) refreshList()
  }, [configured, refreshList])

  async function handleCreate() {
    if (!createSubject.trim() || !createStart.trim() || !createEnd.trim()) {
      setCreateError("请填写会议主题、开始时间、结束时间")
      return
    }
    setCreateBusy(true)
    setCreateError(null)
    try {
      const startIso = new Date(createStart).toISOString()
      const endIso = new Date(createEnd).toISOString()
      let normalizedStart = startIso
      let normalizedEnd = endIso
      try {
        const converted = await convertTencentMeetingTimestamp({ start_time: startIso })
        const result = (converted as { result?: { parsed_time_iso?: string } })?.result
        if (result?.parsed_time_iso) normalizedStart = result.parsed_time_iso
      } catch {
        // 转换失败时退回原始 ISO
      }
      const result = await scheduleTencentMeeting({
        subject: createSubject.trim(),
        start_time: normalizedStart,
        end_time: normalizedEnd,
      })
      const meetingInfo = (result as { result?: { meeting_info?: { meeting_id?: string; meeting_code?: string } } })?.result
        ?.meeting_info
      toast.success(
        meetingInfo?.meeting_code
          ? `已创建会议：${createSubject}（${meetingInfo.meeting_code}）`
          : `已创建会议：${createSubject}`,
      )
      setShowCreate(false)
      setCreateSubject("")
      setCreateStart("")
      setCreateEnd("")
      void refreshList()
    } catch (err) {
      setCreateError(formatAppError(err))
    } finally {
      setCreateBusy(false)
    }
  }

  async function handleOpenDetail(meeting: MeetingItem) {
    setActiveMeeting(meeting)
    setActiveDetail(null)
    setTranscriptResult(null)
    if (meeting.meeting_id) {
      try {
        const detail = await getTencentMeeting({ meeting_id: meeting.meeting_id })
        setActiveDetail(detail)
      } catch (err) {
        setActiveDetail({ error: formatAppError(err) })
      }
    }
  }

  async function handleFetchTranscript() {
    if (!activeMeeting) return
    setTranscriptBusy(true)
    try {
      const result = await fetchTencentMeetingTranscript({
        meetingId: activeMeeting.meeting_id,
        meetingCode: activeMeeting.meeting_code,
        includeMinutes: true,
      })
      setTranscriptResult({
        transcript: result.transcript,
        minutes: result.minutes ?? null,
      })
      toast.success("转写已获取，可在 AI 对话中继续生成会议纪要")
    } catch (err) {
      toast.error(`转写获取失败：${formatAppError(err)}`)
    } finally {
      setTranscriptBusy(false)
    }
  }

  async function handleCancel(meeting: MeetingItem) {
    if (!meeting.meeting_id) return
    if (!window.confirm(`确认取消会议「${meeting.subject ?? meeting.meeting_id}」？`)) return
    try {
      await cancelTencentMeeting({ meeting_id: meeting.meeting_id })
      toast.success("已取消会议")
      setActiveMeeting(null)
      void refreshList()
    } catch (err) {
      toast.error(`取消失败：${formatAppError(err)}`)
    }
  }

  const list = tab === "upcoming" ? upcoming : history

  const isConfigured = configured === true

  return (
    <div className="flex h-full flex-col gap-4 p-6">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold text-neutral-900">会议管理</h1>
          <p className="mt-1 text-sm text-neutral-500">
            预约 / 查询 / 取消腾讯会议，同步转写与智能纪要。
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void refreshList()}
            disabled={!isConfigured || loading}
            className="inline-flex items-center gap-1.5 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm text-neutral-700 hover:bg-neutral-50 disabled:opacity-50"
          >
            {loading ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <RefreshCw className="h-3.5 w-3.5" />}
            刷新
          </button>
          <button
            type="button"
            onClick={() => setShowCreate(true)}
            disabled={!isConfigured}
            className="inline-flex items-center gap-1.5 rounded-md bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
          >
            <CalendarPlus className="h-3.5 w-3.5" />
            预约会议
          </button>
        </div>
      </header>

      {configured === false && (
        <div className="flex items-start gap-3 rounded-lg border border-amber-200 bg-amber-50 p-4">
          <AlertCircle className="mt-0.5 h-5 w-5 shrink-0 text-amber-600" />
          <div className="flex-1 text-sm text-amber-900">
            <p className="font-medium">尚未配置腾讯会议 Token</p>
            <p className="mt-1 text-amber-800">
              前往「设置 → 腾讯会议 MCP」填入
              <a
                className="mx-1 underline"
                href="https://meeting.tencent.com/ai-skill"
                target="_blank"
                rel="noreferrer"
              >
                meeting.tencent.com/ai-skill
              </a>
              获取的 Token。配置后即可预约/查询/转写会议。
            </p>
          </div>
        </div>
      )}

      {error && (
        <div className="flex items-start gap-3 rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-700">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <div className="flex-1">{error}</div>
          <button
            type="button"
            onClick={() => void refreshList()}
            className="text-xs underline"
          >
            重试
          </button>
        </div>
      )}

      <div className="flex gap-1 rounded-md border border-neutral-200 bg-white p-1 text-sm w-fit">
        {(["upcoming", "history"] as const).map((key) => (
          <button
            key={key}
            type="button"
            onClick={() => setTab(key)}
            className={`rounded px-3 py-1 ${
              tab === key
                ? "bg-blue-50 text-blue-700"
                : "text-neutral-600 hover:bg-neutral-50"
            }`}
          >
            {key === "upcoming" ? `未开始 / 进行中（${upcoming.length}）` : `已结束（${history.length}）`}
          </button>
        ))}
      </div>

      <div className="grid flex-1 grid-cols-1 gap-4 lg:grid-cols-[1fr_1.2fr]">
        <section className="overflow-y-auto rounded-lg border border-neutral-200 bg-white">
          {loading && list.length === 0 ? (
            <div className="flex h-40 items-center justify-center text-sm text-neutral-500">
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              正在加载…
            </div>
          ) : list.length === 0 ? (
            <div className="flex h-40 flex-col items-center justify-center text-sm text-neutral-500">
              <Calendar className="mb-2 h-6 w-6 text-neutral-400" />
              {tab === "upcoming" ? "近期暂无未开始的会议" : "暂无历史会议记录"}
            </div>
          ) : (
            <ul className="divide-y divide-neutral-100">
              {list.map((meeting, idx) => {
                const key = meeting.meeting_id ?? meeting.meeting_code ?? `m-${idx}`
                const isActive = activeMeeting?.meeting_id === meeting.meeting_id
                return (
                  <li key={key}>
                    <button
                      type="button"
                      onClick={() => void handleOpenDetail(meeting)}
                      className={`flex w-full items-center justify-between gap-3 px-4 py-3 text-left hover:bg-neutral-50 ${
                        isActive ? "bg-blue-50/50" : ""
                      }`}
                    >
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium text-neutral-900">
                          {meeting.subject ?? "(未命名会议)"}
                        </p>
                        <div className="mt-0.5 flex items-center gap-2 text-xs text-neutral-500">
                          <Clock className="h-3 w-3 shrink-0" />
                          <span>{formatDateTime(meeting.start_time)}</span>
                          {meeting.meeting_code && (
                            <span className="rounded bg-neutral-100 px-1.5 py-0.5 font-mono">
                              {meeting.meeting_code}
                            </span>
                          )}
                        </div>
                      </div>
                      <ChevronRight className="h-4 w-4 shrink-0 text-neutral-400" />
                    </button>
                  </li>
                )
              })}
            </ul>
          )}
        </section>

        <section className="overflow-y-auto rounded-lg border border-neutral-200 bg-white p-4">
          {!activeMeeting ? (
            <div className="flex h-full flex-col items-center justify-center text-sm text-neutral-500">
              <Calendar className="mb-2 h-6 w-6 text-neutral-400" />
              选择左侧会议查看详情
            </div>
          ) : (
            <div className="space-y-3">
              <div className="flex items-start justify-between">
                <div>
                  <h2 className="text-lg font-semibold text-neutral-900">
                    {activeMeeting.subject ?? "(未命名会议)"}
                  </h2>
                  <p className="mt-1 text-xs text-neutral-500">
                    {formatDateTime(activeMeeting.start_time)} ~ {formatDateTime(activeMeeting.end_time)}
                  </p>
                </div>
                {activeMeeting.meeting_code && (
                  <span className="rounded bg-blue-50 px-2 py-1 text-xs font-mono text-blue-700">
                    会议号 {activeMeeting.meeting_code}
                  </span>
                )}
              </div>

              {activeMeeting.meeting_id && (
                <a
                  href={`https://meeting.tencent.com/p/${activeMeeting.meeting_id}`}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1.5 rounded-md border border-blue-200 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
                >
                  <ExternalLink className="h-3.5 w-3.5" />
                  在腾讯会议中打开
                </a>
              )}

              <div className="flex gap-2 border-t border-neutral-100 pt-3">
                <button
                  type="button"
                  onClick={() => void handleFetchTranscript()}
                  disabled={transcriptBusy || !activeMeeting.meeting_id}
                  className="inline-flex items-center gap-1.5 rounded-md bg-neutral-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-neutral-800 disabled:opacity-50"
                >
                  {transcriptBusy ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <Mic className="h-3.5 w-3.5" />
                  )}
                  同步转写 + 智能纪要
                </button>
                {tab === "upcoming" && activeMeeting.meeting_id && (
                  <button
                    type="button"
                    onClick={() => void handleCancel(activeMeeting)}
                    className="inline-flex items-center gap-1.5 rounded-md border border-red-200 bg-white px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-50"
                  >
                    <X className="h-3.5 w-3.5" />
                    取消会议
                  </button>
                )}
              </div>

              {transcriptResult && (
                <div className="space-y-2 border-t border-neutral-100 pt-3">
                  {transcriptResult.minutes && (
                    <details open className="rounded border border-amber-200 bg-amber-50/40 p-3">
                      <summary className="flex cursor-pointer items-center gap-1.5 text-xs font-medium text-amber-800">
                        <Sparkles className="h-3.5 w-3.5" />
                        腾讯会议 AI 智能纪要
                      </summary>
                      <pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap text-xs leading-relaxed text-neutral-800">
                        {transcriptResult.minutes}
                      </pre>
                    </details>
                  )}
                  <details className="rounded border border-neutral-200 bg-neutral-50/40 p-3">
                    <summary className="flex cursor-pointer items-center gap-1.5 text-xs font-medium text-neutral-700">
                      <FileText className="h-3.5 w-3.5" />
                      完整转写文本
                    </summary>
                    <pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap text-xs leading-relaxed text-neutral-800">
                      {transcriptResult.transcript || "(暂无转写)"}
                    </pre>
                  </details>
                </div>
              )}

              {activeDetail != null && (
                <details className="rounded border border-neutral-200 bg-white p-3 text-xs">
                  <summary className="flex cursor-pointer items-center gap-1.5 text-neutral-600">
                    <Search className="h-3.5 w-3.5" />
                    原始响应（MCP 调试）
                  </summary>
                  <pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-neutral-600">
                    {JSON.stringify(activeDetail, null, 2)}
                  </pre>
                </details>
              )}
            </div>
          )}
        </section>
      </div>

      {showCreate && (
        <div
          className="fixed inset-0 z-30 flex items-center justify-center bg-black/40 p-4"
          onClick={() => !createBusy && setShowCreate(false)}
        >
          <div
            className="w-full max-w-md rounded-lg bg-white p-5 shadow-xl"
            onClick={(event) => event.stopPropagation()}
          >
            <h2 className="text-lg font-semibold text-neutral-900">预约腾讯会议</h2>
            <p className="mt-1 text-xs text-neutral-500">
              时间支持 ISO 8601（{`2026-06-14T15:00`}）或本地时间，AI 将自动转换为上海时区。
            </p>
            <div className="mt-4 space-y-3">
              <div>
                <label className="block text-xs font-medium text-neutral-700">会议主题 *</label>
                <input
                  value={createSubject}
                  onChange={(event) => setCreateSubject(event.target.value)}
                  placeholder="例如：客户需求确认会"
                  className="mt-1 w-full rounded border border-neutral-300 px-3 py-1.5 text-sm focus:border-blue-500 focus:outline-none"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-neutral-700">开始时间 *</label>
                <input
                  type="datetime-local"
                  value={createStart}
                  onChange={(event) => setCreateStart(event.target.value)}
                  className="mt-1 w-full rounded border border-neutral-300 px-3 py-1.5 text-sm focus:border-blue-500 focus:outline-none"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-neutral-700">结束时间 *</label>
                <input
                  type="datetime-local"
                  value={createEnd}
                  onChange={(event) => setCreateEnd(event.target.value)}
                  className="mt-1 w-full rounded border border-neutral-300 px-3 py-1.5 text-sm focus:border-blue-500 focus:outline-none"
                />
              </div>
              {createError && (
                <div className="rounded border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
                  {createError}
                </div>
              )}
            </div>
            <div className="mt-5 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setShowCreate(false)}
                disabled={createBusy}
                className="rounded border border-neutral-300 bg-white px-3 py-1.5 text-sm text-neutral-700 hover:bg-neutral-50 disabled:opacity-50"
              >
                取消
              </button>
              <button
                type="button"
                onClick={() => void handleCreate()}
                disabled={createBusy}
                className="inline-flex items-center gap-1.5 rounded bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
              >
                {createBusy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                确认预约
              </button>
            </div>
          </div>
        </div>
      )}

      <footer className="text-xs text-neutral-400">
        由腾讯会议 MCP 提供数据 · {PRODUCT_NAME}
      </footer>
    </div>
  )
}
