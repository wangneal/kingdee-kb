import { useState, useEffect, useCallback } from "react";
import {
  Settings as SettingsIcon,
  Key,
  Server,
  Cpu,
  Hash,
  Loader2,
  HardDrive,
  RefreshCw,
  Save,
  AlertTriangle,
  ArrowLeftRight,
  Eye,
  EyeOff,
  Plus,
  FolderOpen,
  RotateCcw,
  Download,
  Upload,
  Brain,
  Plug,
  Database,
  Pencil,
  Trash2,
  Star,
  Scan,
  X,
  ChevronUp,
  ChevronDown,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getStats,
  initModel,
  getModelStatus,
  getDownloadProgress,
  getEmbeddingModelConfig,
  setEmbeddingModelConfig,
  type KnowledgeStats,
  type EmbeddingModelConfig,
  type EmbeddingProviderType,
  type EmbeddingProviderConfig,
addSensitiveKeyword,
listSensitiveKeywords,
removeSensitiveKeyword,
saveAsrConfig,
getAsrConfigStatus,
type AsrConfigStatus,
exportDatabase,
importDatabase,
type ImportDbResult,
} from "../lib/tauri-commands";
import {
  listLLMProviders,
  addLLMProvider,
  updateLLMProvider,
  deleteLLMProvider,
  setDefaultLLMProvider,
  probeAllProviders,
  probeModelMultimodal,
  getOcrConfig,
  saveOcrConfig,
} from "../lib/skill-commands";
import type {
  LLMProviderConfig,
  LLMProtocol,
  ApiKeyConfig,
  ModelConfig,
  OcrProviderConfig,
} from "../lib/skill-types";

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
    base_url: "http://localhost:11434/v1",
    model: "qwen2.5:7b",
  },
};

/** Embedding provider definitions — label, default base URL, and recommended models */
const EMBEDDING_PROVIDERS: Record<EmbeddingProviderType, { label: string; baseUrl: string; models: string[] }> = {
  local: { label: '本地模型', baseUrl: '', models: [] },
  siliconflow: { label: '硅基流动', baseUrl: 'https://api.siliconflow.cn/v1', models: ['BAAI/bge-m3', 'BAAI/bge-large-zh-v1.5'] },
  openai: { label: 'OpenAI', baseUrl: 'https://api.openai.com/v1', models: ['text-embedding-3-small', 'text-embedding-3-large'] },
  zhipu: { label: '智谱 AI', baseUrl: 'https://open.bigmodel.cn/api/paas/v4', models: ['embedding-3'] },
  dashscope: { label: '阿里灵积', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', models: ['text-embedding-v3', 'text-embedding-v2'] },
  cohere: { label: 'Cohere', baseUrl: 'https://api.cohere.com/v2', models: ['embed-multilingual-v3.0', 'embed-english-v3.0'] },
};

const DEFAULT_EMBEDDING_PROVIDER_CONFIG: EmbeddingProviderConfig = {
  provider: 'local',
  api_key: '',
  base_url: '',
  model_name: '',
};

const EMBEDDING_PROVIDER_STORAGE_KEY = 'kingdeekb_embedding_provider_config';

export default function Settings() {
  const [stats, setStats] = useState<KnowledgeStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [modelReady, setModelReady] = useState(false);
  const [initializing, setInitializing] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [initResult, setInitResult] = useState<{
    ok: boolean;
    msg: string;
  } | null>(null);
  const [embeddingConfig, setEmbeddingConfig] =
    useState<EmbeddingModelConfig>({});
  const [embeddingConfigSaving, setEmbeddingConfigSaving] = useState(false);
  const [embeddingProviderConfig, setEmbeddingProviderConfig] =
    useState<EmbeddingProviderConfig>(DEFAULT_EMBEDDING_PROVIDER_CONFIG);
  const [embeddingProviderSaving, setEmbeddingProviderSaving] = useState(false);
  const [embeddingProviderSaveMsg, setEmbeddingProviderSaveMsg] = useState<string | null>(null);
  const [showEmbeddingApiKey, setShowEmbeddingApiKey] = useState(false);
  const [keywordInput, setKeywordInput] = useState("");
  const [keywords, setKeywords] = useState<string[]>([]);
  const [keywordError, setKeywordError] = useState<string | null>(null);

  // ASR config state
  const [asrConfigStatus, setAsrConfigStatus] = useState<AsrConfigStatus | null>(null);
  const [tencentSecretId, setTencentSecretId] = useState("");
  const [tencentSecretKey, setTencentSecretKey] = useState("");
  const [tencentAppId, setTencentAppId] = useState("");
  const [xfyunAppId, setXfyunAppId] = useState("");
  const [xfyunApiKey, setXfyunApiKey] = useState("");
  const [xfyunApiSecret, setXfyunApiSecret] = useState("");
  const [asrSaving, setAsrSaving] = useState(false);
  const [asrSaveMsg, setAsrSaveMsg] = useState<string | null>(null);

  // kdclub API Key state
  const [kdclubToken, setKdclubToken] = useState("");
  const [showKdclubToken, setShowKdclubToken] = useState(false);
  const [kdclubSaving, setKdclubSaving] = useState(false);
  const [kdclubSaveMsg, setKdclubSaveMsg] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"ai" | "integrations" | "data">("ai");

  // Load config, stats; poll model status (auto-load may still be async in progress)
  useEffect(() => {
    let cancelled = false;

    // 立即加载配置，不等待模型
    Promise.all([
      getStats().catch(() => null),
      getEmbeddingModelConfig().catch(() => ({})),
    ]).then(([s, embeddingCfg]) => {
      if (cancelled) return;
      setStats(s);
      setEmbeddingConfig(embeddingCfg);

      // Load online embedding provider config from localStorage
      try {
        const stored = localStorage.getItem(EMBEDDING_PROVIDER_STORAGE_KEY);
        if (stored) {
          const parsed = JSON.parse(stored) as EmbeddingProviderConfig;
          setEmbeddingProviderConfig({ ...DEFAULT_EMBEDDING_PROVIDER_CONFIG, ...parsed });
        }
      } catch { /* ignore parse errors */ }

      // Load kdclub token from localStorage
      try {
        const kdclubStored = localStorage.getItem("kdclub_pat_token");
        if (kdclubStored) {
          setKdclubToken(kdclubStored);
        }
      } catch { /* ignore */ }
    });

    // 立即停止加载状态，不等待模型
    setLoading(false);

    // 异步轮询模型状态（不阻塞页面）
    let retries = 0;
    const MAX_RETRIES = 30;
    const pollModelStatus = async () => {
      if (cancelled) return;
      try {
        const status = await getModelStatus();
        if (status) {
          setModelReady(true);
          return;
        }
      } catch { /* ignore polling errors */ }
      retries++;
      if (retries < MAX_RETRIES && !cancelled) {
        setTimeout(pollModelStatus, 2000);
      }
    };
    pollModelStatus();

    listSensitiveKeywords().then(setKeywords).catch(() => {});
    getAsrConfigStatus().then(setAsrConfigStatus).catch(() => {});
    return () => { cancelled = true; };
  }, []);

  const handleRefreshStats = useCallback(async () => {
    try {
      const s = await getStats();
      setStats(s);
    } catch {
      // ignore
    }
  }, []);

  const handleInitModel = useCallback(async () => {
    setInitializing(true);
    setDownloadProgress(0);
    setInitResult(null);

    // Start polling progress every 600ms
    const pollInterval = setInterval(async () => {
      try {
        const pct = await getDownloadProgress();
        setDownloadProgress(pct);
      } catch {
        // ignore polling errors
      }
    }, 600);

    try {
      const ok = await initModel();
      clearInterval(pollInterval);
      setDownloadProgress(100);
      setModelReady(ok);
      setInitResult({
        ok,
        msg: ok ? "Embedding 模型已加载完成" : "模型初始化失败",
      });
      setTimeout(() => setInitResult(null), 5000);
    } catch (err) {
      clearInterval(pollInterval);
      setDownloadProgress(0);
      setInitResult({
        ok: false,
        msg: `初始化失败：${err instanceof Error ? err.message : String(err)}`,
      });
    } finally {
      setInitializing(false);
    }
  }, []);

  const handleChooseEmbeddingDir = useCallback(async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择 Embedding 模型目录",
    });
    if (typeof selected === "string") {
      setEmbeddingConfig({ custom_model_dir: selected });
    }
  }, []);

  const handleSaveEmbeddingConfig = useCallback(async () => {
    setEmbeddingConfigSaving(true);
    setInitResult(null);
    try {
      const dir = embeddingConfig.custom_model_dir?.trim() || null;
      const ok = await setEmbeddingModelConfig(dir);
      setModelReady(ok);
      setEmbeddingConfig({ custom_model_dir: dir });
      setInitResult({
        ok,
        msg: dir ? "自定义 Embedding 模型已加载" : "已切换为内置 Embedding 模型",
      });
    } catch (err) {
      setInitResult({
        ok: false,
        msg: `Embedding 模型配置失败：${err instanceof Error ? err.message : String(err)}`,
      });
    } finally {
      setEmbeddingConfigSaving(false);
    }
  }, [embeddingConfig.custom_model_dir]);

  const handleResetEmbeddingConfig = useCallback(async () => {
    setEmbeddingConfig({ custom_model_dir: null });
    setEmbeddingConfigSaving(true);
    setInitResult(null);
    try {
      const ok = await setEmbeddingModelConfig(null);
      setModelReady(ok);
      setInitResult({ ok, msg: "已切换为内置 Embedding 模型" });
    } catch (err) {
      setInitResult({
        ok: false,
        msg: `切换内置模型失败：${err instanceof Error ? err.message : String(err)}`,
      });
    } finally {
      setEmbeddingConfigSaving(false);
    }
  }, []);

  const handleEmbeddingProviderChange = useCallback((provider: EmbeddingProviderType) => {
    const defaults = EMBEDDING_PROVIDERS[provider];
    setEmbeddingProviderConfig((prev) => ({
      ...prev,
      provider,
      // Auto-fill base_url only if it still matches the previous provider default
      base_url: prev.base_url === EMBEDDING_PROVIDERS[prev.provider]?.baseUrl || prev.base_url === ''
        ? defaults.baseUrl
        : prev.base_url,
      // Auto-fill model_name only if it was one of the previous provider's models
      model_name: EMBEDDING_PROVIDERS[prev.provider]?.models.includes(prev.model_name) || prev.model_name === ''
        ? (defaults.models[0] ?? '')
        : prev.model_name,
    }));
  }, []);

  const handleSaveEmbeddingProviderConfig = useCallback(async () => {
    setEmbeddingProviderSaving(true);
    setEmbeddingProviderSaveMsg(null);
    try {
      // Don't persist API key to localStorage — security risk
      const { api_key: _, ...safeConfig } = embeddingProviderConfig;
      localStorage.setItem(EMBEDDING_PROVIDER_STORAGE_KEY, JSON.stringify(safeConfig));
      setEmbeddingProviderSaveMsg("配置已保存");
      setTimeout(() => setEmbeddingProviderSaveMsg(null), 3000);
    } catch (err) {
      setEmbeddingProviderSaveMsg(`保存失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setEmbeddingProviderSaving(false);
    }
  }, [embeddingProviderConfig]);

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-neutral-400" />
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-2xl p-6">
      <div className="mb-6 flex items-center gap-2">
        <SettingsIcon className="h-5 w-5 text-[#1A6BD8]" />
        <h1 className="text-lg font-semibold text-neutral-800">设置</h1>
      </div>

      {/* Tab Navigation */}
      <div className="mb-6 flex gap-1 rounded-lg bg-neutral-100 p-1">
        {([
          { key: "ai" as const, label: "AI 模型", icon: Brain },
          { key: "integrations" as const, label: "集成服务", icon: Plug },
          { key: "data" as const, label: "数据管理", icon: Database },
        ]).map((tab) => (
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

      {/* AI 模型 Tab */}
      {activeTab === "ai" && (
        <div className="space-y-6">
          {/* LLM Provider List */}
          <LLMProviderList />

          {/* OCR Configuration */}
          <OcrConfigCard />

        {/* Embedding Model Card */}
        <section className="rounded-xl border border-neutral-200 bg-white">
        <div className="border-b border-neutral-100 px-5 py-3">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-sm font-semibold text-neutral-700">
                Embedding 模型
              </h2>
              <p className="mt-0.5 text-xs text-neutral-400">
                向量嵌入模型，支持本地 ONNX 模型或在线 API 服务
              </p>
            </div>
            <span
              className={`flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium ${
                modelReady
                  ? "bg-green-50 text-green-700"
                  : "bg-amber-50 text-amber-700"
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
          {/* Provider selector */}
          <div className="mb-4">
            <div className="mb-1.5 flex items-center gap-2">
              <ArrowLeftRight className="h-4 w-4 text-neutral-400" />
              <span className="text-sm font-medium text-neutral-700">模式</span>
            </div>
            <select
              value={embeddingProviderConfig.provider}
              onChange={(e) => handleEmbeddingProviderChange(e.target.value as EmbeddingProviderType)}
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            >
              {(Object.keys(EMBEDDING_PROVIDERS) as EmbeddingProviderType[]).map((key) => (
                <option key={key} value={key}>
                  {EMBEDDING_PROVIDERS[key].label}
                </option>
              ))}
            </select>
          </div>

          {/* Local model UI */}
          {embeddingProviderConfig.provider === 'local' && (
            <>
              <p className="mb-3 text-sm text-neutral-500">
                {modelReady
                  ? "模型已加载，知识库导入和语义搜索功能可用。"
                  : initializing
                    ? `正在下载模型（${downloadProgress}%）... 首次下载约 90MB，请耐心等待`
                    : "模型尚未初始化。首次初始化需要从 HuggingFace 下载模型文件（约 90MB）。"}
              </p>

              {/* Progress bar during download */}
              {initializing && (
                <div className="mb-3">
                  <div className="h-2 w-full overflow-hidden rounded-full bg-neutral-100">
                    <div
                      className="h-full rounded-full bg-[#1A6BD8] transition-all duration-300 ease-out"
                      style={{ width: `${Math.max(downloadProgress, 2)}%` }}
                    />
                  </div>
                  <p className="mt-1 text-xs text-neutral-400">
                    {downloadProgress < 100
                      ? `${downloadProgress}%`
                      : "加载中..."}
                  </p>
                </div>
              )}

              <div className="mb-3 space-y-2">
                <div className="flex items-center gap-2">
                  <input
                    type="text"
                    value={embeddingConfig.custom_model_dir ?? ""}
                    onChange={(e) =>
                      setEmbeddingConfig({ custom_model_dir: e.target.value })
                    }
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
                  目录需包含 model.onnx、tokenizer.json；config.json、tokenizer_config.json、special_tokens_map.json 可选。
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
                  {initializing
                    ? "下载模型中..."
                    : modelReady
                      ? "已初始化"
                      : "初始化模型"}
                </button>

                {initResult && (
                  <span
                    className={`text-xs ${
                      initResult.ok ? "text-green-600" : "text-red-600"
                    }`}
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

          {/* Online provider UI */}
          {embeddingProviderConfig.provider !== 'local' && (
            <>
              <p className="mb-3 text-sm text-neutral-500">
                使用 {EMBEDDING_PROVIDERS[embeddingProviderConfig.provider].label} 在线 Embedding 服务。
                请填写 API Key 和模型配置后保存。
              </p>

              <div className="space-y-3">
                {/* Base URL */}
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

                {/* API Key */}
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

                {/* Model Name */}
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

                {/* Model preset buttons */}
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

              {/* Save button */}
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

        {/* Image Processing Card */}
        <ImageProcessingCard />
      </div>
      )}

      {/* 集成服务 Tab */}
      {activeTab === "integrations" && (
        <div className="space-y-6">
          {/* kdclub API Key Card */}
          <section className="rounded-xl border border-neutral-200 bg-white">
            <div className="border-b border-neutral-100 px-5 py-3">
              <h2 className="text-sm font-semibold text-neutral-700">
                金蝶云社区 API
              </h2>
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
                    {showKdclubToken ? (
                      <EyeOff className="h-4 w-4" />
                    ) : (
                      <Eye className="h-4 w-4" />
                    )}
                  </button>
                </div>
                <p className="mt-1.5 text-[10px] text-neutral-400">
                  在金蝶云社区 → 个人设置 → 访问令牌 获取。格式如 <code className="bg-neutral-100 px-1 rounded">kdt_xxxxxxxx...</code>
                </p>
              </div>

              <div className="flex items-center gap-3">
                <button
                  type="button"
                  onClick={async () => {
                    setKdclubSaving(true);
                    setKdclubSaveMsg(null);
                    try {
                      // Save to localStorage (not to file for security)
                      if (kdclubToken) {
                        localStorage.setItem("kdclub_pat_token", kdclubToken);
                      } else {
                        localStorage.removeItem("kdclub_pat_token");
                      }
                      setKdclubSaveMsg("配置已保存");
                      setTimeout(() => setKdclubSaveMsg(null), 3000);
                    } catch (err) {
                      setKdclubSaveMsg(`保存失败：${err instanceof Error ? err.message : String(err)}`);
                    }
                    setKdclubSaving(false);
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
                {kdclubSaveMsg && (
                  <span className="text-xs text-neutral-500">{kdclubSaveMsg}</span>
                )}
              </div>
            </div>
          </section>

          {/* ASR Config Card */}
          <section className="rounded-xl border border-neutral-200 bg-white">
            <div className="border-b border-neutral-100 px-5 py-3">
              <h2 className="text-sm font-semibold text-neutral-700">
                语音识别服务配置
              </h2>
              <p className="mt-0.5 text-xs text-neutral-400">
                配置在线语音识别服务（腾讯/讯飞），用于替代本地 Whisper 模型
              </p>
            </div>
            <div className="p-5 space-y-4">
              {/* Tencent ASR */}
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
                  <input
                    type="text"
                    placeholder="AppId"
                    value={tencentAppId}
                    onChange={(e) => setTencentAppId(e.target.value)}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                </div>
                <p className="text-[10px] text-neutral-400 mt-1">
                  在腾讯云控制台 → API密钥管理 获取
                </p>
              </div>

              {/* Xfyun ASR */}
              <div className="rounded-lg border border-neutral-200 p-4">
                <h3 className="text-xs font-semibold text-neutral-700 mb-2">
                  讯飞语音听写
                  {asrConfigStatus?.xfyun_configured && (
                    <span className="ml-2 text-green-600">✓ 已配置</span>
                  )}
                </h3>
                <div className="grid grid-cols-3 gap-2">
                  <input
                    type="text"
                    placeholder="AppID"
                    value={xfyunAppId}
                    onChange={(e) => setXfyunAppId(e.target.value)}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                  <input
                    type="text"
                    placeholder="APIKey"
                    value={xfyunApiKey}
                    onChange={(e) => setXfyunApiKey(e.target.value)}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                  <input
                    type="password"
                    placeholder="APISecret"
                    value={xfyunApiSecret}
                    onChange={(e) => setXfyunApiSecret(e.target.value)}
                    className="rounded border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                </div>
                <p className="text-[10px] text-neutral-400 mt-1">
                  在讯飞开放平台 → 我的应用 → 语音听写（流式版） 获取
                </p>
              </div>

              <div className="flex items-center gap-3">
                <button
                  type="button"
                  onClick={async () => {
                    setAsrSaving(true);
                    setAsrSaveMsg(null);
                    try {
                      await saveAsrConfig({
                        tencent_secret_id: tencentSecretId || undefined,
                        tencent_secret_key: tencentSecretKey || undefined,
                        tencent_app_id: tencentAppId ? Number(tencentAppId) : undefined,
                        xfyun_app_id: xfyunAppId || undefined,
                        xfyun_api_key: xfyunApiKey || undefined,
                        xfyun_api_secret: xfyunApiSecret || undefined,
                      });
                      const status = await getAsrConfigStatus();
                      setAsrConfigStatus(status);
                      setAsrSaveMsg("配置已保存");
                      setTimeout(() => setAsrSaveMsg(null), 3000);
                    } catch (err) {
                      setAsrSaveMsg(`保存失败：${err instanceof Error ? err.message : String(err)}`);
                    }
                    setAsrSaving(false);
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
                {asrSaveMsg && (
                  <span className="text-xs text-neutral-500">{asrSaveMsg}</span>
                )}
              </div>
            </div>
          </section>
        </div>
      )}

      {/* 数据管理 Tab */}
      {activeTab === "data" && (
        <div className="space-y-6">
          {/* Storage Stats Card */}
          <section className="rounded-xl border border-neutral-200 bg-white">
            <div className="flex items-center justify-between border-b border-neutral-100 px-5 py-3">
              <div>
                <h2 className="text-sm font-semibold text-neutral-700">
                  存储统计
                </h2>
                <p className="mt-0.5 text-xs text-neutral-400">
                  知识库当前数据概览
                </p>
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

          {/* Desensitization Config Card */}
          <section className="rounded-xl border border-neutral-200 bg-white">
            <div className="border-b border-neutral-100 px-5 py-3">
              <h2 className="text-sm font-semibold text-neutral-700">
                数据脱敏配置
              </h2>
              <p className="mt-0.5 text-xs text-neutral-400">
                管理敏感词库，发送给 LLM 前自动过滤
              </p>
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
                    if (!keywordInput.trim()) return;
                    try {
                      await addSensitiveKeyword(keywordInput.trim());
                      setKeywordInput("");
                      const kw = await listSensitiveKeywords();
                      setKeywords(kw);
                    } catch (e) { setKeywordError(String(e)); setTimeout(() => setKeywordError(null), 5000); }
                  }}
                  className="flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700"
                >
                  <Plus className="h-3.5 w-3.5" /> 添加
                </button>
              </div>
              {keywords.length > 0 && (
                <div className="mt-3 flex flex-wrap gap-2">
                  {keywords.map((kw) => (
                    <span key={kw} className="inline-flex items-center gap-1 rounded-full bg-amber-50 px-2.5 py-1 text-xs text-amber-700">
                      {kw}
                      <button type="button" onClick={async () => {
                        try {
                          await removeSensitiveKeyword(kw);
                          setKeywords(await listSensitiveKeywords());
                        } catch (e) { setKeywordError(String(e)); setTimeout(() => setKeywordError(null), 5000); }
                      }} className="text-amber-400 hover:text-red-500">&times;</button>
                    </span>
                  ))}
                </div>
              )}
              {keywordError && <p className="text-xs text-red-600 mt-1">{keywordError}</p>}
            </div>
          </section>

          {/* Database Backup Card */}
          <DatabaseBackupCard />
        </div>
      )}
    </div>
  );
}

// ── Image Processing Card ──────────────────────────────────────────────

function ImageProcessingCard() {
  const [visionProvider, setVisionProvider] = useState<string>("gpt4v");
  const [visionApiKey, setVisionApiKey] = useState("");
  const [visionBaseUrl, setVisionBaseUrl] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [showVisionKey, setShowVisionKey] = useState(false);

  // Load config from localStorage
  useEffect(() => {
    try {
      const stored = localStorage.getItem("image_processing_config");
      if (stored) {
        const config = JSON.parse(stored);
        setVisionProvider(config.vision_provider || "gpt4v");
        setVisionApiKey(config.vision_api_key || "");
        setVisionBaseUrl(config.vision_base_url || "");
      }
    } catch { /* ignore */ }
  }, []);

  const handleSave = useCallback(async () => {
    setSaving(true);
    setSaveMsg(null);
    try {
      const config = {
        vision_provider: visionProvider,
        vision_api_key: visionApiKey,
        vision_base_url: visionBaseUrl,
      };
      localStorage.setItem("image_processing_config", JSON.stringify(config));
      setSaveMsg("配置已保存");
      setTimeout(() => setSaveMsg(null), 3000);
    } catch (err) {
      setSaveMsg(`保存失败：${err instanceof Error ? err.message : String(err)}`);
    }
    setSaving(false);
  }, [visionProvider, visionApiKey, visionBaseUrl]);

  return (
    <section className="mb-6 rounded-xl border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-5 py-3">
        <h2 className="text-sm font-semibold text-neutral-700">多模态 LLM 备用配置</h2>
        <p className="mt-0.5 text-xs text-neutral-400">
          配置多模态 LLM API，用于理解蓝图、操作手册中的图片（OCR 文字识别请在上方「OCR 文字识别」卡片中配置）
        </p>
      </div>

      <div className="space-y-6 p-5">
        {/* 多模态 LLM 配置 */}
        <div>
          <h3 className="mb-3 text-sm font-medium text-neutral-700">多模态 LLM（图表理解）</h3>
          
          <div className="space-y-3">
            <div>
              <label className="mb-1.5 block text-xs text-neutral-500">LLM 服务商</label>
              <select
                value={visionProvider}
                onChange={(e) => setVisionProvider(e.target.value)}
                className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm"
              >
                <option value="gpt4v">OpenAI GPT-4V（推荐，图表理解最强）</option>
                <option value="qwen_vl">通义千问 VL（中文优秀）</option>
                <option value="glm4v">智谱 GLM-4V</option>
                <option value="claude">Claude Vision</option>
              </select>
            </div>

            <div>
              <label className="mb-1.5 block text-xs text-neutral-500">API Key</label>
              <div className="relative">
                <input
                  type={showVisionKey ? "text" : "password"}
                  value={visionApiKey}
                  onChange={(e) => setVisionApiKey(e.target.value)}
                  placeholder="输入 API Key"
                  className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 pr-10 text-sm"
                />
                <button
                  type="button"
                  onClick={() => setShowVisionKey((v) => !v)}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-neutral-400 hover:text-neutral-600"
                >
                  {showVisionKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </button>
              </div>
            </div>

            <div>
              <label className="mb-1.5 block text-xs text-neutral-500">
                自定义 Base URL（可选）
              </label>
              <input
                type="text"
                value={visionBaseUrl}
                onChange={(e) => setVisionBaseUrl(e.target.value)}
                placeholder="https://api.openai.com/v1"
                className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm"
              />
              <p className="mt-1 text-[10px] text-neutral-400">
                留空使用默认地址。使用代理或自部署服务时可自定义。
              </p>
            </div>
          </div>
        </div>

        {/* 保存按钮 */}
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={handleSave}
            disabled={saving}
            className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
          >
            {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
            保存配置
          </button>
          {saveMsg && (
            <span className="text-xs text-neutral-500">{saveMsg}</span>
          )}
        </div>
      </div>
    </section>
  );
}

// ── Helper Components ─────────────────────────────────────────────────────

function StatCard({
  label,
  value,
  icon,
  isText = false,
}: {
  label: string;
  value: number | string;
  icon: React.ReactNode;
  isText?: boolean;
}) {
  return (
    <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-3">
      <div className="mb-1 flex items-center gap-1.5">
        {icon}
        <span className="text-xs text-neutral-500">{label}</span>
      </div>
      <p
        className={`font-semibold text-neutral-800 ${
          isText ? "text-xs truncate" : "text-lg"
        }`}
      >
        {isText ? value : typeof value === "number" ? value.toLocaleString() : value}
      </p>
    </div>
  );
}

function DatabaseBackupCard() {
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [importResult, setImportResult] = useState<ImportDbResult | null>(null);

  const handleExport = async () => {
    setExporting(true);
    setMsg(null);
    try {
      const targetPath = await open({
        directory: true,
        multiple: false,
        title: "选择导出目录",
      });
      if (!targetPath) { setExporting(false); return; }
      const filePath = `${targetPath}/risk_control_backup.db`;
      await exportDatabase(filePath);
      setMsg({ ok: true, text: `已导出到 ${filePath}` });
    } catch (err) {
      setMsg({ ok: false, text: `导出失败：${err instanceof Error ? err.message : String(err)}` });
    }
    setExporting(false);
  };

  const handleImport = async () => {
    setImporting(true);
    setMsg(null);
    setImportResult(null);
    try {
      const filePath = await open({
        multiple: false,
        filters: [{ name: "SQLite 数据库", extensions: ["db"] }],
        title: "选择备份文件",
      });
      if (!filePath) { setImporting(false); return; }
      const result = await importDatabase(filePath as string);
      setImportResult(result);
      setMsg({ ok: true, text: `导入成功：${result.risk_project_count} 个项目，${result.scope_item_count} 条范围，${result.metric_count} 条指标` });
    } catch (err) {
      setMsg({ ok: false, text: `导入失败：${err instanceof Error ? err.message : String(err)}` });
    }
    setImporting(false);
  };

  return (
    <section className="mt-6 rounded-xl border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-5 py-3">
        <h2 className="text-sm font-semibold text-neutral-700">整库备份</h2>
        <p className="mt-0.5 text-xs text-neutral-400">
          导出/导入风控数据库（项目、范围、指标）
        </p>
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
          <div className="mt-3 grid grid-cols-3 gap-3">
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-2 text-center">
              <p className="text-lg font-semibold text-neutral-800">{importResult.risk_project_count}</p>
              <p className="text-xs text-neutral-500">风控项目</p>
            </div>
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-2 text-center">
              <p className="text-lg font-semibold text-neutral-800">{importResult.scope_item_count}</p>
              <p className="text-xs text-neutral-500">范围条目</p>
            </div>
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-2 text-center">
              <p className="text-lg font-semibold text-neutral-800">{importResult.metric_count}</p>
              <p className="text-xs text-neutral-500">健康指标</p>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}

// ── LLM Provider List ──────────────────────────────────────────────────

const PROTOCOL_LABELS: Record<LLMProtocol, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  local: "本地模型",
};

function LLMProviderList() {
  const [providers, setProviders] = useState<LLMProviderConfig[]>([]);
  const [loading, setLoading] = useState(true);
  const [showForm, setShowForm] = useState(false);
  const [editingProvider, setEditingProvider] = useState<LLMProviderConfig | null>(null);
  const [probing, setProbing] = useState(false);
  const [actionMsg, setActionMsg] = useState<string | null>(null);
  const [expandedProvider, setExpandedProvider] = useState<string | null>(null);

  const loadProviders = useCallback(async () => {
    try {
      const list = await listLLMProviders();
      setProviders(list);
    } catch (err) {
      console.error("Failed to load providers:", err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  const handleDelete = useCallback(async (id: string) => {
    if (!confirm("确定删除此供应商？")) return;
    try {
      await deleteLLMProvider(id);
      await loadProviders();
      setActionMsg("已删除");
      setTimeout(() => setActionMsg(null), 2000);
    } catch (err) {
      setActionMsg(`删除失败：${err instanceof Error ? err.message : String(err)}`);
    }
  }, [loadProviders]);

  const handleSetDefault = useCallback(async (id: string) => {
    try {
      await setDefaultLLMProvider(id);
      await loadProviders();
      setActionMsg("已设为默认");
      setTimeout(() => setActionMsg(null), 2000);
    } catch (err) {
      setActionMsg(`设置失败：${err instanceof Error ? err.message : String(err)}`);
    }
  }, [loadProviders]);

  const handleProbe = useCallback(async () => {
    setProbing(true);
    try {
      const results = await probeAllProviders();
      // Update local state with probe results
      setProviders((prev) =>
        prev.map((p) => ({
          ...p,
          models: p.models.map((m) => {
            const result = results.find((r) => r.provider_id === p.id && r.model_id === m.id);
            return result ? { ...m, is_multimodal: result.is_multimodal } : m;
          }),
        }))
      );
    } catch (err) {
      setActionMsg(`探测失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setProbing(false);
    }
  }, []);

  const handleProbeSingleModel = useCallback(async (providerId: string, modelId: string) => {
    try {
      const isMultimodal = await probeModelMultimodal(providerId, modelId);
      setProviders((prev) =>
        prev.map((p) =>
          p.id === providerId
            ? {
                ...p,
                models: p.models.map((m) =>
                  m.id === modelId ? { ...m, is_multimodal: isMultimodal } : m
                ),
              }
            : p
        )
      );
    } catch (err) {
      setActionMsg(`探测失败：${err instanceof Error ? err.message : String(err)}`);
    }
  }, []);

  const handleFormSubmit = useCallback(async (data: LLMProviderInput) => {
    try {
      if (data.id) {
        await updateLLMProvider(data);
      } else {
        await addLLMProvider({ ...data, id: crypto.randomUUID() });
      }
      await loadProviders();
      setShowForm(false);
      setEditingProvider(null);
      setActionMsg(data.id ? "已更新" : "已添加");
      setTimeout(() => setActionMsg(null), 2000);
    } catch (err) {
      setActionMsg(`保存失败：${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  }, [loadProviders]);

  if (loading) {
    return (
      <section className="rounded-xl border border-neutral-200 bg-white">
        <div className="flex items-center justify-center p-8">
          <Loader2 className="h-5 w-5 animate-spin text-neutral-400" />
        </div>
      </section>
    );
  }

  return (
    <section className="rounded-xl border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-5 py-3">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-sm font-semibold text-neutral-700">LLM 供应商</h2>
            <p className="mt-0.5 text-xs text-neutral-400">
              管理大语言模型供应商配置，支持多供应商、多模型、多 API Key
            </p>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleProbe}
              disabled={probing || providers.length === 0}
              className="flex items-center gap-1.5 rounded-lg border border-neutral-200 px-3 py-1.5 text-xs font-medium text-neutral-600 hover:bg-neutral-50 disabled:opacity-50 transition-colors"
            >
              {probing ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Scan className="h-3.5 w-3.5" />
              )}
              探测多模态
            </button>
            <button
              type="button"
              onClick={() => { setEditingProvider(null); setShowForm(true); }}
              className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#1558B0] transition-colors"
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

        {providers.length === 0 ? (
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
                expanded={expandedProvider === provider.id}
                onToggleExpand={() => setExpandedProvider(expandedProvider === provider.id ? null : provider.id)}
                onSetDefault={() => handleSetDefault(provider.id)}
                onEdit={() => { setEditingProvider(provider); setShowForm(true); }}
                onDelete={() => handleDelete(provider.id)}
                onProbeModel={handleProbeSingleModel}
              />
            ))}
          </div>
        )}
      </div>

      {/* Provider Form Dialog */}
      {showForm && (
        <ProviderFormDialog
          provider={editingProvider}
          onSubmit={handleFormSubmit}
          onClose={() => { setShowForm(false); setEditingProvider(null); }}
        />
      )}
    </section>
  );
}

/** Provider card with expandable models and API keys */
function ProviderCard({
  provider,
  expanded,
  onToggleExpand,
  onSetDefault,
  onEdit,
  onDelete,
  onProbeModel,
}: {
  provider: LLMProviderConfig;
  expanded: boolean;
  onToggleExpand: () => void;
  onSetDefault: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onProbeModel: (providerId: string, modelId: string) => void;
}) {
  const defaultModel = provider.models.find((m) => m.is_default) || provider.models[0];

  return (
    <div
      className={`rounded-lg border transition-colors ${
        provider.is_default
          ? "border-[#1A6BD8]/30 bg-blue-50/30"
          : "border-neutral-200 bg-white hover:border-neutral-300"
      }`}
    >
      {/* Header */}
      <div className="flex items-start justify-between p-4">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="text-sm font-medium text-neutral-800">
              {provider.name}
            </span>
            {provider.is_default && (
              <span className="rounded-full bg-[#1A6BD8] px-2 py-0.5 text-[10px] font-medium text-white">
                默认
              </span>
            )}
            <span className="rounded-full bg-neutral-100 px-2 py-0.5 text-[10px] font-medium text-neutral-500">
              {PROTOCOL_LABELS[provider.protocol] || provider.protocol}
            </span>
            <span className="rounded-full bg-neutral-100 px-2 py-0.5 text-[10px] font-medium text-neutral-500">
              {provider.models.length} 模型 · {provider.api_keys.length} Key
            </span>
          </div>
          <div className="mt-1.5 flex flex-wrap gap-x-4 gap-y-1 text-xs text-neutral-500">
            {defaultModel && (
              <span className="flex items-center gap-1">
                <Cpu className="h-3 w-3" />
                {defaultModel.name}
                {defaultModel.is_multimodal === true && (
                  <span className="rounded bg-green-100 px-1 py-0.5 text-[9px] font-medium text-green-700">多模态</span>
                )}
              </span>
            )}
            <span className="flex items-center gap-1">
              <Server className="h-3 w-3" />
              {provider.base_url}
            </span>
          </div>
        </div>
        <div className="flex items-center gap-1.5 ml-4 shrink-0">
          <button
            type="button"
            onClick={onToggleExpand}
            className="rounded-lg p-1.5 text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600 transition-colors"
            title={expanded ? "收起" : "展开"}
          >
            {expanded ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
          </button>
          {!provider.is_default && (
            <button
              type="button"
              onClick={onSetDefault}
              className="rounded-lg p-1.5 text-neutral-400 hover:bg-amber-50 hover:text-amber-600 transition-colors"
              title="设为默认"
            >
              <Star className="h-4 w-4" />
            </button>
          )}
          <button
            type="button"
            onClick={onEdit}
            className="rounded-lg p-1.5 text-neutral-400 hover:bg-blue-50 hover:text-blue-600 transition-colors"
            title="编辑"
          >
            <Pencil className="h-4 w-4" />
          </button>
          <button
            type="button"
            onClick={onDelete}
            className="rounded-lg p-1.5 text-neutral-400 hover:bg-red-50 hover:text-red-600 transition-colors"
            title="删除"
          >
            <Trash2 className="h-4 w-4" />
          </button>
        </div>
      </div>

      {/* Expanded: Models and API Keys */}
      {expanded && (
        <div className="border-t border-neutral-100 px-4 pb-4 pt-3 space-y-4">
          {/* Models */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <h4 className="text-xs font-semibold text-neutral-600">模型列表</h4>
            </div>
            {provider.models.length === 0 ? (
              <p className="text-xs text-neutral-400">暂无模型</p>
            ) : (
              <div className="space-y-1.5">
                {provider.models.map((model) => (
                  <div
                    key={model.id}
                    className="flex items-center justify-between rounded-lg bg-neutral-50 px-3 py-2"
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      <span className="text-xs font-medium text-neutral-700 truncate">{model.name}</span>
                      {model.is_default && (
                        <span className="rounded bg-[#1A6BD8] px-1.5 py-0.5 text-[9px] font-medium text-white">默认</span>
                      )}
                      <span
                        className={`flex items-center gap-1 rounded-full px-1.5 py-0.5 text-[9px] font-medium ${
                          model.is_multimodal === true
                            ? "bg-green-50 text-green-700"
                            : model.is_multimodal === false
                              ? "bg-red-50 text-red-600"
                              : "bg-neutral-100 text-neutral-400"
                        }`}
                      >
                        <span
                          className={`h-1 w-1 rounded-full ${
                            model.is_multimodal === true
                              ? "bg-green-500"
                              : model.is_multimodal === false
                                ? "bg-red-400"
                                : "bg-neutral-300"
                          }`}
                        />
                        {model.is_multimodal === true
                          ? "多模态"
                          : model.is_multimodal === false
                            ? "纯文本"
                            : "未探测"}
                      </span>
                    </div>
                    <div className="flex items-center gap-1 shrink-0">
                      <button
                        type="button"
                        onClick={() => onProbeModel(provider.id, model.id)}
                        className="rounded p-1 text-neutral-400 hover:bg-blue-50 hover:text-blue-600 transition-colors"
                        title="探测多模态"
                      >
                        <Scan className="h-3 w-3" />
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* API Keys */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <h4 className="text-xs font-semibold text-neutral-600">API Keys</h4>
            </div>
            {provider.api_keys.length === 0 ? (
              <p className="text-xs text-neutral-400">暂无 API Key</p>
            ) : (
              <div className="space-y-1.5">
                {provider.api_keys.map((key) => (
                  <div
                    key={key.id}
                    className="flex items-center justify-between rounded-lg bg-neutral-50 px-3 py-2"
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      <Key className="h-3 w-3 text-neutral-400 shrink-0" />
                      <span className="text-xs font-medium text-neutral-700 truncate">{key.name || "未命名"}</span>
                      {key.is_default && (
                        <span className="rounded bg-[#1A6BD8] px-1.5 py-0.5 text-[9px] font-medium text-white">默认</span>
                      )}
                      <span className="text-[10px] text-neutral-400 truncate">
                        {key.key.slice(0, 8)}...{key.key.slice(-4)}
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

// ── Provider Form Dialog ────────────────────────────────────────────────

/** Import for LLMProviderInput type */
type LLMProviderInput = {
  id?: string;
  name: string;
  protocol: string;
  baseUrl: string;
  apiKeys: ApiKeyConfig[];
  models: ModelConfig[];
};

function ProviderFormDialog({
  provider,
  onSubmit,
  onClose,
}: {
  provider: LLMProviderConfig | null;
  onSubmit: (data: LLMProviderInput) => Promise<void>;
  onClose: () => void;
}) {
  const [name, setName] = useState(provider?.name ?? "");
  const [protocol, setProtocol] = useState<LLMProtocol>(provider?.protocol ?? "openai");
  const [baseUrl, setBaseUrl] = useState(provider?.base_url ?? PROVIDER_DEFAULTS.openai.base_url);
  const [showApiKey, setShowApiKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // API Keys state
  const [apiKeys, setApiKeys] = useState<ApiKeyConfig[]>(
    provider?.api_keys.length
      ? provider.api_keys
      : provider?.api_key
        ? [{ id: crypto.randomUUID(), name: "默认 Key", key: provider.api_key, is_default: true }]
        : []
  );

  // Models state
  const [models, setModels] = useState<ModelConfig[]>(
    provider?.models.length
      ? provider.models
      : provider?.model
        ? [{ id: crypto.randomUUID(), name: provider.model, is_default: true, is_multimodal: provider.is_multimodal, last_probe_at: provider.last_probe_at }]
        : []
  );

  const handleProtocolChange = useCallback((newProtocol: LLMProtocol) => {
    setProtocol(newProtocol);
    const defaults = PROVIDER_DEFAULTS[newProtocol];
    if (defaults) {
      const oldDefaults = PROVIDER_DEFAULTS[protocol];
      if (baseUrl === oldDefaults?.base_url || baseUrl === "") {
        setBaseUrl(defaults.base_url);
      }
      // Auto-add default model if empty
      if (models.length === 0) {
        setModels([{ id: crypto.randomUUID(), name: defaults.model, is_default: true, is_multimodal: null, last_probe_at: null }]);
      }
    }
  }, [protocol, baseUrl, models.length]);

  // API Key management
  const handleAddApiKey = useCallback(() => {
    setApiKeys((prev) => [
      ...prev,
      { id: crypto.randomUUID(), name: `Key ${prev.length + 1}`, key: "", is_default: prev.length === 0 },
    ]);
  }, []);

  const handleUpdateApiKey = useCallback((id: string, updates: Partial<ApiKeyConfig>) => {
    setApiKeys((prev) =>
      prev.map((k) => (k.id === id ? { ...k, ...updates } : k))
    );
  }, []);

  const handleRemoveApiKey = useCallback((id: string) => {
    setApiKeys((prev) => {
      const filtered = prev.filter((k) => k.id !== id);
      // If removed was default, set first as default
      if (filtered.length > 0 && !filtered.some((k) => k.is_default)) {
        filtered[0].is_default = true;
      }
      return filtered;
    });
  }, []);

  const handleSetDefaultApiKey = useCallback((id: string) => {
    setApiKeys((prev) =>
      prev.map((k) => ({ ...k, is_default: k.id === id }))
    );
  }, []);

  // Model management
  const handleAddModel = useCallback(() => {
    setModels((prev) => [
      ...prev,
      { id: crypto.randomUUID(), name: "", is_default: prev.length === 0, is_multimodal: null, last_probe_at: null },
    ]);
  }, []);

  const handleUpdateModel = useCallback((id: string, updates: Partial<ModelConfig>) => {
    setModels((prev) =>
      prev.map((m) => (m.id === id ? { ...m, ...updates } : m))
    );
  }, []);

  const handleRemoveModel = useCallback((id: string) => {
    setModels((prev) => {
      const filtered = prev.filter((m) => m.id !== id);
      // If removed was default, set first as default
      if (filtered.length > 0 && !filtered.some((m) => m.is_default)) {
        filtered[0].is_default = true;
      }
      return filtered;
    });
  }, []);

  const handleSetDefaultModel = useCallback((id: string) => {
    setModels((prev) =>
      prev.map((m) => ({ ...m, is_default: m.id === id }))
    );
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || models.length === 0) return;
    setSaving(true);
    setError(null);
    try {
      await onSubmit({
        id: provider?.id,
        name: name.trim(),
        protocol,
        baseUrl,
        apiKeys: apiKeys.filter((k) => k.key.trim()),
        models: models.filter((m) => m.name.trim()),
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const hasValidModels = models.some((m) => m.name.trim());
  const hasValidKeys = protocol === "local" || apiKeys.some((k) => k.key.trim());

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="w-full max-w-lg max-h-[90vh] rounded-xl bg-white shadow-xl flex flex-col">
        <div className="flex items-center justify-between border-b border-neutral-100 px-5 py-3 shrink-0">
          <h3 className="text-sm font-semibold text-neutral-800">
            {provider ? "编辑供应商" : "添加供应商"}
          </h3>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1 text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="flex-1 overflow-y-auto p-5 space-y-4">
          {error && (
            <div className="rounded-lg bg-red-50 border border-red-200 px-3 py-2 text-xs text-red-600 flex items-center gap-1.5 shrink-0 animate-fadeIn">
              <AlertTriangle className="h-3.5 w-3.5" />
              <span>{error}</span>
            </div>
          )}
          {/* Name */}
          <div>
            <label className="mb-1.5 block text-xs font-medium text-neutral-600">
              供应商名称 <span className="text-red-400">*</span>
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="如：OpenAI、通义千问"
              required
              className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />
          </div>

          {/* Protocol */}
          <div>
            <label className="mb-1.5 block text-xs font-medium text-neutral-600">
              协议
            </label>
            <select
              value={protocol}
              onChange={(e) => handleProtocolChange(e.target.value as LLMProtocol)}
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            >
              <option value="openai">OpenAI（Chat Completions）</option>
              <option value="anthropic">Anthropic（Messages）</option>
              <option value="local">本地模型（Ollama / llama.cpp）</option>
            </select>
          </div>

          {/* Base URL */}
          <div>
            <label className="mb-1.5 block text-xs font-medium text-neutral-600">
              Endpoint URL
            </label>
            <input
              type="text"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="https://api.openai.com/v1"
              className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />
          </div>

          {/* API Keys */}
          {protocol !== "local" && (
            <div>
              <div className="flex items-center justify-between mb-2">
                <label className="text-xs font-medium text-neutral-600">
                  API Keys <span className="text-red-400">*</span>
                </label>
                <button
                  type="button"
                  onClick={handleAddApiKey}
                  className="flex items-center gap-1 rounded-lg px-2 py-1 text-[10px] font-medium text-[#1A6BD8] hover:bg-blue-50 transition-colors"
                >
                  <Plus className="h-3 w-3" />
                  添加 Key
                </button>
              </div>
              <div className="space-y-2">
                {apiKeys.map((key) => (
                  <div key={key.id} className="flex items-center gap-2">
                    <input
                      type="text"
                      value={key.name}
                      onChange={(e) => handleUpdateApiKey(key.id, { name: e.target.value })}
                      placeholder="名称"
                      className="w-24 rounded-lg border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                    />
                    <div className="relative flex-1">
                      <input
                        type={showApiKey ? "text" : "password"}
                        value={key.key}
                        onChange={(e) => handleUpdateApiKey(key.id, { key: e.target.value })}
                        placeholder="sk-..."
                        className="w-full rounded-lg border border-neutral-200 px-2 py-1.5 pr-8 text-xs outline-none focus:border-[#1A6BD8]"
                      />
                      <button
                        type="button"
                        onClick={() => setShowApiKey((v) => !v)}
                        className="absolute right-1.5 top-1/2 -translate-y-1/2 text-neutral-400 hover:text-neutral-600"
                        tabIndex={-1}
                      >
                        {showApiKey ? <EyeOff className="h-3 w-3" /> : <Eye className="h-3 w-3" />}
                      </button>
                    </div>
                    <button
                      type="button"
                      onClick={() => handleSetDefaultApiKey(key.id)}
                      className={`rounded p-1 transition-colors ${
                        key.is_default
                          ? "text-amber-500"
                          : "text-neutral-300 hover:text-amber-500"
                      }`}
                      title="设为默认"
                    >
                      <Star className="h-3.5 w-3.5" />
                    </button>
                    <button
                      type="button"
                      onClick={() => handleRemoveApiKey(key.id)}
                      className="rounded p-1 text-neutral-300 hover:text-red-500 transition-colors"
                      title="删除"
                    >
                      <X className="h-3.5 w-3.5" />
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Models */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <label className="text-xs font-medium text-neutral-600">
                模型列表 <span className="text-red-400">*</span>
              </label>
              <button
                type="button"
                onClick={handleAddModel}
                className="flex items-center gap-1 rounded-lg px-2 py-1 text-[10px] font-medium text-[#1A6BD8] hover:bg-blue-50 transition-colors"
              >
                <Plus className="h-3 w-3" />
                添加模型
              </button>
            </div>
            <div className="space-y-2">
              {models.map((model) => (
                <div key={model.id} className="flex items-center gap-2">
                  <input
                    type="text"
                    value={model.name}
                    onChange={(e) => handleUpdateModel(model.id, { name: e.target.value })}
                    placeholder={PROVIDER_DEFAULTS[protocol]?.model || "模型名称"}
                    className="flex-1 rounded-lg border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
                  />
                  {model.is_multimodal !== null && (
                    <span
                      className={`rounded px-1.5 py-0.5 text-[9px] font-medium ${
                        model.is_multimodal
                          ? "bg-green-50 text-green-700"
                          : "bg-red-50 text-red-600"
                      }`}
                    >
                      {model.is_multimodal ? "多模态" : "纯文本"}
                    </span>
                  )}
                  <button
                    type="button"
                    onClick={() => handleSetDefaultModel(model.id)}
                    className={`rounded p-1 transition-colors ${
                      model.is_default
                        ? "text-amber-500"
                        : "text-neutral-300 hover:text-amber-500"
                    }`}
                    title="设为默认"
                  >
                    <Star className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleRemoveModel(model.id)}
                    className="rounded p-1 text-neutral-300 hover:text-red-500 transition-colors"
                    title="删除"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                </div>
              ))}
            </div>
            {models.length === 0 && (
              <p className="mt-1 text-[10px] text-red-500">至少添加一个模型</p>
            )}
          </div>

          {/* Actions */}
          <div className="flex items-center justify-end gap-3 pt-2 border-t border-neutral-100">
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg border border-neutral-200 px-4 py-2 text-sm font-medium text-neutral-600 hover:bg-neutral-50 transition-colors"
            >
              取消
            </button>
            <button
              type="submit"
              disabled={saving || !name.trim() || !hasValidModels || !hasValidKeys}
              className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
            >
              {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
              {provider ? "保存修改" : "添加"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

// ── OCR Config Card ─────────────────────────────────────────────────────

function OcrConfigCard() {
  const [ocrConfig, setOcrConfig] = useState<OcrProviderConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [showForm, setShowForm] = useState(false);
  const [provider, setProvider] = useState<string>("baidu");
  const [name, setName] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [secretKey, setSecretKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);

  useEffect(() => {
    getOcrConfig()
      .then((cfg) => {
        setOcrConfig(cfg);
        if (cfg) {
          setProvider(cfg.provider);
          setName(cfg.name);
          setApiKey(cfg.api_key);
          setSecretKey(cfg.secret_key ?? "");
        }
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const handleSave = useCallback(async () => {
    setSaving(true);
    setSaveMsg(null);
    try {
      await saveOcrConfig({
        id: ocrConfig?.id ?? crypto.randomUUID(),
        name: name.trim() || `${provider === "baidu" ? "百度" : "腾讯"} OCR`,
        provider,
        api_key: apiKey,
        secret_key: secretKey || undefined,
      });
      const updated = await getOcrConfig();
      setOcrConfig(updated);
      setShowForm(false);
      setSaveMsg("OCR 配置已保存");
      setTimeout(() => setSaveMsg(null), 3000);
    } catch (err) {
      setSaveMsg(`保存失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setSaving(false);
    }
  }, [ocrConfig, provider, name, apiKey, secretKey]);

  if (loading) {
    return (
      <section className="rounded-xl border border-neutral-200 bg-white">
        <div className="flex items-center justify-center p-8">
          <Loader2 className="h-5 w-5 animate-spin text-neutral-400" />
        </div>
      </section>
    );
  }

  return (
    <section className="rounded-xl border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-5 py-3">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-sm font-semibold text-neutral-700">OCR 文字识别</h2>
            <p className="mt-0.5 text-xs text-neutral-400">
              配置 OCR 服务，用于图片文字提取
            </p>
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
            {saveMsg && (
              <span className="text-xs text-green-600">{saveMsg}</span>
            )}
          </div>
        ) : (
          <div className="space-y-4">
            <div>
              <label className="mb-1.5 block text-xs font-medium text-neutral-600">OCR 服务商</label>
              <select
                value={provider}
                onChange={(e) => setProvider(e.target.value)}
                className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              >
                <option value="baidu">百度 OCR（推荐，中文最强）</option>
                <option value="tencent">腾讯 OCR</option>
              </select>
            </div>

            <div>
              <label className="mb-1.5 block text-xs font-medium text-neutral-600">名称</label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={provider === "baidu" ? "百度 OCR" : "腾讯 OCR"}
                className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              />
            </div>

            <div>
              <label className="mb-1.5 block text-xs font-medium text-neutral-600">API Key</label>
              <div className="relative">
                <input
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
                <label className="mb-1.5 block text-xs font-medium text-neutral-600">Secret Key</label>
                <input
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
                {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
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
              {saveMsg && (
                <span className="text-xs text-red-600">{saveMsg}</span>
              )}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
