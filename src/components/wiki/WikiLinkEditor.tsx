/**
 * 维基链接编辑器
 *
 * 展示当前页面的 wikilinks 列表（可移除）、添加链接按钮和反向链接区域。
 */

import { ArrowLeft, Link2, Plus, X } from "lucide-react"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  addWikilink,
  getBacklinks,
  getWikilinkTargets,
  removeWikilink,
  type WikiLinkTarget,
  type WikiPageBrief,
} from "../../lib/wiki-commands"
import WikiLinkSearch from "./WikiLinkSearch"

interface WikiLinkEditorProps {
  /** 当前项目标识 */
  project: number
  /** 当前页面 ID */
  pageId: number
  /** 当前页面 slug（用于排除自引用和查询反向链接） */
  pageSlug: string
  /** 初始 wikilinks JSON 字符串（slug 数组） */
  initialWikilinks?: string
  /** wikilinks 变化后的回调 */
  onWikilinksChange?: (slugs: string[]) => void
}

function normalizeSlugs(slugs: string[]): string[] {
  return Array.from(new Set(slugs.map((slug) => slug.trim()).filter(Boolean)))
}

function parseWikilinks(value?: string): string[] {
  if (!value) return []
  try {
    const parsed = JSON.parse(value)
    return Array.isArray(parsed)
      ? normalizeSlugs(parsed.filter((item) => typeof item === "string"))
      : []
  } catch {
    return []
  }
}

function linksKey(slugs: string[]): string {
  return normalizeSlugs(slugs).join("\u0001")
}

function requestKey(project: number, pageSlug: string, slugs: string[]): string {
  return `${project}\u0001${pageSlug}\u0001${linksKey(slugs)}`
}

export default function WikiLinkEditor({
  project,
  pageId,
  pageSlug,
  initialWikilinks,
  onWikilinksChange,
}: WikiLinkEditorProps) {
  // ── 状态 ──
  const [linkSlugs, setLinkSlugs] = useState<string[]>([])
  const [targets, setTargets] = useState<WikiLinkTarget[]>([])
  const [backlinks, setBacklinks] = useState<WikiPageBrief[]>([])
  const [showSearch, setShowSearch] = useState(false)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const loadedRequestKeyRef = useRef("")
  const localLinksKeyRef = useRef("")

  // ── 按 slug 索引目标信息 ──
  const targetMap = useMemo(() => {
    const map = new Map<string, WikiLinkTarget>()
    for (const t of targets) {
      map.set(t.slug, t)
    }
    return map
  }, [targets])

  // ── 初始化：解析 wikilinks 并加载目标详情和反向链接 ──
  useEffect(() => {
    let cancelled = false

    const parsed = parseWikilinks(initialWikilinks)
    const nextRequestKey = requestKey(project, pageSlug, parsed)
    const nextLinksKey = linksKey(parsed)

    if (loadedRequestKeyRef.current === nextRequestKey) {
      if (localLinksKeyRef.current !== nextLinksKey) {
        localLinksKeyRef.current = nextLinksKey
        setLinkSlugs(parsed)
      }
      return
    }

    loadedRequestKeyRef.current = nextRequestKey
    localLinksKeyRef.current = nextLinksKey
    setLinkSlugs(parsed)
    setErrorMessage(null)

    // 批量获取目标详情
    if (parsed.length > 0) {
      getWikilinkTargets(project, parsed)
        .then((t) => {
          if (!cancelled) setTargets(t)
        })
        .catch(() => {
          if (!cancelled) setTargets([])
        })
    } else {
      setTargets([])
    }

    // 获取反向链接
    getBacklinks(project, pageSlug)
      .then((bl) => {
        if (!cancelled) setBacklinks(bl)
      })
      .catch(() => {
        if (!cancelled) setBacklinks([])
      })

    return () => {
      cancelled = true
    }
  }, [initialWikilinks, project, pageSlug])

  // ── 移除链接 ──
  const handleRemove = useCallback(
    async (slug: string) => {
      try {
        const updated = await removeWikilink(pageId, slug)
        // 从返回的 WikiPage 解析最新 slugs
        const parsed = parseWikilinks(updated.wikilinks)
        const newSlugs =
          parsed.length > 0 ? parsed : normalizeSlugs(linkSlugs.filter((s) => s !== slug))
        setLinkSlugs(newSlugs)
        localLinksKeyRef.current = linksKey(newSlugs)
        loadedRequestKeyRef.current = requestKey(project, pageSlug, newSlugs)
        // 刷新目标详情
        if (newSlugs.length > 0) {
          const t = await getWikilinkTargets(project, newSlugs)
          setTargets(t)
        } else {
          setTargets([])
        }
        onWikilinksChange?.(newSlugs)
      } catch {
        setErrorMessage("移除链接失败，请稍后重试")
      }
    },
    [pageId, project, pageSlug, linkSlugs, onWikilinksChange],
  )

  // ── 添加链接（WikiLinkSearch 回调） ──
  const handleAdd = useCallback(
    async (slug: string, _title: string) => {
      try {
        const updated = await addWikilink(pageId, slug)
        // 从返回的 WikiPage 解析最新 slugs
        const parsed = parseWikilinks(updated.wikilinks)
        const newSlugs = parsed.length > 0 ? parsed : normalizeSlugs([...linkSlugs, slug])
        setLinkSlugs(newSlugs)
        localLinksKeyRef.current = linksKey(newSlugs)
        loadedRequestKeyRef.current = requestKey(project, pageSlug, newSlugs)
        // 刷新目标详情
        if (newSlugs.length > 0) {
          const t = await getWikilinkTargets(project, newSlugs)
          setTargets(t)
        } else {
          setTargets([])
        }
        setShowSearch(false)
        onWikilinksChange?.(newSlugs)
      } catch {
        setErrorMessage("添加链接失败，请稍后重试")
      }
    },
    [pageId, project, pageSlug, linkSlugs, onWikilinksChange],
  )

  // ── 页面类型中文标签 ──
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
    <div className="rounded-lg border border-neutral-200 bg-white p-4">
      {/* 标题 */}
      <div className="mb-3 flex items-center gap-2">
        <Link2 className="h-4 w-4 text-neutral-500" />
        <h3 className="text-sm font-medium text-neutral-800">本页链接到</h3>
      </div>
      {errorMessage && (
        <div className="mb-3 rounded-md border border-red-100 bg-red-50 px-2 py-1.5 text-xs text-red-600">
          {errorMessage}
        </div>
      )}

      {/* 已链接页面列表 */}
      <div className="mb-3">
        <p className="mb-1.5 text-xs text-neutral-500">目标页面</p>
        {linkSlugs.length === 0 ? (
          <p className="text-xs text-neutral-400">暂无链接</p>
        ) : (
          <ul className="space-y-1">
            {linkSlugs.map((slug) => {
              const target = targetMap.get(slug)
              return (
                <li
                  key={slug}
                  className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-neutral-50"
                >
                  <button
                    type="button"
                    onClick={() => handleRemove(slug)}
                    className="shrink-0 rounded p-0.5 text-neutral-400 hover:bg-red-50 hover:text-red-500 transition-colors"
                    title="移除链接"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                  <span className="flex-1 truncate text-xs text-neutral-800">
                    {target?.title ?? slug}
                  </span>
                  {target && (
                    <span className="shrink-0 rounded bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">
                      {pageTypeLabel(target.page_type)}
                    </span>
                  )}
                </li>
              )
            })}
          </ul>
        )}
      </div>

      {/* 添加链接按钮 */}
      <button
        type="button"
        onClick={() => setShowSearch(true)}
        className="mb-4 flex items-center gap-1.5 rounded-lg border border-dashed border-neutral-300 px-3 py-2 text-xs text-neutral-500 hover:border-[#1A6BD8] hover:text-[#1A6BD8] transition-colors"
      >
        <Plus className="h-3.5 w-3.5" />
        添加目标页面
      </button>

      {/* 反向链接区域 */}
      <div className="border-t border-neutral-100 pt-3">
        <div className="mb-1.5 flex items-center gap-2">
          <ArrowLeft className="h-3.5 w-3.5 text-neutral-400" />
          <p className="text-xs text-neutral-500">
            引用本页的页面
            {backlinks.length > 0 && (
              <span className="ml-1 text-neutral-400">· {backlinks.length} 个页面引用此页</span>
            )}
          </p>
        </div>
        {backlinks.length === 0 ? (
          <p className="text-xs text-neutral-400">暂无反向链接</p>
        ) : (
          <ul className="space-y-0.5">
            {backlinks.map((bl) => (
              <li
                key={bl.id}
                className="flex items-center gap-2 rounded-md px-2 py-1 text-xs text-neutral-600 hover:bg-neutral-50"
              >
                <span className="flex-1 truncate">{bl.title}</span>
                <span className="shrink-0 rounded bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">
                  {pageTypeLabel(bl.page_type)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* 搜索弹窗 */}
      {showSearch && (
        <WikiLinkSearch
          project={project}
          excludeSlug={pageSlug}
          onSelect={handleAdd}
          onClose={() => setShowSearch(false)}
        />
      )}
    </div>
  )
}
