import { Loader2, Trash2 } from "lucide-react"
import type { SkillPermissionRuleInfo } from "@/lib/tauri-commands"

function formatAuditTime(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "-"
  return new Date(value).toLocaleString()
}

export default function SkillPermissionRulesPanel({
  rules,
  revokingRule,
  onRevoke,
}: {
  rules: SkillPermissionRuleInfo[]
  revokingRule: string | null
  onRevoke: (rule: string) => void
}) {
  return (
    <div className="rounded-lg border border-neutral-200">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-neutral-100 px-3 py-2">
        <div>
          <h3 className="text-xs font-semibold text-neutral-600">脚本授权规则</h3>
          <p className="mt-0.5 text-[11px] text-neutral-400">
            run-skill-script 保存的允许/拒绝规则；撤销后下一次执行会重新请求授权。
          </p>
        </div>
        <span className="rounded-full bg-neutral-100 px-2.5 py-1 text-[11px] font-medium text-neutral-500">
          {rules.length} 条
        </span>
      </div>
      {rules.length === 0 ? (
        <div className="px-3 py-6 text-center text-xs text-neutral-400">暂无已保存授权规则</div>
      ) : (
        <div className="overflow-hidden">
          <table className="w-full text-xs">
            <thead className="bg-neutral-50 text-neutral-500">
              <tr>
                <th className="px-3 py-2 text-left font-medium">规则</th>
                <th className="px-3 py-2 text-left font-medium">效果</th>
                <th className="px-3 py-2 text-left font-medium">保存时间</th>
                <th className="px-3 py-2 text-left font-medium">操作</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-neutral-100">
              {rules.map((rule) => (
                <tr key={rule.rule} className="text-neutral-600">
                  <td className="min-w-0 px-3 py-2">
                    <div className="font-medium text-neutral-800">{rule.skill_name}</div>
                    <div className="mt-0.5 truncate text-neutral-400" title={rule.rule}>
                      {rule.script}
                    </div>
                  </td>
                  <td className="px-3 py-2">
                    <span
                      className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${
                        rule.effect === "allow"
                          ? "bg-green-50 text-green-700"
                          : "bg-red-50 text-red-700"
                      }`}
                    >
                      {rule.effect === "allow" ? "允许" : "拒绝"}
                    </span>
                  </td>
                  <td className="px-3 py-2 text-neutral-400">
                    {formatAuditTime(Number(rule.created_at_ms))}
                  </td>
                  <td className="px-3 py-2">
                    <button
                      type="button"
                      onClick={() => onRevoke(rule.rule)}
                      disabled={revokingRule === rule.rule}
                      className="inline-flex items-center gap-1 rounded-md border border-neutral-200 px-2 py-1 text-[11px] font-medium text-neutral-500 transition-colors hover:bg-neutral-50 disabled:opacity-60"
                      title="撤销此授权规则"
                    >
                      {revokingRule === rule.rule ? (
                        <Loader2 className="h-3 w-3 animate-spin" />
                      ) : (
                        <Trash2 className="h-3 w-3" />
                      )}
                      撤销
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
