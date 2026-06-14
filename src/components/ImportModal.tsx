/**
 * ImportModal — 轻量导入弹窗
 *
 * 支持三种导入方式：粘贴文本、选择文件、选择文件夹。
 * 使用 useImport hook 获取导入函数和配置，不暴露知识编译开关和项目选择。
 */

import { open } from "@tauri-apps/plugin-dialog"
import {
  AlertCircle,
  CheckCircle2,
  ClipboardPaste,
  FileText,
  FolderOpen,
  Loader2,
  Upload,
  X,
} from "lucide-react"
import { useCallback, useEffect, useState } from "react"
import { useImport } from "@/hooks/useImport"
import { getImportDialogDefaultPath } from "@/lib/dialog-options"
import type { IngestionResult } from "@/lib/tauri-commands"

type ImportStatus = "idle" | "loading" | "success" | "error"

interface Feedback {
  status: ImportStatus
  message: string
}

/** Tab 类型 */
type TabKey = "text" | "file" | "folder"

const TABS: { key: TabKey; label: string; icon: typeof ClipboardPaste }[] = [
  { key: "text", label: "粘贴文本", icon: ClipboardPaste },
  { key: "file", label: "选择文件", icon: FileText },
  { key: "folder", label: "选择文件夹", icon: FolderOpen },
]

export default function ImportModal({
  open: isOpen,
  onClose,
  onImported,
  project,
}: {
  open: boolean
  onClose: () => void
  onImported?: () => void | Promise<void>
  /** 项目 ID，不传则使用默认项目 */
  project?: number
}) {
  const { kbCompilationEnabled, importFile, importDirectory, importText } = useImport()

  // 当前激活的 Tab
  const [activeTab, setActiveTab] = useState<TabKey>("text")

  // 文本导入相关状态
  const [title, setTitle] = useState("")
  const [textContent, setTextContent] = useState("")

  // 反馈状态
  const [feedback, setFeedback] = useState<Feedback | null>(null)

  const notifyImported = useCallback(() => {
    void onImported?.()
  }, [onImported])

  const buildSuccessMessage = useCallback(
    (result: IngestionResult) => {
      const base = `导入成功：${result.title}，共 ${result.chunk_count} 个片段`
      if (!kbCompilationEnabled) return `${base}。知识编译未开启，Wiki 页面不会自动生成`
      if (result.kb_compilation_error)
        return `${base}。Wiki 编译失败：${result.kb_compilation_error}`
      if (result.kb_analysis_engine) return `${base}。Wiki 页面已生成，等待审核`
      return base
    },
    [kbCompilationEnabled],
  )

  // 打开弹窗时重置状态
  useEffect(() => {
    if (isOpen) {
      setActiveTab("text")
      setTitle("")
      setTextContent("")
      setFeedback(null)
    }
  }, [isOpen])

  // 成功后 2 秒自动关闭
  useEffect(() => {
    if (feedback?.status === "success") {
      const timer = setTimeout(() => {
        onClose()
      }, 2000)
      return () => clearTimeout(timer)
    }
  }, [feedback, onClose])

  // 文本导入
  const handleTextImport = useCallback(async () => {
    const trimmedTitle = title.trim()
    const trimmedContent = textContent.trim()
    if (!trimmedTitle || !trimmedContent) return

    setFeedback({ status: "loading", message: "正在导入文本..." })
    try {
      const result = await importText(trimmedContent, trimmedTitle, project)
      setFeedback({
        status: "success",
        message: buildSuccessMessage(result),
      })
      notifyImported()
      setTitle("")
      setTextContent("")
    } catch (e) {
      setFeedback({
        status: "error",
        message: `导入失败：${e}`,
      })
    }
  }, [title, textContent, importText, project, buildSuccessMessage, notifyImported])

  // 文件导入
  const handleFileImport = useCallback(async () => {
    setFeedback({ status: "loading", message: "正在选择文件..." })
    try {
      const defaultPath = await getImportDialogDefaultPath()
      const selected = await open({
        title: "选择要导入的文件",
        defaultPath,
        multiple: true,
        filters: [
          {
            name: "文档",
            extensions: ["md", "txt", "html", "pdf", "docx", "xlsx", "xls", "vsdx", "vsd"],
          },
        ],
      })
      if (!selected) {
        setFeedback(null)
        return
      }
      const paths = Array.isArray(selected) ? selected : [selected]
      setFeedback({ status: "loading", message: `正在导入 ${paths.length} 个文件...` })

      let successCount = 0
      let failCount = 0
      for (const path of paths) {
        try {
          await importFile(path, project)
          successCount++
        } catch {
          failCount++
        }
      }

      if (successCount > 0) {
        setFeedback({
          status: "success",
          message:
            failCount > 0
              ? `成功导入 ${successCount} 个文件，${failCount} 个失败`
              : kbCompilationEnabled
                ? `成功导入 ${successCount} 个文件，Wiki 页面已生成或等待审核`
                : `成功导入 ${successCount} 个文件。知识编译未开启，Wiki 页面不会自动生成`,
        })
        notifyImported()
      } else {
        setFeedback({
          status: "error",
          message: `导入失败：${failCount} 个文件均未成功`,
        })
      }
    } catch (e) {
      setFeedback({ status: "error", message: `导入失败：${e}` })
    }
  }, [importFile, project, kbCompilationEnabled, notifyImported])

  // 文件夹导入
  const handleFolderImport = useCallback(async () => {
    setFeedback({ status: "loading", message: "正在选择文件夹..." })
    try {
      const defaultPath = await getImportDialogDefaultPath()
      const selected = await open({
        title: "选择要导入的文件夹",
        defaultPath,
        directory: true,
      })
      if (!selected) {
        setFeedback(null)
        return
      }
      setFeedback({ status: "loading", message: `正在导入文件夹：${selected}...` })

      const result = await importDirectory(selected as string, project)
      const { imported, errors } = result
      if (imported.length > 0) {
        setFeedback({
          status: "success",
          message:
            errors.length > 0
              ? `成功导入 ${imported.length} 个文件，${errors.length} 个失败`
              : kbCompilationEnabled
                ? `成功导入 ${imported.length} 个文件，Wiki 页面已生成或等待审核`
                : `成功导入 ${imported.length} 个文件。知识编译未开启，Wiki 页面不会自动生成`,
        })
        notifyImported()
      } else if (errors.length > 0) {
        setFeedback({
          status: "error",
          message: `导入失败：${errors.length} 个文件均未成功`,
        })
      } else {
        setFeedback({ status: "error", message: "未找到支持的文件格式" })
      }
    } catch (e) {
      setFeedback({ status: "error", message: `导入失败：${e}` })
    }
  }, [importDirectory, project, kbCompilationEnabled, notifyImported])

  // 点击遮罩关闭
  const handleBackdropClick = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (e.target === e.currentTarget) onClose()
    },
    [onClose],
  )

  // Escape 键关闭弹窗
  useEffect(() => {
    if (!isOpen) return
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    document.addEventListener("keydown", handleKeyDown)
    return () => document.removeEventListener("keydown", handleKeyDown)
  }, [isOpen, onClose])

  // 未打开时不渲染
  if (!isOpen) return null

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="导入文档"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      onClick={handleBackdropClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          if (e.target === e.currentTarget) onClose()
        }
      }}
    >
      <div className="relative w-full max-w-lg rounded-xl border border-neutral-200 bg-white shadow-xl">
        {/* 标题栏 */}
        <div className="flex items-center justify-between border-b border-neutral-100 px-5 py-3">
          <h2 className="text-sm font-semibold text-neutral-700">导入文档</h2>
          <button
            type="button"
            onClick={onClose}
            className="text-neutral-400 hover:text-neutral-600 transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Tab 栏 */}
        <div className="flex border-b border-neutral-100">
          {TABS.map((tab) => {
            const Icon = tab.icon
            return (
              <button
                key={tab.key}
                type="button"
                onClick={() => {
                  setActiveTab(tab.key)
                  setFeedback(null)
                }}
                className={`flex flex-1 items-center justify-center gap-1.5 px-4 py-2.5 text-xs font-medium transition-colors ${
                  activeTab === tab.key
                    ? "border-b-2 border-[#1A6BD8] text-[#1A6BD8]"
                    : "text-neutral-500 hover:text-neutral-700"
                }`}
              >
                <Icon className="h-3.5 w-3.5" />
                {tab.label}
              </button>
            )
          })}
        </div>

        {/* 内容区 */}
        <div className="p-5">
          {/* 粘贴文本 Tab */}
          {activeTab === "text" && (
            <div className="space-y-3">
              <input
                type="text"
                placeholder="文档标题"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                className="w-full rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              />
              <textarea
                placeholder="粘贴文本内容..."
                value={textContent}
                onChange={(e) => setTextContent(e.target.value)}
                rows={6}
                className="w-full resize-y rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              />
              <button
                type="button"
                onClick={handleTextImport}
                disabled={!title.trim() || !textContent.trim() || feedback?.status === "loading"}
                className="flex items-center gap-1.5 rounded-md bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-[#1558B0] disabled:cursor-not-allowed disabled:opacity-50"
              >
                {feedback?.status === "loading" ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Upload className="h-4 w-4" />
                )}
                导入
              </button>
            </div>
          )}

          {/* 选择文件 Tab */}
          {activeTab === "file" && (
            <div className="space-y-3">
              <p className="text-xs text-neutral-500">
                支持格式：md、txt、html、pdf、docx、xlsx、xls、vsdx、vsd
              </p>
              <button
                type="button"
                onClick={handleFileImport}
                disabled={feedback?.status === "loading"}
                className="flex w-full items-center justify-center gap-2 rounded-lg border-2 border-dashed border-neutral-200 px-4 py-8 text-sm text-neutral-500 transition-colors hover:border-[#1A6BD8] hover:text-[#1A6BD8] disabled:opacity-50"
              >
                {feedback?.status === "loading" ? (
                  <Loader2 className="h-5 w-5 animate-spin" />
                ) : (
                  <FileText className="h-5 w-5" />
                )}
                点击选择文件
              </button>
            </div>
          )}

          {/* 选择文件夹 Tab */}
          {activeTab === "folder" && (
            <div className="space-y-3">
              <p className="text-xs text-neutral-500">导入文件夹内所有支持格式的文档</p>
              <button
                type="button"
                onClick={handleFolderImport}
                disabled={feedback?.status === "loading"}
                className="flex w-full items-center justify-center gap-2 rounded-lg border-2 border-dashed border-neutral-200 px-4 py-8 text-sm text-neutral-500 transition-colors hover:border-[#1A6BD8] hover:text-[#1A6BD8] disabled:opacity-50"
              >
                {feedback?.status === "loading" ? (
                  <Loader2 className="h-5 w-5 animate-spin" />
                ) : (
                  <FolderOpen className="h-5 w-5" />
                )}
                点击选择文件夹
              </button>
            </div>
          )}

          {/* 反馈提示 */}
          {feedback && feedback.status !== "idle" && (
            <div
              className={`mt-4 flex items-center gap-2 rounded-md p-3 text-sm ${
                feedback.status === "success"
                  ? "bg-green-50 text-green-700"
                  : feedback.status === "error"
                    ? "bg-red-50 text-red-700"
                    : "bg-blue-50 text-blue-700"
              }`}
            >
              {feedback.status === "success" ? (
                <CheckCircle2 className="h-4 w-4 shrink-0" />
              ) : feedback.status === "error" ? (
                <AlertCircle className="h-4 w-4 shrink-0" />
              ) : (
                <Loader2 className="h-4 w-4 shrink-0 animate-spin" />
              )}
              <span>{feedback.message}</span>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
