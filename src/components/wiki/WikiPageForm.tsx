/**
 * Wiki 页面手动创建/编辑表单
 *
 * 用于绕开 KB 编译自动生成路径，让顾问手动记录会议纪要、客户特殊约定等。
 * 复用现有 `createWikiPage` / `updateWikiPage` Tauri 命令。
 */
import { useState } from "react"
import { Loader2, X } from "lucide-react"
import {
  createWikiPage,
  type CreateWikiPage,
  updateWikiPage,
  type WikiPage,
} from "@/lib/tauri-commands"

type Mode = "create" | "edit"

interface WikiPageFormProps {
  mode: Mode
  projectId: number
  /** edit 模式必填；create 模式忽略 */
  initial?: WikiPage
  onSaved: (page: WikiPage) => void
  onCancel: () => void
}

const PAGE_TYPES = ["summary", "reference", "checklist", "process"] as const

function slugify(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .replace(/-{2,}/g, "-")
}

export default function WikiPageForm({
  mode,
  projectId,
  initial,
  onSaved,
  onCancel,
}: WikiPageFormProps) {
  const [title, setTitle] = useState(initial?.title ?? "")
  const [slug, setSlug] = useState(initial?.slug ?? "")
  const [pageType, setPageType] = useState(initial?.page_type ?? "summary")
  const [content, setContent] = useState(initial?.content ?? "")
  const [tagsRaw, setTagsRaw] = useState(() => {
    if (!initial?.tags) return ""
    try {
      const arr = JSON.parse(initial.tags) as string[]
      return arr.join(", ")
    } catch {
      return initial.tags
    }
  })
  const [pageStatus, setPageStatus] = useState(initial?.page_status ?? "draft")
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const isCreate = mode === "create"
  const titleEmpty = title.trim().length === 0
  const slugEmpty = slug.trim().length === 0
  const contentEmpty = isCreate && content.trim().length === 0
  const slugFormatOk = /^[a-z0-9][a-z0-9-]*[a-z0-9]$|^[a-z0-9]$/.test(slug)
  const canSave = !titleEmpty && !slugEmpty && slugFormatOk && !contentEmpty && !saving

  async function handleSubmit() {
    if (!canSave) return
    setSaving(true)
    setError(null)
    try {
      const tagsJson = JSON.stringify(
        tagsRaw
          .split(",")
          .map((t) => t.trim())
          .filter((t) => t.length > 0),
      )
      let saved: WikiPage
      if (isCreate) {
        const data: CreateWikiPage = {
          project_id: projectId,
          slug: slug.trim(),
          title: title.trim(),
          page_type: pageType,
          content,
          tags: tagsJson,
          page_status: pageStatus,
        }
        saved = await createWikiPage(data)
      } else if (initial) {
        saved = await updateWikiPage(initial.id, {
          title: title.trim(),
          content,
          tags: tagsJson,
          page_status: pageStatus,
        })
      } else {
        throw new Error("edit 模式缺少 initial")
      }
      onSaved(saved)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="w-full max-w-2xl rounded-lg bg-white p-5 shadow-xl">
        <div className="mb-4 flex items-center justify-between border-b border-neutral-200 pb-3">
          <h2 className="text-base font-semibold text-neutral-800">
            {isCreate ? "新建 Wiki 页面" : `编辑 Wiki：${initial?.title ?? ""}`}
          </h2>
          <button
            type="button"
            onClick={onCancel}
            className="rounded p-1 text-neutral-500 hover:bg-neutral-100"
            title="关闭"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="space-y-3">
          <div>
            <label className="block text-xs font-medium text-neutral-700">标题 *</label>
            <input
              value={title}
              onChange={(e) => {
                setTitle(e.target.value)
                if (isCreate) setSlug(slugify(e.target.value))
              }}
              placeholder="例如：金蝶云星空 V8.0 客户特殊约定"
              className="mt-1 w-full rounded border border-neutral-300 px-2 py-1.5 text-sm outline-none focus:border-[#1A6BD8]"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-medium text-neutral-700">
                Slug *{" "}
                <span className="text-[10px] font-normal text-neutral-500">
                  (kebab-case)
                </span>
              </label>
              <input
                value={slug}
                onChange={(e) => setSlug(e.target.value)}
                disabled={!isCreate}
                placeholder="v8-customer-special-notes"
                className="mt-1 w-full rounded border border-neutral-300 px-2 py-1.5 text-sm font-mono outline-none focus:border-[#1A6BD8] disabled:bg-neutral-50 disabled:text-neutral-500"
              />
              {slug.length > 0 && !slugFormatOk && (
                <p className="mt-0.5 text-[10px] text-red-600">
                  格式：仅 a-z、0-9、连字符
                </p>
              )}
            </div>
            <div>
              <label className="block text-xs font-medium text-neutral-700">类型</label>
              <select
                value={pageType}
                onChange={(e) => setPageType(e.target.value)}
                disabled={!isCreate}
                className="mt-1 w-full rounded border border-neutral-300 px-2 py-1.5 text-sm outline-none focus:border-[#1A6BD8] disabled:bg-neutral-50"
              >
                {PAGE_TYPES.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div>
            <label className="block text-xs font-medium text-neutral-700">
              标签 <span className="text-[10px] font-normal text-neutral-500">(逗号分隔)</span>
            </label>
            <input
              value={tagsRaw}
              onChange={(e) => setTagsRaw(e.target.value)}
              placeholder="客户, 特殊约定, 财务模块"
              className="mt-1 w-full rounded border border-neutral-300 px-2 py-1.5 text-sm outline-none focus:border-[#1A6BD8]"
            />
          </div>

          <div>
            <label className="block text-xs font-medium text-neutral-700">
              正文 * <span className="text-[10px] font-normal text-neutral-500">(Markdown)</span>
            </label>
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              rows={10}
              placeholder="## 概述&#10;&#10;（必填）&#10;&#10;## 详情&#10;&#10;（详细展开）"
              className="mt-1 w-full rounded border border-neutral-300 px-2 py-1.5 text-sm font-mono outline-none focus:border-[#1A6BD8]"
            />
          </div>

          <div>
            <label className="block text-xs font-medium text-neutral-700">状态</label>
            <select
              value={pageStatus}
              onChange={(e) => setPageStatus(e.target.value)}
              className="mt-1 w-full rounded border border-neutral-300 px-2 py-1.5 text-sm outline-none focus:border-[#1A6BD8]"
            >
              <option value="draft">draft（草稿）</option>
              <option value="published">published（已发布）</option>
              <option value="archived">archived（已归档）</option>
            </select>
          </div>

          {error && (
            <div className="rounded border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
              {error}
            </div>
          )}
        </div>

        <div className="mt-4 flex justify-end gap-2 border-t border-neutral-200 pt-3">
          <button
            type="button"
            onClick={onCancel}
            className="rounded border border-neutral-300 px-3 py-1.5 text-sm text-neutral-700 hover:bg-neutral-50"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void handleSubmit()}
            disabled={!canSave}
            className="flex items-center gap-1.5 rounded bg-[#1A6BD8] px-3 py-1.5 text-sm text-white hover:bg-[#1559B3] disabled:cursor-not-allowed disabled:opacity-40"
          >
            {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {isCreate ? "创建" : "保存"}
          </button>
        </div>
      </div>
    </div>
  )
}
