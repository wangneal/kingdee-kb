/**
 * 维基链接搜索弹窗
 *
 * 按标题模糊搜索候选页面，点击选中后关闭弹窗。
 * 使用 300ms 防抖避免频繁请求。
 */

import { Search, X } from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { searchWikilinkCandidates, type WikiPageBrief } from "../../lib/wiki-commands"

interface WikiLinkSearchProps {
  /** 当前项目标识 */
  project: number
  /** 排除的页面 slug（避免自引用） */
  excludeSlug: string
  /** 选中候选页面后的回调 */
  onSelect: (slug: string, title: string) => void
  /** 关闭弹窗的回调 */
  onClose: () => void
}

export default function WikiLinkSearch({
  project,
  excludeSlug,
  onSelect,
  onClose,
}: WikiLinkSearchProps) {
  const [query, setQuery] = useState("")
  const [results, setResults] = useState<WikiPageBrief[]>([])
  const [isSearching, setIsSearching] = useState(false)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  // 自动聚焦搜索框
  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  // 防抖搜索：输入变化后 300ms 发起请求
  useEffect(() => {
    // 清除上一次定时器
    if (timerRef.current) {
      clearTimeout(timerRef.current)
    }

    // 空查询时清空结果
    if (!query.trim()) {
      setResults([])
      setIsSearching(false)
      return
    }

    setIsSearching(true)
    timerRef.current = setTimeout(async () => {
      try {
        const candidates = await searchWikilinkCandidates(project, query.trim(), excludeSlug)
        setResults(candidates)
      } catch {
        setResults([])
      } finally {
        setIsSearching(false)
      }
    }, 300)

    // 清理函数
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current)
      }
    }
  }, [query, project, excludeSlug])

  // 选中候选页面
  const handleSelect = useCallback(
    (candidate: WikiPageBrief) => {
      onSelect(candidate.slug, candidate.title)
    },
    [onSelect],
  )

  // 页面类型对应的中文标签
  const pageTypeLabel = useCallback((pageType: string): string => {
    const map: Record<string, string> = {
      blueprint: "蓝图",
      fitgap: "Fit-Gap",
      research: "调研",
      reference: "参考",
      summary: "摘要",
      template: "模板",
      other: "其他",
    }
    return map[pageType] ?? pageType
  }, [])

  return (
    // 遮罩层
    <div
      role="dialog"
      tabIndex={-1}
      aria-modal="true"
      aria-label="维基链接搜索"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30"
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose()
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") onClose()
      }}
    >
      {/* 弹窗主体 */}
      <div className="mx-4 w-full max-w-md rounded-xl bg-white shadow-lg">
        {/* 搜索栏 */}
        <div className="flex items-center gap-2 border-b border-neutral-200 px-4 py-3">
          <Search className="h-4 w-4 text-neutral-400" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索页面标题..."
            className="flex-1 text-sm outline-none placeholder:text-neutral-400"
          />
          <button
            type="button"
            onClick={onClose}
            className="rounded p-1 text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* 结果列表 */}
        <div className="max-h-64 overflow-y-auto">
          {isSearching && (
            <div className="flex items-center justify-center py-8">
              <div className="h-4 w-4 animate-spin rounded-full border-2 border-neutral-300 border-t-[#1A6BD8]" />
            </div>
          )}

          {!isSearching && query.trim() && results.length === 0 && (
            <p className="py-8 text-center text-xs text-neutral-400">未找到匹配页面</p>
          )}

          {!isSearching && !query.trim() && (
            <p className="py-8 text-center text-xs text-neutral-400">输入关键词搜索页面</p>
          )}

          {!isSearching &&
            results.map((candidate) => (
              <button
                key={candidate.id}
                type="button"
                onClick={() => handleSelect(candidate)}
                className="flex w-full items-center gap-3 px-4 py-2.5 text-left hover:bg-neutral-50 transition-colors"
              >
                <span className="flex-1 truncate text-sm text-neutral-800">{candidate.title}</span>
                <span className="shrink-0 rounded bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">
                  {pageTypeLabel(candidate.page_type)}
                </span>
              </button>
            ))}
        </div>
      </div>
    </div>
  )
}
