/**
 * 图扩展推荐组件
 *
 * 展示基于知识图谱的关联页面推荐结果。
 * 使用 graph_expand_search 获取多跳邻居，按组合权重排序展示。
 */

import { AlertCircle, Loader2, Network } from "lucide-react"
import { useCallback, useEffect, useState } from "react"
import { type GraphRecommendation, graphExpandSearch } from "../../lib/wiki-commands"

interface GraphRecommendationsProps {
  /** 当前项目标识 */
  project: string
  /** 当前页面 slug */
  slug: string
}

/** 信号类型中文标签 */
const SIGNAL_LABELS: Record<string, string> = {
  wikilink: "wikilink",
  tag: "tag 共现",
  source: "source 共源",
  co_citation: "共引",
}

/** 信号类型对应的背景色 */
const SIGNAL_COLORS: Record<string, string> = {
  wikilink: "bg-blue-100 text-blue-700",
  tag: "bg-emerald-100 text-emerald-700",
  source: "bg-amber-100 text-amber-700",
  co_citation: "bg-purple-100 text-purple-700",
}

export default function GraphRecommendations({ project, slug }: GraphRecommendationsProps) {
  const [recommendations, setRecommendations] = useState<GraphRecommendation[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const fetchRecommendations = useCallback(async () => {
    if (!project || !slug) return

    setLoading(true)
    setError(null)

    try {
      const results = await graphExpandSearch(project, slug)
      setRecommendations(results)
    } catch (err) {
      setError(typeof err === "string" ? err : "获取推荐失败")
      setRecommendations([])
    } finally {
      setLoading(false)
    }
  }, [project, slug])

  useEffect(() => {
    let cancelled = false
    fetchRecommendations().then(() => {
      if (cancelled) {
        setRecommendations([])
        setLoading(false)
      }
    })
    return () => {
      cancelled = true
    }
  }, [fetchRecommendations])

  // ── 权重格式化 ──
  const formatWeight = (weight: number): string => {
    return weight.toFixed(2)
  }

  // ── 加载状态 ──
  if (loading) {
    return (
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <div className="mb-3 flex items-center gap-2">
          <Network className="h-4 w-4 text-neutral-500" />
          <h3 className="text-sm font-medium text-neutral-800">相关页面</h3>
        </div>
        <div className="flex items-center justify-center py-6">
          <Loader2 className="h-5 w-5 animate-spin text-neutral-400" />
          <span className="ml-2 text-xs text-neutral-400">正在检索关联页面...</span>
        </div>
      </div>
    )
  }

  // ── 错误状态 ──
  if (error) {
    return (
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <div className="mb-3 flex items-center gap-2">
          <Network className="h-4 w-4 text-neutral-500" />
          <h3 className="text-sm font-medium text-neutral-800">相关页面</h3>
        </div>
        <div className="flex items-center gap-2 rounded-md bg-red-50 px-3 py-2">
          <AlertCircle className="h-4 w-4 text-red-400" />
          <span className="text-xs text-red-600">{error}</span>
        </div>
      </div>
    )
  }

  // ── 空状态 ──
  if (recommendations.length === 0) {
    return (
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <div className="mb-3 flex items-center gap-2">
          <Network className="h-4 w-4 text-neutral-500" />
          <h3 className="text-sm font-medium text-neutral-800">相关页面</h3>
        </div>
        <p className="text-xs text-neutral-400">暂无关联页面推荐</p>
      </div>
    )
  }

  // ── 正常展示 ──
  return (
    <div className="rounded-lg border border-neutral-200 bg-white p-4">
      {/* 标题 */}
      <div className="mb-3 flex items-center gap-2">
        <Network className="h-4 w-4 text-neutral-500" />
        <h3 className="text-sm font-medium text-neutral-800">
          相关页面
          <span className="ml-1.5 text-xs text-neutral-400">
            · 图扩展 · {recommendations.length} 个推荐
          </span>
        </h3>
      </div>

      {/* 推荐列表 */}
      <ul className="space-y-1">
        {recommendations.map((rec) => (
          <li
            key={rec.slug}
            className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-neutral-50 transition-colors"
          >
            {/* 标题 */}
            <span className="flex-1 truncate text-xs text-neutral-800">
              {rec.title || rec.slug}
            </span>

            {/* 信号标签 */}
            {rec.matched_signals.map((signal) => (
              <span
                key={signal}
                className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium ${SIGNAL_COLORS[signal] ?? "bg-neutral-100 text-neutral-500"}`}
              >
                {SIGNAL_LABELS[signal] ?? signal}
              </span>
            ))}

            {/* 权重 */}
            <span className="shrink-0 text-[10px] text-neutral-400 tabular-nums">
              {formatWeight(rec.combined_weight)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  )
}
