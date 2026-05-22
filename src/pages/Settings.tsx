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
} from "lucide-react";
import {
  getLLMConfig,
  setLLMConfig,
  isLLMConfigured,
  getStats,
  ragQuery,
  type LLMConfig,
  type KnowledgeStats,
} from "../lib/tauri-commands";

const DEFAULT_CONFIG: LLMConfig = {
  api_key: "",
  base_url: "https://api.openai.com/v1",
  model: "gpt-4o",
  max_tokens: 4096,
  temperature: 0.7,
};

export default function Settings() {
  const [config, setConfig] = useState<LLMConfig>(DEFAULT_CONFIG);
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

  // Load existing config and stats
  useEffect(() => {
    Promise.all([
      getLLMConfig().catch(() => DEFAULT_CONFIG),
      isLLMConfigured().catch(() => false),
      getStats().catch(() => null),
    ]).then(([cfg, configured, s]) => {
      setConfig(cfg);
      setConfigured(configured);
      setStats(s);
      setLoading(false);
    });
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
      // Try a simple RAG query to verify the connection
      const response = await ragQuery("测试连接", undefined, []);
      if (response.llm_available) {
        setTestResult({ ok: true, msg: "连接正常" });
      } else {
        setTestResult({
          ok: false,
          msg: "LLM 不可用，请检查 API Key 和网络",
        });
      }
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
          {/* API Key */}
          <FieldRow
            icon={<Key className="h-4 w-4" />}
            label="API Key"
            hint={configured ? "已配置（密钥已安全存储）" : "未配置"}
            hintColor={configured ? "text-green-600" : "text-amber-600"}
          >
            <input
              type="password"
              value={config.api_key}
              onChange={(e) =>
                setConfig((c) => ({ ...c, api_key: e.target.value }))
              }
              placeholder="sk-..."
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />
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
