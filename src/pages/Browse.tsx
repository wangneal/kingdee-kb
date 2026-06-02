import { AlertCircle, BookOpen, CheckCircle } from "lucide-react"
import { useEffect, useState } from "react"
import WikiLinkEditor from "../components/wiki/WikiLinkEditor"
import { useProject } from "../contexts/ProjectContext"
import {
  approveWikiPage,
  getGraphNeighbors,
  getWikiPage,
  listWikiPages,
  type WikiPage,
  type WikiPageBrief,
} from "../lib/tauri-commands"

export default function Browse() {
  const { projectId } = useProject()
  const [wikiPages, setWikiPages] = useState<WikiPageBrief[]>([])
  const [selectedWiki, setSelectedWiki] = useState<WikiPage | null>(null)
  const [neighbors, setNeighbors] = useState<
    { slug: string; title: string; relation: string; weight: number }[]
  >([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    if (!projectId) return
    setLoading(true)
    listWikiPages(projectId)
      .then(setWikiPages)
      .catch(() => setWikiPages([]))
      .finally(() => setLoading(false))
  }, [projectId])

  const pageTypeLabel = (t: string) => {
    if (t === "entity") return "实体"
    if (t === "concept") return "概念"
    return t
  }

  const statusLabel = (s: string) => {
    if (s === "pending") return "待审核"
    if (s === "conflict") return "有冲突"
    return "已确认"
  }

  const statusIcon = (s: string) => {
    if (s === "pending") return <AlertCircle className="h-3 w-3 text-yellow-500" />
    if (s === "conflict") return <AlertCircle className="h-3 w-3 text-red-500" />
    return <CheckCircle className="h-3 w-3 text-green-500" />
  }

  return (
    <div className="flex h-full gap-4 p-6">
      {/* 左侧 Wiki 页面列表 */}
      <div className="w-72 shrink-0 border-r border-neutral-200 pr-4">
        <div className="mb-3 flex items-center justify-between">
          <h3 className="text-sm font-medium text-neutral-600">知识页面</h3>
          <span className="text-xs text-neutral-400">{wikiPages.length} 页</span>
        </div>
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
            {wikiPages.map((wp) => (
              <button
                type="button"
                key={wp.id}
                onClick={async () => {
                  const page = await getWikiPage(wp.id)
                  setSelectedWiki(page)
                  getGraphNeighbors(projectId ?? "", page.slug)
                    .then(setNeighbors)
                    .catch(() => setNeighbors([]))
                }}
                className={`w-full rounded-lg px-3 py-2 text-left text-sm transition-colors ${
                  selectedWiki?.id === wp.id
                    ? "bg-amber-50 text-amber-700 border border-amber-200"
                    : "text-neutral-600 hover:bg-neutral-50 border border-transparent"
                }`}
              >
                <div className="font-medium">{wp.title}</div>
                <div className="mt-0.5 text-[11px] text-neutral-400">
                  {pageTypeLabel(wp.page_type)}
                </div>
              </button>
            ))}
          </div>
        )}
      </div>

      {/* 右侧 Wiki 页面内容 */}
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
                  }}
                  className="rounded-lg bg-amber-500 px-3 py-1.5 text-xs text-white hover:bg-amber-600"
                >
                  批准内容
                </button>
              )}
            </div>
            <div className="prose prose-sm max-w-none prose-headings:text-neutral-800 prose-a:text-amber-600 prose-code:bg-neutral-100 prose-pre:bg-neutral-900 prose-pre:text-neutral-100">
              <pre className="whitespace-pre-wrap text-sm leading-relaxed">
                {selectedWiki.content}
              </pre>
            </div>

            <div className="mt-6">
              <WikiLinkEditor
                project={projectId ?? ""}
                pageId={selectedWiki.id}
                pageSlug={selectedWiki.slug}
                initialWikilinks={selectedWiki.wikilinks}
                onWikilinksChange={(slugs) => {
                  setSelectedWiki((current) =>
                    current && current.id === selectedWiki.id
                      ? { ...current, wikilinks: JSON.stringify(slugs) }
                      : current,
                  )
                  getGraphNeighbors(projectId ?? "", selectedWiki.slug)
                    .then(setNeighbors)
                    .catch(() => setNeighbors([]))
                }}
              />
            </div>

            {/* 关联页面 */}
            {neighbors.length > 0 && (
              <div className="mt-6 border-t border-neutral-200 pt-4">
                <h3 className="mb-3 text-sm font-medium text-neutral-600">关联页面</h3>
                <div className="flex flex-wrap gap-2">
                  {neighbors.map((n) => (
                    <button
                      key={n.slug}
                      type="button"
                      onClick={async () => {
                        try {
                          const pages = await listWikiPages(projectId ?? "")
                          const found = pages.find((p) => p.slug === n.slug)
                          if (found) {
                            const page = await getWikiPage(found.id)
                            setSelectedWiki(page)
                            getGraphNeighbors(projectId ?? "", page.slug)
                              .then(setNeighbors)
                              .catch(() => setNeighbors([]))
                          }
                        } catch {
                          /* 忽略关联页跳转失败 */
                        }
                      }}
                      className="rounded-full border border-amber-200 bg-amber-50 px-3 py-1 text-xs text-amber-700 hover:bg-amber-100"
                    >
                      {n.title}
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
  )
}
