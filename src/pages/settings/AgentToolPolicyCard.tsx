import {
  AlertTriangle,
  Loader2,
  RefreshCw,
  ShieldCheck,
  Wrench,
  Download,
  Eye,
  X,
  Trash2,
} from "lucide-react"
import { useCallback, useEffect, useMemo, useState } from "react"
import type {
  AgentToolAuditRecord,
  AgentToolAuditSummary,
  AgentToolConfig,
  AgentToolOutputContent,
  AgentToolOutputLimits,
  AgentToolProfile,
  SkillPermissionRuleInfo,
} from "@/lib/tauri-commands"
import {
  listAgentToolAudit,
  listAgentToolAuditSummary,
  listAgentToolProfiles,
  getAgentToolConfig,
  setAgentToolConfig,
  readAgentToolOutput,
  listSkillPermissionRules,
  revokeSkillPermissionRule,
} from "@/lib/tauri-commands"

const TOOL_EFFECT_LABELS: Record<AgentToolProfile["effect"], string> = {
  read_only: "只读",
  user_interaction: "用户交互",
  skill_reference: "技能参考",
  skill_environment: "环境准备",
  skill_execution: "脚本执行",
}

const TOOL_RETRY_LABELS: Record<AgentToolProfile["retry"], string> = {
  none: "不重试",
  exponential: "指数退避",
}

const TOOL_ERROR_KIND_LABELS: Record<string, string> = {
  invalid_json: "非法 JSON",
  schema_error: "参数校验",
  tool_error: "工具错误",
  unknown: "未知错误",
}

type AuditFilter = "all" | "ok" | "error" | "truncated" | "empty"
const TOOL_OUTPUT_PREVIEW_BYTES = 512 * 1024
const DEFAULT_AGENT_TOOL_OUTPUT_LIMITS: AgentToolOutputLimits = {
  max_chars: 12000,
  max_bytes: 50 * 1024,
  max_lines: 2000,
}
const AGENT_TOOL_OUTPUT_LIMIT_BOUNDS = {
  max_chars: { min: 1000, max: 200000, label: "字符" },
  max_bytes: { min: 1024, max: 2 * 1024 * 1024, label: "字节" },
  max_lines: { min: 20, max: 20000, label: "行数" },
} satisfies Record<keyof AgentToolOutputLimits, { min: number; max: number; label: string }>

export default function AgentToolPolicyCard() {
  const [profiles, setProfiles] = useState<AgentToolProfile[]>([])
  const [toolConfig, setToolConfig] = useState<AgentToolConfig>({
    disabled_tools: [],
    output_limits: DEFAULT_AGENT_TOOL_OUTPUT_LIMITS,
  })
  const [outputLimitDraft, setOutputLimitDraft] = useState<AgentToolOutputLimits>(
    DEFAULT_AGENT_TOOL_OUTPUT_LIMITS,
  )
  const [auditRecords, setAuditRecords] = useState<AgentToolAuditRecord[]>([])
  const [auditSummary, setAuditSummary] = useState<AgentToolAuditSummary | null>(null)
  const [skillPermissionRules, setSkillPermissionRules] = useState<SkillPermissionRuleInfo[]>([])
  const [auditFilter, setAuditFilter] = useState<AuditFilter>("all")
  const [loading, setLoading] = useState(true)
  const [savingToolId, setSavingToolId] = useState<string | null>(null)
  const [savingOutputLimits, setSavingOutputLimits] = useState(false)
  const [revokingPermissionRule, setRevokingPermissionRule] = useState<string | null>(null)
  const [loadingOutputPath, setLoadingOutputPath] = useState<string | null>(null)
  const [outputPreview, setOutputPreview] = useState<AgentToolOutputContent | null>(null)
  const [error, setError] = useState<string | null>(null)

  const loadProfiles = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [items, config, audits, summary, permissionRules] = await Promise.all([
        listAgentToolProfiles(),
        getAgentToolConfig(),
        listAgentToolAudit(80),
        listAgentToolAuditSummary(200),
        listSkillPermissionRules(),
      ])
      setProfiles(items)
      setToolConfig(config)
      setOutputLimitDraft(config.output_limits)
      setAuditRecords(audits)
      setAuditSummary(summary)
      setSkillPermissionRules(permissionRules)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    loadProfiles()
  }, [loadProfiles])

  const retryCount = profiles.filter((profile) => profile.retry === "exponential").length
  const guardedCount = profiles.filter((profile) => profile.schema_guard).length
  const auditedCount = profiles.filter((profile) => profile.audit).length
  const disabledToolSet = useMemo(
    () => new Set(toolConfig.disabled_tools),
    [toolConfig.disabled_tools],
  )
  const enabledCount = profiles.filter((profile) => !disabledToolSet.has(profile.id)).length
  const filteredAuditRecords = useMemo(() => {
    if (auditFilter === "all") return auditRecords
    if (auditFilter === "truncated") {
      return auditRecords.filter((record) => record.truncated)
    }
    if (auditFilter === "empty") {
      return auditRecords.filter((record) => record.empty_output)
    }
    if (auditFilter === "ok") {
      return auditRecords.filter((record) => record.status === "ok")
    }
    return auditRecords.filter((record) => record.status !== "ok")
  }, [auditFilter, auditRecords])
  const outputLimitsValid = useMemo(
    () =>
      (
        Object.entries(AGENT_TOOL_OUTPUT_LIMIT_BOUNDS) as [
          keyof AgentToolOutputLimits,
          { min: number; max: number; label: string },
        ][]
      ).every(([key, bound]) => {
        const value = outputLimitDraft[key]
        return Number.isInteger(value) && value >= bound.min && value <= bound.max
      }),
    [outputLimitDraft],
  )
  const outputLimitsDirty =
    outputLimitDraft.max_chars !== toolConfig.output_limits.max_chars ||
    outputLimitDraft.max_bytes !== toolConfig.output_limits.max_bytes ||
    outputLimitDraft.max_lines !== toolConfig.output_limits.max_lines

  const handleToggleTool = useCallback(
    async (profile: AgentToolProfile, enabled: boolean) => {
      if (!profile.disable_allowed) return
      setSavingToolId(profile.id)
      setError(null)
      try {
        const disabled = new Set(toolConfig.disabled_tools)
        if (enabled) {
          disabled.delete(profile.id)
        } else {
          disabled.add(profile.id)
        }
        const saved = await setAgentToolConfig({
          disabled_tools: Array.from(disabled),
          output_limits: toolConfig.output_limits,
        })
        setToolConfig(saved)
        if (!outputLimitsDirty) {
          setOutputLimitDraft(saved.output_limits)
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setSavingToolId(null)
      }
    },
    [outputLimitsDirty, toolConfig.disabled_tools, toolConfig.output_limits],
  )

  const handleSaveOutputLimits = useCallback(async () => {
    setSavingOutputLimits(true)
    setError(null)
    try {
      const saved = await setAgentToolConfig({
        disabled_tools: toolConfig.disabled_tools,
        output_limits: outputLimitDraft,
      })
      setToolConfig(saved)
      setOutputLimitDraft(saved.output_limits)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSavingOutputLimits(false)
    }
  }, [outputLimitDraft, toolConfig.disabled_tools])

  const handleReadOutput = useCallback(async (outputPath: string) => {
    setLoadingOutputPath(outputPath)
    setError(null)
    try {
      setOutputPreview(await readAgentToolOutput(outputPath))
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoadingOutputPath(null)
    }
  }, [])

  const handleRevokePermissionRule = useCallback(async (rule: string) => {
    setRevokingPermissionRule(rule)
    setError(null)
    try {
      setSkillPermissionRules(await revokeSkillPermissionRule(rule))
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setRevokingPermissionRule(null)
    }
  }, [])

  return (
    <section className="rounded-xl border border-neutral-200 bg-white">
      <div className="flex items-center justify-between border-b border-neutral-100 px-5 py-3">
        <div>
          <h2 className="text-sm font-semibold text-neutral-700">工具策略</h2>
          <p className="mt-0.5 text-xs text-neutral-400">
            查看 Agent 工具的副作用、重试、参数校验和审计策略。
          </p>
        </div>
        <button
          type="button"
          onClick={loadProfiles}
          disabled={loading}
          className="rounded-lg p-1.5 text-neutral-400 transition-colors hover:bg-neutral-100 hover:text-neutral-600 disabled:opacity-50"
          title="刷新"
        >
          {loading ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="h-4 w-4" />
          )}
        </button>
      </div>

      <div className="space-y-4 p-5">
        <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
          <StatCard
            label="工具数"
            value={profiles.length}
            icon={<Wrench className="h-3.5 w-3.5 text-neutral-400" />}
          />
          <StatCard
            label="已启用"
            value={`${enabledCount}/${profiles.length}`}
            icon={<ShieldCheck className="h-3.5 w-3.5 text-green-500" />}
            isText
          />
          <StatCard
            label="参数校验"
            value={`${guardedCount}/${profiles.length}`}
            icon={<ShieldCheck className="h-3.5 w-3.5 text-green-500" />}
            isText
          />
          <StatCard
            label="可重试"
            value={retryCount}
            icon={<RefreshCw className="h-3.5 w-3.5 text-blue-500" />}
          />
          <StatCard
            label="审计覆盖"
            value={`${auditedCount}/${profiles.length}`}
            icon={<AlertTriangle className="h-3.5 w-3.5 text-amber-500" />}
            isText
          />
        </div>

        {error && (
          <div className="rounded-lg border border-red-100 bg-red-50 px-3 py-2 text-xs text-red-600">
            {error}
          </div>
        )}

        <AgentToolOutputLimitsPanel
          limits={outputLimitDraft}
          dirty={outputLimitsDirty}
          valid={outputLimitsValid}
          saving={savingOutputLimits}
          onChange={setOutputLimitDraft}
          onSave={handleSaveOutputLimits}
        />

        <div className="overflow-hidden rounded-lg border border-neutral-200">
          <table className="w-full text-sm">
            <thead className="bg-neutral-50 text-xs text-neutral-500">
              <tr>
                <th className="px-3 py-2 text-left font-medium">工具</th>
                <th className="px-3 py-2 text-left font-medium">副作用</th>
                <th className="px-3 py-2 text-left font-medium">重试</th>
                <th className="px-3 py-2 text-left font-medium">校验</th>
                <th className="px-3 py-2 text-left font-medium">审计</th>
                <th className="px-3 py-2 text-left font-medium">可用性</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-neutral-100">
              {loading && profiles.length === 0 ? (
                <tr>
                  <td colSpan={6} className="px-3 py-8 text-center text-xs text-neutral-400">
                    <Loader2 className="mx-auto mb-2 h-5 w-5 animate-spin" />
                    加载中
                  </td>
                </tr>
              ) : (
                profiles.map((profile) => (
                  <tr key={profile.id} className="text-neutral-700">
                    <td className="px-3 py-2 font-medium text-neutral-800">{profile.id}</td>
                    <td className="px-3 py-2">
                      <span
                        className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${
                          profile.effect === "read_only"
                            ? "bg-green-50 text-green-700"
                            : "bg-amber-50 text-amber-700"
                        }`}
                      >
                        {TOOL_EFFECT_LABELS[profile.effect]}
                      </span>
                    </td>
                    <td className="px-3 py-2 text-xs text-neutral-500">
                      {TOOL_RETRY_LABELS[profile.retry]}
                    </td>
                    <td className="px-3 py-2 text-xs">
                      <PolicyState enabled={profile.schema_guard} />
                    </td>
                    <td className="px-3 py-2 text-xs">
                      <PolicyState enabled={profile.audit} />
                    </td>
                    <td className="px-3 py-2 text-xs">
                      <ToolAvailabilitySwitch
                        enabled={!disabledToolSet.has(profile.id)}
                        locked={!profile.disable_allowed}
                        saving={savingToolId === profile.id}
                        onChange={(enabled) => handleToggleTool(profile, enabled)}
                      />
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>

        <SkillPermissionRulesPanel
          rules={skillPermissionRules}
          revokingRule={revokingPermissionRule}
          onRevoke={handleRevokePermissionRule}
        />

        <div className="rounded-lg border border-neutral-200">
          <div className="flex flex-wrap items-center justify-between gap-3 border-b border-neutral-100 px-3 py-2">
            <div>
              <h3 className="text-xs font-semibold text-neutral-600">审计摘要</h3>
              <p className="mt-0.5 text-[11px] text-neutral-400">
                后端写入 agent_tool_outputs/tool_calls.jsonl，摘要按最近 200 条计算。
              </p>
            </div>
            {auditSummary && (
              <span className="rounded-full bg-neutral-100 px-2.5 py-1 text-[11px] font-medium text-neutral-500">
                样本 {auditSummary.sampled}
              </span>
            )}
          </div>
          {auditSummary ? (
            <div className="space-y-3 p-3">
              <div className="grid grid-cols-2 gap-2 md:grid-cols-6">
                <AuditMetric label="成功" value={auditSummary.ok} tone="ok" />
                <AuditMetric label="失败" value={auditSummary.error} tone="error" />
                <AuditMetric label="截断" value={auditSummary.truncated} tone="warn" />
                <AuditMetric label="空输出" value={auditSummary.empty_output} />
                <AuditMetric label="平均耗时" value={`${auditSummary.avg_duration_ms}ms`} />
                <AuditMetric label="最大耗时" value={`${auditSummary.max_duration_ms}ms`} />
              </div>

              {(auditSummary.error_kinds.length > 0 || auditSummary.recent_errors.length > 0) && (
                <div className="grid gap-3 md:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]">
                  {auditSummary.error_kinds.length > 0 && (
                    <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-3">
                      <div className="mb-2 text-[11px] font-semibold text-neutral-500">
                        错误类型
                      </div>
                      <div className="flex flex-wrap gap-2">
                        {auditSummary.error_kinds.map((item) => (
                          <span
                            key={item.kind}
                            className="inline-flex items-center gap-1 rounded-full bg-red-50 px-2 py-0.5 text-[11px] font-medium text-red-700"
                            title={item.kind}
                          >
                            {formatToolErrorKind(item.kind)}
                            <span className="text-red-400">{item.count}</span>
                          </span>
                        ))}
                      </div>
                    </div>
                  )}

                  {auditSummary.recent_errors.length > 0 && (
                    <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-3">
                      <div className="mb-2 text-[11px] font-semibold text-neutral-500">
                        最近错误
                      </div>
                      <div className="space-y-2">
                        {auditSummary.recent_errors.map((item) => (
                          <div
                            key={`${item.started_at_ms}-${item.tool}-${item.kind}`}
                            className="min-w-0"
                          >
                            <div className="flex flex-wrap items-center gap-2 text-[11px]">
                              <span className="font-medium text-neutral-700">{item.tool}</span>
                              <span className="rounded-full bg-white px-2 py-0.5 font-medium text-red-600">
                                {formatToolErrorKind(item.kind)}
                              </span>
                              <span className="text-neutral-400">
                                {formatAuditTime(Number(item.started_at_ms))}
                              </span>
                            </div>
                            <p
                              className="mt-1 truncate text-[11px] text-neutral-500"
                              title={item.error}
                            >
                              {item.error || "无错误详情"}
                            </p>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              )}

              {auditSummary.tools.length > 0 && (
                <div className="overflow-hidden rounded-lg border border-neutral-100">
                  <table className="w-full text-xs">
                    <thead className="bg-neutral-50 text-neutral-500">
                      <tr>
                        <th className="px-3 py-2 text-left font-medium">工具</th>
                        <th className="px-3 py-2 text-right font-medium">调用</th>
                        <th className="px-3 py-2 text-right font-medium">失败</th>
                        <th className="px-3 py-2 text-right font-medium">截断</th>
                        <th className="px-3 py-2 text-right font-medium">空输出</th>
                        <th className="px-3 py-2 text-right font-medium">均耗时</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-neutral-100">
                      {auditSummary.tools.map((tool) => (
                        <tr key={tool.tool} className="text-neutral-600">
                          <td className="px-3 py-2 font-medium text-neutral-800">{tool.tool}</td>
                          <td className="px-3 py-2 text-right">{tool.calls}</td>
                          <td
                            className={`px-3 py-2 text-right ${
                              tool.error > 0 ? "font-medium text-red-600" : ""
                            }`}
                          >
                            {tool.error}
                          </td>
                          <td
                            className={`px-3 py-2 text-right ${
                              tool.truncated > 0 ? "font-medium text-amber-600" : ""
                            }`}
                          >
                            {tool.truncated}
                          </td>
                          <td
                            className={`px-3 py-2 text-right ${
                              tool.empty_output > 0 ? "font-medium text-neutral-800" : ""
                            }`}
                          >
                            {tool.empty_output}
                          </td>
                          <td className="px-3 py-2 text-right">{tool.avg_duration_ms}ms</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </div>
          ) : (
            <div className="px-3 py-6 text-center text-xs text-neutral-400">暂无审计摘要</div>
          )}
        </div>

        <div className="rounded-lg border border-neutral-200">
          <div className="flex flex-wrap items-center justify-between gap-3 border-b border-neutral-100 px-3 py-2">
            <h3 className="text-xs font-semibold text-neutral-600">最近工具调用</h3>
            <div className="flex rounded-lg border border-neutral-200 bg-white p-0.5">
              <AuditFilterButton
                active={auditFilter === "all"}
                onClick={() => setAuditFilter("all")}
              >
                全部
              </AuditFilterButton>
              <AuditFilterButton active={auditFilter === "ok"} onClick={() => setAuditFilter("ok")}>
                成功
              </AuditFilterButton>
              <AuditFilterButton
                active={auditFilter === "error"}
                onClick={() => setAuditFilter("error")}
              >
                失败
              </AuditFilterButton>
              <AuditFilterButton
                active={auditFilter === "truncated"}
                onClick={() => setAuditFilter("truncated")}
              >
                截断
              </AuditFilterButton>
              <AuditFilterButton
                active={auditFilter === "empty"}
                onClick={() => setAuditFilter("empty")}
              >
                空输出
              </AuditFilterButton>
            </div>
          </div>
          {auditRecords.length === 0 ? (
            <div className="px-3 py-6 text-center text-xs text-neutral-400">暂无审计记录</div>
          ) : filteredAuditRecords.length === 0 ? (
            <div className="px-3 py-6 text-center text-xs text-neutral-400">当前筛选无记录</div>
          ) : (
            <div className="max-h-96 divide-y divide-neutral-100 overflow-y-auto">
              {filteredAuditRecords.map((record) => (
                <div
                  key={`${record.started_at_ms}-${record.tool}-${record.status}-${record.duration_ms}-${record.args_bytes}`}
                  className="grid gap-2 px-3 py-2 text-xs md:grid-cols-[1fr_auto_auto]"
                >
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-medium text-neutral-800">{record.tool}</span>
                      <span
                        className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${
                          record.status === "ok"
                            ? "bg-green-50 text-green-700"
                            : "bg-red-50 text-red-700"
                        }`}
                      >
                        {record.status === "ok" ? "成功" : "失败"}
                      </span>
                      {record.truncated && (
                        <span className="rounded-full bg-amber-50 px-2 py-0.5 text-[11px] font-medium text-amber-700">
                          已截断
                        </span>
                      )}
                      {record.empty_output && (
                        <span className="rounded-full bg-neutral-100 px-2 py-0.5 text-[11px] font-medium text-neutral-600">
                          空输出
                        </span>
                      )}
                    </div>
                    {record.error && (
                      <p className="mt-1 truncate text-red-500" title={record.error}>
                        {record.error}
                      </p>
                    )}
                    {record.output_path && (
                      <div className="mt-1 flex min-w-0 items-center gap-2">
                        <p className="truncate text-neutral-400" title={record.output_path}>
                          {record.output_path}
                        </p>
                        <button
                          type="button"
                          onClick={() => {
                            if (record.output_path) handleReadOutput(record.output_path)
                          }}
                          disabled={loadingOutputPath === record.output_path}
                          className="inline-flex shrink-0 items-center gap-1 rounded-md border border-neutral-200 px-2 py-0.5 text-[11px] font-medium text-neutral-500 transition-colors hover:bg-neutral-50 disabled:opacity-60"
                          title="查看保存的工具输出"
                        >
                          {loadingOutputPath === record.output_path ? (
                            <Loader2 className="h-3 w-3 animate-spin" />
                          ) : (
                            <Eye className="h-3 w-3" />
                          )}
                          查看
                        </button>
                      </div>
                    )}
                  </div>
                  <div className="text-neutral-500">
                    {record.duration_ms}ms · {record.args_bytes}B
                  </div>
                  <div className="text-neutral-400">{formatAuditTime(record.started_at_ms)}</div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
      {outputPreview && (
        <AgentToolOutputDialog output={outputPreview} onClose={() => setOutputPreview(null)} />
      )}
    </section>
  )
}

function StatCard({
  label,
  value,
  icon,
  isText = false,
}: {
  label: string
  value: number | string
  icon: React.ReactNode
  isText?: boolean
}) {
  return (
    <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-3">
      <div className="mb-1 flex items-center gap-1.5">
        {icon}
        <span className="text-xs text-neutral-500">{label}</span>
      </div>
      <p className={`font-semibold text-neutral-800 ${isText ? "text-xs truncate" : "text-lg"}`}>
        {isText ? value : typeof value === "number" ? value.toLocaleString() : value}
      </p>
    </div>
  )
}

function AuditMetric({
  label,
  value,
  tone = "neutral",
}: {
  label: string
  value: number | string
  tone?: "neutral" | "ok" | "error" | "warn"
}) {
  const toneClass = {
    neutral: "text-neutral-800",
    ok: "text-green-700",
    error: "text-red-700",
    warn: "text-amber-700",
  }[tone]

  return (
    <div className="rounded-lg border border-neutral-100 bg-neutral-50 px-3 py-2">
      <div className="text-[11px] text-neutral-400">{label}</div>
      <div className={`mt-1 text-sm font-semibold ${toneClass}`}>{value}</div>
    </div>
  )
}

function AuditFilterButton({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`rounded-md px-2.5 py-1 text-[11px] font-medium transition-colors ${
        active ? "bg-[#1A6BD8] text-white" : "text-neutral-500 hover:bg-neutral-50"
      }`}
    >
      {children}
    </button>
  )
}

function formatAuditTime(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "-"
  return new Date(value).toLocaleString()
}

function formatToolErrorKind(kind: string) {
  return TOOL_ERROR_KIND_LABELS[kind] ?? kind
}

function PolicyState({ enabled }: { enabled: boolean }) {
  return (
    <span
      className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${
        enabled ? "bg-blue-50 text-blue-700" : "bg-neutral-100 text-neutral-500"
      }`}
    >
      {enabled ? "开启" : "关闭"}
    </span>
  )
}

function ToolAvailabilitySwitch({
  enabled,
  locked,
  saving,
  onChange,
}: {
  enabled: boolean
  locked: boolean
  saving: boolean
  onChange: (enabled: boolean) => void
}) {
  if (locked) {
    return (
      <span className="inline-flex h-6 w-16 items-center justify-center rounded-full bg-neutral-100 text-[11px] font-medium text-neutral-500">
        核心
      </span>
    )
  }

  return (
    <button
      type="button"
      onClick={() => onChange(!enabled)}
      disabled={saving}
      className={`inline-flex h-6 w-16 items-center justify-center gap-1 rounded-full text-[11px] font-medium transition-colors disabled:opacity-60 ${
        enabled ? "bg-green-50 text-green-700" : "bg-neutral-100 text-neutral-500"
      }`}
      title={enabled ? "点击禁用此工具" : "点击启用此工具"}
    >
      {saving ? <Loader2 className="h-3 w-3 animate-spin" /> : enabled ? "启用" : "禁用"}
    </button>
  )
}

function AgentToolOutputDialog({
  output,
  onClose,
}: {
  output: AgentToolOutputContent
  onClose: () => void
}) {
  const [currentOutput, setCurrentOutput] = useState(output)
  const [loadingNext, setLoadingNext] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setCurrentOutput(output)
    setError(null)
  }, [output])

  const loadedEndBytes = currentOutput.offset_bytes + currentOutput.returned_bytes
  const canLoadNext = currentOutput.next_offset_bytes !== null

  const handleLoadNext = useCallback(async () => {
    if (currentOutput.next_offset_bytes === null) return
    setLoadingNext(true)
    setError(null)
    try {
      const nextOutput = await readAgentToolOutput(
        currentOutput.path,
        TOOL_OUTPUT_PREVIEW_BYTES,
        currentOutput.next_offset_bytes,
      )
      setCurrentOutput((prev) => ({
        ...nextOutput,
        offset_bytes: prev.offset_bytes,
        returned_bytes: prev.returned_bytes + nextOutput.returned_bytes,
        content: `${prev.content}${nextOutput.content}`,
      }))
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoadingNext(false)
    }
  }, [currentOutput.next_offset_bytes, currentOutput.path])

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-4">
      <div className="flex max-h-[82vh] w-full max-w-4xl flex-col overflow-hidden rounded-xl bg-white shadow-xl">
        <div className="flex items-start justify-between gap-4 border-b border-neutral-100 px-5 py-3">
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-neutral-800">工具输出</h3>
            <p className="mt-1 truncate text-xs text-neutral-400" title={currentOutput.path}>
              {currentOutput.path}
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1 text-neutral-400 transition-colors hover:bg-neutral-100 hover:text-neutral-600"
            title="关闭"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-neutral-100 px-5 py-2 text-xs text-neutral-500">
          <div className="flex flex-wrap items-center gap-2">
            <span>
              已读取 {formatBytes(currentOutput.offset_bytes)} - {formatBytes(loadedEndBytes)} /{" "}
              {formatBytes(currentOutput.bytes)}
            </span>
            {currentOutput.truncated && (
              <span className="rounded-full bg-amber-50 px-2 py-0.5 font-medium text-amber-700">
                预览已截断
              </span>
            )}
          </div>
          {canLoadNext && (
            <button
              type="button"
              onClick={handleLoadNext}
              disabled={loadingNext}
              className="inline-flex items-center gap-1 rounded-md border border-neutral-200 px-2.5 py-1 text-[11px] font-medium text-neutral-600 transition-colors hover:bg-neutral-50 disabled:opacity-60"
              title="继续读取下一段工具输出"
            >
              {loadingNext ? (
                <Loader2 className="h-3 w-3 animate-spin" />
              ) : (
                <Download className="h-3 w-3" />
              )}
              加载下一段
            </button>
          )}
        </div>
        {error && (
          <div className="border-b border-red-100 bg-red-50 px-5 py-2 text-xs text-red-700">
            {error}
          </div>
        )}
        <pre className="max-h-[60vh] overflow-auto whitespace-pre-wrap break-words bg-neutral-950 p-4 text-xs leading-5 text-neutral-100">
          {currentOutput.content || "（空输出）"}
        </pre>
      </div>
    </div>
  )
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value < 0) return "-"
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / 1024 / 1024).toFixed(1)} MB`
}
