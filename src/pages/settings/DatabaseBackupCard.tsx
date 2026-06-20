import { open } from "@tauri-apps/plugin-dialog"
import { Download, Loader2, Upload } from "lucide-react"
import { useState, useCallback } from "react"
import { exportDatabase, importDatabase } from "@/lib/tauri-commands"
import type { ImportDbResult } from "@/lib/tauri-commands"

export default function DatabaseBackupCard() {
  const [exporting, setExporting] = useState(false)
  const [importing, setImporting] = useState(false)
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null)
  const [importResult, setImportResult] = useState<ImportDbResult | null>(null)

  const handleExport = useCallback(async () => {
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
  }, [])

  const handleImport = useCallback(async () => {
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
  }, [])

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
