/**
 * Markdown 编辑器组件（文档级大纲编辑器）
 *
 * 支持直接编辑整篇调研文档，系统会自动识别其中的标题层级并同步生成大纲。
 * 支持预览模式和防抖自动同步。
 */
import { useCallback, useEffect, useRef, useState } from "react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { useOutline } from "../../contexts/OutlineContext"
import { exportOutline } from "../../lib/outline-commands"
import { useToast } from "../Toast"

interface MarkdownEditorProps {
  /** 会话 ID */
  sessionId: number
  /** 外部文本插入触发器 */
  insertTextTrigger?: { text: string; timestamp: number } | null
}

export default function MarkdownEditor({ sessionId, insertTextTrigger }: MarkdownEditorProps) {
  const { importMarkdown } = useOutline()
  const toast = useToast()
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const textareaRef = useRef<HTMLTextAreaElement | null>(null)

  // 编辑内容状态
  const [content, setContent] = useState("")
  const [loading, setLoading] = useState(false)
  const [showPreview, setShowPreview] = useState(true)
  const isEditingRef = useRef(false)

  // 从后端加载整篇大纲的 Markdown
  const loadDocument = useCallback(async () => {
    setLoading(true)
    try {
      // 导出为层级标题格式作为编辑器内容
      const doc = await exportOutline(sessionId, "markdown_headings")
      setContent(doc)
    } catch (err) {
      console.error("加载大纲文档失败:", err)
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  // 首次加载或切换会话时加载
  useEffect(() => {
    loadDocument()
  }, [loadDocument])

  // 监听外部插入文本的触发
  useEffect(() => {
    if (insertTextTrigger && textareaRef.current) {
      const textarea = textareaRef.current
      const start = textarea.selectionStart
      const end = textarea.selectionEnd
      const text = textarea.value
      const before = text.substring(0, start)
      const after = text.substring(end, text.length)
      const insertedText = insertTextTrigger.text
      const newText = before + insertedText + after

      setContent(newText)
      isEditingRef.current = true

      // 触发防抖保存
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
      saveTimerRef.current = setTimeout(async () => {
        try {
          await importMarkdown(newText)
        } catch (err) {
          console.error("自动同步大纲失败:", err)
        }
      }, 1000)

      // 恢复光标位置
      setTimeout(() => {
        textarea.focus()
        textarea.selectionStart = textarea.selectionEnd = start + insertedText.length
      }, 0)
    }
  }, [insertTextTrigger, importMarkdown])

  // 防抖自动保存并导入大纲
  const debouncedSave = useCallback(
    (value: string) => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
      saveTimerRef.current = setTimeout(async () => {
        try {
          await importMarkdown(value)
        } catch (err) {
          console.error("自动同步大纲失败:", err)
        }
      }, 1000)
    },
    [importMarkdown],
  )

  const handleContentChange = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      const value = e.target.value
      setContent(value)
      isEditingRef.current = true
      debouncedSave(value)
    },
    [debouncedSave],
  )

  // 手动保存
  const handleSave = useCallback(async () => {
    try {
      await importMarkdown(content)
      toast.success("大纲已同步保存")
    } catch (err) {
      toast.error(`保存失败: ${String(err)}`)
    }
  }, [content, importMarkdown, toast])

  // 清除定时器
  useEffect(() => {
    return () => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
    }
  }, [])

  if (loading && !content) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-neutral-400">
        正在加载大纲文档...
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col">
      {/* 工具栏 */}
      <div className="flex items-center justify-between border-b border-neutral-200 px-4 py-2 bg-neutral-50">
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold text-neutral-600">
            直接编辑文档以自动更新大纲标题
          </span>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => setShowPreview((v) => !v)}
            className={`rounded-lg border px-2.5 py-1 text-xs transition-colors ${
              showPreview
                ? "border-[#1A6BD8] text-[#1A6BD8] bg-[#1A6BD8]/5"
                : "border-neutral-200 text-neutral-500 hover:text-neutral-700"
            }`}
          >
            预览
          </button>
          <button
            type="button"
            onClick={handleSave}
            className="rounded-lg bg-[#1A6BD8] px-3 py-1 text-xs font-medium text-white hover:bg-[#1558B0] transition-colors"
          >
            手动同步
          </button>
        </div>
      </div>

      {/* 编辑区 */}
      <div className="flex flex-1 overflow-hidden">
        {/* Markdown 编辑 */}
        <div
          className={`flex-1 overflow-hidden ${showPreview ? "border-r border-neutral-200" : ""}`}
        >
          <textarea
            ref={textareaRef}
            value={content}
            onChange={handleContentChange}
            placeholder="在此直接编辑您的调研文档。使用 '#'、'##'、'###' 来划分标题层级，它们将被自动识别为大纲节点..."
            className="h-full w-full resize-none border-0 bg-white px-6 py-4 text-sm leading-relaxed outline-none font-mono placeholder:text-neutral-300"
            spellCheck={false}
          />
        </div>

        {/* Markdown 预览 */}
        {showPreview && (
          <div className="flex-1 overflow-y-auto bg-white px-6 py-4">
            {content.trim() ? (
              <div className="prose prose-sm max-w-none prose-headings:text-neutral-800 prose-a:text-[#1A6BD8] prose-code:rounded prose-code:bg-neutral-100 prose-code:px-1 prose-pre:bg-neutral-900 prose-pre:text-neutral-100 [&_pre_code]:bg-transparent [&_pre_code]:text-inherit">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
              </div>
            ) : (
              <p className="text-sm text-neutral-300 italic">暂无内容，在此编辑您的第一行文本</p>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
