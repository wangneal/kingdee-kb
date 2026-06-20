import {
  ArrowLeftRight,
  Brain,
  Cpu,
  Database,
  Eye,
  EyeOff,
  HardDrive,
  Hash,
  Key,
  Loader2,
  Plug,
  Plus,
  RefreshCw,
  Server,
  Settings as SettingsIcon,
  ShieldCheck,
} from "lucide-react"
import { useCallback, useEffect, useState } from "react"
import { useSearchParams } from "react-router-dom"
import { useToast } from "@/components/Toast"
import { useAsrConfig } from "@/contexts/AsrConfigContext"
import { TOAST_AUTO_DISMISS_MS } from "@/lib/constants"
import { getKdclubToken, saveKdclubToken } from "@/lib/kdclub-commands"
import {
  addSensitiveKeyword,
  type EmbeddingModelConfig,
  type EmbeddingProviderConfig,
  type EmbeddingProviderType,
  getEmbeddingModelConfig,
  getModelStatus,
  getStats,
  getTencentMeetingConfigStatus,
  listSensitiveKeywords,
  removeSensitiveKeyword,
  type SensitiveKeyword,
  type SensitiveKind,
  saveAsrConfig,
  saveTencentMeetingToken,
  setEmbeddingModelConfig,
  type TencentMeetingConfigStatus,
} from "@/lib/tauri-commands"

import {
  LLMProviderList,
  OcrConfigCard,
  DatabaseBackupCard,
  KnowledgeCompilationCard,
  AgentToolPolicyCard,
} from "./settings"

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

function Settings() {
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

// ── Helper components ─────────────────────────────────────────────────────

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

export { default as AgentToolOutputLimitsPanel } from "./settings/AgentToolOutputLimitsPanel"
export { default as SkillPermissionRulesPanel } from "./settings/SkillPermissionRulesPanel"
export { default as ImageTypeExclusion } from "./settings/ImageTypeExclusion"
export { default as ProviderFormDialog } from "./settings/ProviderFormDialog"

export default Settings
