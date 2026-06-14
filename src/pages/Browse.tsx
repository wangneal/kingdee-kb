import { diffLines } from "diff"
import {
  AlertCircle,
  BookOpen,
  CheckCircle,
  CheckSquare,
  FilePlus,
  FileUp,
  Pencil,
  RefreshCw,
  Square,
  Trash2,
} from "lucide-react"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import ContextMenu, { type ContextMenuItem } from "@/components/ContextMenu"
import ImportModal from "@/components/ImportModal"
import GraphRecommendations from "@/components/wiki/GraphRecommendations"
import WikiLinkEditor from "@/components/wiki/WikiLinkEditor"
import WikiPageForm from "@/components/wiki/WikiPageForm"
import { useProject } from "@/contexts/ProjectContext"
import { TOAST_AUTO_DISMISS_MS } from "@/lib/constants"
import {
  approveAutoWikiPages,
  approveWikiPage,
  batchDeleteWikiPages,
  buildKnowledgeGraph,
  deleteDocument,
  deleteWikiPage,
  getGraphNeighbors,
  getKbRecompileStatus,
  getWikiPage,
  type KbRecompileStatus,
  listDocuments,
  listWikiPages,
  type DocumentMeta,
  rejectWikiPage,
  startKbRecompile,
  type WikiPage,
  type WikiPageBrief,
} from "@/lib/tauri-commands"

function formatOperationError(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export default function Browse() {
  const { currentProjectId } = useProject()
  const [wikiPages, setWikiPages] = useState<WikiPageBrief[]>([])
  const [selectedWiki, setSelectedWiki] = useState<WikiPage | null>(null)
  const [neighbors, setNeighbors] = useState<
    { slug: string; title: string; signal: string; weight: number }[]
  >([])
  const [loading, setLoading] = useState(true)

  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null)
  const [importModalOpen, setImportModalOpen] = useState(false)
  // Wiki 手动创建/编辑表单：null = 关闭；"create" = 新建；{ mode: "edit", page } = 编辑
  const [wikiFormMode, setWikiFormMode] = useState<
    | { kind: "create" }
    | { kind: "edit"; page: WikiPage }
    | null
  >(null)
  const [recompileStatus, setRecompileStatus] = useState<KbRecompileStatus | null>(null)
  const lastHandledRecompileFinishRef = useRef<string | null>(null)
  const [autoApproving, setAutoApproving] = useState(false)
  // 非重编译相关的反馈消息（用于批量删除等操作后的提示），3 秒后自动清除
  const [feedbackMessage, setFeedbackMessage] = useState<string | null>(null)
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set())
  // 左侧栏视图：wiki 知识页面 / documents 原始文档
  const [listView, setListView] = useState<"wiki" | "documents">("wiki")
  const [documents, setDocuments] = useState<DocumentMeta[]>([])
  const [docsLoading, setDocsLoading] = useState(false)
  // 内容视图：current 已批准；candidate 候选全文；diff 行级对比
  const [viewMode, setViewMode] = useState<"current" | "candidate" | "diff">("current")
  // 候选内容为空时自动从 diff 回退到 current 模式
  useEffect(() => {
    if (viewMode === "diff" && selectedWiki?.content_candidate == null) {
      setViewMode("current")
    }
  }, [viewMode, selectedWiki?.content_candidate])

  // 反馈消息 3 秒后自动清除
  useEffect(() => {
    if (feedbackMessage == null) return
    const timer = setTimeout(() => setFeedbackMessage(null), TOAST_AUTO_DISMISS_MS)
    return () => clearTimeout(timer)
  }, [feedbackMessage])

  const refreshWikiPages = useCallback(async () => {
    if (currentProjectId == null) {
      setWikiPages([])
      setSelectedWiki(null)
      setNeighbors([])
      setLoading(false)
      setSelectedIds(new Set())
      return
    }

    setLoading(true)
    try {
      const pages = await listWikiPages(currentProjectId)
      setWikiPages(pages)
    } catch {
      setWikiPages([])
    } finally {
      setLoading(false)
    }
  }, [currentProjectId])

  const refreshDocuments = useCallback(async () => {
    if (currentProjectId == null) {
      setDocuments([])
      setDocsLoading(false)
      return
    }
    setDocsLoading(true)
    try {
      setDocuments(await listDocuments(currentProjectId))
    } catch {
      setDocuments([])
    } finally {
      setDocsLoading(false)
    }
  }, [currentProjectId])

  // 切换到 documents 视图时加载
  useEffect(() => {
    if (listView === "documents") void refreshDocuments()
  }, [listView, refreshDocuments])

  async function handleDeleteDocument(docId: number) {
    if (currentProjectId == null) return
    if (!window.confirm("删除该文档将同时清理其向量索引和全文索引，且不可恢复。确认删除？")) return
    try {
      await deleteDocument(docId, currentProjectId)
      setFeedbackMessage("文档已删除")
      void refreshDocuments()
    } catch (err) {
      setFeedbackMessage(`删除失败：${formatOperationError(err)}`)
    }
  }

  const contextMenuItems: ContextMenuItem[] = useMemo(
    () => [
      {
        label: "导入文档",
        icon: <FileUp size={14} />,
        onClick: () => {
          setImportModalOpen(true)
          setContextMenu(null)
        },
      },
    ],
    [],
  )

  useEffect(() => {
    void refreshWikiPages()
  }, [refreshWikiPages])

  useEffect(() => {
    if (currentProjectId == null) return
    let cancelled = false

    async function pollStatus() {
      try {
        const status = await getKbRecompileStatus()
        if (cancelled) return
        setRecompileStatus(status)
        if (
          status.project_id === currentProjectId &&
          status.finished_at &&
          status.finished_at !== lastHandledRecompileFinishRef.current
        ) {
          lastHandledRecompileFinishRef.current = status.finished_at
          await refreshWikiPages()
        }
      } catch {
        if (!cancelled) setRecompileStatus(null)
      }
    }

    void pollStatus()
    const interval = window.setInterval(pollStatus, 3000)
    return () => {
      cancelled = true
      window.clearInterval(interval)
    }
  }, [currentProjectId, refreshWikiPages])

  const handleContextMenu = useCallback((event: React.MouseEvent) => {
    const target = event.target as HTMLElement
    if (target.closest("button, a, input, textarea, select, pre, code, [contenteditable]")) return
    event.preventDefault()
    setContextMenu({ x: event.clientX, y: event.clientY })
  }, [])

  const loadPage = useCallback(
    async (pageId: number) => {
      const page = await getWikiPage(pageId)
      setSelectedWiki(page)
      // 切换页面时回到"已批准内容"视图，避免混淆
      setViewMode("current")
      if (currentProjectId == null) return
      getGraphNeighbors(currentProjectId, page.slug)
        .then(setNeighbors)
        .catch(() => setNeighbors([]))
    },
    [currentProjectId],
  )

  const handleRecompileFailed = useCallback(async () => {
    if (currentProjectId == null) return
    try {
      const status = await startKbRecompile(currentProjectId)
      setRecompileStatus(status)
    } catch (error) {
      setRecompileStatus({
        status: "failed",
        project_id: currentProjectId,
        force: false,
        retried: 0,
        succeeded: 0,
        failed: [],
        completed_source_keys: [],
        message: `重编译失败：${error instanceof Error ? error.message : String(error)}`,
      })
    }
  }, [currentProjectId])

  // 强制重编译全部源：清掉所有 ingest/analysis cache 后走完整流程。
  // 用于"删 wiki 后想原地重生成"等场景——失败项入口看不到被删的源。
  const handleRecompileAll = useCallback(async () => {
    if (currentProjectId == null) return
    if (
      !window.confirm(
        "强制重编译将清空所有源的编译缓存并重新生成 wiki 页面，可能耗时数分钟。是否继续？",
      )
    )
      return
    try {
      const status = await startKbRecompile(currentProjectId, true)
      setRecompileStatus(status)
    } catch (error) {
      setRecompileStatus({
        status: "failed",
        project_id: currentProjectId,
        force: true,
        retried: 0,
        succeeded: 0,
        failed: [],
        completed_source_keys: [],
        message: `强制重编译失败：${error instanceof Error ? error.message : String(error)}`,
      })
    }
  }, [currentProjectId])

  const handleApproveAutoCandidates = useCallback(async () => {
    if (currentProjectId == null || autoApproving) return
    // 二次确认：自动批准会覆盖人工审核的"草稿/拒绝"标记，操作不可逆
    const confirmed = window.confirm(
      "确定要自动批准当前项目的所有 Wiki 候选页面吗？\n" +
        "该操作会一次性覆盖人工审核状态，已被人工拒绝的页面也会被改判，谨慎操作。",
    )
    if (!confirmed) return
    setAutoApproving(true)
    try {
      const result = await approveAutoWikiPages(currentProjectId)
      setFeedbackMessage(
        result.failed.length > 0
          ? `自动批准完成：成功 ${result.approved} 项，跳过 ${result.skipped} 项，失败 ${result.failed.length} 项`
          : `自动批准完成：成功 ${result.approved} 项，跳过 ${result.skipped} 项`,
      )
      await refreshWikiPages()
      if (selectedWiki) {
        try {
          const updated = await getWikiPage(selectedWiki.id)
          setSelectedWiki(updated)
          if (updated.candidate_status == null) setViewMode("current")
        } catch {
          setSelectedWiki(null)
        }
      }
      try {
        await buildKnowledgeGraph(currentProjectId)
        if (selectedWiki) {
          const nextNeighbors = await getGraphNeighbors(currentProjectId, selectedWiki.slug)
          setNeighbors(nextNeighbors)
        }
      } catch {
        setNeighbors([])
      }
    } catch (error) {
      setFeedbackMessage(`自动批准失败：${error instanceof Error ? error.message : String(error)}`)
    } finally {
      setAutoApproving(false)
    }
  }, [autoApproving, currentProjectId, refreshWikiPages, selectedWiki])

  const pageTypeLabel = (type: string) => {
    if (type === "entity") return "实体"
    if (type === "concept") return "概念"
    return type
  }

  const statusLabel = (status: string) => {
    if (status === "pending") return "待审核"
    if (status === "conflict") return "有冲突"
    return "已确认"
  }

  const statusIcon = (status: string) => {
    if (status === "pending") return <AlertCircle className="h-3 w-3 text-yellow-500" />
    if (status === "conflict") return <AlertCircle className="h-3 w-3 text-red-500" />
    return <CheckCircle className="h-3 w-3 text-green-500" />
  }

  const toggleSelect = useCallback((id: number) => {
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }, [])

  const toggleSelectAll = useCallback(() => {
    setSelectedIds((prev) => {
      if (prev.size === wikiPages.length) return new Set()
      return new Set(wikiPages.map((p) => p.id))
    })
  }, [wikiPages])

  const handleBatchDelete = useCallback(async () => {
    if (selectedIds.size === 0) return
    if (
      !confirm(
        `确认批量删除 ${selectedIds.size} 个 Wiki 页面？将清理编译缓存，但保留源文档与向量索引。`,
      )
    )
      return
    try {
      const count = await batchDeleteWikiPages(Array.from(selectedIds))
      setSelectedIds(new Set())
      if (selectedWiki && selectedIds.has(selectedWiki.id)) {
        setSelectedWiki(null)
        setNeighbors([])
      }
      await refreshWikiPages()
      setFeedbackMessage(`已删除 ${count} 个 Wiki 页面`)
    } catch (error) {
      setFeedbackMessage(`批量删除失败：${formatOperationError(error)}`)
    }
  }, [selectedIds, selectedWiki, refreshWikiPages])

  const handleApproveSelected = useCallback(async () => {
    if (!selectedWiki) return
    if (!confirm("确认批准候选内容？此操作将覆盖当前已批准版本。")) return
    try {
      const updated = await approveWikiPage(selectedWiki.id)
      setSelectedWiki(updated)
      setViewMode("current")
      await refreshWikiPages()
      setFeedbackMessage("候选内容已批准")
    } catch (error) {
      setFeedbackMessage(`批准失败：${formatOperationError(error)}`)
    }
  }, [selectedWiki, refreshWikiPages])

  const handleRejectSelected = useCallback(async () => {
    if (!selectedWiki) return
    if (!confirm("确认拒绝候选内容？此操作将丢弃 LLM 生成的新版本，保留已批准版本。")) return
    try {
      const updated = await rejectWikiPage(selectedWiki.id)
      setSelectedWiki(updated)
      setViewMode("current")
      await refreshWikiPages()
      setFeedbackMessage("候选内容已拒绝")
    } catch (error) {
      setFeedbackMessage(`拒绝失败：${formatOperationError(error)}`)
    }
  }, [selectedWiki, refreshWikiPages])

  const handleDeleteSelected = useCallback(async () => {
    if (!selectedWiki) return
    if (
      !confirm(
        `确认删除 Wiki 页面「${selectedWiki.title}」？将清理编译缓存，但保留源文档与向量索引。`,
      )
    ) {
      return
    }
    try {
      await deleteWikiPage(selectedWiki.id)
      setSelectedWiki(null)
      setNeighbors([])
      await refreshWikiPages()
      setFeedbackMessage("Wiki 页面已删除")
    } catch (error) {
      setFeedbackMessage(`删除失败：${formatOperationError(error)}`)
    }
  }, [selectedWiki, refreshWikiPages])

  const allSelected = wikiPages.length > 0 && selectedIds.size === wikiPages.length
  const recompiling = recompileStatus?.status === "running"
  const recompileMessage =
    recompileStatus?.project_id === currentProjectId
      ? recompileStatus.message
      : recompileStatus?.status === "running"
        ? `项目 ${recompileStatus.project_id ?? "未知"} 正在执行知识编译`
        : null
  const approvedContentEmpty = selectedWiki != null && selectedWiki.content.trim().length === 0
  const hasCandidateContent =
    selectedWiki?.content_candidate != null && selectedWiki.content_candidate.trim().length > 0

  return (
    <>
      {/* biome-ignore lint/a11y/noStaticElementInteractions: 面板右键菜单是标准交互模式 */}
      <div className="flex h-full gap-4 p-6" onContextMenu={handleContextMenu}>
        <div className="w-72 shrink-0 border-r border-neutral-200 pr-4">
          <div className="mb-3 flex gap-1 rounded-md border border-neutral-200 bg-white p-1 text-xs">
            <button
              type="button"
              onClick={() => setListView("wiki")}
              className={`flex-1 rounded px-2 py-1 transition-colors ${
                listView === "wiki" ? "bg-blue-50 text-blue-700" : "text-neutral-500 hover:bg-neutral-50"
              }`}
            >
              知识页面
            </button>
            <button
              type="button"
              onClick={() => setListView("documents")}
              className={`flex-1 rounded px-2 py-1 transition-colors ${
                listView === "documents" ? "bg-blue-50 text-blue-700" : "text-neutral-500 hover:bg-neutral-50"
              }`}
            >
              原始文档
            </button>
          </div>

          {/* Wiki 手动操作：新建 + 编辑当前选中 */}
          {listView === "wiki" && currentProjectId != null && (
            <div className="mb-3 flex gap-1">
              <button
                type="button"
                onClick={() => setWikiFormMode({ kind: "create" })}
                className="flex flex-1 items-center justify-center gap-1 rounded-md border border-neutral-200 bg-white px-2 py-1.5 text-xs text-neutral-700 hover:border-[#1A6BD8] hover:text-[#1A6BD8]"
                title="手动新建 Wiki 页面（绕开 KB 编译自动生成）"
              >
                <FilePlus className="h-3.5 w-3.5" />
                新建页面
              </button>
              <button
                type="button"
                onClick={() => {
                  if (selectedWiki) {
                    setWikiFormMode({ kind: "edit", page: selectedWiki })
                  }
                }}
                disabled={!selectedWiki}
                className="flex flex-1 items-center justify-center gap-1 rounded-md border border-neutral-200 bg-white px-2 py-1.5 text-xs text-neutral-700 hover:border-[#1A6BD8] hover:text-[#1A6BD8] disabled:cursor-not-allowed disabled:opacity-40"
                title={selectedWiki ? "编辑当前选中的 Wiki 页面" : "先在右侧选中要编辑的页面"}
              >
                <Pencil className="h-3.5 w-3.5" />
                编辑当前
              </button>
            </div>
          )}

          {/* 原始文档视图 */}
          {listView === "documents" ? (
            <div className="space-y-1">
              {docsLoading ? (
                <div className="py-8 text-center text-sm text-neutral-400">加载中...</div>
              ) : documents.length === 0 ? (
                <div className="py-8 text-center text-sm text-neutral-400">
                  暂无原始文档
                </div>
              ) : (
                documents.map((doc) => (
                  <div
                    key={doc.id}
                    className="group flex items-center justify-between gap-2 rounded-md px-2 py-1.5 hover:bg-neutral-50"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm font-medium text-neutral-700">
                        {doc.title || "(未命名)"}
                      </div>
                      <div className="mt-0.5 truncate text-[11px] text-neutral-400">
                        {doc.source_path || "—"}
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={() => void handleDeleteDocument(doc.id)}
                      className="shrink-0 rounded p-1 text-neutral-300 opacity-0 transition-opacity hover:bg-red-50 hover:text-red-500 group-hover:opacity-100"
                      title="删除文档"
                      aria-label="删除文档"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  </div>
                ))
              )}
            </div>
          ) : (
            <>
          <div className="mb-3 flex items-center justify-between gap-2">
            <span className="text-xs text-neutral-400">{wikiPages.length} 页</span>
          </div>

          <div className="mb-3 grid grid-cols-2 gap-1.5">
            {selectedIds.size > 0 && (
              <button
                type="button"
                onClick={handleBatchDelete}
                title={`删除已选 ${selectedIds.size} 页`}
                aria-label={`删除已选 ${selectedIds.size} 页`}
                className="inline-flex h-8 items-center justify-center gap-1 rounded-md border border-red-200 px-2 text-xs text-red-500 transition-colors hover:bg-red-50"
              >
                <Trash2 className="h-3.5 w-3.5" />
                <span>删除 {selectedIds.size}</span>
              </button>
            )}
            <button
              type="button"
              onClick={handleApproveAutoCandidates}
              disabled={currentProjectId == null || autoApproving}
              title="自动批准低风险候选"
              aria-label="自动批准低风险候选"
              className="inline-flex h-8 items-center justify-center gap-1 rounded-md border border-emerald-200 bg-emerald-50 px-2 text-xs text-emerald-700 transition-colors hover:bg-emerald-100 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <CheckCircle className="h-3.5 w-3.5" />
              <span>{autoApproving ? "批准中" : "自动批准"}</span>
            </button>
            <button
              type="button"
              onClick={handleRecompileFailed}
              disabled={currentProjectId == null || recompiling}
              title="重编译失败项"
              aria-label="重编译失败项"
              className="inline-flex h-8 items-center justify-center gap-1 rounded-md border border-neutral-200 px-2 text-xs text-neutral-500 transition-colors hover:border-amber-200 hover:bg-amber-50 hover:text-amber-700 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${recompiling ? "animate-spin" : ""}`} />
              <span>失败项</span>
            </button>
            <button
              type="button"
              onClick={handleRecompileAll}
              disabled={currentProjectId == null || recompiling}
              title="强制重编译全部"
              aria-label="强制重编译全部"
              className="inline-flex h-8 items-center justify-center gap-1 rounded-md border border-orange-200 bg-orange-50 px-2 text-xs text-orange-700 transition-colors hover:bg-orange-100 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${recompiling ? "animate-spin" : ""}`} />
              <span>全部重编</span>
            </button>
          </div>

          {(recompileMessage || feedbackMessage) && (
            <div className="mb-3 rounded-md border border-amber-100 bg-amber-50 px-2 py-1.5 text-xs text-amber-700">
              {recompileMessage || feedbackMessage}
            </div>
          )}

          {loading ? (
            <div className="py-8 text-center text-sm text-neutral-400">加载中...</div>
          ) : wikiPages.length === 0 ? (
            <div className="py-8 text-center text-sm text-neutral-400">
              <BookOpen className="mx-auto mb-2 h-8 w-8 opacity-30" />
              <p>暂无 Wiki 页面</p>
              <p className="mt-1 text-xs">导入文档后，知识编译将自动生成</p>
            </div>
          ) : (
            <div className="space-y-1">
              <button
                type="button"
                onClick={toggleSelectAll}
                className="flex w-full items-center gap-2 rounded-lg px-3 py-1.5 text-left text-xs text-neutral-500 hover:bg-neutral-50 border border-transparent"
              >
                {allSelected ? (
                  <CheckSquare className="h-3.5 w-3.5 text-amber-600" />
                ) : (
                  <Square className="h-3.5 w-3.5" />
                )}
                {allSelected ? "取消全选" : "全选"}
              </button>
              {wikiPages.map((wikiPage) => (
                <div
                  key={wikiPage.id}
                  className={`flex items-center gap-1 rounded-lg border px-3 py-2 text-left text-sm transition-colors ${
                    selectedWiki?.id === wikiPage.id
                      ? "bg-amber-50 text-amber-700 border-amber-200"
                      : "text-neutral-600 hover:bg-neutral-50 border-transparent"
                  }`}
                >
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation()
                      toggleSelect(wikiPage.id)
                    }}
                    className="shrink-0"
                  >
                    {selectedIds.has(wikiPage.id) ? (
                      <CheckSquare className="h-3.5 w-3.5 text-amber-600" />
                    ) : (
                      <Square className="h-3.5 w-3.5 text-neutral-400" />
                    )}
                  </button>
                  <button
                    type="button"
                    onClick={() => loadPage(wikiPage.id)}
                    className="flex-1 min-w-0"
                  >
                    <div className="font-medium truncate">{wikiPage.title}</div>
                    <div className="mt-0.5 text-[11px] text-neutral-400">
                      {pageTypeLabel(wikiPage.page_type)}
                    </div>
                  </button>
                </div>
              ))}
            </div>
          )}
            </>
          )}
        </div>

        {listView === "wiki" && (
        <div className="flex-1 overflow-y-auto">
          {selectedWiki ? (
            <div>
              <div className="mb-4 flex items-start justify-between">
                <div>
                  <h2 className="text-lg font-semibold text-neutral-800">{selectedWiki.title}</h2>
                  <div className="mt-1 flex flex-wrap gap-2 text-xs text-neutral-400">
                    <span>slug: {selectedWiki.slug}</span>
                    <span>版本: v{selectedWiki.version}</span>
                    <span>{pageTypeLabel(selectedWiki.page_type)}</span>
                    {selectedWiki.candidate_status && (
                      <span className="flex items-center gap-1">
                        {statusIcon(selectedWiki.candidate_status)}
                        {statusLabel(selectedWiki.candidate_status)}
                        {selectedWiki.candidate_version != null && (
                          <span className="ml-1 text-neutral-300">
                            → v{selectedWiki.candidate_version}
                          </span>
                        )}
                      </span>
                    )}
                  </div>
                </div>
                {selectedWiki.candidate_status && (
                  <div className="flex items-center gap-2">
                    {/* 三态视图切换：已批准 / 候选 / 行级 diff */}
                    <div className="inline-flex rounded-lg border border-amber-200 bg-amber-50 p-0.5 text-xs">
                      {(
                        [
                          { key: "current", label: "已批准" },
                          { key: "diff", label: "查看差异" },
                          { key: "candidate", label: "候选" },
                        ] as const
                      ).map((opt) => (
                        <button
                          key={opt.key}
                          type="button"
                          onClick={() => setViewMode(opt.key)}
                          className={
                            viewMode === opt.key
                              ? "rounded-md bg-amber-500 px-2.5 py-1 font-medium text-white"
                              : "rounded-md px-2.5 py-1 text-amber-700 hover:bg-amber-100"
                          }
                        >
                          {opt.label}
                        </button>
                      ))}
                    </div>
                    <button
                      type="button"
                      onClick={() => void handleApproveSelected()}
                      className="rounded-lg bg-amber-500 px-3 py-1.5 text-xs text-white hover:bg-amber-600"
                    >
                      批准内容
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleRejectSelected()}
                      className="rounded-lg border border-neutral-300 px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50"
                    >
                      拒绝候选
                    </button>
                  </div>
                )}
                <button
                  type="button"
                  onClick={() => void handleDeleteSelected()}
                  className="rounded-lg border border-red-200 px-3 py-1.5 text-xs text-red-500 hover:bg-red-50 hover:text-red-600"
                >
                  <Trash2 className="inline-block h-3 w-3 mr-1" />
                  删除
                </button>
              </div>

              {selectedWiki.candidate_status && viewMode !== "current" && (
                <div className="mb-3 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-700">
                  {viewMode === "candidate"
                    ? `正在预览 LLM 候选内容（v${selectedWiki.candidate_version ?? "?"}），尚未批准。`
                    : "行级 diff：绿色为新增，红色为删除。请确认后选择「批准内容」或「拒绝候选」。"}
                </div>
              )}

              {viewMode === "diff" ? (
                selectedWiki.content_candidate ? (
                  <WikiContentDiff
                    oldText={selectedWiki.content}
                    newText={selectedWiki.content_candidate}
                  />
                ) : (
                  <div className="rounded border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-700">
                    候选内容为空，无法生成 diff。
                  </div>
                )
              ) : (
                <div className="prose prose-sm max-w-none prose-headings:text-neutral-800 prose-a:text-amber-600 prose-code:bg-neutral-100 prose-pre:bg-neutral-900 prose-pre:text-neutral-100 [&_pre_code]:bg-transparent [&_pre_code]:text-inherit">
                  {viewMode === "current" && approvedContentEmpty ? (
                    <div className="rounded-md border border-amber-100 bg-amber-50 px-3 py-2 text-xs text-amber-700">
                      {hasCandidateContent
                        ? "当前还没有已批准内容。请切到候选视图检查内容，确认后点击批准内容。"
                        : "当前还没有已批准内容，也没有待批准候选。"}
                      {hasCandidateContent && (
                        <button
                          type="button"
                          onClick={() => setViewMode("candidate")}
                          className="ml-2 rounded-md border border-amber-200 bg-white px-2 py-0.5 text-amber-700 hover:bg-amber-100"
                        >
                          查看候选
                        </button>
                      )}
                    </div>
                  ) : (
                    <pre className="whitespace-pre-wrap text-sm leading-relaxed">
                      {viewMode === "candidate" && selectedWiki.content_candidate
                        ? selectedWiki.content_candidate
                        : selectedWiki.content}
                    </pre>
                  )}
                </div>
              )}

              {currentProjectId != null && (
                <div className="mt-6">
                  <WikiLinkEditor
                    project={currentProjectId}
                    pageId={selectedWiki.id}
                    pageSlug={selectedWiki.slug}
                    initialWikilinks={selectedWiki.wikilinks}
                    onWikilinksChange={(slugs) => {
                      setSelectedWiki((current) =>
                        current && current.id === selectedWiki.id
                          ? { ...current, wikilinks: JSON.stringify(slugs) }
                          : current,
                      )
                      void (async () => {
                        try {
                          await buildKnowledgeGraph(currentProjectId)
                          const nextNeighbors = await getGraphNeighbors(
                            currentProjectId,
                            selectedWiki.slug,
                          )
                          setNeighbors(nextNeighbors)
                        } catch {
                          setNeighbors([])
                        }
                      })()
                    }}
                  />
                </div>
              )}

              {neighbors.length > 0 && currentProjectId != null && (
                <div className="mt-6 border-t border-neutral-200 pt-4">
                  <h3 className="mb-3 text-sm font-medium text-neutral-600">图谱关联页面</h3>
                  <div className="flex flex-wrap gap-2">
                    {neighbors.map((neighbor) => (
                      <button
                        key={neighbor.slug}
                        type="button"
                        onClick={async () => {
                          try {
                            const pages = await listWikiPages(currentProjectId)
                            const found = pages.find((page) => page.slug === neighbor.slug)
                            if (found) {
                              await loadPage(found.id)
                            }
                          } catch {
                            /* 忽略关联页跳转失败 */
                          }
                        }}
                        className="rounded-full border border-amber-200 bg-amber-50 px-3 py-1 text-xs text-amber-700 hover:bg-amber-100"
                      >
                        {neighbor.title}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {currentProjectId != null && selectedWiki && (
                <div className="mt-6 border-t border-neutral-200 pt-4">
                  <GraphRecommendations project={currentProjectId} slug={selectedWiki.slug} />
                </div>
              )}
            </div>
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-neutral-400">
              <div className="text-center">
                <BookOpen className="mx-auto mb-2 h-10 w-10 opacity-20" />
                <p>选择一个 Wiki 页面查看内容</p>
              </div>
            </div>
          )}
        </div>
        )}
      </div>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenuItems}
          onClose={() => setContextMenu(null)}
        />
      )}

      <ImportModal
        open={importModalOpen}
        onClose={() => setImportModalOpen(false)}
        onImported={refreshWikiPages}
        project={currentProjectId ?? undefined}
      />

      {wikiFormMode && currentProjectId != null && (
        <WikiPageForm
          mode={wikiFormMode.kind}
          projectId={currentProjectId}
          initial={wikiFormMode.kind === "edit" ? wikiFormMode.page : undefined}
          onCancel={() => setWikiFormMode(null)}
          onSaved={(page) => {
            setWikiFormMode(null)
            setFeedbackMessage(
              wikiFormMode.kind === "create"
                ? `已创建 Wiki 页面：${page.title}`
                : `已保存：${page.title}`,
            )
            // 刷新列表 + 选中刚保存的页面
            void refreshWikiPages().then(() => {
              setSelectedWiki(page)
            })
          }}
        />
      )}
    </>
  )
}

/**
 * 行级 diff 渲染（基于 jsdiff）
 * - 绿色背景：候选新增
 * - 红色背景：候选删除（即原 content 有但候选没有）
 * - 无背景：未变化
 */
function WikiContentDiff({ oldText, newText }: { oldText: string; newText: string }) {
  const parts = useMemo(() => diffLines(oldText, newText), [oldText, newText])
  const addedCount = parts.filter((p) => p.added).length
  const removedCount = parts.filter((p) => p.removed).length

  return (
    <div>
      <div className="mb-2 flex gap-3 text-xs text-neutral-500">
        <span className="text-emerald-600">+ 新增 {addedCount} 段</span>
        <span className="text-rose-600">- 删除 {removedCount} 段</span>
      </div>
      <div className="rounded border border-neutral-200 bg-neutral-50 font-mono text-xs leading-relaxed">
        {parts.map((part) => {
          const bgClass = part.added
            ? "bg-emerald-50 text-emerald-900"
            : part.removed
              ? "bg-rose-50 text-rose-900"
              : "text-neutral-700"
          const prefix = part.added ? "+ " : part.removed ? "- " : "  "
          // 用 状态 + 长度 + 内容前 64 字符 做稳定 key
          // 长度作二级鉴别，避免两段恰好前 64 字符相同且状态相同时 key 冲突
          const valueKey = `${part.added ? "+" : part.removed ? "-" : "="}|${part.value.length}|${part.value.slice(0, 64)}`
          return (
            <div key={valueKey} className={`flex ${bgClass}`}>
              <span className="select-none pr-2 text-neutral-300">{prefix}</span>
              <pre className="whitespace-pre-wrap break-all py-0.5">
                {part.value.replace(/\n$/, "")}
              </pre>
            </div>
          )
        })}
      </div>
    </div>
  )
}
