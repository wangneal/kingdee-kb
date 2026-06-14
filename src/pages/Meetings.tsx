import {
  AlertCircle,
  Calendar,
  CalendarPlus,
  CheckCircle2,
  ChevronRight,
  Clock,
  ExternalLink,
  FileText,
  Link2,
  Loader2,
  Mic,
  RefreshCw,
  ScrollText,
  Search,
  Sparkles,
  Unlink,
  X,
} from "lucide-react"
import { useCallback, useEffect, useMemo, useState } from "react"
import { useToast } from "../components/Toast"
import { useProject } from "../contexts/ProjectContext"
import { formatAppError } from "../lib/app-error"
import { PRODUCT_NAME } from "../lib/constants"
import {
  cancelTencentMeeting,
  convertTencentMeetingTimestamp,
  fetchMeetingTranscript,
  generateMeetingMinutes,
  getMeetingWithAssets,
  ignoreUnlinkedMeeting,
  linkMeetingToProject,
  listMeetings,
  readProjectActivityLog,
  scheduleTencentMeeting,
  syncTencentMeetings,
  type LocalMeeting,
  type MeetingWithAssets,
} from "../lib/tauri-commands"

type Tab = "linked" | "unlinked" | "all"

function formatDateTime(iso?: string | null): string {
  if (!iso) return "—"
  const cleaned = iso
    .replace("T", " ")
    .replace(/[+-]\d{2}:\d{2}$/, "")
    .replace(/\.\d+$/, "")
  return cleaned
}

function statusLabel(status: string): string {
  switch (status) {
    case "scheduled": return "未开始"
    case "ongoing": return "进行中"
    case "ended": return "已结束"
    case "cancelled": return "已取消"
    default: return status
  }
}

function statusColor(status: string): string {
  switch (status) {
    case "scheduled": return "bg-blue-50 text-blue-700"
    case "ongoing": return "bg-green-50 text-green-700"
    case "ended": return "bg-neutral-100 text-neutral-600"
    case "cancelled": return "bg-red-50 text-red-600"
    default: return "bg-neutral-100 text-neutral-600"
  }
}

export default function Meetings() {
  const { currentProjectId } = useProject()
  const toast = useToast()

  const [tab, setTab] = useState<Tab>("linked")
  const [meetings, setMeetings] = useState<LocalMeeting[]>([])
  const [loading, setLoading] = useState(false)
  const [syncing, setSyncing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [query, setQuery] = useState("")

  // 详情面板
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [detail, setDetail] = useState<MeetingWithAssets | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)
  const [transcriptBusy, setTranscriptBusy] = useState(false)
  const [minutesBusy, setMinutesBusy] = useState(false)

  // 预约会议
  const [showCreate, setShowCreate] = useState(false)
  const [createSubject, setCreateSubject] = useState("")
  const [createStart, setCreateStart] = useState("")
  const [createEnd, setCreateEnd] = useState("")
  const [createBusy, setCreateBusy] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)

  // 活动日志
  const [activityLog, setActivityLog] = useState<string | null>(null)
  const [activityLoading, setActivityLoading] = useState(false)

  const refreshList = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const params: Parameters<typeof listMeetings>[0] = { limit: 200 }
      if (tab === "linked") {
        params.projectId = currentProjectId ?? undefined
        params.linkStatus = "linked"
      } else if (tab === "unlinked") {
        params.linkStatus = "unlinked"
      }
      if (query.trim()) {
        params.query = query.trim()
      }
      const result = await listMeetings(params)
      setMeetings(result)
    } catch (err) {
      setError(formatAppError(err))
    } finally {
      setLoading(false)
    }
  }, [tab, currentProjectId, query])

  useEffect(() => {
    void refreshList()
  }, [refreshList])

  async function handleSync() {
    setSyncing(true)
    try {
      const count = await syncTencentMeetings(30)
      toast.success(`已同步 ${count} 场会议`)
      void refreshList()
    } catch (err) {
      toast.error(`同步失败：${formatAppError(err)}`)
    } finally {
      setSyncing(false)
    }
  }

  async function handleOpenDetail(id: number) {
    setSelectedId(id)
    setDetailLoading(true)
    setDetail(null)
    try {
      const result = await getMeetingWithAssets(id)
      setDetail(result)
    } catch (err) {
      toast.error(formatAppError(err))
    } finally {
      setDetailLoading(false)
    }
  }

  async function handleLinkToProject(meetingId: number) {
    if (!currentProjectId) {
      toast.error("请先选择一个项目")
      return
    }
    try {
      await linkMeetingToProject(meetingId, currentProjectId)
      toast.success("已关联到当前项目")
      void refreshList()
      if (selectedId === meetingId) void handleOpenDetail(meetingId)
    } catch (err) {
      toast.error(formatAppError(err))
    }
  }

  async function handleIgnore(meetingId: number) {
    try {
      await ignoreUnlinkedMeeting(meetingId)
      toast.success("已标记为忽略")
      void refreshList()
    } catch (err) {
      toast.error(formatAppError(err))
    }
  }

  async function handleFetchTranscript(meetingId: number) {
    setTranscriptBusy(true)
    try {
      await fetchMeetingTranscript(meetingId, currentProjectId ?? undefined)
      toast.success("转写已获取")
      void handleOpenDetail(meetingId)
    } catch (err) {
      toast.error(`转写获取失败：${formatAppError(err)}`)
    } finally {
      setTranscriptBusy(false)
    }
  }

  async function handleGenerateMinutes(meetingId: number) {
    setMinutesBusy(true)
    try {
      const result = await generateMeetingMinutes(meetingId)
      const filePath = typeof result.file_path === "string" ? result.file_path : ""
      toast.success(`纪要已生成${filePath ? `：${filePath}` : ""}`)
      void handleOpenDetail(meetingId)
    } catch (err) {
      toast.error(`纪要生成失败：${formatAppError(err)}`)
    } finally {
      setMinutesBusy(false)
    }
  }

  async function handleCancel(meeting: LocalMeeting) {
    if (!window.confirm(`确认取消会议「${meeting.subject}」？`)) return
    try {
      await cancelTencentMeeting({ meeting_id: meeting.meeting_id })
      toast.success("已取消会议")
      setSelectedId(null)
      setDetail(null)
      void refreshList()
    } catch (err) {
      toast.error(`取消失败：${formatAppError(err)}`)
    }
  }

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
      try {
        const converted = await convertTencentMeetingTimestamp({ start_time: startIso })
        const result = (converted as { result?: { parsed_time_iso?: string } })?.result
        if (result?.parsed_time_iso) normalizedStart = result.parsed_time_iso
      } catch { /* ignore */ }
      await scheduleTencentMeeting({
        subject: createSubject.trim(),
        start_time: normalizedStart,
        end_time: endIso,
      })
      toast.success(`已创建会议：${createSubject}`)
      setShowCreate(false)
      setCreateSubject("")
      setCreateStart("")
      setCreateEnd("")
      // 同步新会议到本地
      void handleSync()
    } catch (err) {
      setCreateError(formatAppError(err))
    } finally {
      setCreateBusy(false)
    }
  }

  async function handleOpenActivityLog() {
    if (!currentProjectId) {
      toast.error("请先选择一个项目")
      return
    }
    setActivityLoading(true)
    setActivityLog(null)
    try {
      const content = await readProjectActivityLog(currentProjectId)
      setActivityLog(content)
    } catch (err) {
      toast.error(`读取活动日志失败：${formatAppError(err)}`)
    } finally {
      setActivityLoading(false)
    }
  }

  // 解析决策/待办 JSON（容错：解析失败返回空数组）
  const decisions = useMemo(() => {
    try {
      const arr = JSON.parse(detail?.minutes?.decisions_json ?? "[]")
      return Array.isArray(arr) ? (arr as string[]).filter(Boolean) : []
    } catch {
      return []
    }
  }, [detail?.minutes?.decisions_json])

  const todos = useMemo(() => {
    try {
      const arr = JSON.parse(detail?.minutes?.todos_json ?? "[]")
      return Array.isArray(arr) ? (arr as string[]).filter(Boolean) : []
    } catch {
      return []
    }
  }, [detail?.minutes?.todos_json])

  const isLinked = detail?.meeting.link_status === "linked"
  const hasTranscript = !!detail?.transcript
  const hasMinutes = !!detail?.minutes

  return (
    <div className="flex h-full flex-col gap-4 p-6">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold text-neutral-900">会议管理</h1>
          <p className="mt-1 text-sm text-neutral-500">
            同步腾讯会议，管理转写与项目纪要。
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void handleOpenActivityLog()}
            disabled={!currentProjectId}
            className="inline-flex items-center gap-1.5 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm text-neutral-700 hover:bg-neutral-50 disabled:opacity-50"
          >
            <ScrollText className="h-3.5 w-3.5" />
            活动日志
          </button>
          <button
            type="button"
            onClick={() => void handleSync()}
            disabled={syncing}
            className="inline-flex items-center gap-1.5 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm text-neutral-700 hover:bg-neutral-50 disabled:opacity-50"
          >
            {syncing ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <RefreshCw className="h-3.5 w-3.5" />
            )}
            同步腾讯会议
          </button>
          <button
            type="button"
            onClick={() => void refreshList()}
            disabled={loading}
            className="inline-flex items-center gap-1.5 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm text-neutral-700 hover:bg-neutral-50 disabled:opacity-50"
          >
            {loading ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <RefreshCw className="h-3.5 w-3.5" />
            )}
            刷新
          </button>
          <button
            type="button"
            onClick={() => setShowCreate(true)}
            className="inline-flex items-center gap-1.5 rounded-md bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700"
          >
            <CalendarPlus className="h-3.5 w-3.5" />
            预约会议
          </button>
        </div>
      </header>

      {error && (
        <div className="flex items-start gap-3 rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-700">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <div className="flex-1">{error}</div>
          <button type="button" onClick={() => void refreshList()} className="text-xs underline">
            重试
          </button>
        </div>
      )}

      <div className="flex items-center gap-3">
        <div className="flex gap-1 rounded-md border border-neutral-200 bg-white p-1 text-sm w-fit">
          {([
            { key: "linked" as const, label: "当前项目" },
            { key: "unlinked" as const, label: "未关联" },
            { key: "all" as const, label: "全部" },
          ]).map(({ key, label }) => (
            <button
              key={key}
              type="button"
              onClick={() => setTab(key)}
              className={`rounded px-3 py-1 ${
                tab === key ? "bg-blue-50 text-blue-700" : "text-neutral-600 hover:bg-neutral-50"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="relative flex-1 max-w-xs">
          <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-neutral-400" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索会议主题或会议号…"
            className="w-full rounded border border-neutral-200 bg-white py-1.5 pl-8 pr-3 text-sm focus:border-blue-500 focus:outline-none"
          />
        </div>
      </div>

      <div className="grid flex-1 grid-cols-1 gap-4 lg:grid-cols-[1fr_1.2fr]">
        <section className="overflow-y-auto rounded-lg border border-neutral-200 bg-white">
          {loading && meetings.length === 0 ? (
            <div className="flex h-40 items-center justify-center text-sm text-neutral-500">
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              正在加载…
            </div>
          ) : meetings.length === 0 ? (
            <div className="flex h-40 flex-col items-center justify-center text-sm text-neutral-500">
              <Calendar className="mb-2 h-6 w-6 text-neutral-400" />
              {tab === "linked" ? "当前项目暂无会议" : tab === "unlinked" ? "暂无未关联会议" : '暂无会议，点击“同步腾讯会议”拉取'}
            </div>
          ) : (
            <ul className="divide-y divide-neutral-100">
              {meetings.map((m) => (
                <li key={m.id}>
                  <button
                    type="button"
                    onClick={() => void handleOpenDetail(m.id)}
                    className={`flex w-full items-center justify-between gap-3 px-4 py-3 text-left hover:bg-neutral-50 ${
                      selectedId === m.id ? "bg-blue-50/50" : ""
                    }`}
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <p className="truncate text-sm font-medium text-neutral-900">
                          {m.subject || "(未命名会议)"}
                        </p>
                        <span className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium ${statusColor(m.status)}`}>
                          {statusLabel(m.status)}
                        </span>
                      </div>
                      <div className="mt-0.5 flex items-center gap-2 text-xs text-neutral-500">
                        <Clock className="h-3 w-3 shrink-0" />
                        <span>{formatDateTime(m.start_time)}</span>
                        {m.meeting_code && (
                          <span className="rounded bg-neutral-100 px-1.5 py-0.5 font-mono">
                            {m.meeting_code}
                          </span>
                        )}
                        {m.link_status === "unlinked" && (
                          <span className="rounded bg-amber-50 px-1.5 py-0.5 text-amber-700">未关联</span>
                        )}
                      </div>
                    </div>
                    <ChevronRight className="h-4 w-4 shrink-0 text-neutral-400" />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="overflow-y-auto rounded-lg border border-neutral-200 bg-white p-4">
          {!detail && !detailLoading ? (
            <div className="flex h-full flex-col items-center justify-center text-sm text-neutral-500">
              <Calendar className="mb-2 h-6 w-6 text-neutral-400" />
              选择左侧会议查看详情
            </div>
          ) : detailLoading ? (
            <div className="flex h-40 items-center justify-center text-sm text-neutral-500">
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              加载中…
            </div>
          ) : detail ? (
            <div className="space-y-3">
              <div className="flex items-start justify-between">
                <div>
                  <h2 className="text-lg font-semibold text-neutral-900">
                    {detail.meeting.subject || "(未命名会议)"}
                  </h2>
                  <p className="mt-1 text-xs text-neutral-500">
                    {formatDateTime(detail.meeting.start_time)} ~ {formatDateTime(detail.meeting.end_time)}
                  </p>
                  {detail.project_name && (
                    <p className="mt-1 flex items-center gap-1 text-xs text-blue-700">
                      <Link2 className="h-3 w-3" />
                      项目：{detail.project_name}
                    </p>
                  )}
                </div>
                <div className="flex flex-col items-end gap-1">
                  <span className={`rounded px-2 py-0.5 text-[10px] font-medium ${statusColor(detail.meeting.status)}`}>
                    {statusLabel(detail.meeting.status)}
                  </span>
                  {detail.meeting.meeting_code && (
                    <span className="rounded bg-blue-50 px-2 py-1 text-xs font-mono text-blue-700">
                      会议号 {detail.meeting.meeting_code}
                    </span>
                  )}
                </div>
              </div>

              {detail.meeting.meeting_id && (
                <a
                  href={`https://meeting.tencent.com/p/${detail.meeting.meeting_id}`}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1.5 rounded-md border border-blue-200 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
                >
                  <ExternalLink className="h-3.5 w-3.5" />
                  在腾讯会议中打开
                </a>
              )}

              {/* 操作按钮 */}
              <div className="flex flex-wrap gap-2 border-t border-neutral-100 pt-3">
                {!isLinked && currentProjectId && (
                  <>
                    <button
                      type="button"
                      onClick={() => void handleLinkToProject(detail.meeting.id)}
                      className="inline-flex items-center gap-1.5 rounded-md bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-700"
                    >
                      <Link2 className="h-3.5 w-3.5" />
                      关联到当前项目
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleIgnore(detail.meeting.id)}
                      className="inline-flex items-center gap-1.5 rounded-md border border-neutral-200 bg-white px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50"
                    >
                      <Unlink className="h-3.5 w-3.5" />
                      忽略
                    </button>
                  </>
                )}

                {isLinked && detail.meeting.status === "ended" && (
                  <>
                    <button
                      type="button"
                      onClick={() => void handleFetchTranscript(detail.meeting.id)}
                      disabled={transcriptBusy}
                      className="inline-flex items-center gap-1.5 rounded-md bg-neutral-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-neutral-800 disabled:opacity-50"
                    >
                      {transcriptBusy ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Mic className="h-3.5 w-3.5" />
                      )}
                      {hasTranscript ? "重新同步转写" : "同步转写"}
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleGenerateMinutes(detail.meeting.id)}
                      disabled={minutesBusy || !hasTranscript}
                      title={!hasTranscript ? "请先同步转写" : "生成项目纪要"}
                      className="inline-flex items-center gap-1.5 rounded-md bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
                    >
                      {minutesBusy ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Sparkles className="h-3.5 w-3.5" />
                      )}
                      {hasMinutes ? "重新生成纪要" : "生成项目纪要"}
                    </button>
                  </>
                )}

                {isLinked && (detail.meeting.status === "scheduled" || detail.meeting.status === "ongoing") && detail.meeting.meeting_id && (
                  <button
                    type="button"
                    onClick={() => void handleCancel(detail.meeting)}
                    className="inline-flex items-center gap-1.5 rounded-md border border-red-200 bg-white px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-50"
                  >
                    <X className="h-3.5 w-3.5" />
                    取消会议
                  </button>
                )}
              </div>

              {/* 转写内容 */}
              {detail.transcript && (
                <details className="rounded border border-neutral-200 bg-neutral-50/40 p-3">
                  <summary className="flex cursor-pointer items-center gap-1.5 text-xs font-medium text-neutral-700">
                    <CheckCircle2 className="h-3.5 w-3.5 text-green-600" />
                    转写文本（{detail.transcript.transcript_text.length} 字）
                  </summary>
                  <pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap text-xs leading-relaxed text-neutral-800">
                    {detail.transcript.transcript_text}
                  </pre>
                </details>
              )}

              {/* 纪要内容 */}
              {detail.minutes && (
                <div className="space-y-2">
                  <div className="flex items-center gap-2 text-xs text-green-700">
                    <CheckCircle2 className="h-3.5 w-3.5" />
                    纪要已生成
                  </div>

                  {/* 关键决策（结构化展示） */}
                  {decisions.length > 0 && (
                    <div className="rounded border border-blue-200 bg-blue-50/40 p-3">
                      <div className="flex items-center gap-1.5 text-xs font-medium text-blue-800">
                        <CheckCircle2 className="h-3.5 w-3.5" />
                        关键决策（{decisions.length}）
                      </div>
                      <ol className="mt-2 list-decimal space-y-1 pl-5 text-xs leading-relaxed text-neutral-800">
                        {decisions.map((d, i) => (
                          <li key={i}>{d}</li>
                        ))}
                      </ol>
                    </div>
                  )}

                  {/* 待办事项（结构化展示） */}
                  {todos.length > 0 && (
                    <div className="rounded border border-amber-200 bg-amber-50/40 p-3">
                      <div className="flex items-center gap-1.5 text-xs font-medium text-amber-800">
                        <Clock className="h-3.5 w-3.5" />
                        待办事项（{todos.length}）
                      </div>
                      <ul className="mt-2 space-y-1 text-xs leading-relaxed text-neutral-800">
                        {todos.map((t, i) => (
                          <li key={i} className="flex items-start gap-1.5">
                            <span className="mt-0.5 inline-block h-3 w-3 shrink-0 rounded-sm border border-neutral-400" />
                            <span>{t}</span>
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}

                  <details className="rounded border border-amber-200 bg-amber-50/40 p-3">
                    <summary className="flex cursor-pointer items-center gap-1.5 text-xs font-medium text-amber-800">
                      <Sparkles className="h-3.5 w-3.5" />
                      完整纪要正文
                    </summary>
                    <pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap text-xs leading-relaxed text-neutral-800">
                      {detail.minutes.content_md}
                    </pre>
                  </details>
                  <div className="text-xs text-neutral-500">
                    <FileText className="mr-1 inline h-3 w-3" />
                    文件：{detail.minutes.file_path}
                  </div>
                </div>
              )}

              {!isLinked && !currentProjectId && (
                <div className="rounded border border-amber-200 bg-amber-50 p-3 text-xs text-amber-800">
                  请先选择一个项目，才能关联和生成纪要。
                </div>
              )}
            </div>
          ) : null}
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
              时间支持 ISO 8601（{`2026-06-14T15:00`}）或本地时间。
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

      {/* 活动日志弹窗 */}
      {activityLog !== null && (
        <div
          className="fixed inset-0 z-30 flex items-center justify-center bg-black/40 p-4"
          onClick={() => setActivityLog(null)}
        >
          <div
            className="flex max-h-[80vh] w-full max-w-2xl flex-col rounded-lg bg-white shadow-xl"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="flex items-center justify-between border-b border-neutral-100 p-4">
              <div className="flex items-center gap-1.5">
                <ScrollText className="h-4 w-4 text-neutral-600" />
                <h2 className="text-sm font-semibold text-neutral-900">项目活动日志</h2>
              </div>
              <button
                type="button"
                onClick={() => setActivityLog(null)}
                className="rounded p-1 text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <div className="flex-1 overflow-auto p-4">
              {activityLoading ? (
                <div className="flex h-20 items-center justify-center text-sm text-neutral-500">
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  加载中…
                </div>
              ) : activityLog.trim() ? (
                <pre className="whitespace-pre-wrap text-xs leading-relaxed text-neutral-800">
                  {activityLog}
                </pre>
              ) : (
                <div className="flex h-20 flex-col items-center justify-center text-sm text-neutral-400">
                  <FileText className="mb-1 h-5 w-5" />
                  暂无活动日志记录
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      <footer className="text-xs text-neutral-400">本地会议存储 + 腾讯会议 MCP · {PRODUCT_NAME}</footer>
    </div>
  )
}
