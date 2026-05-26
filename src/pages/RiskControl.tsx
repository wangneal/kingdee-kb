import { useState, useEffect, useRef, useCallback } from "react";
import { Shield, AlertTriangle, BookOpen, Plus, Trash2, Send, Loader2, AlertCircle, CheckCircle, ShieldAlert, Brain, Download, Upload, Search, FileUp, ChevronDown } from "lucide-react";
import {
  type ContractScopeItem,
  type ScopeCreepResult,
  type ProjectHealthScore,
  type DefenseScriptResult,
  type RiskProject,
  type CandidateScopeItem,
  type ImportDbResult,
  listScopeItems,
  addScopeItem,
  deleteScopeItem,
  checkScopeCreep,
  getProjectHealth,
  generateRiskReport,
  generateDefenseScript,
  analyzeFitGap,
  reactChat,
  listenReActEvents,
  exportReport,
  listRiskProjects,
  createRiskProject,
  deleteRiskProject,
  extractScopeFromDocument,
  confirmScopeItems,
  exportDatabase,
  importDatabase,
  desensitizeText,
} from "../lib/tauri-commands";

type Tab = "scope" | "health" | "scripts" | "analysis" | "backup";

export default function RiskControl() {
  const [tab, setTab] = useState<Tab>("scope");
  const [projects, setProjects] = useState<RiskProject[]>([]);
  const [selectedProjectId, setSelectedProjectId] = useState<number | null>(null);
  const [showNewProject, setShowNewProject] = useState(false);
  const [newProjectName, setNewProjectName] = useState("");
  const [newClientName, setNewClientName] = useState("");

  const tabs: { key: Tab; label: string; icon: typeof Shield }[] = [
    { key: "scope", label: "需求蔓延警报", icon: AlertTriangle },
    { key: "health", label: "项目健康度", icon: Shield },
    { key: "scripts", label: "防身话术库", icon: BookOpen },
    { key: "analysis", label: "AI 深度分析", icon: Brain },
    { key: "backup", label: "备份恢复", icon: Download },
  ];

  const refreshProjects = useCallback(async () => {
    try {
      const list = await listRiskProjects();
      setProjects(list);
      if (list.length > 0) {
        setSelectedProjectId(prev => prev ?? list[0].id);
      }
    } catch (e) { console.error("加载项目列表失败:", e); }
  }, []);

  useEffect(() => { refreshProjects(); }, [refreshProjects]);

  const handleCreateProject = async () => {
    if (!newProjectName.trim()) return;
    try {
      let safeClientName = newClientName.trim();
      if (safeClientName) {
        const result = await desensitizeText(safeClientName);
        safeClientName = result.safe_text;
      }
      const id = await createRiskProject(newProjectName.trim(), safeClientName || undefined);
      setNewProjectName("");
      setNewClientName("");
      setShowNewProject(false);
      setSelectedProjectId(id);
      await refreshProjects();
    } catch (e) { alert("创建项目失败: " + String(e)); }
  };

  const handleDeleteProject = async () => {
    if (selectedProjectId === null) return;
    if (!confirm("确定删除当前项目？此操作不可撤销。")) return;
    try {
      await deleteRiskProject(selectedProjectId);
      setSelectedProjectId(null);
      await refreshProjects();
    } catch (e) { alert("删除项目失败: " + String(e)); }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
        <div className="flex items-center gap-2">
          <ShieldAlert className="h-5 w-5 text-amber-600" />
          <h1 className="text-base font-semibold text-neutral-800">双轨风险把控舱</h1>
        </div>
        <div className="flex items-center gap-2">
          <div className="relative">
            <select
              value={selectedProjectId ?? ""}
              onChange={(e) => setSelectedProjectId(e.target.value ? Number(e.target.value) : null)}
              className="appearance-none rounded-lg border border-neutral-200 bg-white py-1.5 pl-3 pr-8 text-xs font-medium text-neutral-700 outline-none focus:border-amber-500"
            >
              {projects.length === 0 && <option value="">暂无项目</option>}
              {projects.map(p => <option key={p.id} value={p.id}>{p.name}{p.client_name ? ` — ${p.client_name}` : ""}</option>)}
            </select>
            <ChevronDown className="pointer-events-none absolute right-2 top-1/2 h-3 w-3 -translate-y-1/2 text-neutral-400" />
          </div>
          <button type="button" onClick={() => setShowNewProject(true)}
            className="flex items-center gap-1 rounded-lg bg-amber-600 px-2.5 py-1.5 text-xs font-medium text-white hover:bg-amber-700 transition-colors">
            <Plus className="h-3 w-3" />新建
          </button>
          {selectedProjectId !== null && (
            <button type="button" onClick={handleDeleteProject}
              className="flex items-center gap-1 rounded-lg border border-red-200 px-2.5 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 transition-colors">
              <Trash2 className="h-3 w-3" />删除
            </button>
          )}
        </div>
      </div>

      {/* 新建项目对话框 */}
      {showNewProject && (
        <div className="border-b border-neutral-200 bg-amber-50 px-6 py-3">
          <div className="flex items-start gap-2">
            <div className="flex flex-col gap-1.5">
              <input value={newProjectName} onChange={(e) => setNewProjectName(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") handleCreateProject(); }}
                placeholder="输入项目名称..."
                className="w-64 rounded-lg border border-amber-200 px-3 py-1.5 text-xs outline-none focus:border-amber-500" />
              <input value={newClientName} onChange={(e) => setNewClientName(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") handleCreateProject(); }}
                placeholder="输入客户名（自动脱敏）..."
                className="w-64 rounded-lg border border-amber-200 px-3 py-1.5 text-xs outline-none focus:border-amber-500" />
            </div>
            <div className="flex items-center gap-2 pt-0.5">
              <button type="button" onClick={handleCreateProject}
                className="rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700">确认</button>
              <button type="button" onClick={() => { setShowNewProject(false); setNewProjectName(""); setNewClientName(""); }}
                className="rounded-lg border border-neutral-200 px-3 py-1.5 text-xs text-neutral-500 hover:bg-neutral-100">取消</button>
            </div>
          </div>
          <p className="mt-1.5 text-[10px] text-neutral-400">客户名将自动脱敏（敏感词替换为占位符），可在脱敏管理中添加敏感词</p>
        </div>
      )}

      <div className="flex border-b border-neutral-200 bg-white px-6">
        {tabs.map(({ key, label, icon: Icon }) => (
          <button key={key} type="button" onClick={() => setTab(key)}
            className={`flex items-center gap-1.5 border-b-2 px-4 py-2.5 text-xs font-medium transition-colors ${
              tab === key ? "border-amber-500 text-amber-700" : "border-transparent text-neutral-500 hover:text-neutral-700"
            }`}
          ><Icon className="h-3.5 w-3.5" />{label}</button>
        ))}
      </div>
      <div className="flex-1 overflow-y-auto p-6">
        {tab === "scope" && <ScopeTab projectId={selectedProjectId} />}
        {tab === "health" && <HealthTab projectId={selectedProjectId} />}
        {tab === "scripts" && <ScriptsTab projectId={selectedProjectId} />}
        {tab === "analysis" && <AnalysisTab projectId={selectedProjectId} />}
        {tab === "backup" && <BackupTab />}
      </div>
    </div>
  );
}

function ScopeTab({ projectId }: { projectId: number | null }) {
  const [items, setItems] = useState<ContractScopeItem[]>([]);
  const [checkResult, setCheckResult] = useState<ScopeCreepResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [newReq, setNewReq] = useState("");
  const [newCat, setNewCat] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const [showExtract, setShowExtract] = useState(false);
  const [extractDocId, setExtractDocId] = useState("");
  const [extractLoading, setExtractLoading] = useState(false);
  const [candidates, setCandidates] = useState<CandidateScopeItem[]>([]);
  const [confirmLoading, setConfirmLoading] = useState(false);

  const refresh = useCallback(async () => {
    if (projectId === null) return;
    try {
      const list = await listScopeItems(projectId);
      setItems(list);
    } catch (e) { console.error("加载范围列表失败:", e); }
  }, [projectId]);

  useEffect(() => { refresh(); }, [refresh]);

  const handleCheck = async () => {
    if (!newReq.trim() || projectId === null) return;
    setLoading(true);
    try {
      const r = await checkScopeCreep(projectId, newReq.trim());
      setCheckResult(r);
    } catch (e) { alert(String(e)); }
    setLoading(false);
  };

  const handleAdd = async () => {
    if (!newCat.trim() || !newDesc.trim() || projectId === null) return;
    await addScopeItem(projectId, newCat.trim(), newDesc.trim(), true, "");
    setNewCat(""); setNewDesc("");
    refresh();
  };

  const handleExtract = async () => {
    if (!extractDocId.trim() || projectId === null) return;
    setExtractLoading(true);
    setCandidates([]);
    try {
      const result = await extractScopeFromDocument(projectId, Number(extractDocId));
      setCandidates(result);
    } catch (e) { alert("提取失败: " + String(e)); }
    setExtractLoading(false);
  };

  const handleConfirmImport = async () => {
    if (projectId === null || candidates.length === 0) return;
    setConfirmLoading(true);
    try {
      await confirmScopeItems(projectId, candidates);
      setCandidates([]);
      setShowExtract(false);
      setExtractDocId("");
      await refresh();
    } catch (e) { alert("导入失败: " + String(e)); }
    setConfirmLoading(false);
  };

  if (projectId === null) {
    return (
      <div className="flex flex-col items-center justify-center pt-20">
        <Search className="mb-3 h-10 w-10 text-neutral-300" />
        <p className="text-sm text-neutral-500">请先在顶部选择或创建一个项目</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      {/* 范围检查 */}
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <h2 className="mb-3 text-sm font-semibold text-neutral-700">检查新需求是否超范围</h2>
        <div className="flex gap-2">
          <input value={newReq} onChange={(e) => setNewReq(e.target.value)} placeholder="输入新需求描述..." className="flex-1 rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500" />
          <button type="button" onClick={handleCheck} disabled={loading || !newReq.trim()}
            className="flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
          >{loading ? <Loader2 className="h-3 w-3 animate-spin" /> : <Send className="h-3 w-3" />}检查</button>
        </div>
        {checkResult && (
          <div className={`mt-3 rounded-lg border p-3 ${
            checkResult.risk_level === "red" ? "border-red-200 bg-red-50" :
            checkResult.risk_level === "yellow" ? "border-yellow-200 bg-yellow-50" : "border-green-200 bg-green-50"
          }`}>
            <div className="mb-1 flex items-center gap-2">
              {checkResult.risk_level === "red" ? <AlertCircle className="h-4 w-4 text-red-600" /> :
               checkResult.risk_level === "yellow" ? <AlertTriangle className="h-4 w-4 text-yellow-600" /> :
               <CheckCircle className="h-4 w-4 text-green-600" />}
              <span className={`text-xs font-semibold ${
                checkResult.risk_level === "red" ? "text-red-700" :
                checkResult.risk_level === "yellow" ? "text-yellow-700" : "text-green-700"
              }`}>{checkResult.risk_label}</span>
            </div>
            <p className="text-xs text-neutral-600">{checkResult.explanation}</p>
            <p className="mt-1 text-xs font-medium text-neutral-700">建议：{checkResult.suggestion}</p>
          </div>
        )}
      </div>

      {/* 合同范围列表 */}
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-neutral-700">合同范围定义</h2>
          <div className="flex gap-2">
            <input value={newCat} onChange={(e) => setNewCat(e.target.value)} placeholder="分类" className="w-24 rounded-lg border border-neutral-200 px-2 py-1 text-xs outline-none" />
            <input value={newDesc} onChange={(e) => setNewDesc(e.target.value)} placeholder="范围描述" className="w-40 rounded-lg border border-neutral-200 px-2 py-1 text-xs outline-none" />
            <button type="button" onClick={handleAdd} className="flex items-center gap-1 rounded bg-amber-600 px-2 py-1 text-xs text-white hover:bg-amber-700"><Plus className="h-3 w-3" />添加</button>
            <button type="button" onClick={() => setShowExtract(!showExtract)}
              className="flex items-center gap-1 rounded border border-amber-200 bg-amber-50 px-2 py-1 text-xs text-amber-700 hover:bg-amber-100">
              <FileUp className="h-3 w-3" />从文档提取
            </button>
          </div>
        </div>

        {/* 文档提取对话框 */}
        {showExtract && (
          <div className="mb-4 rounded-lg border border-amber-200 bg-amber-50 p-3">
            <div className="flex items-center gap-2">
              <input value={extractDocId} onChange={(e) => setExtractDocId(e.target.value)}
                placeholder="输入知识库文档 ID..." type="number"
                className="w-48 rounded-lg border border-amber-200 px-2 py-1 text-xs outline-none focus:border-amber-500" />
              <button type="button" onClick={handleExtract} disabled={extractLoading || !extractDocId.trim()}
                className="flex items-center gap-1 rounded bg-amber-600 px-3 py-1 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50">
                {extractLoading ? <Loader2 className="h-3 w-3 animate-spin" /> : <Search className="h-3 w-3" />}提取
              </button>
            </div>
            {candidates.length > 0 && (
              <div className="mt-3 space-y-2">
                <p className="text-xs font-medium text-amber-800">提取到 {candidates.length} 个候选范围项：</p>
                {candidates.map((c) => (
                  <div key={`${c.category}-${c.description}-${c.confidence}`} className="flex items-center justify-between rounded border border-amber-100 bg-white px-3 py-2">
                    <div className="flex items-center gap-2">
                      <span className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                        c.is_in_scope ? "bg-green-100 text-green-700" : "bg-red-100 text-red-700"
                      }`}>{c.is_in_scope ? "范围内" : "排除"}</span>
                      <span className="text-xs font-medium text-neutral-600">{c.category}</span>
                      <span className="text-xs text-neutral-500">{c.description}</span>
                    </div>
                    <span className="rounded bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">
                      置信度 {(c.confidence * 100).toFixed(0)}%
                    </span>
                  </div>
                ))}
                <button type="button" onClick={handleConfirmImport} disabled={confirmLoading}
                  className="flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50">
                  {confirmLoading ? <Loader2 className="h-3 w-3 animate-spin" /> : <CheckCircle className="h-3 w-3" />}确认导入
                </button>
              </div>
            )}
          </div>
        )}

        {items.length === 0 ? <p className="text-xs text-neutral-400">暂无范围定义</p> : (
          <div className="space-y-1">
            {items.map((item) => (
              <div key={item.id} className="flex items-center justify-between rounded border border-neutral-100 px-3 py-2">
                <div className="flex items-center gap-2">
                  <span className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                    item.is_in_scope ? "bg-green-100 text-green-700" : "bg-red-100 text-red-700"
                  }`}>{item.is_in_scope ? "范围内" : "排除"}</span>
                  <span className="text-xs font-medium text-neutral-600">{item.category}</span>
                  <span className="text-xs text-neutral-500">{item.description}</span>
                </div>
                <button type="button" onClick={async () => { await deleteScopeItem(item.id); refresh(); }}
                  className="text-neutral-300 hover:text-red-500"><Trash2 className="h-3 w-3" /></button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function HealthTab({ projectId }: { projectId: number | null }) {
  const [health, setHealth] = useState<ProjectHealthScore | null>(null);
  const [loading, setLoading] = useState(false);
  const [aiLoading, setAiLoading] = useState(false);
  const [aiReport, setAiReport] = useState("");
  const [fitGapInput, setFitGapInput] = useState("");
  const [fitGapResult, setFitGapResult] = useState("");
  const [fitGapLoading, setFitGapLoading] = useState(false);

  const refresh = useCallback(async () => {
    if (projectId === null) return;
    setLoading(true);
    try { setHealth(await getProjectHealth(projectId)); } catch (e) { alert(String(e)); }
    setLoading(false);
  }, [projectId]);

  useEffect(() => { refresh(); }, [refresh]);

  const handleAIAnalysis = useCallback(async () => {
    if (!health || aiLoading || projectId === null) return;
    setAiLoading(true);
    setAiReport("");
    const context = `项目健康评分: ${health.overall_score}/100, 风险等级: ${health.risk_level}\n` +
      health.dimensions.map(d => `- ${d.name}: ${d.score}/100 (${d.detail})`).join("\n");
    try {
      const report = await generateRiskReport(projectId, context);
      setAiReport(report);
    } catch (e) { alert("分析失败: " + String(e)); }
    setAiLoading(false);
  }, [health, aiLoading, projectId]);

  const colorClass = (level: string) =>
    level === "critical" ? "text-red-600" : level === "high" ? "text-orange-600" :
    level === "medium" ? "text-yellow-600" : "text-green-600";

  const bgClass = (level: string) =>
    level === "critical" ? "bg-red-50 border-red-200" : level === "high" ? "bg-orange-50 border-orange-200" :
    level === "medium" ? "bg-yellow-50 border-yellow-200" : "bg-green-50 border-green-200";

  if (projectId === null) {
    return (
      <div className="flex flex-col items-center justify-center pt-20">
        <Shield className="mb-3 h-10 w-10 text-neutral-300" />
        <p className="text-sm text-neutral-500">请先在顶部选择或创建一个项目</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-3xl">
      {loading ? <div className="flex justify-center pt-10"><Loader2 className="h-5 w-5 animate-spin text-neutral-400" /></div> :
      health ? (
        <div className="space-y-4">
          {/* 总评分 */}
          <div className={`rounded-lg border p-6 ${bgClass(health.risk_level)}`}>
            <div className="mb-2 flex items-center gap-2">
              <Shield className={`h-5 w-5 ${colorClass(health.risk_level)}`} />
              <span className={`text-lg font-bold ${colorClass(health.risk_level)}`}>
                {health.overall_score.toFixed(0)}/100
              </span>
              <span className={`rounded px-2 py-0.5 text-xs font-medium ${
                health.risk_level === "critical" ? "bg-red-100 text-red-700" :
                health.risk_level === "high" ? "bg-orange-100 text-orange-700" :
                health.risk_level === "medium" ? "bg-yellow-100 text-yellow-700" : "bg-green-100 text-green-700"
              }`}>{health.risk_level === "critical" ? "危急" : health.risk_level === "high" ? "高风险" :
                 health.risk_level === "medium" ? "关注" : "健康"}</span>
            </div>
            <p className="text-xs text-neutral-600">{health.trend}</p>
            {health.alert_count > 0 && (
              <p className="mt-1 text-xs font-medium text-red-600">⚠ {health.alert_count} 项指标需要关注</p>
            )}
          </div>

          {/* 各维度 */}
          <div className="grid gap-3 sm:grid-cols-2">
            {health.dimensions.map((d) => (
              <div key={d.name} className="rounded-lg border border-neutral-200 bg-white p-4">
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-xs font-medium text-neutral-700">{d.name}</span>
                  <span className={`text-xs font-bold ${d.score >= 50 ? "text-red-600" : d.score >= 30 ? "text-yellow-600" : "text-green-600"}`}>
                    {d.score.toFixed(0)}/100
                  </span>
                </div>
                <div className="h-2 rounded-full bg-neutral-100">
                  <div className={`h-full rounded-full transition-all ${
                    d.score >= 50 ? "bg-red-500" : d.score >= 30 ? "bg-yellow-500" : "bg-green-500"
                  }`} style={{ width: `${d.score}%` }} />
                </div>
                <p className="mt-1 text-[10px] text-neutral-400">{d.detail}</p>
              </div>
            ))}
          </div>

          <button type="button" onClick={handleAIAnalysis} disabled={aiLoading}
            className="flex w-full items-center justify-center gap-1.5 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50 transition-colors">
            {aiLoading ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Brain className="h-3.5 w-3.5" />}
            {aiLoading ? "分析中..." : "AI 风险分析"}
          </button>
          {aiReport && (
            <div className="mt-2 space-y-2">
              <div className="rounded-lg border border-amber-100 bg-amber-50 p-3 text-xs leading-relaxed text-neutral-700 whitespace-pre-wrap">{aiReport}</div>
              <button type="button" onClick={async () => {
                try {
                  const { save } = await import("@tauri-apps/plugin-dialog");
                  const path = await save({ filters: [{ name: "Markdown", extensions: ["md"] }] });
                  if (path) await exportReport(aiReport, path);
                } catch(e) { alert("导出失败: " + String(e)); }
              }} className="flex items-center gap-1 rounded bg-neutral-100 px-2 py-1 text-[10px] text-neutral-500 hover:bg-neutral-200">
                <Download className="h-3 w-3" />导出报告
              </button>
            </div>
          )}

          </div>
        ) : <p className="text-center text-sm text-neutral-400">加载失败</p>}

      {/* Fit-Gap 分析 */}
      <div className="mt-6 rounded-lg border border-neutral-200 bg-white p-4">
        <h2 className="mb-3 text-sm font-semibold text-neutral-700">Fit-Gap 差异分析</h2>
        <textarea value={fitGapInput} onChange={(e) => setFitGapInput(e.target.value)} rows={3}
          placeholder="输入需求列表，每行一条，如：&#10;1. 总账模块支持多币种&#10;2. 需要定制化报表引擎"
          className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500" />
        <button type="button" onClick={async () => {
          if (!fitGapInput.trim()) return;
          setFitGapLoading(true);
          try { setFitGapResult(await analyzeFitGap(fitGapInput)); }
          catch (e) { setFitGapResult("分析失败: " + String(e)); }
          setFitGapLoading(false);
        }} disabled={fitGapLoading || !fitGapInput.trim()}
          className="mt-2 flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50">
          {fitGapLoading ? <Loader2 className="h-3 w-3 animate-spin" /> : null}
          {fitGapLoading ? "分析中..." : "开始分析"}
        </button>
        {fitGapResult && (
          <div className="mt-3 space-y-2">
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-3">
              <pre className="text-xs leading-relaxed text-neutral-700 whitespace-pre-wrap font-sans">{fitGapResult}</pre>
            </div>
            <div className="flex justify-end">
              <button type="button" onClick={async () => {
                try {
                  const { save } = await import("@tauri-apps/plugin-dialog");
                  const path = await save({ filters: [{ name: "Markdown", extensions: ["md"] }] });
                  if (path) await exportReport(fitGapResult, path);
                } catch(e) { alert("导出失败: " + String(e)); }
              }} className="flex items-center gap-1 rounded bg-neutral-100 px-2 py-1 text-[10px] text-neutral-500 hover:bg-neutral-200">
                <Download className="h-3 w-3" />导出分析
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function ScriptsTab({ projectId }: { projectId: number | null }) {
  void projectId;
  const [scenario, setScenario] = useState("");
  const [context, setContext] = useState("");
  const [tone, setTone] = useState("push_back");
  const [result, setResult] = useState<DefenseScriptResult | null>(null);
  const [loading, setLoading] = useState(false);

  const handleGenerate = async () => {
    if (!scenario.trim()) return;
    setLoading(true);
    try {
      const r = await generateDefenseScript({ scenario: scenario.trim(), context: context.trim(), tone });
      setResult(r);
    } catch (e) { alert(String(e)); }
    setLoading(false);
  };

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <h2 className="mb-3 text-sm font-semibold text-neutral-700">生成防身话术</h2>
        <div className="space-y-3">
          <div>
            <label className="mb-1 block text-[10px] font-medium text-neutral-500">场景描述</label>
            <textarea value={scenario} onChange={(e) => setScenario(e.target.value)} rows={2} placeholder="如：客户要求在合同范围外增加一个全新的报表模块" className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500" />
          </div>
          <div>
            <label className="mb-1 block text-[10px] font-medium text-neutral-500">上下文（可选）</label>
            <textarea value={context} onChange={(e) => setContext(e.target.value)} rows={2} placeholder="补充背景信息..." className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500" />
          </div>
          <div className="flex items-center gap-3">
            <label className="text-[10px] font-medium text-neutral-500">沟通基调</label>
            <select value={tone} onChange={(e) => setTone(e.target.value)} className="rounded-lg border border-neutral-200 px-2 py-1 text-xs outline-none">
              <option value="push_back">委婉拒绝</option>
              <option value="guide">引导说服</option>
              <option value="escalate">升级讨论</option>
            </select>
            <button type="button" onClick={handleGenerate} disabled={loading || !scenario.trim()}
              className="ml-auto flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
            >{loading ? <Loader2 className="h-3 w-3 animate-spin" /> : <Send className="h-3 w-3" />}生成话术</button>
          </div>
        </div>
      </div>

      {result && (
        <div className="space-y-3">
          <p className="text-xs font-semibold text-neutral-700">{result.scenario_label}</p>
          {result.scripts.map((s, i) => (
            <div key={i} className="rounded-lg border border-amber-100 bg-amber-50 p-4">
              <span className="mb-1 inline-block rounded bg-amber-200 px-2 py-0.5 text-[10px] font-medium text-amber-800">{s.phase}</span>
              <p className="text-sm leading-relaxed text-neutral-700">{s.content}</p>
              <p className="mt-1 text-[10px] italic text-amber-700">💡 {s.tip}</p>
            </div>
          ))}
          {result && (
            <div className="flex justify-end pt-1">
              <button type="button" onClick={async () => {
                const md = `# 防身话术\n\n## ${result.scenario_label}\n\n${result.scripts.map(s => `### ${s.phase}\n\n${s.content}\n\n> 💡 ${s.tip}\n`).join("\n")}`;
                try {
                  const { save } = await import("@tauri-apps/plugin-dialog");
                  const path = await save({ filters: [{ name: "Markdown", extensions: ["md"] }] });
                  if (path) await exportReport(md, path);
                } catch(e) { alert("导出失败: " + String(e)); }
              }} className="flex items-center gap-1 rounded bg-neutral-100 px-2 py-1 text-[10px] text-neutral-500 hover:bg-neutral-200">
                <Download className="h-3 w-3" />导出话术
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ─── Backup Tab: 整库备份恢复 ────────────────────────────────────────────

function BackupTab() {
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [importResult, setImportResult] = useState<ImportDbResult | null>(null);

  const handleExport = async () => {
    setExporting(true);
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({
        defaultPath: "kingdee-backup.db",
        filters: [{ name: "数据库备份", extensions: ["db"] }],
      });
      if (path) {
        await exportDatabase(path);
        alert("备份导出成功！");
      }
    } catch (e) { alert("导出失败: " + String(e)); }
    setExporting(false);
  };

  const handleImport = async () => {
    setImporting(true);
    setImportResult(null);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [{ name: "数据库备份", extensions: ["db"] }],
      });
      if (selected) {
        const result = await importDatabase(selected as string);
        setImportResult(result);
      }
    } catch (e) { alert("导入失败: " + String(e)); }
    setImporting(false);
  };

  const formatBytes = (bytes: number) => {
    if (bytes < 1024) return bytes + " B";
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
    return (bytes / (1024 * 1024)).toFixed(1) + " MB";
  };

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <h2 className="mb-4 text-sm font-semibold text-neutral-700">整库备份与恢复</h2>
        <div className="flex gap-4">
          <div className="flex-1 rounded-lg border border-neutral-100 p-4">
            <div className="mb-3 flex items-center gap-2">
              <Download className="h-4 w-4 text-amber-600" />
              <span className="text-xs font-medium text-neutral-700">导出备份</span>
            </div>
            <p className="mb-3 text-[10px] text-neutral-400">将当前数据库完整导出为 .db 文件，可用于迁移或灾难恢复。</p>
            <button type="button" onClick={handleExport} disabled={exporting}
              className="flex w-full items-center justify-center gap-1 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50 transition-colors">
              {exporting ? <Loader2 className="h-3 w-3 animate-spin" /> : <Download className="h-3 w-3" />}
              {exporting ? "导出中..." : "导出整库备份"}
            </button>
          </div>
          <div className="flex-1 rounded-lg border border-neutral-100 p-4">
            <div className="mb-3 flex items-center gap-2">
              <Upload className="h-4 w-4 text-amber-600" />
              <span className="text-xs font-medium text-neutral-700">导入备份</span>
            </div>
            <p className="mb-3 text-[10px] text-neutral-400">从 .db 备份文件恢复数据库。注意：此操作将覆盖当前数据。</p>
            <button type="button" onClick={handleImport} disabled={importing}
              className="flex w-full items-center justify-center gap-1 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs font-medium text-amber-700 hover:bg-amber-100 disabled:opacity-50 transition-colors">
              {importing ? <Loader2 className="h-3 w-3 animate-spin" /> : <Upload className="h-3 w-3" />}
              {importing ? "导入中..." : "导入整库备份"}
            </button>
          </div>
        </div>
      </div>

      {importResult && (
        <div className="rounded-lg border border-green-200 bg-green-50 p-4">
          <div className="mb-2 flex items-center gap-2">
            <CheckCircle className="h-4 w-4 text-green-600" />
            <span className="text-xs font-semibold text-green-700">导入成功</span>
          </div>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <div className="rounded border border-green-100 bg-white p-2 text-center">
              <p className="text-[10px] text-neutral-400">文件大小</p>
              <p className="text-xs font-bold text-neutral-700">{formatBytes(importResult.db_size_bytes)}</p>
            </div>
            <div className="rounded border border-green-100 bg-white p-2 text-center">
              <p className="text-[10px] text-neutral-400">风控项目</p>
              <p className="text-xs font-bold text-neutral-700">{importResult.risk_project_count}</p>
            </div>
            <div className="rounded border border-green-100 bg-white p-2 text-center">
              <p className="text-[10px] text-neutral-400">范围条目</p>
              <p className="text-xs font-bold text-neutral-700">{importResult.scope_item_count}</p>
            </div>
            <div className="rounded border border-green-100 bg-white p-2 text-center">
              <p className="text-[10px] text-neutral-400">健康指标</p>
              <p className="text-xs font-bold text-neutral-700">{importResult.metric_count}</p>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ─── Analysis Tab: ReAct 深度分析对话 ──────────────────────────────────

interface ChatMsg {
  id: string;
  role: "user" | "assistant";
  content: string;
  loading?: boolean;
}

function AnalysisTab({ projectId }: { projectId: number | null }) {
  void projectId;
  const [messages, setMessages] = useState<ChatMsg[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const msgRef = useRef("");
  const sessionRef = useRef<string | null>(null);
  const chatEndRef = useRef<HTMLDivElement>(null);

  // Listen for ReAct streaming events
  useEffect(() => {
    let cancelled = false;
    listenReActEvents((event) => {
      // Support both snake_case and camelCase (Tauri v2 may convert)
      const eventSessionId = event.session_id || (event as any).sessionId;
      if (eventSessionId !== sessionRef.current) return;
      if (event.type === "text_delta") {
        msgRef.current += event.content;
        setMessages((prev) => {
          const next = [...prev];
          const last = next[next.length - 1];
          if (last && last.role === "assistant") {
            next[next.length - 1] = { ...last, content: msgRef.current };
          }
          return next;
        });
      }
      if (event.type === "done" || event.type === "error") {
        setLoading(false);
        setMessages((prev) => {
          const next = [...prev];
          const last = next[next.length - 1];
          if (last && last.role === "assistant") {
            next[next.length - 1] = { ...last, loading: false };
          }
          return next;
        });
        sessionRef.current = null;
      }
    }).then((fn) => {
      if (cancelled) { fn(); return; }
    });
    return () => { cancelled = true; };
  }, []);

  // Auto-scroll
  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const handleSend = useCallback(async () => {
    const text = input.trim();
    if (!text || loading) return;
    setInput("");

    const newMsg: ChatMsg = { id: `m${Date.now()}`, role: "user", content: text };
    const placeholder: ChatMsg = { id: `m${Date.now() + 1}`, role: "assistant", content: "", loading: true };
    setMessages((prev) => [...prev, newMsg, placeholder]);
    msgRef.current = "";

    setLoading(true);
    try {
      // Generate session ID first before calling reactChat
      const sid = `risk_${Date.now()}`;
      sessionRef.current = sid;
      await reactChat(
        text,
        "你是在 KingdeeKB 双轨风险把控舱中的风控专家。分析以下问题时，你可以：\n" +
        "1) 使用 search_knowledge 搜索知识库中的风险案例和最佳实践\n" +
        "2) 使用 check_scope_creep 检查新需求是否超范围\n" +
        "3) 使用 get_project_health 获取项目健康评分\n" +
        "4) 使用 analyze_fit_gap 做差异分析\n" +
        "5) 使用 generate_defense_script 生成应对话术\n" +
        "给出专业、简洁、可执行的回答。",
        sid
      );
    } catch {
      setLoading(false);
      setMessages((prev) => {
        const next = [...prev];
        next[next.length - 1] = { ...next[next.length - 1], content: "分析失败，请重试", loading: false };
        return next;
      });
    }
  }, [input, loading]);

  return (
    <div className="mx-auto flex max-w-3xl flex-col" style={{ height: "calc(100vh - 12rem)" }}>
      {/* Chat messages */}
      <div className="flex-1 space-y-3 overflow-y-auto rounded-lg border border-neutral-200 bg-white p-4">
        {messages.length === 0 && (
          <div className="flex h-full items-center justify-center">
            <div className="text-center">
              <Brain className="mx-auto mb-2 h-8 w-8 text-amber-400" />
              <p className="text-sm font-medium text-neutral-500">AI 深度风险分析</p>
              <p className="mt-1 text-xs text-neutral-400">输入问题开始分析项目风险、范围蔓延、客户沟通策略等</p>
            </div>
          </div>
        )}
        {messages.map((msg) => (
          <div key={msg.id} className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}>
            <div className={`max-w-[80%] rounded-lg px-3 py-2 text-xs leading-relaxed ${
              msg.role === "user"
                ? "bg-amber-600 text-white"
                : "bg-neutral-100 text-neutral-700"
            }`}>
              {msg.loading && !msg.content ? (
                <span className="flex items-center gap-1"><Loader2 className="h-3 w-3 animate-spin" />分析中</span>
              ) : (
                <span className="whitespace-pre-wrap">{msg.content}</span>
              )}
            </div>
          </div>
        ))}
        <div ref={chatEndRef} />
      </div>

      {/* Input */}
      <div className="mt-3 flex gap-2">
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); } }}
          placeholder="输入风险分析问题..."
          className="flex-1 rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
          disabled={loading}
        />
        <button type="button" onClick={handleSend} disabled={loading || !input.trim()}
          className="flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50">
          {loading ? <Loader2 className="h-3 w-3 animate-spin" /> : <Send className="h-3 w-3" />}
          发送
        </button>
      </div>
    </div>
  );
}
