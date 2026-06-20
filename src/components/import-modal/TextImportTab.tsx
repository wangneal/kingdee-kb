/**
 * TextImportTab — 粘贴文本导入 Tab
 *
 * 包含标题输入、内容输入和导入按钮。
 */

import { Loader2, Upload } from "lucide-react"
import type { ImportStatus } from "./types"

export interface TextImportTabProps {
  title: string
  textContent: string
  feedback: { status: ImportStatus; message: string } | null
  onTitleChange: (value: string) => void
  onContentChange: (value: string) => void
  onImport: () => void
}

export default function TextImportTab({
  title,
  textContent,
  feedback,
  onTitleChange,
  onContentChange,
  onImport,
}: TextImportTabProps) {
  const isLoading = feedback?.status === "loading"
  const isDisabled = !title.trim() || !textContent.trim() || isLoading

  return (
    <div className="space-y-3">
      <input
        type="text"
        placeholder="文档标题"
        value={title}
        onChange={(e) => onTitleChange(e.target.value)}
        className="w-full rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
      />
      <textarea
        placeholder="粘贴文本内容..."
        value={textContent}
        onChange={(e) => onContentChange(e.target.value)}
        rows={6}
        className="w-full resize-y rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
      />
      <button
        type="button"
        onClick={onImport}
        disabled={isDisabled}
        className="flex items-center gap-1.5 rounded-md bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-[#1558B0] disabled:cursor-not-allowed disabled:opacity-50"
      >
        {isLoading ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <Upload className="h-4 w-4" />
        )}
        导入
      </button>
    </div>
  )
}
