import { AlertCircle, BookOpen, CheckCircle, FileUp, RefreshCw } from "lucide-react"
import { useCallback, useEffect, useMemo, useState } from "react"
import ContextMenu, { type ContextMenuItem } from "../components/ContextMenu"
import ImportModal from "../components/ImportModal"
import WikiLinkEditor from "../components/wiki/WikiLinkEditor"
import { useProject } from "../contexts/ProjectContext"
import {
  approveWikiPage,
  getGraphNeighbors,
  getWikiPage,
  listWikiPages,
  recompileFailedKbSources,
  type WikiPage,
  type WikiPageBrief,
} from "../lib/tauri-commands"

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
  const [recompiling, setRecompiling] = useState(false)
  const [recompileMessage, setRecompileMessage] = useState<string | null>(null)

  const refreshWikiPages = useCallback(async () => {
    if (currentProjectId == null) {
      setWikiPages([])
      setSelectedWiki(null)
      setNeighbors([])
      setLoading(false)
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
      if (currentProjectId == null) return
      getGraphNeighbors(currentProjectId, page.slug)
        .then(setNeighbors)
        .catch(() => setNeighbors([]))
    },
    [currentProjectId],
  )

  const handleRecompileFailed = useCallback(async () => {
    if (currentProjectId == null) return
    setRecompiling(true)
    setRecompileMessage(null)
    try {
      const result = await recompileFailedKbSources(currentProjectId)
      setRecompileMessage(
        result.failed.length > 0
          ? `重编译完成：成功 ${result.succeeded}/${result.retried} 项，失败 ${result.failed.length} 项`
          : `重编译完成：成功 ${result.succeeded}/${result.retried} 项`,
      )
      await refreshWikiPages()
    } catch (error) {
      setRecompileMessage(`重编译失败：${error instanceof Error ? error.message : String(error)}`)
    } finally {
      setRecompiling(false)
    }
  }, [currentProjectId, refreshWikiPages])

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

  return (
    <>
      {/* biome-ignore lint/a11y/noStaticElementInteractions: 面板右键菜单是标准交互模式 */}
      <div className="flex h-full gap-4 p-6" onContextMenu={handleContextMenu}>
        <div className="w-72 shrink-0 border-r border-neutral-200 pr-4">
          <div className="mb-3 flex items-center justify-between gap-2">
            <h3 className="text-sm font-medium text-neutral-600">知识页面</h3>
            <div className="flex items-center gap-2">
              <span className="text-xs text-neutral-400">{wikiPages.length} 页</span>
              <button
                type="button"
                onClick={handleRecompileFailed}
                disabled={currentProjectId == null || recompiling}
                className="inline-flex items-center gap-1 rounded-md border border-neutral-200 px-2 py-1 text-xs text-neutral-500 transition-colors hover:border-amber-200 hover:bg-amber-50 hover:text-amber-700 disabled:cursor-not-allowed disabled:opacity-50"
              >
                <RefreshCw className={`h-3 w-3 ${recompiling ? "animate-spin" : ""}`} />
                重编译失败项
              </button>
            </div>
          </div>

          {recompileMessage && (
            <div className="mb-3 rounded-md border border-amber-100 bg-amber-50 px-2 py-1.5 text-xs text-amber-700">
              {recompileMessage}
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
              {wikiPages.map((wikiPage) => (
                <button
                  type="button"
                  key={wikiPage.id}
                  onClick={() => loadPage(wikiPage.id)}
                  className={`w-full rounded-lg px-3 py-2 text-left text-sm transition-colors ${
                    selectedWiki?.id === wikiPage.id
                      ? "bg-amber-50 text-amber-700 border border-amber-200"
                      : "text-neutral-600 hover:bg-neutral-50 border border-transparent"
                  }`}
                >
                  <div className="font-medium">{wikiPage.title}</div>
                  <div className="mt-0.5 text-[11px] text-neutral-400">
                    {pageTypeLabel(wikiPage.page_type)}
                  </div>
                </button>
              ))}
            </div>
          )}
        </div>

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
                      </span>
                    )}
                  </div>
                </div>
                {selectedWiki.candidate_status === "pending" && (
                  <button
                    type="button"
                    onClick={async () => {
                      const updated = await approveWikiPage(selectedWiki.id)
                      setSelectedWiki(updated)
                      await refreshWikiPages()
                    }}
                    className="rounded-lg bg-amber-500 px-3 py-1.5 text-xs text-white hover:bg-amber-600"
                  >
                    批准内容
                  </button>
                )}
              </div>

              <div className="prose prose-sm max-w-none prose-headings:text-neutral-800 prose-a:text-amber-600 prose-code:bg-neutral-100 prose-pre:bg-neutral-900 prose-pre:text-neutral-100 [&_pre_code]:bg-transparent [&_pre_code]:text-inherit">
                <pre className="whitespace-pre-wrap text-sm leading-relaxed">
                  {selectedWiki.content}
                </pre>
              </div>

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
                      getGraphNeighbors(currentProjectId, selectedWiki.slug)
                        .then(setNeighbors)
                        .catch(() => setNeighbors([]))
                    }}
                  />
                </div>
              )}

              {neighbors.length > 0 && currentProjectId != null && (
                <div className="mt-6 border-t border-neutral-200 pt-4">
                  <h3 className="mb-3 text-sm font-medium text-neutral-600">关联页面</h3>
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
    </>
  )
}
