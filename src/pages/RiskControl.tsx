import { useState, useEffect, useRef } from "react";
import { Shield, AlertTriangle, BookOpen, Plus, Trash2, Send, Loader2, AlertCircle, CheckCircle, ShieldAlert, Brain } from "lucide-react";
import {
  type ContractScopeItem,
  type ScopeCreepResult,
  type ProjectHealthScore,
  type DefenseScriptResult,
  listScopeItems,
  addScopeItem,
  deleteScopeItem,
  checkScopeCreep,
  getProjectHealth,
  generateDefenseScript,
  analyzeFitGap,
  reactChat,
  listenReActEvents,
} from "../lib/tauri-commands";

type Tab = "scope" | "health" | "scripts";

export default function RiskControl() {
  const [tab, setTab] = useState<Tab>("scope");
  const tabs: { key: Tab; label: string; icon: typeof Shield }[] = [
    { key: "scope", label: "需求蔓延警报", icon: AlertTriangle },
    { key: "health", label: "项目健康度", icon: Shield },
    { key: "scripts", label: "防身话术库", icon: BookOpen },
  ];

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center gap-2 border-b border-neutral-200 px-6">
        <ShieldAlert className="h-5 w-5 text-amber-600" />
        <h1 className="text-base font-semibold text-neutral-800">双轨风险把控舱</h1>
      </div>
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
        {tab === "scope" && <ScopeTab />}
        {tab === "health" && <HealthTab />}
        {tab === "scripts" && <ScriptsTab />}
      </div>
    </div>
  );
}

function ScopeTab() {
  const [items, setItems] = useState<ContractScopeItem[]>([]);
  const [checkResult, setCheckResult] = useState<ScopeCreepResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [newReq, setNewReq] = useState("");
  const [newCat, setNewCat] = useState("");
  const [newDesc, setNewDesc] = useState("");

  useEffect(() => { refresh(); }, []);

  const refresh = async () => {
    const list = await listScopeItems();
    setItems(list);
  };

  const handleCheck = async () => {
    if (!newReq.trim()) return;
    setLoading(true);
    try {
      const r = await checkScopeCreep(newReq.trim());
      setCheckResult(r);
    } catch (e) { alert(String(e)); }
    setLoading(false);
  };

  const handleAdd = async () => {
    if (!newCat.trim() || !newDesc.trim()) return;
    await addScopeItem(newCat.trim(), newDesc.trim(), true, "");
    setNewCat(""); setNewDesc("");
    refresh();
  };

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
          </div>
        </div>
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

function HealthTab() {
  const [health, setHealth] = useState<ProjectHealthScore | null>(null);
  const [loading, setLoading] = useState(false);
  const [aiLoading, setAiLoading] = useState(false);
  const [aiReport, setAiReport] = useState("");
  const aiReportRef = useRef("");
  const [fitGapInput, setFitGapInput] = useState("");
  const [fitGapResult, setFitGapResult] = useState("");
  const [fitGapLoading, setFitGapLoading] = useState(false);

  const refresh = async () => {
    setLoading(true);
    try { setHealth(await getProjectHealth()); } catch (e) { alert(String(e)); }
    setLoading(false);
  };

  useEffect(() => { refresh(); }, []);

  // Listen for AI analysis results
  useEffect(() => {
    const p = listenReActEvents((event) => {
      if (event.type === "text_delta") {
        aiReportRef.current += event.content;
        setAiReport(aiReportRef.current);
      }
      if (event.type === "done" || event.type === "error") {
        setAiLoading(false);
      }
    });
    return () => { p.then((fn) => fn()); };
  }, []);

  const handleAIAnalysis = async () => {
    if (!health || aiLoading) return;
    setAiLoading(true);
    setAiReport("");
    aiReportRef.current = "";
    const ctx = "score:" + health.overall_score + " level:" + health.risk_level;
    try { await reactChat("analyze:" + ctx, "ERP risk expert."); }
    catch(e) { setAiLoading(false); }
  };

  const colorClass = (level: string) =>
    level === "critical" ? "text-red-600" : level === "high" ? "text-orange-600" :
    level === "medium" ? "text-yellow-600" : "text-green-600";

  const bgClass = (level: string) =>
    level === "critical" ? "bg-red-50 border-red-200" : level === "high" ? "bg-orange-50 border-orange-200" :
    level === "medium" ? "bg-yellow-50 border-yellow-200" : "bg-green-50 border-green-200";

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
          {aiReport && <div className="mt-2 rounded-lg border border-amber-100 bg-amber-50 p-3 text-xs leading-relaxed text-neutral-700 whitespace-pre-wrap">{aiReport}</div>}

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
          <div className="mt-3 rounded-lg border border-neutral-100 bg-neutral-50 p-3">
            <pre className="text-xs leading-relaxed text-neutral-700 whitespace-pre-wrap font-sans">{fitGapResult}</pre>
          </div>
        )}
      </div>
    </div>
  );
}

function ScriptsTab() {
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
        </div>
      )}
    </div>
  );
}
