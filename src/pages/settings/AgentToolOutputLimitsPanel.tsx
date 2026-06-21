import { Loader2 } from "lucide-react"
import type { AgentToolOutputLimits } from "@/lib/tauri-commands"

const AGENT_TOOL_OUTPUT_LIMIT_BOUNDS = {
  max_chars: { min: 1000, max: 200000, label: "字符" },
  max_bytes: { min: 1024, max: 2 * 1024 * 1024, label: "字节" },
  max_lines: { min: 20, max: 20000, label: "行数" },
} satisfies Record<keyof AgentToolOutputLimits, { min: number; max: number; label: string }>

export default function AgentToolOutputLimitsPanel({
  limits,
  dirty,
  valid,
  saving,
  onChange,
  onSave,
}: {
  limits: AgentToolOutputLimits
  dirty: boolean
  valid: boolean
  saving: boolean
  onChange: (limits: AgentToolOutputLimits) => void
  onSave: () => void
}) {
  const updateLimit = (key: keyof AgentToolOutputLimits, value: string) => {
    const parsed = Number.parseInt(value, 10)
    onChange({
      ...limits,
      [key]: Number.isFinite(parsed) ? parsed : 0,
    })
  }

  return (
    <div className="rounded-lg border border-neutral-200">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-neutral-100 px-3 py-2">
        <div>
          <h3 className="text-xs font-semibold text-neutral-600">输出截断限制</h3>
          <p className="mt-0.5 text-[11px] text-neutral-400">
            超出限制时返回预览，并把完整输出保存到审计输出目录。
          </p>
        </div>
        <div className="flex items-center gap-1.5">
          {saving ? (
            <span className="flex items-center gap-1 text-xs text-neutral-400">
              <Loader2 className="h-3.5 w-3.5 animate-spin text-[#1A6BD8]" />
              自动保存中...
            </span>
          ) : !valid ? (
            <span className="text-xs text-red-500">格式错误</span>
          ) : dirty ? (
            <span className="text-xs text-amber-500">有修改未保存</span>
          ) : (
            <span className="flex items-center gap-1 text-xs text-green-600">
              <span className="h-1.5 w-1.5 rounded-full bg-green-500" />
              已自动保存
            </span>
          )}
        </div>
      </div>
      <div className="grid gap-3 p-3 md:grid-cols-3">
        {(
          Object.entries(AGENT_TOOL_OUTPUT_LIMIT_BOUNDS) as [
            keyof AgentToolOutputLimits,
            { min: number; max: number; label: string },
          ][]
        ).map(([key, bound]) => {
          const value = limits[key]
          const invalid = !Number.isInteger(value) || value < bound.min || value > bound.max
          return (
            <label key={key} className="block">
              <span className="mb-1 flex items-center justify-between text-[11px] font-medium text-neutral-500">
                {bound.label}
                <span className={invalid ? "text-red-500" : "text-neutral-400"}>
                  {bound.min.toLocaleString()} - {bound.max.toLocaleString()}
                </span>
              </span>
              <input
                type="number"
                min={bound.min}
                max={bound.max}
                step={key === "max_bytes" ? 1024 : 1}
                value={Number.isFinite(value) ? value : 0}
                onChange={(event) => updateLimit(key, event.target.value)}
                onBlur={() => {
                  if (dirty && valid && !saving) {
                    onSave()
                  }
                }}
                className={`w-full rounded-lg border px-3 py-2 text-sm outline-none focus:ring-1 ${
                  invalid
                    ? "border-red-200 bg-red-50 text-red-700 focus:border-red-400 focus:ring-red-100"
                    : "border-neutral-200 bg-white text-neutral-700 focus:border-[#1A6BD8] focus:ring-[#1A6BD8]/20"
                }`}
              />
            </label>
          )
        })}
      </div>
    </div>
  )
}
