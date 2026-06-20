/**
 * 脑图视图组件
 *
 * 使用 markmap 将大纲树渲染为交互式思维导图，
 * 支持缩放、平移和导出 PNG。
 */

import { AlertCircle, Download, Loader2, Network } from "lucide-react"
import { formatAppError } from "@/lib/app-error"
import type { IPureNode } from "markmap-common"
import { Transformer } from "markmap-lib"
import { Markmap } from "markmap-view"
import { useCallback, useEffect, useRef, useState } from "react"
import { useOutline } from "@/contexts/OutlineContext"
import { exportOutline } from "@/lib/outline-commands"

/** Transformer 实例（模块级复用） */
const transformer = new Transformer()

interface MindmapViewProps {
  /** 当前调研会话 ID */
  sessionId: number
}

export default function MindmapView({ sessionId }: MindmapViewProps) {
  const svgRef = useRef<SVGSVGElement>(null)
  const markmapRef = useRef<Markmap | null>(null)
  const [root, setRoot] = useState<IPureNode | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // 监听大纲数据变化（通过 OutlineContext 的 nodes 数量判断）
  const { nodes } = useOutline()

  /**
   * 加载大纲数据并转换为 markmap 树结构
   */
  const loadMarkdown = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const markdown = await exportOutline(sessionId, "markdown_headings")
      if (!markdown.trim()) {
        setRoot(null)
        return
      }
      const { root: transformedRoot } = transformer.transform(markdown)
      setRoot(transformedRoot)
    } catch (err) {
      const msg = formatAppError(err)
      setError(msg)
      console.error("[MindmapView] 加载大纲失败:", msg)
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  // 会话切换或大纲节点变化时重新加载
  useEffect(() => {
    void nodes.length
    loadMarkdown()
  }, [loadMarkdown, nodes.length])

  /**
   * 渲染 markmap 到 SVG
   */
  useEffect(() => {
    if (!svgRef.current || !root) return

    // 销毁旧实例
    if (markmapRef.current) {
      markmapRef.current.destroy()
      markmapRef.current = null
    }

    // 清空 SVG 内容
    while (svgRef.current.firstChild) {
      svgRef.current.removeChild(svgRef.current.firstChild)
    }

    // 创建新的 markmap 实例
    markmapRef.current = Markmap.create(
      svgRef.current,
      {
        zoom: true,
        pan: true,
        autoFit: true,
        fitRatio: 0.95,
        initialExpandLevel: 3,
        duration: 300,
        spacingHorizontal: 80,
        spacingVertical: 10,
      },
      root,
    )

    // 组件卸载时销毁
    return () => {
      if (markmapRef.current) {
        markmapRef.current.destroy()
        markmapRef.current = null
      }
    }
  }, [root])

  /**
   * 导出当前脑图为 PNG 图片
   */
  const exportPng = useCallback(async () => {
    if (!svgRef.current) return

    const svg = svgRef.current
    const svgData = new XMLSerializer().serializeToString(svg)
    const canvas = document.createElement("canvas")
    const ctx = canvas.getContext("2d")
    if (!ctx) return

    const img = new Image()
    img.onload = () => {
      // 2x 分辨率导出
      canvas.width = img.width * 2
      canvas.height = img.height * 2
      ctx.scale(2, 2)
      ctx.drawImage(img, 0, 0)

      canvas.toBlob((blob) => {
        if (!blob) return
        const url = URL.createObjectURL(blob)
        const link = document.createElement("a")
        link.download = `脑图_${new Date().toISOString().slice(0, 10)}.png`
        link.href = url
        link.click()
        URL.revokeObjectURL(url)
      }, "image/png")
    }
    img.src = `data:image/svg+xml;base64,${btoa(unescape(encodeURIComponent(svgData)))}`
  }, [])

  return (
    <div className="absolute inset-0 flex flex-col overflow-hidden">
      {/* 工具栏 */}
      <div className="flex items-center justify-between border-b border-neutral-200 px-4 py-2">
        <div className="flex items-center gap-2">
          <Network className="h-4 w-4 text-[#1A6BD8]" />
          <span className="text-xs font-semibold text-neutral-700">脑图视图</span>
          {root && <span className="text-[10px] text-neutral-400">{nodes.length} 个节点</span>}
        </div>
        <button
          type="button"
          onClick={exportPng}
          disabled={!root}
          className="flex items-center gap-1.5 rounded-lg border border-neutral-200 px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          <Download className="h-3.5 w-3.5" />
          导出 PNG
        </button>
      </div>

      {/* 脑图内容区 */}
      <div className="relative flex-1 overflow-hidden bg-white">
        {/* 加载状态 */}
        {loading && (
          <div className="absolute inset-0 z-10 flex items-center justify-center bg-white/80">
            <div className="flex items-center gap-2 text-xs text-neutral-500">
              <Loader2 className="h-4 w-4 animate-spin" />
              加载脑图数据...
            </div>
          </div>
        )}

        {/* 错误状态 */}
        {error && (
          <div className="absolute inset-0 z-10 flex items-center justify-center bg-white/80">
            <div className="flex flex-col items-center gap-2 text-center">
              <AlertCircle className="h-8 w-8 text-red-400" />
              <p className="text-xs text-red-500">{error}</p>
              <button
                type="button"
                onClick={loadMarkdown}
                className="rounded-lg border border-neutral-200 px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50 transition-colors"
              >
                重试
              </button>
            </div>
          </div>
        )}

        {/* 空状态 */}
        {!loading && !error && !root && (
          <div className="absolute inset-0 flex items-center justify-center">
            <div className="flex flex-col items-center gap-2 text-center">
              <Network className="h-10 w-10 text-neutral-200" />
              <p className="text-sm text-neutral-400">暂无大纲节点</p>
              <p className="text-xs text-neutral-300">请先在大纲视图中添加节点</p>
            </div>
          </div>
        )}

        {/* SVG 渲染区 */}
        <svg ref={svgRef} className="h-full w-full" style={{ minHeight: "100%" }} />
      </div>
    </div>
  )
}
