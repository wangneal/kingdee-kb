// 图谱页状态提示横幅：边=0 → 提示构建；wikilink=0 → 提示 LLM 未生成 [[slug]] 引用
import { AlertTriangle, Info } from "lucide-react"
import type { GraphStats } from "../../lib/tauri-commands"

export type BannerVariant = "info" | "warning"

export interface BannerContent {
  variant: BannerVariant
  title: string
  body: string
  hint?: string
}

/** 纯函数：stats → banner 内容（不显示则 null） */
export function getBannerContent(stats: GraphStats | null): BannerContent | null {
  if (!stats) return null

  if (stats.total_edges === 0) {
    return {
      variant: "info",
      title: "图谱尚未构建",
      body: "当前项目没有知识图谱。点击右上角「构建图谱」从已批准页面提取关联。",
      hint: "首次构建可能需要 10-60 秒，取决于页面数量。",
    }
  }

  // wikilink=0 但其他边>0：LLM 未在 content_candidate 输出 [[slug]]
  const wikilinkCount = stats.signal_breakdown.wikilink ?? 0
  if (wikilinkCount === 0) {
    return {
      variant: "warning",
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
      {isWarning ? (
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
