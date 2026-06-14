/**
 * 知识图谱页面
 *
 * 展示项目的知识图谱统计、节点列表和关联推荐。
 * 支持手动构建/重建图谱。
 */
import { BookOpen, Loader2, Network, RefreshCw } from "lucide-react"
import { useEffect, useState } from "react"
import { useToast } from "@/components/Toast"
import GraphRecommendations from "@/components/wiki/GraphRecommendations"
import GraphStatsBanner from "@/components/wiki/GraphStatsBanner"
import { useAppError } from "@/contexts/AppErrorContext"
import { useProject } from "@/contexts/ProjectContext"
import { formatAppError, parseAppError } from "@/lib/app-error"
import {
  buildKnowledgeGraph,
  type GraphStats,
  getGraphStats,
  listWikiPages,
  type WikiPageBrief,
} from "@/lib/tauri-commands"

const SIGNAL_LABELS: Record<string, string> = {
  wikilink: "wikilink",
  tag: "tag 共现",
  source: "source 共源",
  co_citation: "共引",
}

const PAGE_TYPE_LABELS: Record<string, string> = {
  entity: "实体",
  concept: "概念",
}

export default function KnowledgeGraph() {
  const { currentProjectId } = useProject()
  const toast = useToast()
  const { showLlmKeyError } = useAppError()
  const [stats, setStats] = useState<GraphStats | null>(null)
  const [pages, setPages] = useState<WikiPageBrief[]>([])
  const [loading, setLoading] = useState(true)
  const [building, setBuilding] = useState(false)
  const [selectedSlug, setSelectedSlug] = useState<string | null>(null)

  // 加载统计和页面列表
  useEffect(() => {
    if (currentProjectId == null) {
      setStats(null)
      setPages([])
      setSelectedSlug(null)
      setLoading(false)
      return
    }
    setLoading(true)
    setStats(null)
    setPages([])
    setSelectedSlug(null)
    Promise.all([
      getGraphStats(currentProjectId)
        .then(setStats)
        .catch(() => setStats(null)),
      listWikiPages(currentProjectId)
        .then(setPages)
        .catch(() => setPages([])),
    ]).finally(() => setLoading(false))
  }, [currentProjectId])

  // 构建图谱
  const handleBuild = async () => {
    if (currentProjectId == null) return
    setBuilding(true)
    try {
      const edges = await buildKnowledgeGraph(currentProjectId)
      toast.success(`知识图谱构建完成，共 ${edges} 条边`)
      const [s, pageList] = await Promise.all([
        getGraphStats(currentProjectId),
        listWikiPages(currentProjectId),
      ])
      setStats(s)
      setPages(pageList)
    } catch (err) {
      // LLM Key 失效 → 弹配置对话框而非普通 toast
      const parsed = parseAppError(err)
      if (parsed?.code === "LLM_INVALID_KEY") {
        showLlmKeyError(parsed)
      } else {
        toast.error(`构建失败: ${formatAppError(err)}`)
      }
    } finally {
      setBuilding(false)
    }
  }

  // 加载状态
  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-5 w-5 animate-spin text-neutral-400" />
        <span className="ml-2 text-sm text-neutral-500">加载中...</span>
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
        <div className="flex items-center gap-2">
          <Network className="h-5 w-5 text-[#1A6BD8]" />
          <h1 className="text-base font-semibold text-neutral-800">知识图谱</h1>
          {stats && (
            <span className="text-xs text-neutral-400">
              {stats.total_nodes} 个节点 · {stats.total_edges} 条边
            </span>
          )}
        </div>
        <button
          type="button"
          onClick={handleBuild}
          disabled={building || currentProjectId == null}
          className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
        >
          {building ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" />
          )}
          {building ? "构建中..." : "构建图谱"}
        </button>
      </div>

      <div className="flex flex-1 flex-col overflow-hidden">
        {/* wikilink=0 / 图谱为空 时显示提示 banner，详情见 GraphStatsBanner 注释 */}
        <GraphStatsBanner stats={stats} />

        <div className="flex flex-1 overflow-hidden">
          {/* 左侧：统计 + 页面列表 */}
          <div className="w-72 shrink-0 overflow-y-auto border-r border-neutral-200 p-4">
            {/* 统计卡片 */}
            {stats ? (
              <div className="mb-4 space-y-2">
                <h3 className="text-xs font-semibold text-neutral-700">图统计</h3>
                <div className="grid grid-cols-2 gap-2">
                  <div className="rounded-lg border border-neutral-200 bg-white p-3 text-center">
                    <p className="text-lg font-semibold text-neutral-800">{stats.total_nodes}</p>
                    <p className="text-[10px] text-neutral-400">节点数</p>
                  </div>
                  <div className="rounded-lg border border-neutral-200 bg-white p-3 text-center">
                    <p className="text-lg font-semibold text-neutral-800">{stats.total_edges}</p>
                    <p className="text-[10px] text-neutral-400">边数</p>
                  </div>
                </div>
                {stats.avg_degree > 0 && (
                  <div className="rounded-lg border border-neutral-200 bg-white p-3 text-center">
                    <p className="text-lg font-semibold text-neutral-800">
                      {stats.avg_degree.toFixed(1)}
                    </p>
                    <p className="text-[10px] text-neutral-400">平均度数</p>
                  </div>
                )}
                {Object.keys(stats.signal_breakdown).length > 0 && (
                  <div className="rounded-lg border border-neutral-200 bg-white p-3">
                    <p className="mb-1.5 text-[10px] font-medium text-neutral-500">信号分布</p>
                    <div className="space-y-1">
                      {Object.entries(stats.signal_breakdown).map(([signal, count]) => (
                        <div key={signal} className="flex items-center justify-between text-xs">
                          <span className="text-neutral-600">
                            {SIGNAL_LABELS[signal] || signal}
                          </span>
                          <span className="font-mono text-neutral-400">{count}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            ) : (
              <div className="mb-4 rounded-lg border border-dashed border-neutral-200 bg-neutral-50 p-4 text-center">
                <Network className="mx-auto mb-1 h-6 w-6 text-neutral-300" />
                <p className="text-xs text-neutral-500">图谱未构建</p>
                <p className="mt-1 text-[10px] text-neutral-400">点击"构建图谱"生成</p>
              </div>
            )}

            {/* 页面列表 */}
            <div>
              <h3 className="mb-2 text-xs font-semibold text-neutral-700">
                知识页面
                <span className="ml-1 font-normal text-neutral-400">({pages.length})</span>
              </h3>
              {pages.length === 0 ? (
                <div className="py-6 text-center">
                  <BookOpen className="mx-auto mb-2 h-6 w-6 text-neutral-200" />
                  <p className="text-xs text-neutral-400">暂无页面</p>
                </div>
              ) : (
                <div className="space-y-0.5">
                  {pages.map((page) => (
                    <button
                      key={page.id}
                      type="button"
                      onClick={() => setSelectedSlug(page.slug)}
                      className={`w-full rounded-md px-2 py-1.5 text-left text-xs transition-colors ${
                        selectedSlug === page.slug
                          ? "bg-[#1A6BD8]/10 text-[#1A6BD8]"
                          : "text-neutral-600 hover:bg-neutral-100"
                      }`}
                    >
                      <span className="font-medium">{page.title}</span>
                      <span className="ml-1.5 text-[10px] text-neutral-400">
                        {PAGE_TYPE_LABELS[page.page_type] || page.page_type}
                      </span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* 右侧：关联推荐 */}
          <div className="flex-1 overflow-y-auto p-6">
            {selectedSlug && currentProjectId != null ? (
              <GraphRecommendations project={currentProjectId} slug={selectedSlug} />
            ) : (
              <div className="flex h-full items-center justify-center">
                <div className="text-center">
                  <Network className="mx-auto mb-3 h-10 w-10 text-neutral-200" />
                  <p className="text-sm text-neutral-400">选择一个节点查看关联推荐</p>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
