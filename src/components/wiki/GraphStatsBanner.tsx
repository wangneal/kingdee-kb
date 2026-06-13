/**
 * 知识图谱状态提示横幅
 *
 * 解决 #4 P0 bug：用户进图谱页看到"0 边"或"无 wikilink 边"时不知是 bug 还是数据问题。
 * 本组件根据 GraphStats 给出可执行建议，避免用户以为是程序问题。
 *
 * 显示规则（由 `getBannerContent` 决定，可独立单测）：
 * 1. stats 为 null：什么都不显示
 * 2. total_edges === 0：图谱未构建，蓝色 info banner 提示"请先点击构建图谱"
 * 3. signal_breakdown.wikilink === 0 且 total_edges > 0：黄色 warning banner，提示
 *    "wikilink 边缺失 — 知识编译 LLM 未在 content_candidate 中生成 [[slug]] 引用"
 * 4. 其他情况：什么都不显示
 */
import { AlertTriangle, Info } from "lucide-react"
import type { GraphStats } from "../../lib/tauri-commands"

export type BannerVariant = "info" | "warning" | null

export interface BannerContent {
  variant: NonNullable<BannerVariant>
  icon: "info" | "warning"
  title: string
  body: string
  hint?: string
}

/// 决定 banner 是否显示以及显示什么 —— 纯函数，易测
export function getBannerContent(stats: GraphStats | null): BannerContent | null {
  if (!stats) return null

  // 1. 图谱完全没边
  if (stats.total_edges === 0) {
    return {
      variant: "info",
      icon: "info",
      title: "图谱尚未构建",
      body: "当前项目没有知识图谱。点击右上角「构建图谱」从已批准页面提取关联。",
      hint: "首次构建可能需要 10-60 秒，取决于页面数量。",
    }
  }

  // 2. wikilink 边缺失（其他信号有边但 wikilink=0）
  const wikilinkCount = stats.signal_breakdown.wikilink ?? 0
  if (wikilinkCount === 0) {
    return {
      variant: "warning",
      icon: "warning",
      title: "wikilink 边缺失",
      body: "知识图谱未生成 wikilink 边。常见原因：知识编译 LLM 未在 content_candidate 中生成 [[slug]] 形式的引用。",
      hint: "已批准的 wiki 页面之间仍会通过 tag / source / co_citation 信号建立关联。可在「设置 → LLM 提示词」中加强 [[slug]] 强制要求。",
    }
  }

  return null
}

interface GraphStatsBannerProps {
  stats: GraphStats | null
}

export default function GraphStatsBanner({ stats }: GraphStatsBannerProps) {
  const content = getBannerContent(stats)
  if (!content) return null

  const isWarning = content.variant === "warning"

  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="graph-stats-banner"
      data-variant={content.variant}
      className={`mx-6 mt-3 flex gap-2.5 rounded-lg border p-3 ${
        isWarning ? "border-amber-200 bg-amber-50" : "border-sky-200 bg-sky-50"
      }`}
    >
      {content.icon === "warning" ? (
        <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-600" />
      ) : (
        <Info className="mt-0.5 h-4 w-4 shrink-0 text-sky-600" />
      )}
      <div className="flex-1 text-xs">
        <p className={`font-medium ${isWarning ? "text-amber-900" : "text-sky-900"}`}>
          {content.title}
        </p>
        <p className={`mt-0.5 leading-relaxed ${isWarning ? "text-amber-800" : "text-sky-800"}`}>
          {content.body}
        </p>
        {content.hint && (
          <p
            className={`mt-1 text-[11px] leading-relaxed ${isWarning ? "text-amber-700" : "text-sky-700"}`}
          >
            {content.hint}
          </p>
        )}
      </div>
    </div>
  )
}
