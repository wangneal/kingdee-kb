import { Eye, EyeOff, Loader2 } from "lucide-react"
import { useCallback, useEffect, useState } from "react"
import { TOAST_AUTO_DISMISS_MS } from "@/lib/constants"
import { getOcrConfig, saveOcrConfig } from "@/lib/skill-commands"
import type { OcrProviderConfig } from "@/lib/skill-types"
import ImageTypeExclusion from "./ImageTypeExclusion"

const OCR_PROVIDER_LABEL: Record<string, string> = {
  baidu: "百度",
  tencent: "腾讯",
  mistral: "Mistral",
}

export default function OcrConfigCard() {
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
