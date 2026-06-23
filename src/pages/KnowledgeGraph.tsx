/**
 * 知识图谱页面
 *
 * 展示项目的知识图谱 —— 力导向节点连线图 + 统计面板 + 关联推荐。
 * 支持手动构建/重建图谱。
 */
import { Loader2, Network, RefreshCw } from "lucide-react"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import ForceGraph2D from "react-force-graph-2d"
import { useToast } from "@/components/Toast"
import GraphRecommendations from "@/components/wiki/GraphRecommendations"
import GraphStatsBanner from "@/components/wiki/GraphStatsBanner"
import { useAppError } from "@/contexts/AppErrorContext"
import { useProject } from "@/contexts/ProjectContext"
import { formatAppError, parseAppError } from "@/lib/app-error"
import {
  buildKnowledgeGraph,
  type FullGraph,
  type FullGraphEdge,
  type FullGraphNode,
  type GraphStats,
  getFullGraph,
  getGraphStats,
} from "@/lib/tauri-commands"

// ── 常量 ──────────────────────────────────────────────────────────────────

const SIGNAL_COLORS: Record<string, string> = {
  wikilink: "#1A6BD8",
  tag: "#10B981",
  source: "#F59E0B",
  co_citation: "#8B5CF6",
}

const SIGNAL_LABELS: Record<string, string> = {
  wikilink: "wikilink",
  tag: "tag 共现",
  source: "source 共源",
  co_citation: "共引",
}

const PAGE_TYPE_COLORS: Record<string, string> = {
  entity: "#1A6BD8",
  concept: "#10B981",
  blueprint: "#F59E0B",
  fitgap: "#EF4444",
  research: "#8B5CF6",
  unknown: "#9CA3AF",
}

// ── GraphData 适配 react-force-graph-2d ──────────────────────────────────

interface GraphNode {
  id: string
  name: string
  pageType: string
  degree: number
  color: string
  x?: number
  y?: number
}

interface GraphLink {
  source: string
  target: string
  signal: string
  weight: number
  color: string
}

interface GraphData {
  nodes: GraphNode[]
  links: GraphLink[]
}

function toGraphData(fg: FullGraph): GraphData {
  return {
    nodes: fg.nodes.map((n: FullGraphNode) => ({
      id: n.slug,
      name: n.title,
      pageType: n.page_type,
      degree: n.degree,
      color: PAGE_TYPE_COLORS[n.page_type] ?? PAGE_TYPE_COLORS.unknown,
    })),
    links: fg.edges.map((e: FullGraphEdge) => ({
      source: e.source,
      target: e.target,
      signal: e.signal,
      weight: e.weight,
      color: SIGNAL_COLORS[e.signal] ?? "#CBD5E1",
    })),
  }
}

// ── 组件 ──────────────────────────────────────────────────────────────────

export default function KnowledgeGraph() {
  const { currentProjectId } = useProject()
  const toast = useToast()
  const { showLlmKeyError } = useAppError()

  const [stats, setStats] = useState<GraphStats | null>(null)
  const [fullGraph, setFullGraph] = useState<FullGraph | null>(null)
  const [loading, setLoading] = useState(true)
  const [building, setBuilding] = useState(false)
  const [selectedSlug, setSelectedSlug] = useState<string | null>(null)

  // 容器尺寸跟踪
  const containerRef = useRef<HTMLDivElement>(null)
  const [dimensions, setDimensions] = useState({ width: 800, height: 600 })

  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect
        if (width > 0 && height > 0) {
          setDimensions({ width, height })
        }
      }
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // 加载数据
  useEffect(() => {
    if (currentProjectId == null) {
      setStats(null)
      setFullGraph(null)
      setSelectedSlug(null)
      setLoading(false)
      return
    }
    setLoading(true)
    setStats(null)
    setFullGraph(null)
    setSelectedSlug(null)
    Promise.all([
      getGraphStats(currentProjectId)
        .then(setStats)
        .catch(() => setStats(null)),
      getFullGraph(currentProjectId)
        .then(setFullGraph)
        .catch(() => setFullGraph(null)),
    ]).finally(() => setLoading(false))
  }, [currentProjectId])

  // 构建图谱
  const handleBuild = async () => {
    if (currentProjectId == null) return
    setBuilding(true)
    try {
      const edges = await buildKnowledgeGraph(currentProjectId)
      toast.success(`知识图谱构建完成，共 ${edges} 条边`)
      const [s, fg] = await Promise.all([
        getGraphStats(currentProjectId),
        getFullGraph(currentProjectId),
      ])
      setStats(s)
      setFullGraph(fg)
    } catch (err) {
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

  // 转换为 react-force-graph 数据
  const graphData: GraphData = useMemo(
    () => (fullGraph ? toGraphData(fullGraph) : { nodes: [], links: [] }),
    [fullGraph],
  )

  // 节点点击
  const handleNodeClick = useCallback((node: GraphNode) => {
    setSelectedSlug(node.id)
  }, [])

  // 自定义节点绘制（带标签）
  const nodeCanvasObject = useCallback(
    (node: GraphNode, ctx: CanvasRenderingContext2D, globalScale: number) => {
      const label = node.name
      const fontSize = Math.max(10, 14 / globalScale)
      const radius = Math.max(4, 4 + Math.min(node.degree, 10))
      const isSelected = node.id === selectedSlug

      // 节点圆圈
      ctx.beginPath()
      ctx.arc(node.x ?? 0, node.y ?? 0, radius, 0, 2 * Math.PI)
      ctx.fillStyle = node.color
      ctx.fill()
      if (isSelected) {
        ctx.strokeStyle = "#000"
        ctx.lineWidth = 2 / globalScale
        ctx.stroke()
      }

      // 标签
      ctx.font = `${fontSize}px sans-serif`
      ctx.textAlign = "center"
      ctx.textBaseline = "top"
      ctx.fillStyle = "#1F2937"
      ctx.fillText(label, node.x ?? 0, (node.y ?? 0) + radius + 2)

      return undefined
    },
    [selectedSlug],
  )

  // 加载中
  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-neutral-400" />
      </div>
    )
  }

  const hasGraph = fullGraph != null && fullGraph.nodes.length > 0

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
        <GraphStatsBanner stats={stats} />

        <div className="flex flex-1 overflow-hidden">
          {/* 左侧：统计 + 图例 */}
          <div className="w-64 shrink-0 overflow-y-auto border-r border-neutral-200 p-4">
            {/* 统计卡片 */}
            {stats ? (
              <div className="mb-4 space-y-2">
                <h3 className="text-xs font-semibold text-neutral-700">图统计</h3>
                <div className="grid grid-cols-2 gap-2">
                  <div className="rounded-lg border border-neutral-200 bg-white p-3 text-center">
                    <p className="text-lg font-semibold text-neutral-800">
                      {stats.total_nodes}
                    </p>
                    <p className="text-[10px] text-neutral-400">节点数</p>
                  </div>
                  <div className="rounded-lg border border-neutral-200 bg-white p-3 text-center">
                    <p className="text-lg font-semibold text-neutral-800">
                      {stats.total_edges}
                    </p>
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
                    <p className="mb-1.5 text-[10px] font-medium text-neutral-500">
                      信号分布
                    </p>
                    <div className="space-y-1">
                      {Object.entries(stats.signal_breakdown).map(
                        ([signal, count]) => (
                          <div
                            key={signal}
                            className="flex items-center justify-between text-xs"
                          >
                            <span className="flex items-center gap-1.5">
                              <span
                                className="inline-block h-2 w-2 rounded-full"
                                style={{
                                  backgroundColor:
                                    SIGNAL_COLORS[signal] ?? "#CBD5E1",
                                }}
                              />
                              <span className="text-neutral-600">
                                {SIGNAL_LABELS[signal] || signal}
                              </span>
                            </span>
                            <span className="font-mono text-neutral-400">
                              {count}
                            </span>
                          </div>
                        ),
                      )}
                    </div>
                  </div>
                )}
              </div>
            ) : (
              <div className="mb-4 rounded-lg border border-dashed border-neutral-200 bg-neutral-50 p-4 text-center">
                <Network className="mx-auto mb-1 h-6 w-6 text-neutral-300" />
                <p className="text-xs text-neutral-500">图谱未构建</p>
                <p className="mt-1 text-[10px] text-neutral-400">
                  点击"构建图谱"生成
                </p>
              </div>
            )}

            {/* 图例 */}
            {hasGraph && (
              <div className="space-y-2">
                <h3 className="text-xs font-semibold text-neutral-700">图例</h3>
                <div className="rounded-lg border border-neutral-200 bg-white p-3 space-y-2">
                  <p className="text-[10px] font-medium text-neutral-500">
                    边类型
                  </p>
                  {Object.entries(SIGNAL_COLORS).map(([signal, color]) => (
                    <div
                      key={signal}
                      className="flex items-center gap-1.5 text-xs"
                    >
                      <span
                        className="inline-block h-0.5 w-4 rounded"
                        style={{ backgroundColor: color }}
                      />
                      <span className="text-neutral-600">
                        {SIGNAL_LABELS[signal]}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>

          {/* 右侧：力导向图 */}
          <div ref={containerRef} className="relative flex-1 overflow-hidden">
            {hasGraph ? (
              <ForceGraph2D
                graphData={graphData}
                width={dimensions.width}
                height={dimensions.height}
                nodeCanvasObject={nodeCanvasObject}
                nodePointerAreaPaint={(
                  node: GraphNode,
                  color: string,
                  ctx: CanvasRenderingContext2D,
                ) => {
                  const radius = Math.max(4, 4 + Math.min(node.degree, 10))
                  ctx.fillStyle = color
                  ctx.beginPath()
                  ctx.arc(node.x ?? 0, node.y ?? 0, radius + 4, 0, 2 * Math.PI)
                  ctx.fill()
                }}
                linkColor={(link: GraphLink) => link.color}
                linkWidth={(link: GraphLink) => Math.max(0.5, link.weight)}
                linkDirectionalArrowLength={4}
                linkDirectionalArrowRelPos={0.5}
                onNodeClick={handleNodeClick}
                cooldownTicks={100}
                d3AlphaDecay={0.02}
                d3VelocityDecay={0.3}
              />
            ) : (
              <div className="flex h-full items-center justify-center">
                <div className="text-center">
                  <Network className="mx-auto mb-3 h-10 w-10 text-neutral-200" />
                  <p className="text-sm text-neutral-400">
                    {currentProjectId == null
                      ? "请先选择项目"
                      : "图谱为空，请先构建图谱"}
                  </p>
                </div>
              </div>
            )}

            {/* 选中节点的关联推荐浮层 */}
            {selectedSlug && currentProjectId != null && (
              <div className="absolute right-4 top-4 w-72 max-h-[60%] overflow-y-auto rounded-lg border border-neutral-200 bg-white shadow-lg">
                <div className="flex items-center justify-between border-b border-neutral-100 px-3 py-2">
                  <span className="text-xs font-semibold text-neutral-700">
                    关联推荐
                  </span>
                  <button
                    type="button"
                    onClick={() => setSelectedSlug(null)}
                    className="text-neutral-400 hover:text-neutral-600 text-xs"
                  >
                    ✕
                  </button>
                </div>
                <div className="p-3">
                  <GraphRecommendations
                    project={currentProjectId}
                    slug={selectedSlug}
                  />
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
