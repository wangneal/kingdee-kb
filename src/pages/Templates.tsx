import { FileSpreadsheet, FileText, Layers, Loader2 } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import { useNavigate } from "react-router-dom"
import { scanTemplates, type TemplateInfo } from "../lib/tauri-commands"

/** 8 project phases, matching Rust PHASE_NAMES constant */
const PHASES = [
  "项目管理",
  "启动阶段",
  "需求阶段",
  "方案阶段",
  "构建阶段",
  "测试阶段",
  "上线阶段",
  "验收阶段",
]

export default function Templates() {
  const navigate = useNavigate()
  const [templates, setTemplates] = useState<TemplateInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [activePhase, setActivePhase] = useState<string | null>(null)

  useEffect(() => {
    ;(async () => {
      try {
        const data = await scanTemplates()
        setTemplates(data)
        if (data.length > 0) {
          // Default to first phase that has templates
          const firstPhase = PHASES.find((p) => data.some((t) => t.phase === p))
          setActivePhase(firstPhase ?? null)
        }
      } catch (e) {
        console.warn("[Templates] 加载模板失败:", e)
        setError(String(e))
      } finally {
        setLoading(false)
      }
    })()
  }, [])

  const phaseGroups = useMemo(() => {
    const map = new Map<string, TemplateInfo[]>()
    for (const t of templates) {
      const list = map.get(t.phase) ?? []
      list.push(t)
      map.set(t.phase, list)
    }
    // Sort templates within each phase by phase_index
    for (const [, list] of map) {
      list.sort((a, b) => a.phase_index - b.phase_index)
    }
    return map
  }, [templates])

  const filteredTemplates = useMemo(() => {
    if (!activePhase) return templates
    return phaseGroups.get(activePhase) ?? []
  }, [templates, activePhase, phaseGroups])

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-[#1A6BD8]" />
        <span className="ml-2 text-sm text-neutral-500">加载模板…</span>
      </div>
    )
  }

  if (error) {
    return (
      <div className="flex h-full items-center justify-center">
        <p className="text-sm text-red-500">加载失败: {error}</p>
      </div>
    )
  }

  return (
    <div className="flex h-full">
      {/* Left panel — Phase sidebar */}
      <div className="w-56 shrink-0 border-r border-neutral-200 bg-white overflow-auto">
        <div className="sticky top-0 border-b border-neutral-100 bg-white px-4 py-3">
          <h2 className="text-sm font-semibold text-neutral-700">项目阶段</h2>
          <p className="text-xs text-neutral-400 mt-0.5">{templates.length} 个模板</p>
        </div>

        <div className="py-1">
          {PHASES.map((phase) => {
            const count = phaseGroups.get(phase)?.length ?? 0
            return (
              <button
                key={phase}
                type="button"
                onClick={() => setActivePhase(phase)}
                className={`flex w-full items-center gap-2 px-4 py-2 text-left text-sm transition-colors ${
                  activePhase === phase
                    ? "bg-[#1A6BD8]/10 text-[#1A6BD8] font-medium"
                    : "text-neutral-600 hover:bg-neutral-50"
                }`}
              >
                <Layers className="h-3.5 w-3.5 shrink-0" />
                <span className="flex-1 truncate">{phase}</span>
                {count > 0 && <span className="text-xs text-neutral-400">{count}</span>}
              </button>
            )
          })}
        </div>
      </div>

      {/* Right panel — Template grid */}
      <div className="flex-1 overflow-auto bg-neutral-50 p-6">
        {filteredTemplates.length === 0 ? (
          <div className="flex h-full items-center justify-center text-neutral-400">
            <div className="text-center">
              <FileText className="mx-auto h-12 w-12 text-neutral-300" />
              <p className="mt-2 text-sm">暂无模板</p>
            </div>
          </div>
        ) : (
          <>
            <h3 className="mb-4 text-sm font-medium text-neutral-600">
              {activePhase ?? "全部模板"}
              <span className="ml-2 text-xs text-neutral-400">{filteredTemplates.length} 个</span>
            </h3>

            <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">
              {filteredTemplates.map((tpl) => (
                <button
                  key={tpl.id}
                  type="button"
                  onClick={() => navigate(`/wizard/${tpl.id}`)}
                  className="group flex flex-col items-start rounded-lg border border-neutral-200 bg-white p-4 text-left transition-all hover:border-[#1A6BD8]/40 hover:shadow-md"
                >
                  {/* Icon + format badge */}
                  <div className="flex w-full items-center justify-between">
                    {tpl.format === "docx" ? (
                      <FileText className="h-8 w-8 text-[#1A6BD8]" />
                    ) : (
                      <FileSpreadsheet className="h-8 w-8 text-emerald-600" />
                    )}
                    <span
                      className={`rounded px-1.5 py-0.5 text-[10px] font-medium uppercase ${
                        tpl.format === "docx"
                          ? "bg-[#1A6BD8]/10 text-[#1A6BD8]"
                          : "bg-emerald-50 text-emerald-600"
                      }`}
                    >
                      {tpl.format}
                    </span>
                  </div>

                  {/* Template name */}
                  <p className="mt-3 line-clamp-2 text-sm font-medium text-neutral-800 group-hover:text-[#1A6BD8]">
                    {tpl.name}
                  </p>

                  {/* Phase + file size */}
                  <p className="mt-1.5 text-xs text-neutral-400">
                    {tpl.phase} · {(tpl.file_size / 1024).toFixed(0)} KB
                  </p>
                </button>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  )
}
