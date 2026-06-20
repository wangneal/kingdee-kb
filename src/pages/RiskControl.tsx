import { AlertTriangle, BookOpen, Brain, Shield, ShieldAlert } from "lucide-react"
import { useState } from "react"
import { useProject } from "@/contexts/ProjectContext"
import type { Tab } from "./risk-control"
import { AnalysisTab, HealthTab, ScopeTab, ScriptsTab } from "./risk-control"

export default function RiskControl() {
  const { currentProjectId, currentProject, loading: projectLoading } = useProject()
  const [tab, setTab] = useState<Tab>("scope")
  const activeProjectId = currentProjectId

  const tabs: { key: Tab; label: string; icon: typeof ShieldAlert }[] = [
    { key: "scope", label: "需求蔓延警报", icon: AlertTriangle },
    { key: "health", label: "项目健康度", icon: Shield },
    { key: "scripts", label: "话术生成器", icon: BookOpen },
    { key: "analysis", label: "AI 深度分析", icon: Brain },
  ]

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
        <div className="flex items-center gap-2">
          <ShieldAlert className="h-5 w-5 text-amber-600" />
          <h1 className="text-base font-semibold text-neutral-800">双轨风险把控舱</h1>
        </div>
        <p className="text-xs font-medium text-neutral-500">
          当前项目：{projectLoading ? "加载中" : (currentProject?.name ?? "未选择项目")}
        </p>
      </div>

      <div className="flex border-b border-neutral-200 bg-white px-6">
        {tabs.map(({ key, label, icon: Icon }) => (
          <button
            key={key}
            type="button"
            onClick={() => setTab(key)}
            className={`flex items-center gap-1.5 border-b-2 px-4 py-2.5 text-xs font-medium transition-colors ${
              tab === key
                ? "border-amber-500 text-amber-700"
                : "border-transparent text-neutral-500 hover:text-neutral-700"
            }`}
          >
            <Icon className="h-3.5 w-3.5" />
            {label}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-y-auto p-6">
        {tab === "scope" && <ScopeTab projectId={activeProjectId} />}
        {tab === "health" && <HealthTab projectId={activeProjectId} />}
        {tab === "scripts" && <ScriptsTab projectId={activeProjectId} />}
        {tab === "analysis" && <AnalysisTab projectId={activeProjectId} />}
      </div>
    </div>
  )
}
