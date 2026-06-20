import { useEffect, useState } from "react"
import { getExcludedImageTypes, setExcludedImageTypes } from "@/lib/skill-commands"

const IMAGE_CATEGORY_OPTIONS: { value: string; label: string; desc: string }[] = [
  { value: "image", label: "普通图像", desc: "照片/Logo/装饰图" },
  { value: "graph", label: "图表", desc: "流程图/架构图" },
  { value: "table", label: "表格", desc: "表格截图" },
  { value: "text", label: "文字截图", desc: "纯文字图片" },
]

export default function ImageTypeExclusion() {
  const [excluded, setExcluded] = useState<string[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    getExcludedImageTypes()
      .then(setExcluded)
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const toggle = async (type: string) => {
    const next = excluded.includes(type) ? excluded.filter((t) => t !== type) : [...excluded, type]
    setExcluded(next)
    try {
      await setExcludedImageTypes(next)
    } catch (err) {
      setExcluded(excluded)
      console.error("设置图片排除类型失败", err)
    }
  }

  if (loading) return null

  return (
    <div className="rounded-lg border border-neutral-200 bg-neutral-50/50 p-3">
      <p className="mb-2 text-xs font-medium text-neutral-600">图片处理排除类型</p>
      <p className="mb-2.5 text-[11px] text-neutral-400">
        勾选的类型在导入时跳过处理，减少噪声和成本（默认排除装饰图）
      </p>
      <div className="grid grid-cols-2 gap-2">
        {IMAGE_CATEGORY_OPTIONS.map((opt) => (
          <label
            key={opt.value}
            className="flex cursor-pointer items-start gap-2 rounded-md border border-neutral-200 bg-white px-2.5 py-1.5 hover:border-[#1A6BD8]/40"
          >
            <input
              type="checkbox"
              checked={excluded.includes(opt.value)}
              onChange={() => toggle(opt.value)}
              className="mt-0.5 h-3.5 w-3.5 accent-[#1A6BD8]"
            />
            <div className="min-w-0">
              <div className="text-xs font-medium text-neutral-700">{opt.label}</div>
              <div className="text-[10px] text-neutral-400">{opt.desc}</div>
            </div>
          </label>
        ))}
      </div>
    </div>
  )
}
