/**
 * 破坏性操作的二次确认对话框
 *
 * 要求用户**输入目标词（confirmWord）才能确认**——
 * 比 window.confirm 更安全，防止误触。
 *
 * 用途：硬删除项目、批量删除 Wiki 页、清除缓存等不可逆操作。
 */
import { useEffect, useState } from "react"

interface DestructiveConfirmDialogProps {
  /** 触发显示的目标对象（null = 关闭） */
  open: { id: number | string; name: string } | null
  /** 对话框标题（红色高亮） */
  title: string
  /** 警示说明文字（红色） */
  message: string
  /** 提示文字：把 confirmWord 用 <code> 包裹 */
  hint?: string
  /** 确认按钮文案（默认 "永久删除"） */
  confirmLabel?: string
  /** 用户必须输入的目标词（默认 = open.name） */
  confirmWord?: string
  /** 确认回调 */
  onConfirm: (id: number | string) => void
  /** 取消回调 */
  onCancel: () => void
}

export function DestructiveConfirmDialog({
  open,
  title,
  message,
  hint,
  confirmLabel = "永久删除",
  confirmWord,
  onConfirm,
  onCancel,
}: DestructiveConfirmDialogProps) {
  const [input, setInput] = useState("")
  const target = confirmWord ?? open?.name ?? ""
  const canConfirm = open != null && input === target

  // 关闭时清空输入（避免下次打开残留）
  useEffect(() => {
    if (open == null) setInput("")
  }, [open])

  if (open == null) return null

  return (
    <div
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="destructive-confirm-title"
      aria-describedby="destructive-confirm-message"
      className="mt-2 rounded-md border border-red-200 bg-red-50 p-2 text-xs"
    >
      <p id="destructive-confirm-title" className="font-medium text-red-700">
        {title}
      </p>
      <p id="destructive-confirm-message" className="mt-1 text-red-600">
        {message}
      </p>
      {hint && (
        <p className="mt-1 text-neutral-600">
          {hint}
        </p>
      )}
      <input
        value={input}
        onChange={(event) => setInput(event.target.value)}
        className="mt-1 w-full rounded border border-red-200 px-2 py-1 text-xs outline-none focus:border-red-400"
        placeholder={target}
        autoFocus
        aria-label="确认词"
      />
      <div className="mt-2 flex justify-end gap-1">
        <button
          type="button"
          onClick={onCancel}
          className="rounded px-2 py-1 text-neutral-600 hover:bg-neutral-100"
        >
          取消
        </button>
        <button
          type="button"
          onClick={() => onConfirm(open.id)}
          disabled={!canConfirm}
          className="rounded bg-red-600 px-2 py-1 text-white hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-40"
        >
          {confirmLabel}
        </button>
      </div>
    </div>
  )
}
