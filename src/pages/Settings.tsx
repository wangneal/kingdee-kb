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
  FolderOpen,
  HardDrive,
  Hash,
  Key,
  Loader2,
  Pencil,
  Plug,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
  Scan,
  Server,
  Settings as SettingsIcon,
  Star,
  Trash2,
  Upload,
  X,
} from "lucide-react"
import { useCallback, useEffect, useMemo, useState } from "react"
import { useSearchParams } from "react-router-dom"
import {
  addLLMProvider,
  deleteLLMProvider,
  getOcrConfig,
  listLLMProviders,
  probeAllProviders,
  probeModelMultimodal,
  saveOcrConfig,
  setDefaultLLMProvider,
  updateLLMProvider,
} from "../lib/skill-commands"
import type {
  ApiKeyConfig,
  LLMProtocol,
  LLMProviderConfig,
  ModelConfig,
  OcrProviderConfig,
} from "../lib/skill-types"
import {
  type AsrConfigStatus,
  addSensitiveKeyword,
  type EmbeddingModelConfig,
  type EmbeddingProviderConfig,
  type EmbeddingProviderType,
  exportDatabase,
  getKbCompilationEnabled,
  getAsrConfigStatus,
  getDownloadProgress,
  getEmbeddingModelConfig,
  getModelStatus,
  getStats,
  type ImportDbResult,
  importDatabase,
  initModel,
  type KnowledgeStats,
  listSensitiveKeywords,
  removeSensitiveKeyword,
  saveAsrConfig,
  setKbCompilationEnabled,
  setEmbeddingModelConfig,
} from "../lib/tauri-commands"

const PROVIDER_DEFAULTS: Record<string, { base_url: string; model: string }> = {
  openai: {
    base_url: "https://api.openai.com/v1",
    model: "gpt-4o",
  },
  anthropic: {
    base_url: "https://api.anthropic.com/v1",
    model: "claude-3-5-sonnet-20241022",
  },
  local: {
    base_url: "http://localhost:11434",
    model: "qwen2.5:7b",
  },
}

/** Embedding 供应商定义：标签、默认 Base URL、推荐模型 */
const EMBEDDING_PROVIDERS: Record<
  EmbeddingProviderType,
  { label: string; baseUrl: string; models: string[] }
> = {
  local: { label: "本地模型", baseUrl: "", models: [] },
  siliconflow: {
    label: "硅基流动",
    baseUrl: "https://api.siliconflow.cn/v1",
    models: ["BAAI/bge-m3", "BAAI/bge-large-zh-v1.5"],
  },
  openai: {
    label: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    models: ["text-embedding-3-small", "text-embedding-3-large"],
  },
  zhipu: {
    label: "智谱 AI",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    models: ["embedding-3"],
  },
  dashscope: {
    label: "阿里灵积",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    models: ["text-embedding-v3", "text-embedding-v2"],
  },
  cohere: {
    label: "Cohere",
    baseUrl: "https://api.cohere.com/v2",
    models: ["embed-multilingual-v3.0", "embed-english-v3.0"],
  },
  custom: {
    label: "自定义 (OpenAI 兼容)",
    baseUrl: "",
    models: [],
  },
}

const DEFAULT_EMBEDDING_PROVIDER_CONFIG: EmbeddingProviderConfig = {
  provider: "local",
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

export default function Settings() {
  const [stats, setStats] = useState<KnowledgeStats | null>(null)
  const [loading, setLoading] = useState(true)
  const [modelReady, setModelReady] = useState(false)
  const [initializing, setInitializing] = useState(false)
  const [downloadProgress, setDownloadProgress] = useState(0)
  const [initResult, setInitResult] = useState<{
    ok: boolean
    msg: string
  } | null>(null)
  const [embeddingConfig, setEmbeddingConfig] = useState<EmbeddingModelConfig>({})
  const [embeddingConfigSaving, setEmbeddingConfigSaving] = useState(false)
  const [embeddingProviderConfig, setEmbeddingProviderConfig] = useState<EmbeddingProviderConfig>(
    DEFAULT_EMBEDDING_PROVIDER_CONFIG,
  )
  const [embeddingProviderSaving, setEmbeddingProviderSaving] = useState(false)
  const [embeddingProviderSaveMsg, setEmbeddingProviderSaveMsg] = useState<string | null>(null)
  const [showEmbeddingApiKey, setShowEmbeddingApiKey] = useState(false)
  const [keywordInput, setKeywordInput] = useState("")
  const [keywords, setKeywords] = useState<string[]>([])
  const [keywordError, setKeywordError] = useState<string | null>(null)

  // ASR 配置状态
  const [asrConfigStatus, setAsrConfigStatus] = useState<AsrConfigStatus | null>(null)
  const [tencentSecretId, setTencentSecretId] = useState("")
  const [tencentSecretKey, setTencentSecretKey] = useState("")
  const [asrSaving, setAsrSaving] = useState(false)
  const [asrSaveMsg, setAsrSaveMsg] = useState<string | null>(null)

  // kdclub API Key 状态
  const [kdclubToken, setKdclubToken] = useState("")
  const [showKdclubToken, setShowKdclubToken] = useState(false)
  const [kdclubSaving, setKdclubSaving] = useState(false)
  const [kdclubSaveMsg, setKdclubSaveMsg] = useState<string | null>(null)
  const [activeTab, setActiveTab] = useState<"ai" | "integrations" | "data">("ai")

  // 从 URL 参数读取 section，支持状态栏 deep link
  const [searchParams] = useSearchParams()
  useEffect(() => {
    const section = searchParams.get("section")
    if (section === "llm" || section === "embedding") setActiveTab("ai")
    else if (section === "integrations") setActiveTab("integrations")
    else if (section === "data" || section === "kb") setActiveTab("data")
  }, [searchParams])

  // 上下文工程覆盖状态
  const [overrideModelId, setOverrideModelId] = useState("")
  const [overrideContextWindow, setOverrideContextWindow] = useState("")
  const [overrideMaxOutput, setOverrideMaxOutput] = useState("")
  const [overrideSaveMsg, setOverrideSaveMsg] = useState<string | null>(null)
  const [overrideKey, setOverrideKey] = useState(0)

  // 合并内置规格和 localStorage 覆盖
  const mergedModelSpecs = useMemo(() => {
    void overrideKey
    const overrides: Record<string, { context_window?: number; max_output?: number }> = JSON.parse(
      localStorage.getItem("model_spec_overrides") || "{}",
    )
    const specs = [...MODEL_SPECS]
    for (const [id, val] of Object.entries(overrides)) {
      const existing = specs.findIndex((s) => s.id === id)
      if (existing >= 0) {
        specs[existing] = { ...specs[existing], ...val }
      } else {
        specs.push({
          id,
          context_window: val.context_window ?? 128000,
          max_output: val.max_output ?? 8192,
          thinking: false,
        })
      }
    }
    return specs
  }, [overrideKey])

  // 加载配置和统计信息，并轮询模型状态（自动加载可能仍在异步执行）
  useEffect(() => {
    let cancelled = false

    // 立即加载配置，不等待模型
    Promise.all([getStats().catch(() => null), getEmbeddingModelConfig().catch(() => ({}))]).then(
      ([s, embeddingCfg]) => {
        if (cancelled) return
        setStats(s)
        setEmbeddingConfig(embeddingCfg)

        // 从 localStorage 加载在线 Embedding 供应商配置
        try {
          const stored = localStorage.getItem(EMBEDDING_PROVIDER_STORAGE_KEY)
          if (stored) {
            const parsed = JSON.parse(stored) as EmbeddingProviderConfig
            setEmbeddingProviderConfig({ ...DEFAULT_EMBEDDING_PROVIDER_CONFIG, ...parsed })
          }
        } catch {
          /* 忽略解析错误 */
        }

        // 从 localStorage 加载 kdclub token
        try {
          const kdclubStored = localStorage.getItem("kdclub_pat_token")
          if (kdclubStored) {
            setKdclubToken(kdclubStored)
          }
        } catch {
          /* 忽略读取错误 */
        }
      },
    )

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
    getAsrConfigStatus()
      .then(setAsrConfigStatus)
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

  const handleInitModel = useCallback(async () => {
    setInitializing(true)
    setDownloadProgress(0)
    setInitResult(null)

    // 每 600ms 轮询下载进度
    const pollInterval = setInterval(async () => {
      try {
        const pct = await getDownloadProgress()
        setDownloadProgress(pct)
      } catch {
        // 忽略轮询错误
      }
    }, 600)

    try {
      const ok = await initModel()
      clearInterval(pollInterval)
      setDownloadProgress(100)
      setModelReady(ok)
      setInitResult({
        ok,
        msg: ok ? "Embedding 模型已加载完成" : "模型初始化失败",
      })
      setTimeout(() => setInitResult(null), 5000)
    } catch (err) {
      clearInterval(pollInterval)
      setDownloadProgress(0)
      setInitResult({
        ok: false,
        msg: `初始化失败：${err instanceof Error ? err.message : String(err)}`,
      })
    } finally {
      setInitializing(false)
    }
  }, [])

  const handleChooseEmbeddingDir = useCallback(async () => {
    const { open } = await import("@tauri-apps/plugin-dialog")
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择 Embedding 模型目录",
    })
    if (typeof selected === "string") {
      setEmbeddingConfig({ custom_model_dir: selected })
    }
  }, [])

  const handleSaveEmbeddingConfig = useCallback(async () => {
    setEmbeddingConfigSaving(true)
    setInitResult(null)
    try {
      const dir = embeddingConfig.custom_model_dir?.trim() || null
      const ok = await setEmbeddingModelConfig(dir)
      setModelReady(ok)
      setEmbeddingConfig({ custom_model_dir: dir })
      setInitResult({
        ok,
        msg: dir ? "自定义 Embedding 模型已加载" : "已切换为内置 Embedding 模型",
      })
    } catch (err) {
      setInitResult({
        ok: false,
        msg: `Embedding 模型配置失败：${err instanceof Error ? err.message : String(err)}`,
      })
    } finally {
      setEmbeddingConfigSaving(false)
    }
  }, [embeddingConfig.custom_model_dir])

  const handleResetEmbeddingConfig = useCallback(async () => {
    setEmbeddingConfig({ custom_model_dir: null })
    setEmbeddingConfigSaving(true)
    setInitResult(null)
    try {
      const ok = await setEmbeddingModelConfig(null)
      setModelReady(ok)
      setInitResult({ ok, msg: "已切换为内置 Embedding 模型" })
    } catch (err) {
      setInitResult({
        ok: false,
        msg: `切换内置模型失败：${err instanceof Error ? err.message : String(err)}`,
      })
    } finally {
      setEmbeddingConfigSaving(false)
    }
  }, [])

  const handleEmbeddingProviderChange = useCallback((provider: EmbeddingProviderType) => {
    const defaults = EMBEDDING_PROVIDERS[provider]
    setEmbeddingProviderConfig((prev) => ({
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
    }))
  }, [])

  const handleSaveEmbeddingProviderConfig = useCallback(async () => {
    setEmbeddingProviderSaving(true)
    setEmbeddingProviderSaveMsg(null)
    try {
      // 不将 API Key 持久化到 localStorage，避免安全风险
      const { api_key: _, ...safeConfig } = embeddingProviderConfig
      localStorage.setItem(EMBEDDING_PROVIDER_STORAGE_KEY, JSON.stringify(safeConfig))
      setEmbeddingProviderSaveMsg("配置已保存")
      setTimeout(() => setEmbeddingProviderSaveMsg(null), 3000)
    } catch (err) {
      setEmbeddingProviderSaveMsg(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setEmbeddingProviderSaving(false)
    }
  }, [embeddingProviderConfig])

  const handleSaveOverride = useCallback(() => {
    if (!overrideModelId.trim()) return
    try {
      const overrides = JSON.parse(localStorage.getItem("model_spec_overrides") || "{}")
      overrides[overrideModelId.trim()] = {
        context_window: Number(overrideContextWindow) || 128000,
        max_output: Number(overrideMaxOutput) || 8192,
      }
      localStorage.setItem("model_spec_overrides", JSON.stringify(overrides))
      setOverrideKey((k) => k + 1)
      setOverrideSaveMsg("已保存")
      setTimeout(() => setOverrideSaveMsg(null), 3000)
    } catch {
      setOverrideSaveMsg("保存失败")
    }
  }, [overrideModelId, overrideContextWindow, overrideMaxOutput])

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-neutral-400" />
      </div>
    )
  }

  return (
    <div className="mx-auto max-w-2xl p-6">
      <div className="mb-6 flex items-center gap-2">
        <SettingsIcon className="h-5 w-5 text-[#1A6BD8]" />
        <h1 className="text-lg font-semibold text-neutral-800">设置</h1>
      </div>

      {/* 标签导航 */}
      <div className="mb-6 flex gap-1 rounded-lg bg-neutral-100 p-1">
        {[
          { key: "ai" as const, label: "AI 模型", icon: Brain },
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
                    向量嵌入模型，支持本地 ONNX 模型或在线 API 服务
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
                  {modelReady ? "已就绪" : "未初始化"}
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

              {/* 本地模型界面 */}
              {embeddingProviderConfig.provider === "local" && (
                <>
                  <p className="mb-3 text-sm text-neutral-500">
                    {modelReady
                      ? "模型已加载，知识库导入和语义搜索功能可用。"
                      : initializing
                        ? `正在下载模型（${downloadProgress}%）... 首次下载约 90MB，请耐心等待`
                        : "模型尚未初始化。首次初始化需要从 HuggingFace 下载模型文件（约 90MB）。"}
                  </p>

                  {/* 下载进度条 */}
                  {initializing && (
                    <div className="mb-3">
                      <div className="h-2 w-full overflow-hidden rounded-full bg-neutral-100">
                        <div
                          className="h-full rounded-full bg-[#1A6BD8] transition-all duration-300 ease-out"
                          style={{ width: `${Math.max(downloadProgress, 2)}%` }}
                        />
                      </div>
                      <p className="mt-1 text-xs text-neutral-400">
                        {downloadProgress < 100 ? `${downloadProgress}%` : "加载中..."}
                      </p>
                    </div>
                  )}

                  <div className="mb-3 space-y-2">
                    <div className="flex items-center gap-2">
                      <input
                        type="text"
                        value={embeddingConfig.custom_model_dir ?? ""}
                        onChange={(e) => setEmbeddingConfig({ custom_model_dir: e.target.value })}
                        placeholder="默认使用内置 BGE 模型；可选择本地 ONNX 模型目录"
                        className="flex-1 rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
                      />
                      <button
                        type="button"
                        onClick={handleChooseEmbeddingDir}
                        className="rounded-lg border border-neutral-200 p-2 text-neutral-500 hover:bg-neutral-50"
                        title="选择目录"
                      >
                        <FolderOpen className="h-4 w-4" />
                      </button>
                      <button
                        type="button"
                        onClick={handleResetEmbeddingConfig}
                        disabled={embeddingConfigSaving}
                        className="rounded-lg border border-neutral-200 p-2 text-neutral-500 hover:bg-neutral-50 disabled:opacity-50"
                        title="使用内置模型"
                      >
                        <RotateCcw className="h-4 w-4" />
                      </button>
                    </div>
                    <p className="text-xs text-neutral-400">
                      目录需包含
                      model.onnx、tokenizer.json；config.json、tokenizer_config.json、special_tokens_map.json
                      可选。
                    </p>
                  </div>

                  <div className="flex items-center gap-3">
                    <button
                      type="button"
                      onClick={handleSaveEmbeddingConfig}
                      disabled={embeddingConfigSaving}
                      className="flex items-center gap-1.5 rounded-lg border border-neutral-200 px-4 py-2 text-sm font-medium text-neutral-600 hover:bg-neutral-50 disabled:opacity-50 transition-colors"
                    >
                      {embeddingConfigSaving ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <Save className="h-4 w-4" />
                      )}
                      保存模型设置
                    </button>
                    <button
                      type="button"
                      onClick={handleInitModel}
                      disabled={initializing || modelReady}
                      className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
                    >
                      {initializing ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <RefreshCw className="h-4 w-4" />
                      )}
                      {initializing ? "下载模型中..." : modelReady ? "已初始化" : "初始化模型"}
                    </button>

                    {initResult && (
                      <span
                        className={`text-xs ${initResult.ok ? "text-green-600" : "text-red-600"}`}
                      >
                        {initResult.msg}
                      </span>
                    )}
                  </div>

                  {!modelReady && !initializing && (
                    <div className="mt-3 flex items-center gap-2 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700">
                      <AlertTriangle className="h-3.5 w-3.5" />
                      未初始化时，AI 对话将无法使用知识库语义搜索，仅使用关键词匹配和 LLM 自身能力
                    </div>
                  )}
                </>
              )}

              {/* 在线供应商界面 */}
              {embeddingProviderConfig.provider !== "local" && (
                <>
                  <p className="mb-3 text-sm text-neutral-500">
                    使用 {EMBEDDING_PROVIDERS[embeddingProviderConfig.provider].label} 在线
                    Embedding 服务。 请填写 API Key 和模型配置后保存。
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
                        placeholder={EMBEDDING_PROVIDERS[embeddingProviderConfig.provider].baseUrl}
                        className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
                      />
                    </div>

                    {/* API Key 配置 */}
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
                            onClick={() =>
                              setEmbeddingProviderConfig((c) => ({ ...c, model_name: m }))
                            }
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

                  {/* 保存按钮 */}
                  <div className="mt-4 flex items-center gap-3">
                    <button
                      type="button"
                      onClick={handleSaveEmbeddingProviderConfig}
                      disabled={embeddingProviderSaving}
                      className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
                    >
                      {embeddingProviderSaving ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <Save className="h-4 w-4" />
                      )}
                      保存配置
                    </button>
                    {embeddingProviderSaveMsg && (
                      <span className="text-xs text-neutral-500">{embeddingProviderSaveMsg}</span>
                    )}
                  </div>
                </>
              )}
            </div>
          </section>

          {/* 上下文工程配置区 */}
          <section className="rounded-xl border border-neutral-200 bg-white">
            <div className="border-b border-neutral-100 px-5 py-3">
              <div className="flex items-center gap-2">
                <Cpu className="h-4 w-4 text-[#1A6BD8]" />
                <div>
                  <h2 className="text-sm font-semibold text-neutral-700">上下文工程</h2>
                  <p className="mt-0.5 text-xs text-neutral-400">模型上下文窗口规格与预算配置</p>
                </div>
              </div>
            </div>
            <div className="p-5">
              {/* 模型规格表 */}
              <div className="mb-4 overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-neutral-100 text-left text-xs text-neutral-500">
                      <th className="pb-2 pr-3 font-medium">模型</th>
                      <th className="pb-2 pr-3 font-medium">上下文窗口</th>
                      <th className="pb-2 pr-3 font-medium">最大输出</th>
                      <th className="pb-2 font-medium">思维链</th>
                    </tr>
                  </thead>
                  <tbody>
                    {mergedModelSpecs.map((spec) => (
                      <tr key={spec.id} className="border-b border-neutral-50">
                        <td className="py-1.5 pr-3 font-mono text-xs">{spec.id}</td>
                        <td className="py-1.5 pr-3 text-xs">
                          {(spec.context_window / 1000).toFixed(0)}K
                        </td>
                        <td className="py-1.5 pr-3 text-xs">
                          {(spec.max_output / 1000).toFixed(0)}K
                        </td>
                        <td className="py-1.5 text-xs">{spec.thinking ? "✓" : "—"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              {/* 手动覆盖表单 */}
              <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-3">
                <h3 className="mb-2 text-xs font-semibold text-neutral-600">手动覆盖</h3>
                <p className="mb-2 text-[10px] text-neutral-400">
                  为未收录的模型指定上下文窗口参数。保存后覆盖内置规格。
                </p>
                <div className="grid grid-cols-3 gap-2">
                  <input
                    type="text"
                    placeholder="模型 ID"
                    value={overrideModelId}
                    onChange={(e) => setOverrideModelId(e.target.value)}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                  <input
                    type="number"
                    placeholder="上下文窗口"
                    value={overrideContextWindow}
                    onChange={(e) => setOverrideContextWindow(e.target.value)}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                  <input
                    type="number"
                    placeholder="最大输出"
                    value={overrideMaxOutput}
                    onChange={(e) => setOverrideMaxOutput(e.target.value)}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                </div>
                <div className="mt-2 flex items-center gap-2">
                  <button
                    type="button"
                    onClick={handleSaveOverride}
                    className="rounded bg-[#1A6BD8] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#1558B0]"
                  >
                    保存覆盖
                  </button>
                  {overrideSaveMsg && (
                    <span className="text-xs text-neutral-500">{overrideSaveMsg}</span>
                  )}
                </div>
              </div>
            </div>
          </section>
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

              <div className="flex items-center gap-3">
                <button
                  type="button"
                  onClick={async () => {
                    setKdclubSaving(true)
                    setKdclubSaveMsg(null)
                    try {
                      // 保存到 localStorage，不写入文件以降低安全风险
                      if (kdclubToken) {
                        localStorage.setItem("kdclub_pat_token", kdclubToken)
                      } else {
                        localStorage.removeItem("kdclub_pat_token")
                      }
                      setKdclubSaveMsg("配置已保存")
                      setTimeout(() => setKdclubSaveMsg(null), 3000)
                    } catch (err) {
                      setKdclubSaveMsg(
                        `保存失败：${err instanceof Error ? err.message : String(err)}`,
                      )
                    }
                    setKdclubSaving(false)
                  }}
                  disabled={kdclubSaving}
                  className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
                >
                  {kdclubSaving ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Save className="h-4 w-4" />
                  )}
                  保存配置
                </button>
                {kdclubSaveMsg && <span className="text-xs text-neutral-500">{kdclubSaveMsg}</span>}
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
                <h3 className="text-xs font-semibold text-neutral-700 mb-2">
                  腾讯云语音识别
                  {asrConfigStatus?.tencent_configured && (
                    <span className="ml-2 text-green-600">✓ 已配置</span>
                  )}
                </h3>
                <div className="grid grid-cols-3 gap-2">
                  <input
                    type="text"
                    placeholder="SecretId"
                    value={tencentSecretId}
                    onChange={(e) => setTencentSecretId(e.target.value)}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                  <input
                    type="password"
                    placeholder="SecretKey"
                    value={tencentSecretKey}
                    onChange={(e) => setTencentSecretKey(e.target.value)}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                </div>
                <p className="text-[10px] text-neutral-400 mt-1">
                  在腾讯云控制台 → API密钥管理 获取 SecretId/SecretKey（AppId 无需填写）
                </p>
              </div>

              <div className="flex items-center gap-3">
                <button
                  type="button"
                  onClick={async () => {
                    setAsrSaving(true)
                    setAsrSaveMsg(null)
                    try {
                      await saveAsrConfig({
                        tencent_secret_id: tencentSecretId || undefined,
                        tencent_secret_key: tencentSecretKey || undefined,
                      })
                      const status = await getAsrConfigStatus()
                      setAsrConfigStatus(status)
                      setAsrSaveMsg("配置已保存")
                      setTimeout(() => setAsrSaveMsg(null), 3000)
                    } catch (err) {
                      setAsrSaveMsg(`保存失败：${err instanceof Error ? err.message : String(err)}`)
                    }
                    setAsrSaving(false)
                  }}
                  disabled={asrSaving}
                  className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
                >
                  {asrSaving ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Save className="h-4 w-4" />
                  )}
                  保存配置
                </button>
                {asrSaveMsg && <span className="text-xs text-neutral-500">{asrSaveMsg}</span>}
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
                <button
                  type="button"
                  onClick={async () => {
                    if (!keywordInput.trim()) return
                    try {
                      await addSensitiveKeyword(keywordInput.trim())
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
                      key={kw}
                      className="inline-flex items-center gap-1 rounded-full bg-amber-50 px-2.5 py-1 text-xs text-amber-700"
                    >
                      {kw}
                      <button
                        type="button"
                        onClick={async () => {
                          try {
                            await removeSensitiveKeyword(kw)
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
        setExporting(false)
        return
      }
      const filePath = `${targetPath}/risk_control_backup.db`
      await exportDatabase(filePath)
      setMsg({ ok: true, text: `已导出到 ${filePath}` })
    } catch (err) {
      setMsg({ ok: false, text: `导出失败：${err instanceof Error ? err.message : String(err)}` })
    }
    setExporting(false)
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
        setImporting(false)
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
    }
    setImporting(false)
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

function LLMProviderList() {
  const [providers, setProviders] = useState<LLMProviderConfig[]>([])
  const [loading, setLoading] = useState(true)
  const [actionMsg, setActionMsg] = useState<string | null>(null)
  const [dialogProvider, setDialogProvider] = useState<LLMProviderConfig | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [probing, setProbing] = useState(false)

  const loadProviders = useCallback(async () => {
    setLoading(true)
    try {
      const items = await listLLMProviders()
      setProviders(items)
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
              管理大语言模型供应商配置，支持多供应商、多模型、多 API Key。
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
            <p className="text-xs">点击「添加供应商」开始配置</p>
          </div>
        ) : (
          <div className="space-y-3">
            {providers.map((provider) => (
              <ProviderCard
                key={provider.id}
                provider={provider}
                onEdit={() => {
                  setDialogProvider(provider)
                  setDialogOpen(true)
                }}
                onDelete={() => handleDelete(provider.id)}
                onSetDefault={() => handleSetDefault(provider.id)}
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
  onEdit,
  onDelete,
  onSetDefault,
}: {
  provider: LLMProviderConfig
  onEdit: () => void
  onDelete: () => void
  onSetDefault: () => void
}) {
  const defaultModel = provider.models.find((model) => model.is_default) ?? provider.models[0]
  const defaultKey = provider.api_keys.find((key) => key.is_default) ?? provider.api_keys[0]

  return (
    <div
      className={`rounded-lg border p-4 transition-colors ${provider.is_default ? "border-[#1A6BD8]/30 bg-blue-50/30" : "border-neutral-200 bg-white hover:border-neutral-300"}`}
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
          </div>
          <div className="mt-2 grid gap-1 text-xs text-neutral-500 md:grid-cols-2">
            <span className="truncate">
              默认模型：{defaultModel?.name || "未设置"}
            </span>
            <span className="truncate">
              默认 Key：{defaultKey?.name || "未设置"}
            </span>
            <span className="truncate md:col-span-2">
              Base URL：{provider.base_url || "未设置"}
            </span>
          </div>
        </div>
        <div className="ml-2 flex shrink-0 items-center gap-1.5">
          {!provider.is_default && (
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
  const providerDefaults = PROVIDER_DEFAULTS[defaultProtocol]
  const [name, setName] = useState(provider?.name ?? "")
  const [protocol, setProtocol] = useState<LLMProtocol>(defaultProtocol)
  const [baseUrl, setBaseUrl] = useState(provider?.base_url ?? providerDefaults.base_url)
  const [apiKey, setApiKey] = useState("")
  const [modelsText, setModelsText] = useState(
    provider?.models.map((model) => model.name).join("\n") ?? providerDefaults.model,
  )
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)

  const handleProtocolChange = (nextProtocol: LLMProtocol) => {
    setProtocol(nextProtocol)
    if (!provider) {
      setBaseUrl(PROVIDER_DEFAULTS[nextProtocol].base_url)
      setModelsText(PROVIDER_DEFAULTS[nextProtocol].model)
    }
  }

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
      return {
        id: existing?.id ?? crypto.randomUUID(),
        name: modelName,
        is_default: index === 0,
        is_multimodal: existing?.is_multimodal ?? null,
        last_probe_at: existing?.last_probe_at ?? null,
      }
    })

    try {
      const payload = {
        id: provider?.id,
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
              placeholder={provider ? "已配置，留空不修改" : "sk-..."}
            />
          )}

          <label className="block">
            <span className="mb-1.5 block text-xs font-medium text-neutral-600">
              模型列表（每行一个，第一行为默认模型）
            </span>
            <textarea
              value={modelsText}
              onChange={(event) => setModelsText(event.target.value)}
              rows={5}
              className="w-full rounded-lg border border-neutral-200 px-3 py-2 font-mono text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />
          </label>

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
  const [enabled, setEnabled] = useState(false)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    getKbCompilationEnabled()
      .then((value) => {
        if (!cancelled) setEnabled(value)
      })
      .catch(() => {
        if (!cancelled) setEnabled(false)
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const handleToggle = useCallback(async (next: boolean) => {
    setEnabled(next)
    setSaving(true)
    setMessage(null)
    try {
      await setKbCompilationEnabled(next)
      setMessage(next ? "已开启知识编译" : "已关闭知识编译")
      setTimeout(() => setMessage(null), 3000)
    } catch (error) {
      setEnabled(!next)
      setMessage(`保存失败：${error instanceof Error ? error.message : String(error)}`)
    } finally {
      setSaving(false)
    }
  }, [])

  return (
    <section className="rounded-xl border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-5 py-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <h2 className="text-sm font-semibold text-neutral-700">知识编译</h2>
            <p className="mt-0.5 text-xs text-neutral-400">
              导入后尝试使用 LLM 生成 Wiki 候选页面；LLM 不可用或 30 秒超时时，会降级为快速分析模式（非 LLM）。
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

function OcrConfigCard() {
  const [ocrConfig, setOcrConfig] = useState<OcrProviderConfig | null>(null)
  const [loading, setLoading] = useState(true)
  const [showForm, setShowForm] = useState(false)
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

  const handleSave = useCallback(async () => {
    setSaving(true)
    setSaveMsg(null)
    try {
      await saveOcrConfig({
        id: ocrConfig?.id ?? crypto.randomUUID(),
        name: name.trim() || `${provider === "baidu" ? "百度" : "腾讯"} OCR`,
        provider,
        apiKey,
        secretKey: secretKey || undefined,
      })
      const updated = await getOcrConfig()
      setOcrConfig(updated)
      setShowForm(false)
      setSaveMsg("OCR 配置已保存")
      setTimeout(() => setSaveMsg(null), 3000)
    } catch (err) {
      setSaveMsg(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }, [ocrConfig, provider, name, apiKey, secretKey])

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
          {ocrConfig && (
            <span className="flex items-center gap-1 rounded-full bg-green-50 px-2.5 py-0.5 text-xs font-medium text-green-700">
              <span className="h-1.5 w-1.5 rounded-full bg-green-500" />
              已配置
            </span>
          )}
        </div>
      </div>

      <div className="p-5">
        {ocrConfig && !showForm ? (
          <div className="space-y-3">
            <div className="rounded-lg border border-neutral-200 p-4">
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-sm font-medium text-neutral-800">{ocrConfig.name}</p>
                  <p className="mt-1 text-xs text-neutral-500">
                    服务商：{ocrConfig.provider === "baidu" ? "百度 OCR" : "腾讯 OCR"}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => setShowForm(true)}
                  className="rounded-lg p-1.5 text-neutral-400 hover:bg-blue-50 hover:text-blue-600 transition-colors"
                  title="编辑"
                >
                  <Pencil className="h-4 w-4" />
                </button>
              </div>
            </div>
            {saveMsg && <span className="text-xs text-green-600">{saveMsg}</span>}
          </div>
        ) : (
          <div className="space-y-4">
            <div>
              <label htmlFor="ocr-provider" className="mb-1.5 block text-xs font-medium text-neutral-600">
                OCR 服务商
              </label>
              <select
                id="ocr-provider"
                value={provider}
                onChange={(e) => setProvider(e.target.value)}
                className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              >
                <option value="baidu">百度 OCR（推荐，中文最强）</option>
                <option value="tencent">腾讯 OCR</option>
              </select>
            </div>

            <div>
              <label htmlFor="ocr-name" className="mb-1.5 block text-xs font-medium text-neutral-600">名称</label>
              <input
                id="ocr-name"
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={provider === "baidu" ? "百度 OCR" : "腾讯 OCR"}
                className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              />
            </div>

            <div>
              <label htmlFor="ocr-api-key" className="mb-1.5 block text-xs font-medium text-neutral-600">API Key</label>
              <div className="relative">
                <input
                  id="ocr-api-key"
                  type={showApiKey ? "text" : "password"}
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
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
                <label htmlFor="ocr-secret-key" className="mb-1.5 block text-xs font-medium text-neutral-600">
                  Secret Key
                </label>
                <input
                  id="ocr-secret-key"
                  type="password"
                  value={secretKey}
                  onChange={(e) => setSecretKey(e.target.value)}
                  placeholder="输入 Secret Key"
                  className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
                />
              </div>
            )}

            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={handleSave}
                disabled={saving || !apiKey.trim()}
                className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
              >
                {saving ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Save className="h-4 w-4" />
                )}
                保存配置
              </button>
              {ocrConfig && (
                <button
                  type="button"
                  onClick={() => setShowForm(false)}
                  className="rounded-lg border border-neutral-200 px-4 py-2 text-sm font-medium text-neutral-600 hover:bg-neutral-50 transition-colors"
                >
                  取消
                </button>
              )}
              {saveMsg && <span className="text-xs text-red-600">{saveMsg}</span>}
            </div>
          </div>
        )}
      </div>
    </section>
  )
}
