import { Loader2, Save, Scan, X, Download } from "lucide-react"
import { useState } from "react"
import {
  addLLMProvider,
  fetchLLMEndpointModels,
  probeModelMultimodal,
  updateLLMProvider,
} from "@/lib/skill-commands"
import type { ApiKeyConfig, LLMProtocol, LLMProviderConfig, ModelConfig } from "@/lib/skill-types"
import { PROVIDER_PRESETS, DEFAULT_PROVIDER_PRESET_ID, PROVIDER_DEFAULTS, providerModelsText } from "./constants"

type ModelSpecEntry = { context_window: number; max_output_tokens: number }

const MODEL_SPECS = [
  { id: "gpt-4o", context_window: 128000, max_output: 16384, thinking: false },
  { id: "gpt-4o-mini", context_window: 128000, max_output: 16384, thinking: false },
  { id: "gpt-4.1", context_window: 1047576, max_output: 32768, thinking: false },
  { id: "gpt-4.1-mini", context_window: 1047576, max_output: 32768, thinking: false },
  { id: "gpt-4.1-nano", context_window: 1047576, max_output: 32768, thinking: false },
  { id: "o3", context_window: 200000, max_output: 100000, thinking: true },
  { id: "o3-mini", context_window: 200000, max_output: 100000, thinking: true },
  { id: "o4-mini", context_window: 200000, max_output: 100000, thinking: true },
  { id: "claude-sonnet-4-20250514", context_window: 200000, max_output: 64000, thinking: true },
  { id: "claude-3.5-sonnet", context_window: 200000, max_output: 8192, thinking: false },
  { id: "deepseek-r1", context_window: 128000, max_output: 8192, thinking: true },
  { id: "deepseek-v3", context_window: 128000, max_output: 8192, thinking: false },
  { id: "qwen-max", context_window: 32768, max_output: 8192, thinking: false },
  { id: "qwen-plus", context_window: 131072, max_output: 8192, thinking: false },
  { id: "glm-4-plus", context_window: 128000, max_output: 4096, thinking: false },
]

function getModelSpecDefault(modelName: string, provider?: LLMProviderConfig | null): ModelSpecEntry {
  const existing = provider?.models.find((m) => m.name === modelName)
  if (existing?.context_window && existing?.max_output_tokens) {
    return {
      context_window: existing.context_window,
      max_output_tokens: existing.max_output_tokens,
    }
  }
  const builtin = MODEL_SPECS.find((s) => s.id === modelName)
  if (builtin)
    return { context_window: builtin.context_window, max_output_tokens: builtin.max_output }
  return { context_window: 128000, max_output_tokens: 8192 }
}

function ProviderTextInput({
  label,
  value,
  onChange,
  type = "text",
  placeholder,
}: {
  label: string
  value: string
  onChange: (value: string) => void
  type?: string
  placeholder?: string
}) {
  return (
    <label className="block">
      <span className="mb-1.5 block text-xs font-medium text-neutral-600">{label}</span>
      <input
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
      />
    </label>
  )
}

export default function ProviderFormDialog({
  provider,
  onClose,
  onSaved,
}: {
  provider: LLMProviderConfig | null
  onClose: () => void
  onSaved: () => void
}) {
  const defaultProtocol = provider?.protocol ?? "openai"
  const defaultPreset =
    PROVIDER_PRESETS.find((preset) => preset.id === DEFAULT_PROVIDER_PRESET_ID) ??
    PROVIDER_PRESETS[0]
  const initialBaseUrl = provider?.base_url ?? defaultPreset.base_url
  const initialModelsText =
    provider?.models.map((model) => model.name).join("\n") ?? providerModelsText(defaultPreset)
  const [selectedPresetId, setSelectedPresetId] = useState(provider ? "current" : defaultPreset.id)
  const [name, setName] = useState(provider?.name ?? defaultPreset.label)
  const [protocol, setProtocol] = useState<LLMProtocol>(defaultProtocol)
  const [baseUrl, setBaseUrl] = useState(initialBaseUrl)
  const [apiKey, setApiKey] = useState("")
  const [modelsText, setModelsText] = useState(initialModelsText)
  const [endpointModels, setEndpointModels] = useState<string[]>([])
  const [fetchingEndpointModels, setFetchingEndpointModels] = useState(false)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)

  const [modelSpecs, setModelSpecs] = useState<Record<string, ModelSpecEntry>>(() => {
    const initial: Record<string, ModelSpecEntry> = {}
    const names = initialModelsText
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter(Boolean)
    for (const n of names) initial[n] = getModelSpecDefault(n, provider)
    return initial
  })

  const handleModelsTextChange = (text: string) => {
    setModelsText(text)
    const names = text
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter(Boolean)
    setModelSpecs((prev) => {
      const next: Record<string, ModelSpecEntry> = {}
      for (const n of names) {
        next[n] = prev[n] ?? getModelSpecDefault(n, provider)
      }
      return next
    })
  }

  const handleProtocolChange = (nextProtocol: LLMProtocol) => {
    setProtocol(nextProtocol)
    if (!provider) {
      setSelectedPresetId(`custom-${nextProtocol}`)
      setBaseUrl(PROVIDER_DEFAULTS[nextProtocol].base_url)
      handleModelsTextChange(PROVIDER_DEFAULTS[nextProtocol].model)
    }
  }

  const handlePresetChange = (presetId: string) => {
    setSelectedPresetId(presetId)
    if (presetId === "current" && provider) {
      setName(provider.name)
      setProtocol(provider.protocol)
      setBaseUrl(provider.base_url)
      handleModelsTextChange(provider.models.map((model) => model.name).join("\n"))
      setEndpointModels([])
      setMessage(null)
      return
    }
    const preset = PROVIDER_PRESETS.find((item) => item.id === presetId)
    if (!preset) return
    setName(preset.label)
    setProtocol(preset.protocol)
    setBaseUrl(preset.base_url)
    handleModelsTextChange(providerModelsText(preset))
    setEndpointModels([])
    setMessage(preset.note ?? null)
  }

  const selectedPreset = PROVIDER_PRESETS.find((preset) => preset.id === selectedPresetId)

  const handleProbeModel = async (modelId: string) => {
    if (!provider) {
      setMessage("请先保存供应商后再检测模型")
      return
    }
    setMessage("正在检测模型...")
    try {
      const isMultimodal = await probeModelMultimodal(provider.id, modelId)
      setMessage(isMultimodal ? "检测通过：支持多模态" : "检测完成：纯文本模型")
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error))
    }
  }

  const handleFetchEndpointModels = async () => {
    setFetchingEndpointModels(true)
    setMessage("正在从端点获取模型列表...")
    try {
      const existingKey =
        provider?.api_keys.find((key) => key.is_default)?.key ?? provider?.api_keys[0]?.key ?? ""
      const result = await fetchLLMEndpointModels({
        protocol,
        baseUrl: baseUrl.trim(),
        apiKey: apiKey.trim() || existingKey,
      })
      setEndpointModels(result.models)
      setMessage(`已获取 ${result.models.length} 个模型${result.cached ? "（来自内存缓存）" : ""}`)
    } catch (error) {
      setEndpointModels([])
      const detail = error instanceof Error ? error.message : String(error)
      setMessage(`获取模型列表失败：${detail}。可继续手动填写模型 ID。`)
    } finally {
      setFetchingEndpointModels(false)
    }
  }

  const modelNamesFromText = () =>
    modelsText
      .split(/\r?\n/)
      .map((item) => item.trim())
      .filter(Boolean)

  const handleUseEndpointModel = (modelName: string) => {
    const next = [modelName, ...modelNamesFromText().filter((name) => name !== modelName)]
    handleModelsTextChange(next.join("\n"))
  }

  const handleUseAllEndpointModels = () => {
    const existing = modelNamesFromText()
    const next = [...existing, ...endpointModels.filter((name) => !existing.includes(name))]
    handleModelsTextChange(next.join("\n"))
  }

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault()
    setSaving(true)
    setMessage(null)

    const modelNames = modelsText
      .split(/\r?\n/)
      .map((item) => item.trim())
      .filter(Boolean)

    if (!name.trim()) {
      setMessage("请输入供应商名称")
      setSaving(false)
      return
    }

    if (modelNames.length === 0) {
      setMessage("请至少填写一个模型名称")
      setSaving(false)
      return
    }

    if (!baseUrl.trim()) {
      setMessage("请填写 Endpoint URL")
      setSaving(false)
      return
    }

    if (protocol === "local" && baseUrl.trim().replace(/\/+$/, "").endsWith("/v1")) {
      setMessage("Local 协议仅支持 Ollama 原生根地址，Endpoint URL 不能以 /v1 结尾")
      setSaving(false)
      return
    }

    const existingKeys = provider?.api_keys ?? []
    const apiKeys: ApiKeyConfig[] = apiKey.trim()
      ? [
          {
            id: existingKeys[0]?.id ?? crypto.randomUUID(),
            name: existingKeys[0]?.name ?? "默认 Key",
            key: apiKey.trim(),
            is_default: true,
          },
        ]
      : existingKeys

    if (protocol !== "local" && apiKeys.length === 0) {
      setMessage("请填写 API Key")
      setSaving(false)
      return
    }

    const models: ModelConfig[] = modelNames.map((modelName, index) => {
      const existing = provider?.models.find((model) => model.name === modelName)
      const spec = modelSpecs[modelName]
      return {
        id: existing?.id ?? crypto.randomUUID(),
        name: modelName,
        is_default: index === 0,
        is_multimodal: existing?.is_multimodal ?? null,
        last_probe_at: existing?.last_probe_at ?? null,
        context_window: spec?.context_window ?? null,
        max_output_tokens: spec?.max_output_tokens ?? null,
        supports_thinking: existing?.supports_thinking ?? null,
      }
    })

    try {
      const payload = {
        id: provider?.id ?? crypto.randomUUID(),
        name: name.trim(),
        protocol,
        baseUrl: baseUrl.trim(),
        apiKeys,
        models,
      }
      if (provider) {
        await updateLLMProvider(payload)
      } else {
        await addLLMProvider(payload)
      }
      await onSaved()
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error))
    } finally {
      setSaving(false)
    }
  }

  const modelRows = provider?.models ?? []

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="flex max-h-[90vh] w-full max-w-lg flex-col rounded-xl bg-white shadow-xl">
        <div className="flex shrink-0 items-center justify-between border-b border-neutral-100 px-5 py-3">
          <h3 className="text-sm font-semibold text-neutral-800">
            {provider ? "编辑供应商" : "添加供应商"}
          </h3>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1 text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
            title="关闭"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="flex-1 space-y-4 overflow-y-auto p-5">
          <label className="block">
            <span className="mb-1.5 block text-xs font-medium text-neutral-600">供应商</span>
            <select
              value={selectedPresetId}
              onChange={(event) => handlePresetChange(event.target.value)}
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            >
              {provider && <option value="current">当前配置</option>}
              {PROVIDER_PRESETS.map((preset) => (
                <option key={preset.id} value={preset.id}>
                  {preset.category} · {preset.label}
                </option>
              ))}
            </select>
          </label>

          <ProviderTextInput
            label="供应商名称"
            value={name}
            onChange={setName}
            placeholder="例如 OpenAI、MiMo、通义千问"
          />

          <label className="block">
            <span className="mb-1.5 block text-xs font-medium text-neutral-600">协议</span>
            <select
              value={protocol}
              onChange={(event) => handleProtocolChange(event.target.value as LLMProtocol)}
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            >
              <option value="openai">OpenAI（Chat Completions）</option>
              <option value="anthropic">Anthropic（Messages）</option>
              <option value="local">本地模型（Ollama 原生协议）</option>
            </select>
          </label>

          <ProviderTextInput
            label="Endpoint URL"
            value={baseUrl}
            onChange={setBaseUrl}
            placeholder="https://api.openai.com/v1"
          />
          {protocol === "local" && (
            <p className="text-xs text-neutral-500">
              Local 使用 Ollama 原生接口。llama.cpp 或其他 OpenAI 兼容服务请选择 OpenAI 协议。
            </p>
          )}
          {protocol !== "local" && (
            <ProviderTextInput
              label={provider ? "API Key（留空则保持原值）" : "API Key"}
              value={apiKey}
              onChange={setApiKey}
              type="password"
              placeholder={
                provider ? "已配置，留空不修改" : (selectedPreset?.api_key_placeholder ?? "sk-...")
              }
            />
          )}

          <label className="block">
            <span className="mb-1.5 flex items-center justify-between gap-3">
              <span className="text-xs font-medium text-neutral-600">
                模型列表（每行一个，第一行为默认模型）
              </span>
              <button
                type="button"
                onClick={handleFetchEndpointModels}
                disabled={fetchingEndpointModels || !baseUrl.trim() || protocol === "local"}
                className="flex shrink-0 items-center gap-1 rounded-md border border-neutral-200 bg-white px-2 py-1 text-xs font-medium text-neutral-600 transition-colors hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-50"
                title={
                  protocol === "local"
                    ? "Local 协议使用 Ollama 原生接口，请用 OpenAI 协议配置兼容端点"
                    : "从端点 /v1/models 获取模型列表"
                }
              >
                {fetchingEndpointModels ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Download className="h-3.5 w-3.5" />
                )}
                获取模型
              </button>
            </span>
            <textarea
              value={modelsText}
              onChange={(event) => handleModelsTextChange(event.target.value)}
              rows={5}
              className="w-full rounded-lg border border-neutral-200 px-3 py-2 font-mono text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />
          </label>

          {/* 模型规格配置 */}
          {Object.keys(modelSpecs).length > 0 && (
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-3">
              <h3 className="mb-2 text-xs font-semibold text-neutral-600">模型规格</h3>
              <p className="mb-2 text-[10px] text-neutral-400">
                设置每个模型的上下文窗口和最大输出 token 数。留空则使用后端自动探测或默认值。
              </p>
              <div className="max-h-40 overflow-y-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-neutral-200 text-left text-neutral-500">
                      <th className="pb-1.5 pr-2 font-medium">模型</th>
                      <th className="pb-1.5 pr-2 font-medium" style={{ width: "100px" }}>
                        上下文窗口
                      </th>
                      <th className="pb-1.5 font-medium" style={{ width: "100px" }}>
                        最大输出
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {Object.entries(modelSpecs).map(([modelName, spec]) => (
                      <tr key={modelName} className="border-b border-neutral-100">
                        <td className="py-1 pr-2 font-mono text-neutral-700">{modelName}</td>
                        <td className="py-1 pr-2">
                          <input
                            type="number"
                            value={spec.context_window}
                            onChange={(e) => {
                              const val = Number(e.target.value) || 128000
                              setModelSpecs((prev) => ({
                                ...prev,
                                [modelName]: { ...prev[modelName], context_window: val },
                              }))
                            }}
                            className="w-full rounded border border-neutral-200 px-1.5 py-0.5 text-xs outline-none focus:border-[#1A6BD8]"
                          />
                        </td>
                        <td className="py-1">
                          <input
                            type="number"
                            value={spec.max_output_tokens}
                            onChange={(e) => {
                              const val = Number(e.target.value) || 8192
                              setModelSpecs((prev) => ({
                                ...prev,
                                [modelName]: { ...prev[modelName], max_output_tokens: val },
                              }))
                            }}
                            className="w-full rounded border border-neutral-200 px-1.5 py-0.5 text-xs outline-none focus:border-[#1A6BD8]"
                          />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {endpointModels.length > 0 && (
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-3">
              <div className="mb-2 flex items-center justify-between gap-3">
                <span className="text-xs font-semibold text-neutral-600">端点模型</span>
                <button
                  type="button"
                  onClick={handleUseAllEndpointModels}
                  className="text-xs font-medium text-[#1A6BD8] hover:text-[#1558B0]"
                >
                  使用全部
                </button>
              </div>
              <div className="flex max-h-28 flex-wrap gap-2 overflow-y-auto">
                {endpointModels.map((modelName) => (
                  <button
                    key={modelName}
                    type="button"
                    onClick={() => handleUseEndpointModel(modelName)}
                    className="rounded-md border border-neutral-200 bg-white px-2.5 py-1.5 font-mono text-xs text-neutral-600 transition-colors hover:border-[#1A6BD8]/40 hover:bg-blue-50 hover:text-[#1A6BD8]"
                  >
                    {modelName}
                  </button>
                ))}
              </div>
            </div>
          )}

          {provider && modelRows.length > 0 && (
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-3">
              <div className="mb-2 text-xs font-semibold text-neutral-600">多模态检测</div>
              <div className="flex flex-wrap gap-2">
                {modelRows.map((model) => (
                  <button
                    key={model.id}
                    type="button"
                    onClick={() => handleProbeModel(model.id)}
                    className="flex items-center gap-1.5 rounded-lg border border-neutral-200 bg-white px-2.5 py-1.5 text-xs text-neutral-600 transition-colors hover:bg-neutral-100"
                  >
                    <Scan className="h-3.5 w-3.5" />
                    {model.name}
                    {model.is_multimodal === true && (
                      <span className="rounded bg-green-100 px-1 py-0.5 text-[9px] text-green-700">
                        多模态
                      </span>
                    )}
                    {model.is_multimodal === false && (
                      <span className="rounded bg-red-100 px-1 py-0.5 text-[9px] text-red-600">
                        纯文本
                      </span>
                    )}
                  </button>
                ))}
              </div>
            </div>
          )}

          {message && (
            <div className="rounded-lg bg-blue-50 px-3 py-2 text-xs text-blue-700">{message}</div>
          )}

          <div className="flex items-center justify-end gap-3 border-t border-neutral-100 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg border border-neutral-200 px-4 py-2 text-sm font-medium text-neutral-600 transition-colors hover:bg-neutral-50"
            >
              取消
            </button>
            <button
              type="submit"
              disabled={saving}
              className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-[#1558B0] disabled:opacity-50"
            >
              {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
              {provider ? "保存修改" : "添加"}
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}
