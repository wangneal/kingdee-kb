import { open } from "@tauri-apps/plugin-dialog"
import {
  AlertTriangle,
  ArrowLeftRight,
  Brain,
  Cpu,
  Database,
  Download,
  Eye,
  EyeOff,
  HardDrive,
  Hash,
  Key,
  Loader2,
  Pencil,
  Plug,
  Plus,
  RefreshCw,
  Save,
  Scan,
  Server,
  Settings as SettingsIcon,
  ShieldCheck,
  Star,
  Trash2,
  Upload,
  Wrench,
  X,
} from "lucide-react"
import { useCallback, useEffect, useMemo, useState } from "react"
import { useSearchParams } from "react-router-dom"
import { useToast } from "@/components/Toast"
import { useAsrConfig } from "@/contexts/AsrConfigContext"
import { useKbCompilation } from "@/contexts/KbCompilationContext"
import { TOAST_AUTO_DISMISS_MS } from "@/lib/constants"
import { getKdclubToken, saveKdclubToken } from "@/lib/kdclub-commands"
import {
  addLLMProvider,
  deleteLLMProvider,
  fetchLLMEndpointModels,
  getExcludedImageTypes,
  getOcrConfig,
  getProviderPolicy,
  listLLMProviders,
  probeAllProviders,
  probeModelMultimodal,
  saveOcrConfig,
  setDefaultLLMProvider,
  setExcludedImageTypes,
  setProviderPolicy,
  updateLLMProvider,
} from "@/lib/skill-commands"
import type {
  ApiKeyConfig,
  LLMProtocol,
  LLMProviderConfig,
  ModelConfig,
  OcrProviderConfig,
  ProviderPolicyConfig,
} from "@/lib/skill-types"
import {
  type AgentToolAuditRecord,
  type AgentToolAuditSummary,
  type AgentToolConfig,
  type AgentToolOutputContent,
  type AgentToolOutputLimits,
  type AgentToolProfile,
  addSensitiveKeyword,
  type EmbeddingModelConfig,
  type EmbeddingProviderConfig,
  type EmbeddingProviderType,
  exportDatabase,
  getAgentToolConfig,
  getEmbeddingModelConfig,
  getModelStatus,
  getStats,
  getTencentMeetingConfigStatus,
  type ImportDbResult,
  importDatabase,
  type KnowledgeStats,
  listAgentToolAudit,
  listAgentToolAuditSummary,
  listAgentToolProfiles,
  listSensitiveKeywords,
  listSkillPermissionRules,
  readAgentToolOutput,
  removeSensitiveKeyword,
  revokeSkillPermissionRule,
  type SensitiveKeyword,
  type SensitiveKind,
  type SkillPermissionRuleInfo,
  saveAsrConfig,
  saveTencentMeetingToken,
  setAgentToolConfig,
  setEmbeddingModelConfig,
  type TencentMeetingConfigStatus,
} from "@/lib/tauri-commands"

type ProviderPreset = {
  id: string
  label: string
  category: string
  protocol: LLMProtocol
  base_url: string
  models: string[]
  api_key_placeholder?: string
  note?: string
}

const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    id: "openai",
    label: "OpenAI",
    category: "国际",
    protocol: "openai",
    base_url: "https://api.openai.com/v1",
    models: ["gpt-4.1", "gpt-4o", "gpt-4o-mini"],
    api_key_placeholder: "sk-...",
  },
  {
    id: "anthropic",
    label: "Anthropic Claude",
    category: "国际",
    protocol: "anthropic",
    base_url: "https://api.anthropic.com/v1",
    models: ["claude-sonnet-4-20250514", "claude-3-5-sonnet-20241022"],
    api_key_placeholder: "sk-ant-...",
  },
  {
    id: "google-gemini",
    label: "Google Gemini",
    category: "国际",
    protocol: "openai",
    base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
    models: ["gemini-2.5-pro", "gemini-2.5-flash"],
  },
  {
    id: "deepseek",
    label: "DeepSeek",
    category: "国内",
    protocol: "openai",
    base_url: "https://api.deepseek.com/v1",
    models: ["deepseek-chat", "deepseek-reasoner"],
  },
  {
    id: "dashscope",
    label: "阿里云百炼 / 通义千问",
    category: "国内",
    protocol: "openai",
    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    models: ["qwen-plus", "qwen-max", "qwen3-235b-a22b"],
  },
  {
    id: "zhipu",
    label: "智谱 AI",
    category: "国内",
    protocol: "openai",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
    models: ["glm-4-plus", "glm-4-air", "glm-4-flash"],
  },
  {
    id: "moonshot",
    label: "Moonshot / Kimi",
    category: "国内",
    protocol: "openai",
    base_url: "https://api.moonshot.cn/v1",
    models: ["moonshot-v1-128k", "moonshot-v1-32k", "kimi-k2-0711-preview"],
  },
  {
    id: "siliconflow",
    label: "硅基流动",
    category: "国内",
    protocol: "openai",
    base_url: "https://api.siliconflow.cn/v1",
    models: ["deepseek-ai/DeepSeek-V3", "deepseek-ai/DeepSeek-R1", "Qwen/Qwen3-235B-A22B"],
  },
  {
    id: "minimax-cn",
    label: "MiniMax（中国）",
    category: "国内",
    protocol: "openai",
    base_url: "https://api.minimax.chat/v1",
    models: ["MiniMax-Text-01", "abab6.5s-chat", "abab6.5g-chat"],
  },
  {
    id: "baichuan",
    label: "百川智能",
    category: "国内",
    protocol: "openai",
    base_url: "https://api.baichuan-ai.com/v1",
    models: ["Baichuan4", "Baichuan3-Turbo"],
  },
  {
    id: "openrouter",
    label: "OpenRouter",
    category: "聚合网关",
    protocol: "openai",
    base_url: "https://openrouter.ai/api/v1",
    models: ["anthropic/claude-sonnet-4", "openai/gpt-4o", "deepseek/deepseek-chat"],
  },
  {
    id: "vercel-ai-gateway",
    label: "Vercel AI Gateway",
    category: "聚合网关",
    protocol: "openai",
    base_url: "https://ai-gateway.vercel.sh/v1",
    models: ["openai/gpt-4o", "anthropic/claude-sonnet-4", "google/gemini-2.5-pro"],
  },
  {
    id: "portkey",
    label: "Portkey",
    category: "聚合网关",
    protocol: "openai",
    base_url: "https://api.portkey.ai/v1",
    models: ["gpt-4o", "@anthropic-prod/claude-sonnet-4-20250514"],
  },
  {
    id: "groq",
    label: "Groq",
    category: "国际",
    protocol: "openai",
    base_url: "https://api.groq.com/openai/v1",
    models: ["llama-3.3-70b-versatile", "deepseek-r1-distill-llama-70b"],
  },
  {
    id: "mistral",
    label: "Mistral AI",
    category: "国际",
    protocol: "openai",
    base_url: "https://api.mistral.ai/v1",
    models: ["mistral-large-latest", "codestral-latest"],
  },
  {
    id: "xai",
    label: "xAI",
    category: "国际",
    protocol: "openai",
    base_url: "https://api.x.ai/v1",
    models: ["grok-3", "grok-3-mini"],
  },
  {
    id: "together",
    label: "Together AI",
    category: "国际",
    protocol: "openai",
    base_url: "https://api.together.xyz/v1",
    models: ["meta-llama/Llama-3.3-70B-Instruct-Turbo", "deepseek-ai/DeepSeek-R1"],
  },
  {
    id: "fireworks",
    label: "Fireworks AI",
    category: "国际",
    protocol: "openai",
    base_url: "https://api.fireworks.ai/inference/v1",
    models: [
      "accounts/fireworks/models/llama-v3p1-405b-instruct",
      "accounts/fireworks/models/deepseek-r1",
    ],
  },
  {
    id: "nvidia",
    label: "NVIDIA NIM",
    category: "国际",
    protocol: "openai",
    base_url: "https://integrate.api.nvidia.com/v1",
    models: ["nvidia/llama-3.1-nemotron-70b-instruct", "deepseek-ai/deepseek-r1"],
  },
  {
    id: "ollama",
    label: "Ollama 本地",
    category: "本地",
    protocol: "local",
    base_url: "http://localhost:11434",
    models: ["qwen2.5:7b", "llama3.1:8b", "deepseek-r1:8b"],
  },
  {
    id: "lm-studio",
    label: "LM Studio / vLLM",
    category: "本地",
    protocol: "openai",
    base_url: "http://localhost:1234/v1",
    models: ["local-model"],
    api_key_placeholder: "任意非空值",
  },
  {
    id: "custom-openai",
    label: "自定义 OpenAI 兼容",
    category: "自定义",
    protocol: "openai",
    base_url: "",
    models: [""],
  },
  {
    id: "custom-anthropic",
    label: "自定义 Anthropic 兼容",
    category: "自定义",
    protocol: "anthropic",
    base_url: "",
    models: [""],
  },
  {
    id: "custom-local",
    label: "自定义 Ollama 原生",
    category: "自定义",
    protocol: "local",
    base_url: "http://localhost:11434",
    models: ["local-model"],
  },
]

const DEFAULT_PROVIDER_PRESET_ID = "openai"

const PROVIDER_DEFAULTS: Record<LLMProtocol, { base_url: string; model: string }> = {
  openai: {
    base_url: "https://api.openai.com/v1",
    model: "gpt-4.1",
  },
  anthropic: {
    base_url: "https://api.anthropic.com/v1",
    model: "claude-sonnet-4-20250514",
  },
  local: {
    base_url: "http://localhost:11434",
    model: "qwen2.5:7b",
  },
}

function providerModelsText(preset: ProviderPreset): string {
  return preset.models.filter(Boolean).join("\n")
}

/** Embedding 供应商定义：标签、默认 Base URL、推荐模型 */
const EMBEDDING_PROVIDERS: Record<
  EmbeddingProviderType,
  { label: string; baseUrl: string; models: string[]; requiresApiKey: boolean }
> = {
  ollama: {
    label: "Ollama (本地)",
    baseUrl: "http://localhost:11434",
    models: ["nomic-embed-text"],
    requiresApiKey: false,
  },
  siliconflow: {
    label: "硅基流动",
    baseUrl: "https://api.siliconflow.cn/v1",
    models: ["BAAI/bge-m3", "BAAI/bge-large-zh-v1.5"],
    requiresApiKey: true,
  },
  openai: {
    label: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    models: ["text-embedding-3-small", "text-embedding-3-large"],
    requiresApiKey: true,
  },
  zhipu: {
    label: "智谱 AI",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    models: ["embedding-3"],
    requiresApiKey: true,
  },
  dashscope: {
    label: "阿里灵积",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    models: ["text-embedding-v3", "text-embedding-v2"],
    requiresApiKey: true,
  },
  cohere: {
    label: "Cohere",
    baseUrl: "https://api.cohere.com/v2",
    models: ["embed-multilingual-v3.0", "embed-english-v3.0"],
    requiresApiKey: true,
  },
  custom: {
    label: "自定义 (OpenAI 兼容)",
    baseUrl: "",
    models: [],
    requiresApiKey: true,
  },
}

const DEFAULT_EMBEDDING_PROVIDER_CONFIG: EmbeddingProviderConfig = {
  provider: "ollama",
  api_key: "",
  base_url: "",
  model_name: "",
}

const EMBEDDING_PROVIDER_STORAGE_KEY = "kingdeekb_embedding_provider_config"

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

/** 敏感词类型的中文标签（用于列表展示） */
function kindLabel(kind: SensitiveKind): string {
  switch (kind) {
    case "name":
      return "人名"
    case "term":
      return "术语"
    case "code":
      return "编号"
    case "custom":
      return "自定义"
  }
}

export default function Settings() {
  const toast = useToast()
  // ASR 配置状态：跨 Settings/ResearchAssistant 共享
  const { status: asrConfigStatus, reload: reloadAsrConfig } = useAsrConfig()
  const [stats, setStats] = useState<KnowledgeStats | null>(null)
  const [loading, setLoading] = useState(true)
  const [modelReady, setModelReady] = useState(false)
  const [embeddingProviderConfig, setEmbeddingProviderConfig] = useState<EmbeddingProviderConfig>(
    DEFAULT_EMBEDDING_PROVIDER_CONFIG,
  )
  const [embeddingProviderSaving, setEmbeddingProviderSaving] = useState(false)
  const [embeddingProviderSaveMsg, setEmbeddingProviderSaveMsg] = useState<string | null>(null)
  const [showEmbeddingApiKey, setShowEmbeddingApiKey] = useState(false)
  const [keywordInput, setKeywordInput] = useState("")
  const [keywordKind, setKeywordKind] = useState<SensitiveKind>("custom")
  const [keywords, setKeywords] = useState<SensitiveKeyword[]>([])
  const [keywordError, setKeywordError] = useState<string | null>(null)

  // ASR 配置状态
  const [tencentSecretId, setTencentSecretId] = useState("")
  const [tencentSecretKey, setTencentSecretKey] = useState("")
  const [asrSaving, setAsrSaving] = useState(false)
  const [asrSaveMsg, setAsrSaveMsg] = useState<string | null>(null)
  const [tencentMeetingStatus, setTencentMeetingStatus] =
    useState<TencentMeetingConfigStatus | null>(null)
  const [tencentMeetingToken, setTencentMeetingToken] = useState("")
  const [showTencentMeetingToken, setShowTencentMeetingToken] = useState(false)
  const [tencentMeetingSaving, setTencentMeetingSaving] = useState(false)
  const [tencentMeetingSaveMsg, setTencentMeetingSaveMsg] = useState<string | null>(null)

  // kdclub API Key 状态
  const [kdclubToken, setKdclubToken] = useState("")
  const [showKdclubToken, setShowKdclubToken] = useState(false)
  const [kdclubSaving, setKdclubSaving] = useState(false)
  const [kdclubSaveMsg, setKdclubSaveMsg] = useState<string | null>(null)
  const [activeTab, setActiveTab] = useState<"ai" | "agent" | "integrations" | "data">("ai")

  // 从 URL 参数读取 section，支持状态栏 deep link
  const [searchParams] = useSearchParams()
  useEffect(() => {
    const section = searchParams.get("section")
    if (section === "llm" || section === "embedding") setActiveTab("ai")
    else if (section === "agent" || section === "tools") setActiveTab("agent")
    else if (section === "integrations") setActiveTab("integrations")
    else if (section === "data" || section === "kb") setActiveTab("data")
  }, [searchParams])

  // 一次性清理已废弃的 localStorage 模型规格覆盖（已迁移到 ModelConfig 字段）
  useEffect(() => {
    try {
      const raw = localStorage.getItem("model_spec_overrides")
      if (raw && JSON.parse(raw) && Object.keys(JSON.parse(raw)).length > 0) {
        toast.info("模型规格配置已迁移：请在供应商编辑中为每个模型设置上下文窗口")
        localStorage.removeItem("model_spec_overrides")
      }
    } catch {
      localStorage.removeItem("model_spec_overrides")
    }
  }, [toast.info])

  // 自动保存 kdclub token：走系统钥匙串，不留 localStorage 明文
  const handleSaveKdclubToken = useCallback(
    async (token?: string) => {
      const val = token !== undefined ? token : kdclubToken
      setKdclubSaving(true)
      setKdclubSaveMsg(null)
      try {
        await saveKdclubToken(val)
        setKdclubSaveMsg("已自动保存")
        setTimeout(() => setKdclubSaveMsg(null), TOAST_AUTO_DISMISS_MS)
      } catch (err) {
        setKdclubSaveMsg(`自动保存失败：${err instanceof Error ? err.message : String(err)}`)
      } finally {
        setKdclubSaving(false)
      }
    },
    [kdclubToken],
  )

  // 自动保存腾讯会议 token
  const handleSaveTencentMeetingToken = useCallback(
    async (token?: string) => {
      const val = token !== undefined ? token : tencentMeetingToken
      if (!val.trim()) return
      setTencentMeetingSaving(true)
      setTencentMeetingSaveMsg(null)
      try {
        await saveTencentMeetingToken(val.trim())
        const status = await getTencentMeetingConfigStatus()
        setTencentMeetingStatus(status)
        setTencentMeetingToken("")
        setTencentMeetingSaveMsg("已自动保存")
        setTimeout(() => setTencentMeetingSaveMsg(null), TOAST_AUTO_DISMISS_MS)
      } catch (err) {
        setTencentMeetingSaveMsg(
          `自动保存失败：${err instanceof Error ? err.message : String(err)}`,
        )
      } finally {
        setTencentMeetingSaving(false)
      }
    },
    [tencentMeetingToken],
  )

  // 自动保存 ASR 配置
  const handleSaveAsrConfig = useCallback(
    async (secretId?: string, secretKey?: string) => {
      const id = secretId !== undefined ? secretId : tencentSecretId
      const key = secretKey !== undefined ? secretKey : tencentSecretKey
      // 只有非空时才保存，避免首次加载未完成时触发清空
      if (!id.trim() && !key.trim()) return
      setAsrSaving(true)
      setAsrSaveMsg(null)
      try {
        await saveAsrConfig({
          tencent_secret_id: id || undefined,
          tencent_secret_key: key || undefined,
        })
        await reloadAsrConfig()
        setAsrSaveMsg("已自动保存")
        setTimeout(() => setAsrSaveMsg(null), TOAST_AUTO_DISMISS_MS)
      } catch (err) {
        setAsrSaveMsg(`自动保存失败：${err instanceof Error ? err.message : String(err)}`)
      } finally {
        setAsrSaving(false)
      }
    },
    [tencentSecretId, tencentSecretKey, reloadAsrConfig],
  )

  // 加载配置和统计信息，并轮询模型状态（自动加载可能仍在异步执行）
  useEffect(() => {
    let cancelled = false

    // 立即加载配置，不等待模型
    Promise.all([
      getStats().catch(() => null),
      getEmbeddingModelConfig().catch(() => ({}) as EmbeddingModelConfig),
    ]).then(([s, embeddingCfg]) => {
      if (cancelled) return
      setStats(s)

      // 从后端配置或 localStorage 加载 Embedding 供应商配置
      // 后端配置优先（确保重启后配置不丢失）
      let loaded = false
      if (embeddingCfg.provider) {
        try {
          const backendConfig: EmbeddingProviderConfig = {
            provider: embeddingCfg.provider as EmbeddingProviderType,
            api_key: embeddingCfg.api_key ?? "",
            base_url: embeddingCfg.base_url ?? "",
            model_name: embeddingCfg.model_name ?? "",
          }
          setEmbeddingProviderConfig({ ...DEFAULT_EMBEDDING_PROVIDER_CONFIG, ...backendConfig })
          loaded = true
        } catch {
          /* 后端配置解析失败，回退到 localStorage */
        }
      }
      if (!loaded) {
        try {
          const stored = localStorage.getItem(EMBEDDING_PROVIDER_STORAGE_KEY)
          if (stored) {
            const parsed = JSON.parse(stored) as EmbeddingProviderConfig
            setEmbeddingProviderConfig({ ...DEFAULT_EMBEDDING_PROVIDER_CONFIG, ...parsed })
          }
        } catch {
          /* 忽略解析错误 */
        }
      }

      // 从后端 keyring 加载 kdclub token（异步：异步回调里不能用 await）
      getKdclubToken()
        .then((kdclubStored) => {
          if (kdclubStored) {
            setKdclubToken(kdclubStored)
          }
        })
        .catch(() => {
          /* 忽略读取错误 */
        })
    })

    // 立即停止加载状态，不等待模型
    setLoading(false)

    // 异步轮询模型状态（不阻塞页面）
    let retries = 0
    const MAX_RETRIES = 30
    const pollModelStatus = async () => {
      if (cancelled) return
      try {
        const status = await getModelStatus()
        if (status) {
          setModelReady(true)
          return
        }
      } catch {
        /* 忽略轮询错误 */
      }
      retries++
      if (retries < MAX_RETRIES && !cancelled) {
        setTimeout(pollModelStatus, 2000)
      }
    }
    pollModelStatus()

    listSensitiveKeywords()
      .then(setKeywords)
      .catch(() => {})
    // ASR config 状态由 AsrConfigContext 统一管理，无需本地副本
    getTencentMeetingConfigStatus()
      .then(setTencentMeetingStatus)
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [])

  const handleRefreshStats = useCallback(async () => {
    try {
      const s = await getStats()
      setStats(s)
    } catch {
      // 忽略刷新失败
    }
  }, [])

  const handleSaveEmbeddingProviderConfig = useCallback(
    async (targetConfig?: EmbeddingProviderConfig) => {
      const configToSave = targetConfig || embeddingProviderConfig
      setEmbeddingProviderSaving(true)
      setEmbeddingProviderSaveMsg(null)
      try {
        // 不将 API Key 持久化到 localStorage，避免安全风险
        const { api_key: _, ...safeConfig } = configToSave
        localStorage.setItem(EMBEDDING_PROVIDER_STORAGE_KEY, JSON.stringify(safeConfig))

        // 同步配置到后端
        await setEmbeddingModelConfig(
          configToSave.provider,
          configToSave.api_key,
          configToSave.base_url,
          configToSave.model_name,
        )
        setModelReady(true)

        setEmbeddingProviderSaveMsg("配置已自动保存")
        setTimeout(() => setEmbeddingProviderSaveMsg(null), TOAST_AUTO_DISMISS_MS)
      } catch (err) {
        setEmbeddingProviderSaveMsg(
          `自动保存失败：${err instanceof Error ? err.message : String(err)}`,
        )
      } finally {
        setEmbeddingProviderSaving(false)
      }
    },
    [embeddingProviderConfig],
  )

  const handleEmbeddingProviderChange = useCallback(
    (provider: EmbeddingProviderType) => {
      const defaults = EMBEDDING_PROVIDERS[provider]
      setEmbeddingProviderConfig((prev) => {
        const next = {
          ...prev,
          provider,
          // 仅当 base_url 仍为上一供应商默认值时自动填充
          base_url:
            prev.base_url === EMBEDDING_PROVIDERS[prev.provider]?.baseUrl || prev.base_url === ""
              ? defaults.baseUrl
              : prev.base_url,
          // 仅当 model_name 属于上一供应商预设模型时自动填充
          model_name:
            EMBEDDING_PROVIDERS[prev.provider]?.models.includes(prev.model_name) ||
            prev.model_name === ""
              ? (defaults.models[0] ?? "")
              : prev.model_name,
        }
        handleSaveEmbeddingProviderConfig(next)
        return next
      })
    },
    [handleSaveEmbeddingProviderConfig],
  )

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-neutral-400" />
      </div>
    )
  }

  return (
    <div className="mx-auto w-full max-w-2xl lg:max-w-4xl xl:max-w-5xl 2xl:max-w-6xl p-6">
      <div className="mb-6 flex items-center gap-2">
        <SettingsIcon className="h-5 w-5 text-[#1A6BD8]" />
        <h1 className="text-lg font-semibold text-neutral-800">设置</h1>
      </div>

      {/* 标签导航 */}
      <div className="mb-6 flex gap-1 rounded-lg bg-neutral-100 p-1">
        {[
          { key: "ai" as const, label: "AI 模型", icon: Brain },
          { key: "agent" as const, label: "Agent 工具", icon: ShieldCheck },
          { key: "integrations" as const, label: "集成服务", icon: Plug },
          { key: "data" as const, label: "数据管理", icon: Database },
        ].map((tab) => (
          <button
            key={tab.key}
            type="button"
            onClick={() => setActiveTab(tab.key)}
            className={`flex flex-1 items-center justify-center gap-1.5 rounded-md px-3 py-2 text-sm font-medium transition-colors ${
              activeTab === tab.key
                ? "bg-white text-[#1A6BD8] shadow-sm"
                : "text-neutral-500 hover:text-neutral-700"
            }`}
          >
            <tab.icon className="h-4 w-4" />
            {tab.label}
          </button>
        ))}
      </div>

      {/* AI 模型页签 */}
      {activeTab === "ai" && (
        <div className="space-y-6">
          {/* LLM 供应商列表 */}
          <LLMProviderList />

          {/* 知识编译配置 */}
          <KnowledgeCompilationCard />

          {/* OCR 配置 */}
          <OcrConfigCard />

          {/* Embedding 模型卡片 */}
          <section className="rounded-xl border border-neutral-200 bg-white">
            <div className="border-b border-neutral-100 px-5 py-3">
              <div className="flex items-center justify-between">
                <div>
                  <h2 className="text-sm font-semibold text-neutral-700">Embedding 模型</h2>
                  <p className="mt-0.5 text-xs text-neutral-400">
                    向量嵌入模型，支持 Ollama 本地部署或在线 API 服务
                  </p>
                </div>
                <span
                  className={`flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium ${
                    modelReady ? "bg-green-50 text-green-700" : "bg-amber-50 text-amber-700"
                  }`}
                >
                  <span
                    className={`h-1.5 w-1.5 rounded-full ${
                      modelReady ? "bg-green-500" : "bg-amber-500"
                    }`}
                  />
                  {modelReady ? "已配置" : "未配置"}
                </span>
              </div>
            </div>

            <div className="p-5">
              {/* 供应商选择器 */}
              <div className="mb-4">
                <div className="mb-1.5 flex items-center gap-2">
                  <ArrowLeftRight className="h-4 w-4 text-neutral-400" />
                  <span className="text-sm font-medium text-neutral-700">模式</span>
                </div>
                <select
                  value={embeddingProviderConfig.provider}
                  onChange={(e) =>
                    handleEmbeddingProviderChange(e.target.value as EmbeddingProviderType)
                  }
                  className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
                >
                  {(Object.keys(EMBEDDING_PROVIDERS) as EmbeddingProviderType[]).map((key) => (
                    <option key={key} value={key}>
                      {EMBEDDING_PROVIDERS[key].label}
                    </option>
                  ))}
                </select>
              </div>

              {/* 供应商配置界面 */}
              <p className="mb-3 text-sm text-neutral-500">
                使用 {EMBEDDING_PROVIDERS[embeddingProviderConfig.provider].label}
                Embedding 服务。
                {embeddingProviderConfig.provider === "ollama"
                  ? " 请确保 Ollama 已启动并拉取 embedding 模型。"
                  : " 请填写 API Key 和模型配置后保存。"}
              </p>

              <div className="space-y-3">
                {/* Base URL 配置 */}
                <div>
                  <div className="mb-1.5 flex items-center gap-2">
                    <Server className="h-4 w-4 text-neutral-400" />
                    <span className="text-sm font-medium text-neutral-700">API Base URL</span>
                  </div>
                  <input
                    type="text"
                    value={embeddingProviderConfig.base_url}
                    onChange={(e) =>
                      setEmbeddingProviderConfig((c) => ({ ...c, base_url: e.target.value }))
                    }
                    onBlur={() => handleSaveEmbeddingProviderConfig()}
                    placeholder={EMBEDDING_PROVIDERS[embeddingProviderConfig.provider].baseUrl}
                    className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
                  />
                </div>

                {/* API Key 配置（仅需要 API Key 的供应商显示） */}
                {EMBEDDING_PROVIDERS[embeddingProviderConfig.provider].requiresApiKey && (
                  <div>
                    <div className="mb-1.5 flex items-center gap-2">
                      <Key className="h-4 w-4 text-neutral-400" />
                      <span className="text-sm font-medium text-neutral-700">API Key</span>
                    </div>
                    <div className="relative flex items-center">
                      <input
                        type={showEmbeddingApiKey ? "text" : "password"}
                        value={embeddingProviderConfig.api_key}
                        onChange={(e) =>
                          setEmbeddingProviderConfig((c) => ({ ...c, api_key: e.target.value }))
                        }
                        onBlur={() => handleSaveEmbeddingProviderConfig()}
                        placeholder="输入 API Key..."
                        className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 pr-10 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
                      />
                      <button
                        type="button"
                        onClick={() => setShowEmbeddingApiKey((v) => !v)}
                        className="absolute right-2 text-neutral-400 hover:text-neutral-600 transition-colors"
                        tabIndex={-1}
                        aria-label={showEmbeddingApiKey ? "隐藏 API Key" : "显示 API Key"}
                      >
                        {showEmbeddingApiKey ? (
                          <EyeOff className="h-4 w-4" />
                        ) : (
                          <Eye className="h-4 w-4" />
                        )}
                      </button>
                    </div>
                  </div>
                )}

                {/* 模型名称 */}
                <div>
                  <div className="mb-1.5 flex items-center gap-2">
                    <Cpu className="h-4 w-4 text-neutral-400" />
                    <span className="text-sm font-medium text-neutral-700">模型名称</span>
                  </div>
                  <input
                    type="text"
                    list={`embedding-models-${embeddingProviderConfig.provider}`}
                    value={embeddingProviderConfig.model_name}
                    onChange={(e) =>
                      setEmbeddingProviderConfig((c) => ({ ...c, model_name: e.target.value }))
                    }
                    onBlur={() => handleSaveEmbeddingProviderConfig()}
                    placeholder="选择或输入模型名称"
                    className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
                  />
                  <datalist id={`embedding-models-${embeddingProviderConfig.provider}`}>
                    {EMBEDDING_PROVIDERS[embeddingProviderConfig.provider].models.map((m) => (
                      <option key={m} value={m} />
                    ))}
                  </datalist>
                </div>

                {/* 模型预设按钮 */}
                {EMBEDDING_PROVIDERS[embeddingProviderConfig.provider].models.length > 0 && (
                  <div className="flex flex-wrap gap-2">
                    {EMBEDDING_PROVIDERS[embeddingProviderConfig.provider].models.map((m) => (
                      <button
                        key={m}
                        type="button"
                        onClick={() => {
                          const nextConfig = { ...embeddingProviderConfig, model_name: m }
                          setEmbeddingProviderConfig(nextConfig)
                          handleSaveEmbeddingProviderConfig(nextConfig)
                        }}
                        className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
                          embeddingProviderConfig.model_name === m
                            ? "bg-[#1A6BD8] text-white"
                            : "bg-neutral-100 text-neutral-600 hover:bg-neutral-200"
                        }`}
                      >
                        {m}
                      </button>
                    ))}
                  </div>
                )}
              </div>

              {/* 自动保存状态提示 */}
              <div className="mt-4 flex items-center gap-3">
                {embeddingProviderSaving ? (
                  <span className="flex items-center gap-1 text-xs text-neutral-400">
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    自动保存中...
                  </span>
                ) : embeddingProviderSaveMsg ? (
                  <span className="text-xs text-green-600">{embeddingProviderSaveMsg}</span>
                ) : null}
              </div>
            </div>
          </section>
        </div>
      )}

      {/* Agent 工具页签 */}
      {activeTab === "agent" && (
        <div className="space-y-6">
          <AgentToolPolicyCard />
        </div>
      )}

      {/* 集成服务页签 */}
      {activeTab === "integrations" && (
        <div className="space-y-6">
          {/* kdclub API Key 卡片 */}
          <section className="rounded-xl border border-neutral-200 bg-white">
            <div className="border-b border-neutral-100 px-5 py-3">
              <h2 className="text-sm font-semibold text-neutral-700">金蝶云社区 API</h2>
              <p className="mt-0.5 text-xs text-neutral-400">
                配置金蝶云社区 PAT Token，用于产品智能问答功能
              </p>
            </div>

            <div className="space-y-4 p-5">
              <div>
                <div className="mb-1.5 flex items-center gap-2">
                  <Key className="h-4 w-4 text-neutral-400" />
                  <span className="text-sm font-medium text-neutral-700">PAT Token</span>
                </div>
                <div className="relative flex items-center">
                  <input
                    type={showKdclubToken ? "text" : "password"}
                    value={kdclubToken}
                    onChange={(e) => setKdclubToken(e.target.value)}
                    onBlur={() => handleSaveKdclubToken()}
                    placeholder="kdt_xxxxxxxx..."
                    className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 pr-10 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
                  />
                  <button
                    type="button"
                    onClick={() => setShowKdclubToken((v) => !v)}
                    className="absolute right-2 text-neutral-400 hover:text-neutral-600 transition-colors"
                    tabIndex={-1}
                    aria-label={showKdclubToken ? "隐藏 Token" : "显示 Token"}
                  >
                    {showKdclubToken ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                  </button>
                </div>
                <p className="mt-1.5 text-[10px] text-neutral-400">
                  在金蝶云社区 → 个人设置 → 访问令牌 获取。格式如{" "}
                  <code className="bg-neutral-100 px-1 rounded">kdt_xxxxxxxx...</code>
                </p>
              </div>

              {/* 自动保存状态提示 */}
              <div className="flex items-center gap-3">
                {kdclubSaving ? (
                  <span className="flex items-center gap-1 text-xs text-neutral-400">
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    自动保存中...
                  </span>
                ) : kdclubSaveMsg ? (
                  <span className="text-xs text-green-600">{kdclubSaveMsg}</span>
                ) : null}
              </div>
            </div>
          </section>

          {/* ASR 配置卡片 */}
          <section className="rounded-xl border border-neutral-200 bg-white">
            <div className="border-b border-neutral-100 px-5 py-3">
              <h2 className="text-sm font-semibold text-neutral-700">语音识别服务配置</h2>
              <p className="mt-0.5 text-xs text-neutral-400">
                配置在线语音识别服务（腾讯/讯飞），用于替代本地 Whisper 模型
              </p>
            </div>
            <div className="p-5 space-y-4">
              {/* 腾讯 ASR */}
              <div className="rounded-lg border border-neutral-200 p-4">
                <div className="flex items-center justify-between mb-2">
                  <h3 className="text-xs font-semibold text-neutral-700">
                    腾讯云语音识别
                    {asrConfigStatus?.tencent_configured && (
                      <span className="ml-2 text-green-600">✓ 已配置</span>
                    )}
                  </h3>
                  {asrSaving ? (
                    <span className="flex items-center gap-1 text-[11px] text-neutral-400">
                      <Loader2 className="h-3 w-3 animate-spin text-[#1A6BD8]" />
                      自动保存中...
                    </span>
                  ) : asrSaveMsg ? (
                    <span className="text-[11px] text-green-600">{asrSaveMsg}</span>
                  ) : null}
                </div>
                <div className="grid grid-cols-3 gap-2">
                  <input
                    type="text"
                    placeholder="SecretId"
                    value={tencentSecretId}
                    onChange={(e) => setTencentSecretId(e.target.value)}
                    onBlur={() => handleSaveAsrConfig()}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                  <input
                    type="password"
                    placeholder="SecretKey"
                    value={tencentSecretKey}
                    onChange={(e) => setTencentSecretKey(e.target.value)}
                    onBlur={() => handleSaveAsrConfig()}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                </div>
                <p className="text-[10px] text-neutral-400 mt-1">
                  在腾讯云控制台 → API密钥管理 获取 SecretId/SecretKey（AppId 无需填写）
                </p>
              </div>

              <div className="rounded-lg border border-neutral-200 p-4">
                <div className="flex items-center justify-between mb-2">
                  <h3 className="text-xs font-semibold text-neutral-700">
                    腾讯会议 MCP
                    {tencentMeetingStatus?.configured && (
                      <span className="ml-2 text-green-600">✓ 已配置</span>
                    )}
                  </h3>
                  {tencentMeetingSaving ? (
                    <span className="flex items-center gap-1 text-[11px] text-neutral-400">
                      <Loader2 className="h-3 w-3 animate-spin text-[#1A6BD8]" />
                      自动保存中...
                    </span>
                  ) : tencentMeetingSaveMsg ? (
                    <span className="text-[11px] text-green-600">{tencentMeetingSaveMsg}</span>
                  ) : null}
                </div>
                <div className="flex gap-2">
                  <input
                    type={showTencentMeetingToken ? "text" : "password"}
                    placeholder={
                      tencentMeetingStatus?.configured ? "已配置，留空则保持原值" : "Token"
                    }
                    value={tencentMeetingToken}
                    onChange={(event) => setTencentMeetingToken(event.target.value)}
                    onBlur={() => handleSaveTencentMeetingToken()}
                    className="min-w-0 flex-1 rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                  <button
                    type="button"
                    onClick={() => setShowTencentMeetingToken((value) => !value)}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs text-neutral-500 hover:bg-neutral-50"
                  >
                    {showTencentMeetingToken ? "隐藏" : "显示"}
                  </button>
                </div>
                <p className="text-[10px] text-neutral-400 mt-1">
                  在腾讯会议 AI Skill 页面获取 Token，用于预约/查询/取消会议、同步转写、获取 AI
                  智能纪要。详见会议管理页。
                </p>
              </div>
            </div>
          </section>
        </div>
      )}

      {/* 数据管理页签 */}
      {activeTab === "data" && (
        <div className="space-y-6">
          {/* 存储统计卡片 */}
          <section className="rounded-xl border border-neutral-200 bg-white">
            <div className="flex items-center justify-between border-b border-neutral-100 px-5 py-3">
              <div>
                <h2 className="text-sm font-semibold text-neutral-700">存储统计</h2>
                <p className="mt-0.5 text-xs text-neutral-400">知识库当前数据概览</p>
              </div>
              <button
                type="button"
                onClick={handleRefreshStats}
                className="rounded-lg p-1.5 text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600 transition-colors"
                title="刷新"
              >
                <RefreshCw className="h-4 w-4" />
              </button>
            </div>

            <div className="p-5">
              {stats ? (
                <div className="grid grid-cols-3 gap-4">
                  <StatCard
                    label="文档数"
                    value={stats.document_count}
                    icon={<HardDrive className="h-4 w-4 text-[#1A6BD8]" />}
                  />
                  <StatCard
                    label="分块数"
                    value={stats.chunk_count}
                    icon={<Hash className="h-4 w-4 text-[#1A6BD8]" />}
                  />
                  <StatCard
                    label="数据库"
                    value={stats.db_path.split(/[/\\]/).pop() || "—"}
                    icon={<Server className="h-4 w-4 text-[#1A6BD8]" />}
                    isText
                  />
                </div>
              ) : (
                <p className="text-sm text-neutral-400">无法加载统计数据</p>
              )}
            </div>
          </section>

          {/* 脱敏配置卡片 */}
          <section className="rounded-xl border border-neutral-200 bg-white">
            <div className="border-b border-neutral-100 px-5 py-3">
              <h2 className="text-sm font-semibold text-neutral-700">数据脱敏配置</h2>
              <p className="mt-0.5 text-xs text-neutral-400">管理敏感词库，发送给 LLM 前自动过滤</p>
            </div>
            <div className="p-5">
              <p className="mb-3 text-xs text-neutral-500">
                当前内置规则：身份证号、手机号、邮箱、金额、银行卡号。
                可添加自定义敏感词（如企业高管姓名、内部项目代号）。
              </p>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={keywordInput}
                  onChange={(e) => setKeywordInput(e.target.value)}
                  placeholder="输入敏感词..."
                  className="flex-1 rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-amber-500"
                />
                <select
                  value={keywordKind}
                  onChange={(e) => setKeywordKind(e.target.value as SensitiveKind)}
                  className="rounded-lg border border-neutral-200 px-2 py-2 text-sm outline-none focus:border-amber-500 bg-white"
                  title="敏感词类型（决定占位符标签，影响 LLM 推理质量）"
                >
                  <option value="name">人名</option>
                  <option value="term">术语/代号</option>
                  <option value="code">编号</option>
                  <option value="custom">自定义</option>
                </select>
                <button
                  type="button"
                  onClick={async () => {
                    if (!keywordInput.trim()) return
                    try {
                      await addSensitiveKeyword(keywordInput.trim(), keywordKind)
                      setKeywordInput("")
                      const kw = await listSensitiveKeywords()
                      setKeywords(kw)
                    } catch (e) {
                      setKeywordError(String(e))
                      setTimeout(() => setKeywordError(null), 5000)
                    }
                  }}
                  className="flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700"
                >
                  <Plus className="h-3.5 w-3.5" /> 添加
                </button>
              </div>
              {keywords.length > 0 && (
                <div className="mt-3 flex flex-wrap gap-2">
                  {keywords.map((kw) => (
                    <span
                      key={kw.text}
                      className="inline-flex items-center gap-1 rounded-full bg-amber-50 px-2.5 py-1 text-xs text-amber-700"
                    >
                      {kw.text}
                      <span className="rounded bg-amber-200/60 px-1 text-[10px] text-amber-800">
                        {kindLabel(kw.kind)}
                      </span>
                      <button
                        type="button"
                        onClick={async () => {
                          try {
                            await removeSensitiveKeyword(kw.text)
                            setKeywords(await listSensitiveKeywords())
                          } catch (e) {
                            setKeywordError(String(e))
                            setTimeout(() => setKeywordError(null), 5000)
                          }
                        }}
                        className="text-amber-400 hover:text-red-500"
                      >
                        &times;
                      </button>
                    </span>
                  ))}
                </div>
              )}
              {keywordError && <p className="text-xs text-red-600 mt-1">{keywordError}</p>}
            </div>
          </section>

          {/* 数据库备份卡片 */}
          <DatabaseBackupCard />
        </div>
      )}
    </div>
  )
}

// ── 辅助组件 ─────────────────────────────────────────────────────

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

function AgentToolPolicyCard() {
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

function AgentToolOutputLimitsPanel({
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

function SkillPermissionRulesPanel({
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

function DatabaseBackupCard() {
  const [exporting, setExporting] = useState(false)
  const [importing, setImporting] = useState(false)
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null)
  const [importResult, setImportResult] = useState<ImportDbResult | null>(null)

  const handleExport = async () => {
    setExporting(true)
    setMsg(null)
    try {
      const targetPath = await open({
        directory: true,
        multiple: false,
        title: "选择导出目录",
      })
      if (!targetPath) {
        return
      }
      const filePath = `${targetPath}/risk_control_backup.db`
      await exportDatabase(filePath)
      setMsg({ ok: true, text: `已导出到 ${filePath}` })
    } catch (err) {
      setMsg({ ok: false, text: `导出失败：${err instanceof Error ? err.message : String(err)}` })
    } finally {
      setExporting(false)
    }
  }

  const handleImport = async () => {
    setImporting(true)
    setMsg(null)
    setImportResult(null)
    try {
      const filePath = await open({
        multiple: false,
        filters: [{ name: "SQLite 数据库", extensions: ["db"] }],
        title: "选择备份文件",
      })
      if (!filePath) {
        return
      }
      const result = await importDatabase(filePath as string)
      setImportResult(result)
      setMsg({
        ok: true,
        text: `导入成功：${result.document_count} 条范围，${result.chunk_count} 条指标`,
      })
    } catch (err) {
      setMsg({ ok: false, text: `导入失败：${err instanceof Error ? err.message : String(err)}` })
    } finally {
      setImporting(false)
    }
  }

  return (
    <section className="mt-6 rounded-xl border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-5 py-3">
        <h2 className="text-sm font-semibold text-neutral-700">整库备份</h2>
        <p className="mt-0.5 text-xs text-neutral-400">导出/导入风控数据库（项目、范围、指标）</p>
      </div>
      <div className="p-5">
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={handleExport}
            disabled={exporting}
            className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
          >
            {exporting ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Download className="h-4 w-4" />
            )}
            导出备份
          </button>
          <button
            type="button"
            onClick={handleImport}
            disabled={importing}
            className="flex items-center gap-1.5 rounded-lg border border-neutral-200 px-4 py-2 text-sm font-medium text-neutral-600 hover:bg-neutral-50 disabled:opacity-50 transition-colors"
          >
            {importing ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Upload className="h-4 w-4" />
            )}
            导入备份
          </button>
          {msg && (
            <span className={`text-xs ${msg.ok ? "text-green-600" : "text-red-600"}`}>
              {msg.text}
            </span>
          )}
        </div>
        {importResult && (
          <div className="mt-3 grid grid-cols-2 gap-3">
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-2 text-center">
              <p className="text-lg font-semibold text-neutral-800">
                {importResult.document_count}
              </p>
              <p className="text-xs text-neutral-500">范围条目</p>
            </div>
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-2 text-center">
              <p className="text-lg font-semibold text-neutral-800">{importResult.chunk_count}</p>
              <p className="text-xs text-neutral-500">健康指标</p>
            </div>
          </div>
        )}
      </div>
    </section>
  )
}

// ── LLM 供应商列表 ──────────────────────────────────────────────────

const PROTOCOL_LABELS: Record<LLMProtocol, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  local: "本地模型",
}

const DEFAULT_PROVIDER_POLICY: ProviderPolicyConfig = {
  default_effect: "allow",
  rules: [],
}

function isProviderAllowedByPolicy(policy: ProviderPolicyConfig, providerId: string): boolean {
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

function LLMProviderList() {
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

function ProviderFormDialog({
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

  // ── 模型规格配置（context_window / max_output_tokens）──
  type ModelSpecEntry = { context_window: number; max_output_tokens: number }
  const getModelSpecDefault = (modelName: string): ModelSpecEntry => {
    // 1. 已有配置优先
    const existing = provider?.models.find((m) => m.name === modelName)
    if (existing?.context_window && existing?.max_output_tokens) {
      return {
        context_window: existing.context_window,
        max_output_tokens: existing.max_output_tokens,
      }
    }
    // 2. 内置规格
    const builtin = MODEL_SPECS.find((s) => s.id === modelName)
    if (builtin)
      return { context_window: builtin.context_window, max_output_tokens: builtin.max_output }
    // 3. 默认值
    return { context_window: 128000, max_output_tokens: 8192 }
  }
  const [modelSpecs, setModelSpecs] = useState<Record<string, ModelSpecEntry>>(() => {
    const initial: Record<string, ModelSpecEntry> = {}
    const names = initialModelsText
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter(Boolean)
    for (const n of names) initial[n] = getModelSpecDefault(n)
    return initial
  })

  // textarea 变化时同步 modelSpecs
  const handleModelsTextChange = (text: string) => {
    setModelsText(text)
    const names = text
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter(Boolean)
    setModelSpecs((prev) => {
      const next: Record<string, ModelSpecEntry> = {}
      for (const n of names) {
        next[n] = prev[n] ?? getModelSpecDefault(n)
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

function KnowledgeCompilationCard() {
  // 知识编译开关由全局 KbCompilationContext 管理：Import.tsx 与 Settings.tsx 共享，
  // 任一页面切换后另一页面立即同步，消除原先的跨页不同步。
  const { enabled, loading, saving, setEnabled } = useKbCompilation()
  const [message, setMessage] = useState<string | null>(null)

  const handleToggle = useCallback(
    async (next: boolean) => {
      setMessage(null)
      try {
        await setEnabled(next)
        setMessage(next ? "已开启知识编译" : "已关闭知识编译")
        setTimeout(() => setMessage(null), TOAST_AUTO_DISMISS_MS)
      } catch (error) {
        setMessage(`保存失败：${error instanceof Error ? error.message : String(error)}`)
      }
    },
    [setEnabled],
  )

  return (
    <section className="rounded-xl border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-5 py-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <h2 className="text-sm font-semibold text-neutral-700">知识编译</h2>
            <p className="mt-0.5 text-xs text-neutral-400">
              导入后尝试使用 LLM 生成 Wiki 候选页面；LLM 不可用或 30
              秒超时时，会降级为快速分析模式（非 LLM）。
            </p>
          </div>
          <span
            className={`flex shrink-0 items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium ${
              enabled ? "bg-green-50 text-green-700" : "bg-neutral-100 text-neutral-500"
            }`}
          >
            <span
              className={`h-1.5 w-1.5 rounded-full ${enabled ? "bg-green-500" : "bg-neutral-400"}`}
            />
            {enabled ? "已开启" : "已关闭"}
          </span>
        </div>
      </div>
      <div className="flex items-center justify-between gap-4 p-5">
        <div className="flex items-center gap-2 text-xs text-neutral-500">
          <Cpu className="h-4 w-4 text-neutral-400" />
          <span>{enabled ? "优先 LLM，失败时自动降级" : "仅执行普通入库，不生成 Wiki 候选"}</span>
        </div>
        <label className="flex items-center gap-2 text-xs text-neutral-600">
          <input
            type="checkbox"
            checked={enabled}
            disabled={loading || saving}
            onChange={(event) => handleToggle(event.target.checked)}
            className="h-4 w-4 rounded border-neutral-300 text-[#1A6BD8] focus:ring-[#1A6BD8]"
          />
          开启
        </label>
      </div>
      {message && (
        <div className="border-t border-neutral-100 px-5 py-2 text-xs text-neutral-500">
          {message}
        </div>
      )}
    </section>
  )
}

const OCR_PROVIDER_LABEL: Record<string, string> = {
  baidu: "百度",
  tencent: "腾讯",
  mistral: "Mistral",
}

function OcrConfigCard() {
  const [ocrConfig, setOcrConfig] = useState<OcrProviderConfig | null>(null)
  const [loading, setLoading] = useState(true)
  const [provider, setProvider] = useState<string>("baidu")
  const [name, setName] = useState("")
  const [apiKey, setApiKey] = useState("")
  const [secretKey, setSecretKey] = useState("")
  const [showApiKey, setShowApiKey] = useState(false)
  const [saving, setSaving] = useState(false)
  const [saveMsg, setSaveMsg] = useState<string | null>(null)

  useEffect(() => {
    getOcrConfig()
      .then((cfg) => {
        setOcrConfig(cfg)
        if (cfg) {
          setProvider(cfg.provider)
          setName(cfg.name)
          setApiKey(cfg.api_key)
          setSecretKey(cfg.secret_key ?? "")
        }
      })
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const handleSave = useCallback(
    async (
      targetProvider?: string,
      targetName?: string,
      targetApiKey?: string,
      targetSecretKey?: string,
    ) => {
      const p = targetProvider !== undefined ? targetProvider : provider
      const n = targetName !== undefined ? targetName : name
      const key = targetApiKey !== undefined ? targetApiKey : apiKey
      const secret = targetSecretKey !== undefined ? targetSecretKey : secretKey

      // 只有在 API Key 不为空时才保存，避免清空配置
      if (!key.trim()) return

      setSaving(true)
      setSaveMsg(null)
      try {
        await saveOcrConfig({
          id: ocrConfig?.id ?? crypto.randomUUID(),
          name: n.trim() || `${OCR_PROVIDER_LABEL[p] ?? p} OCR`,
          provider: p,
          apiKey: key.trim(),
          secretKey: secret.trim() || undefined,
        })
        const updated = await getOcrConfig()
        setOcrConfig(updated)
        setSaveMsg("已自动保存")
        setTimeout(() => setSaveMsg(null), TOAST_AUTO_DISMISS_MS)
      } catch (err) {
        setSaveMsg(`自动保存失败：${err instanceof Error ? err.message : String(err)}`)
      } finally {
        setSaving(false)
      }
    },
    [ocrConfig, provider, name, apiKey, secretKey],
  )

  if (loading) {
    return (
      <section className="rounded-xl border border-neutral-200 bg-white">
        <div className="flex items-center justify-center p-8">
          <Loader2 className="h-5 w-5 animate-spin text-neutral-400" />
        </div>
      </section>
    )
  }

  return (
    <section className="rounded-xl border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-5 py-3">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-sm font-semibold text-neutral-700">OCR 文字识别</h2>
            <p className="mt-0.5 text-xs text-neutral-400">配置 OCR 服务，用于图片文字提取</p>
          </div>
          <div className="flex items-center gap-1.5 text-xs">
            {saving ? (
              <span className="flex items-center gap-1 text-neutral-400">
                <Loader2 className="h-3.5 w-3.5 animate-spin text-[#1A6BD8]" />
                自动保存中...
              </span>
            ) : saveMsg ? (
              <span className="text-green-600 font-medium">{saveMsg}</span>
            ) : ocrConfig ? (
              <span className="flex items-center gap-1 rounded-full bg-green-50 px-2.5 py-0.5 font-medium text-green-700">
                <span className="h-1.5 w-1.5 rounded-full bg-green-500" />
                已配置
              </span>
            ) : null}
          </div>
        </div>
      </div>

      <div className="p-5">
        <div className="space-y-4">
          <div>
            <label
              htmlFor="ocr-provider"
              className="mb-1.5 block text-xs font-medium text-neutral-600"
            >
              OCR 服务商
            </label>
            <select
              id="ocr-provider"
              value={provider}
              onChange={(e) => {
                const nextProvider = e.target.value
                setProvider(nextProvider)
                handleSave(nextProvider)
              }}
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            >
              <option value="baidu">百度 OCR（推荐，中文最强）</option>
              <option value="tencent">腾讯 OCR</option>
              <option value="mistral">Mistral OCR（表格/图表/版式最强）</option>
            </select>
          </div>

          <div>
            <label htmlFor="ocr-name" className="mb-1.5 block text-xs font-medium text-neutral-600">
              名称
            </label>
            <input
              id="ocr-name"
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onBlur={() => handleSave()}
              placeholder={`${OCR_PROVIDER_LABEL[provider] ?? provider} OCR`}
              className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />
          </div>

          <div>
            <label
              htmlFor="ocr-api-key"
              className="mb-1.5 block text-xs font-medium text-neutral-600"
            >
              API Key
            </label>
            <div className="relative">
              <input
                id="ocr-api-key"
                type={showApiKey ? "text" : "password"}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                onBlur={() => handleSave()}
                placeholder="输入 API Key"
                className="w-full rounded-lg border border-neutral-200 px-3 py-2 pr-10 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              />
              <button
                type="button"
                onClick={() => setShowApiKey((v) => !v)}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-neutral-400 hover:text-neutral-600"
                tabIndex={-1}
              >
                {showApiKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
              </button>
            </div>
          </div>

          {provider === "baidu" && (
            <div>
              <label
                htmlFor="ocr-secret-key"
                className="mb-1.5 block text-xs font-medium text-neutral-600"
              >
                Secret Key
              </label>
              <input
                id="ocr-secret-key"
                type="password"
                value={secretKey}
                onChange={(e) => setSecretKey(e.target.value)}
                onBlur={() => handleSave()}
                placeholder="输入 Secret Key"
                className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              />
            </div>
          )}

          <ImageTypeExclusion />
        </div>
      </div>
    </section>
  )
}

const IMAGE_CATEGORY_OPTIONS: { value: string; label: string; desc: string }[] = [
  { value: "image", label: "普通图像", desc: "照片/Logo/装饰图" },
  { value: "graph", label: "图表", desc: "流程图/架构图" },
  { value: "table", label: "表格", desc: "表格截图" },
  { value: "text", label: "文字截图", desc: "纯文字图片" },
]

function ImageTypeExclusion() {
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
