import { useState, useEffect, useCallback } from "react";
import {
  Settings as SettingsIcon,
  Key,
  Server,
  Cpu,
  Thermometer,
  Hash,
  Loader2,
  CheckCircle2,
  XCircle,
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
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getLLMConfig,
  setLLMConfig,
  isLLMConfigured,
  getStats,
  testLLMConnection,
  initModel,
  getModelStatus,
  getDownloadProgress,
  getEmbeddingModelConfig,
  setEmbeddingModelConfig,
  type LLMConfig,
  type KnowledgeStats,
  type EmbeddingModelConfig,
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

/** Local model presets — popular Chinese-friendly models & servers */
const LOCAL_PRESETS: { label: string; base_url: string; model: string }[] = [
  { label: "Ollama + Qwen2.5 7B", base_url: "http://localhost:11434/v1", model: "qwen2.5:7b" },
  { label: "Ollama + DeepSeek-R1 7B", base_url: "http://localhost:11434/v1", model: "deepseek-r1:7b" },
  { label: "Ollama + Yi 34B", base_url: "http://localhost:11434/v1", model: "yi:34b" },
  { label: "llama.cpp server", base_url: "http://localhost:8080/v1", model: "qwen2.5-7b-q4" },
];

const DEFAULT_CONFIG: LLMConfig = {
  provider: "openai",
  api_key: "",
  base_url: PROVIDER_DEFAULTS.openai.base_url,
  model: PROVIDER_DEFAULTS.openai.model,
  max_tokens: 4096,
  temperature: 0.7,
};

export default function Settings() {
  const [config, setConfig] = useState<LLMConfig>(DEFAULT_CONFIG);
  const [showLocalPresets, setShowLocalPresets] = useState(false);
  const [stats, setStats] = useState<KnowledgeStats | null>(null);
  const [configured, setConfigured] = useState(false);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{
    ok: boolean;
    msg: string;
  } | null>(null);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [showApiKey, setShowApiKey] = useState(false);
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
  const [keywordInput, setKeywordInput] = useState("");
  const [keywords, setKeywords] = useState<string[]>([]);

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

  // Load config, stats; poll model status (auto-load may still be async in progress)
  useEffect(() => {
    let cancelled = false;
    let retries = 0;
    const MAX_RETRIES = 60; // up to 60s wait for async model auto-load

    Promise.all([
      getLLMConfig().catch(() => DEFAULT_CONFIG),
      isLLMConfigured().catch(() => false),
      getStats().catch(() => null),
      getEmbeddingModelConfig().catch(() => ({})),
    ]).then(([cfg, configured, s, embeddingCfg]) => {
      if (cancelled) return;
      setConfig({
        provider: cfg.provider ?? "openai",
        api_key: cfg.api_key,
        base_url: cfg.base_url,
        model: cfg.model,
        max_tokens: cfg.max_tokens,
        temperature: cfg.temperature,
      });
      setConfigured(configured);
      setStats(s);
      setEmbeddingConfig(embeddingCfg);
    });

    // Poll model status until ready or timeout
    const pollModelStatus = async () => {
      if (cancelled) return;
      try {
        const status = await getModelStatus();
        if (status) {
          setModelReady(true);
          setLoading(false);
          return;
        }
      } catch { /* ignore polling errors */ }
      retries++;
      if (retries >= MAX_RETRIES) {
        setModelReady(false);
        setLoading(false);
        return;
      }
      setTimeout(pollModelStatus, 1000);
    };
    pollModelStatus();

    listSensitiveKeywords().then(setKeywords).catch(() => {});
    getAsrConfigStatus().then(setAsrConfigStatus).catch(() => {});
    return () => { cancelled = true; };
  }, []);

  const handleSave = useCallback(async () => {
    setSaving(true);
    setSaveMsg(null);
    try {
      await setLLMConfig(config);
      const nowConfigured = await isLLMConfigured();
      setConfigured(nowConfigured);
      setSaveMsg("配置已保存");
      setTimeout(() => setSaveMsg(null), 3000);
    } catch (err) {
      setSaveMsg(`保存失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setSaving(false);
    }
  }, [config]);

  const handleTest = useCallback(async () => {
    setTesting(true);
    setTestResult(null);
    try {
      // Directly test LLM API connectivity without RAG pipeline
      const msg = await testLLMConnection();
      setTestResult({ ok: true, msg });
    } catch (err) {
      setTestResult({
        ok: false,
        msg: `连接失败：${err instanceof Error ? err.message : String(err)}`,
      });
    } finally {
      setTesting(false);
    }
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

      {/* LLM Configuration Card */}
      <section className="mb-6 rounded-xl border border-neutral-200 bg-white">
        <div className="border-b border-neutral-100 px-5 py-3">
          <h2 className="text-sm font-semibold text-neutral-700">
            LLM 配置
          </h2>
          <p className="mt-0.5 text-xs text-neutral-400">
            配置大语言模型 API 连接参数
          </p>
        </div>

        <div className="space-y-4 p-5">
          {/* Provider Selection */}
          <FieldRow
            icon={<ArrowLeftRight className="h-4 w-4" />}
            label="协议"
            hint="选择 LLM 供应商协议"
          >
            <select
              value={config.provider}
              onChange={(e) => {
                const provider = e.target.value as "openai" | "anthropic";
                const defaults = PROVIDER_DEFAULTS[provider];
                setConfig((c) => ({
                  ...c,
                  provider,
                  // Auto-fill defaults only if the current values still match the previous provider defaults
                  base_url:
                    c.base_url === PROVIDER_DEFAULTS[c.provider]?.base_url
                      ? defaults.base_url
                      : c.base_url,
                  model:
                    c.model === PROVIDER_DEFAULTS[c.provider]?.model
                      ? defaults.model
                      : c.model,
                }));
              }}
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            >
              <option value="openai">OpenAI（Chat Completions）</option>
              <option value="anthropic">Anthropic（Messages）</option>
              <option value="local">本地模型（Ollama / llama.cpp）</option>
            </select>
          </FieldRow>

          {/* Local model presets */}
          {config.provider === "local" && (
            <FieldRow
              icon={<Cpu className="h-4 w-4" />}
              label="本地模型预设"
              hint="一键配置常见中文模型"
            >
              <div className="space-y-1">
                <button
                  type="button"
                  onClick={() => setShowLocalPresets((v) => !v)}
                  className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-600 text-left hover:bg-neutral-50 transition-colors"
                >
                  {showLocalPresets ? "收起" : "选择预设..."}
                </button>
                {showLocalPresets && (
                  <div className="rounded-lg border border-neutral-200 bg-white shadow-sm">
                    {LOCAL_PRESETS.map((preset) => (
                      <button
                        key={preset.label}
                        type="button"
                        onClick={() => {
                          setConfig((c) => ({
                            ...c,
                            base_url: preset.base_url,
                            model: preset.model,
                          }));
                          setShowLocalPresets(false);
                        }}
                        className="block w-full px-3 py-2 text-left text-sm text-neutral-700 hover:bg-neutral-50 first:rounded-t-lg last:rounded-b-lg transition-colors"
                      >
                        {preset.label}
                        <span className="ml-2 text-xs text-neutral-400">
                          {preset.model}
                        </span>
                      </button>
                    ))}
                  </div>
                )}
              </div>
            </FieldRow>
          )}

          {/* API Key */}
          <FieldRow
            icon={<Key className="h-4 w-4" />}
            label="API Key"
            hint={configured ? "已配置（密钥已安全存储）" : "未配置"}
            hintColor={configured ? "text-green-600" : "text-amber-600"}
          >
            <div className="relative flex items-center">
              <input
                type={showApiKey ? "text" : "password"}
                value={config.api_key}
                onChange={(e) =>
                  setConfig((c) => ({ ...c, api_key: e.target.value }))
                }
                placeholder="sk-..."
                className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 pr-10 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              />
              <button
                type="button"
                onClick={() => setShowApiKey((v) => !v)}
                className="absolute right-2 text-neutral-400 hover:text-neutral-600 transition-colors"
                tabIndex={-1}
                aria-label={showApiKey ? "隐藏 API Key" : "显示 API Key"}
              >
                {showApiKey ? (
                  <EyeOff className="h-4 w-4" />
                ) : (
                  <Eye className="h-4 w-4" />
                )}
              </button>
            </div>
          </FieldRow>

          {/* Base URL */}
          <FieldRow
            icon={<Server className="h-4 w-4" />}
            label="Endpoint"
            hint="API 基础地址"
          >
            <input
              type="text"
              value={config.base_url}
              onChange={(e) =>
                setConfig((c) => ({ ...c, base_url: e.target.value }))
              }
              placeholder="https://api.openai.com/v1"
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />
          </FieldRow>

          {/* Model */}
          <FieldRow
            icon={<Cpu className="h-4 w-4" />}
            label="Model"
            hint="使用的模型名称"
          >
            <input
              type="text"
              value={config.model}
              onChange={(e) =>
                setConfig((c) => ({ ...c, model: e.target.value }))
              }
              placeholder="gpt-4o"
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />
          </FieldRow>

          {/* Temperature */}
          <FieldRow
            icon={<Thermometer className="h-4 w-4" />}
            label="Temperature"
            hint={`${config.temperature.toFixed(1)}（越低越确定）`}
          >
            <input
              type="range"
              min="0"
              max="2"
              step="0.1"
              value={config.temperature}
              onChange={(e) =>
                setConfig((c) => ({
                  ...c,
                  temperature: parseFloat(e.target.value),
                }))
              }
              className="w-full accent-[#1A6BD8]"
            />
          </FieldRow>

          {/* Max Tokens */}
          <FieldRow
            icon={<Hash className="h-4 w-4" />}
            label="Max Tokens"
            hint="回答最大 token 数"
          >
            <input
              type="number"
              min="256"
              max="128000"
              step="256"
              value={config.max_tokens}
              onChange={(e) =>
                setConfig((c) => ({
                  ...c,
                  max_tokens: parseInt(e.target.value, 10) || 4096,
                }))
              }
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />
          </FieldRow>

          {/* Action buttons */}
          <div className="flex items-center gap-3 pt-2">
            <button
              type="button"
              onClick={handleSave}
              disabled={saving}
              className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
            >
              {saving ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Save className="h-4 w-4" />
              )}
              保存配置
            </button>

            <button
              type="button"
              onClick={handleTest}
              disabled={testing || !configured}
              className="flex items-center gap-1.5 rounded-lg border border-neutral-200 px-4 py-2 text-sm font-medium text-neutral-600 hover:bg-neutral-50 disabled:opacity-50 transition-colors"
            >
              {testing ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <RefreshCw className="h-4 w-4" />
              )}
              测试连接
            </button>

            {saveMsg && (
              <span className="text-xs text-neutral-500">{saveMsg}</span>
            )}
          </div>

          {/* Test result */}
          {testResult && (
            <div
              className={`flex items-center gap-2 rounded-lg px-3 py-2 text-sm ${
                testResult.ok
                  ? "bg-green-50 text-green-700"
                  : "bg-red-50 text-red-700"
              }`}
            >
              {testResult.ok ? (
                <CheckCircle2 className="h-4 w-4" />
              ) : (
                <XCircle className="h-4 w-4" />
              )}
              {testResult.msg}
            </div>
          )}

          {/* Not configured warning */}
          {!configured && (
            <div className="flex items-center gap-2 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700">
              <AlertTriangle className="h-3.5 w-3.5" />
              未配置 API Key，AI 对话功能将不可用
            </div>
          )}
        </div>
      </section>

      {/* Embedding Model Card */}
      <section className="mb-6 rounded-xl border border-neutral-200 bg-white">
        <div className="border-b border-neutral-100 px-5 py-3">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-sm font-semibold text-neutral-700">
                Embedding 模型
              </h2>
              <p className="mt-0.5 text-xs text-neutral-400">
                向量嵌入模型，用于语义搜索（首次初始化需下载模型文件）
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
        </div>
      </section>

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
      <section className="mt-6 rounded-xl border border-neutral-200 bg-white">
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
                } catch (e) { alert(String(e)); }
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
                    } catch (e) { alert(String(e)); }
                  }} className="text-amber-400 hover:text-red-500">&times;</button>
                </span>
              ))}
            </div>
          )}
        </div>
      </section>

      {/* ASR Config Card */}
      <section className="mt-6 rounded-xl border border-neutral-200 bg-white">
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

      {/* Database Backup Card */}
      <DatabaseBackupCard />
    </div>
  );
}

// ── Helper Components ─────────────────────────────────────────────────────

function FieldRow({
  icon,
  label,
  hint,
  hintColor = "text-neutral-400",
  children,
}: {
  icon: React.ReactNode;
  label: string;
  hint?: string;
  hintColor?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div className="mb-1.5 flex items-center gap-2">
        <span className="text-neutral-400">{icon}</span>
        <span className="text-sm font-medium text-neutral-700">{label}</span>
        {hint && (
          <span className={`text-xs ${hintColor}`}>{hint}</span>
        )}
      </div>
      {children}
    </div>
  );
}

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
