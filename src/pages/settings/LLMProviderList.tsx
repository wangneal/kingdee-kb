import { Brain, Loader2, Pencil, Plus, Scan, ShieldCheck, Star, Trash2 } from "lucide-react"
import { useCallback, useEffect, useState } from "react"
import type { LLMProviderConfig, ProviderPolicyConfig } from "@/lib/skill-types"
import {
  deleteLLMProvider,
  getProviderPolicy,
  listLLMProviders,
  probeAllProviders,
  setDefaultLLMProvider,
  setProviderPolicy,
} from "@/lib/skill-commands"
import ProviderFormDialog from "./ProviderFormDialog"
import { PROVIDER_PRESETS } from "./constants"

const PROTOCOL_LABELS: Record<LLMProviderConfig["protocol"], string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  local: "本地模型",
}

const DEFAULT_PROVIDER_POLICY: ProviderPolicyConfig = {
  default_effect: "allow",
  rules: [],
}

export function isProviderAllowedByPolicy(policy: ProviderPolicyConfig, providerId: string): boolean {
  for (const rule of [...policy.rules].reverse()) {
    if (rule.action !== "provider.use") continue
    if (
      rule.resource === "*" ||
      rule.resource === providerId ||
      rule.resource === `${providerId}:*`
    ) {
      return rule.effect === "allow"
    }
  }
  return policy.default_effect === "allow"
}

function ProviderCard({
  provider,
  policyAllowed,
  onEdit,
  onDelete,
  onSetDefault,
  onTogglePolicy,
}: {
  provider: LLMProviderConfig
  policyAllowed: boolean
  onEdit: () => void
  onDelete: () => void
  onSetDefault: () => void
  onTogglePolicy: () => void
}) {
  const defaultModel = provider.models.find((model) => model.is_default) ?? provider.models[0]
  const defaultKey = provider.api_keys.find((key) => key.is_default) ?? provider.api_keys[0]

  return (
    <div
      className={`rounded-lg border p-4 transition-colors ${
        !policyAllowed
          ? "border-red-200 bg-red-50/30"
          : provider.is_default
            ? "border-[#1A6BD8]/30 bg-blue-50/30"
            : "border-neutral-200 bg-white hover:border-neutral-300"
      }`}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="truncate text-sm font-semibold text-neutral-800">{provider.name}</h3>
            {provider.is_default && (
              <span className="rounded-full bg-[#1A6BD8] px-2 py-0.5 text-[10px] font-medium text-white">
                默认
              </span>
            )}
            <span className="rounded-full bg-neutral-100 px-2 py-0.5 text-[10px] font-medium text-neutral-500">
              {PROTOCOL_LABELS[provider.protocol] ?? provider.protocol}
            </span>
            <span className="rounded-full bg-neutral-100 px-2 py-0.5 text-[10px] font-medium text-neutral-500">
              {provider.models.length} 模型 · {provider.api_keys.length} Key
            </span>
            <span
              className={`rounded-full px-2 py-0.5 text-[10px] font-medium ${
                policyAllowed ? "bg-emerald-100 text-emerald-700" : "bg-red-100 text-red-700"
              }`}
            >
              {policyAllowed ? "允许运行" : "已禁用"}
            </span>
          </div>
          <div className="mt-2 grid gap-1 text-xs text-neutral-500 md:grid-cols-2">
            <span className="truncate">
              默认模型：{defaultModel?.name || "未设置"}
              {defaultModel?.context_window
                ? ` (${(defaultModel.context_window / 1000).toFixed(0)}K ctx)`
                : ""}
            </span>
            <span className="truncate">默认 Key：{defaultKey?.name || "未设置"}</span>
            <span className="truncate md:col-span-2">
              Base URL：{provider.base_url || "未设置"}
            </span>
          </div>
        </div>
        <div className="ml-2 flex shrink-0 items-center gap-1.5">
          {!provider.is_default && policyAllowed && (
            <button
              type="button"
              onClick={onSetDefault}
              className="rounded-lg p-1.5 text-neutral-400 transition-colors hover:bg-amber-50 hover:text-amber-600"
              title="设为默认"
            >
              <Star className="h-4 w-4" />
            </button>
          )}
          <button
            type="button"
            onClick={onTogglePolicy}
            className={`rounded-lg p-1.5 transition-colors ${
              policyAllowed
                ? "text-neutral-400 hover:bg-red-50 hover:text-red-600"
                : "text-red-500 hover:bg-emerald-50 hover:text-emerald-600"
            }`}
            title={policyAllowed ? "禁用运行态使用" : "允许运行态使用"}
          >
            <ShieldCheck className="h-4 w-4" />
          </button>
          <button
            type="button"
            onClick={onEdit}
            className="rounded-lg p-1.5 text-neutral-400 transition-colors hover:bg-blue-50 hover:text-blue-600"
            title="编辑"
          >
            <Pencil className="h-4 w-4" />
          </button>
          <button
            type="button"
            onClick={onDelete}
            className="rounded-lg p-1.5 text-neutral-400 transition-colors hover:bg-red-50 hover:text-red-600"
            title="删除"
          >
            <Trash2 className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  )
}

export default function LLMProviderList() {
  const [providers, setProviders] = useState<LLMProviderConfig[]>([])
  const [providerPolicy, setProviderPolicyState] =
    useState<ProviderPolicyConfig>(DEFAULT_PROVIDER_POLICY)
  const [loading, setLoading] = useState(true)
  const [actionMsg, setActionMsg] = useState<string | null>(null)
  const [dialogProvider, setDialogProvider] = useState<LLMProviderConfig | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [probing, setProbing] = useState(false)

  const loadProviders = useCallback(async () => {
    setLoading(true)
    try {
      const [items, policy] = await Promise.all([listLLMProviders(), getProviderPolicy()])
      setProviders(items)
      setProviderPolicyState(policy)
    } catch (error) {
      setActionMsg(error instanceof Error ? error.message : String(error))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    loadProviders()
  }, [loadProviders])

  const handleProbeAll = async () => {
    setProbing(true)
    setActionMsg("正在检测所有供应商...")
    try {
      const results = await probeAllProviders()
      setProviders((prev) =>
        prev.map((provider) => ({
          ...provider,
          models: provider.models.map((model) => {
            const result = results.find(
              (item) => item.provider_id === provider.id && item.model_id === model.id,
            )
            return result ? { ...model, is_multimodal: result.is_multimodal } : model
          }),
        })),
      )
      setActionMsg("检测完成")
    } catch (error) {
      setActionMsg(error instanceof Error ? error.message : String(error))
    } finally {
      setProbing(false)
    }
  }

  const handleSetDefault = async (id: string) => {
    try {
      await setDefaultLLMProvider(id)
      await loadProviders()
      setActionMsg("已设为默认供应商")
    } catch (error) {
      setActionMsg(error instanceof Error ? error.message : String(error))
    }
  }

  const handleToggleProviderPolicy = async (providerId: string) => {
    const allowed = isProviderAllowedByPolicy(providerPolicy, providerId)
    const nextRules = providerPolicy.rules.filter(
      (rule) => rule.resource !== providerId && rule.resource !== `${providerId}:*`,
    )
    nextRules.push({
      effect: allowed ? "deny" : "allow",
      action: "provider.use",
      resource: providerId,
    })
    try {
      const nextPolicy = await setProviderPolicy({
        ...providerPolicy,
        rules: nextRules,
      })
      setProviderPolicyState(nextPolicy)
      setActionMsg(allowed ? "已禁用该供应商运行态使用" : "已允许该供应商运行态使用")
    } catch (error) {
      setActionMsg(error instanceof Error ? error.message : String(error))
    }
  }

  const handleDelete = async (id: string) => {
    if (!confirm("确定删除此供应商？")) return
    try {
      await deleteLLMProvider(id)
      await loadProviders()
      setActionMsg("已删除")
    } catch (error) {
      setActionMsg(error instanceof Error ? error.message : String(error))
    }
  }

  const handleSaved = async () => {
    setDialogOpen(false)
    setDialogProvider(null)
    await loadProviders()
    setActionMsg("供应商配置已保存")
  }

  return (
    <section className="rounded-xl border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-5 py-3">
        <div className="flex items-center justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold text-neutral-700">LLM 供应商</h2>
            <p className="mt-0.5 text-xs text-neutral-400">
              管理大语言模型供应商配置，内置 {PROVIDER_PRESETS.length} 个供应商模板。
            </p>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleProbeAll}
              disabled={probing || providers.length === 0}
              className="flex items-center gap-1.5 rounded-lg border border-neutral-200 px-3 py-1.5 text-xs font-medium text-neutral-600 transition-colors hover:bg-neutral-50 disabled:opacity-50"
            >
              {probing ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Scan className="h-3.5 w-3.5" />
              )}
              多模态检测
            </button>
            <button
              type="button"
              onClick={() => {
                setDialogProvider(null)
                setDialogOpen(true)
              }}
              className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-[#1558B0]"
            >
              <Plus className="h-3.5 w-3.5" />
              添加供应商
            </button>
          </div>
        </div>
      </div>

      <div className="p-5">
        {actionMsg && (
          <div className="mb-3 rounded-lg bg-blue-50 px-3 py-2 text-xs text-blue-700">
            {actionMsg}
          </div>
        )}

        {loading ? (
          <div className="flex items-center justify-center p-8">
            <Loader2 className="h-5 w-5 animate-spin text-neutral-400" />
          </div>
        ) : providers.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-8 text-neutral-400">
            <Brain className="mb-2 h-8 w-8" />
            <p className="text-sm">暂无供应商配置</p>
            <p className="text-xs">点击「添加供应商」选择模板后填写 API Key 和模型 ID</p>
          </div>
        ) : (
          <div className="space-y-3">
            {providers.map((provider) => (
              <ProviderCard
                key={provider.id}
                provider={provider}
                policyAllowed={isProviderAllowedByPolicy(providerPolicy, provider.id)}
                onEdit={() => {
                  setDialogProvider(provider)
                  setDialogOpen(true)
                }}
                onDelete={() => handleDelete(provider.id)}
                onSetDefault={() => handleSetDefault(provider.id)}
                onTogglePolicy={() => handleToggleProviderPolicy(provider.id)}
              />
            ))}
          </div>
        )}
      </div>

      {dialogOpen && (
        <ProviderFormDialog
          provider={dialogProvider}
          onClose={() => {
            setDialogOpen(false)
            setDialogProvider(null)
          }}
          onSaved={handleSaved}
        />
      )}
    </section>
  )
}
