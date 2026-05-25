import { useState, useEffect, useRef, useCallback } from "react";
import { Shield, AlertTriangle, BookOpen, Plus, Trash2, Send, Loader2, AlertCircle, CheckCircle, ShieldAlert, Brain, Download } from "lucide-react";
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
  exportReport,
} from "../lib/tauri-commands";

type Tab = "scope" | "health" | "scripts" | "analysis";

export default function RiskControl() {
  const [tab, setTab] = useState<Tab>("scope");
  const tabs: { key: Tab; label: string; icon: typeof Shield }[] = [
    { key: "scope", label: "需求蔓延警�?, icon: AlertTriangle },
    { key: "health", label: "项目健康�?, icon: Shield },
    { key: "scripts", label: "防身话术�?, icon: BookOpen },
    { key: "analysis", label: "AI 深度分析", icon: Brain },
  ];

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center gap-2 border-b border-neutral-200 px-6">
        <ShieldAlert className="h-5 w-5 text-amber-600" />
        <h1 className="text-base font-semibold text-neutral-800">双轨风险把控�?/h1>
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
        {tab === "analysis" && <AnalysisTab />}
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

  const refresh = useCallback(async () => {
    const list = await listScopeItems();
    setItems(list);
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const handleCheck = async () => {
    if (!newReq.trim()) return;
    setLoading(true);
    try {
      const r = await checkScopeCreep(newReq.trim());
      setCheckResult(r);
    } catch (e) { console.warn("[Risk] 检查范围蔓延失败:", e); alert(String(e)); }
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
      {/* 范围检�?*/}
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <h2 className="mb-3 text-sm font-semibold text-neutral-700">检查新需求是否超范围</h2>
        <div className="flex gap-2">
          <input value={newReq} onChange={(e) => setNewReq(e.target.value)} placeholder="输入新需求描�?.." className="flex-1 rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500" />
          <button type="button" onClick={handleCheck} disabled={loading || !newReq.trim()}
            className="flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
          >{loading ? <Loader2 className="h-3 w-3 animate-spin" /> : <Send className="h-3 w-3" />}检�?/button>
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
                  }`}>{item.is_in_scope ? "范围�? : "排除"}</span>
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
  const aiSessionRef = useRef<string | null>(null);
  const [fitGapInput, setFitGapInput] = useState("");
  const [fitGapResult, setFitGapResult] = useState("");
  const [fitGapLoading, setFitGapLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try { setHealth(await getProjectHealth()); } catch (e) { console.warn("[Risk] 刷新项目健康失败:", e); alert(String(e)); }
    setLoading(false);
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  // Listen for AI analysis results (filtered by session)
  useEffect(() => {
    const p = listenReActEvents((event) => {
      if (event.session_id !== aiSessionRef.current) return;
      if (event.type === "text_delta") {
        aiReportRef.current += event.content;
        setAiReport(aiReportRef.current);
      }
      if (event.type === "done" || event.type === "error") {
        setAiLoading(false);
        aiSessionRef.current = null;
      }
    });
    return () => { p.then((fn) => fn()); };
  }, []);

  const handleAIAnalysis = async () => {
    if (!health || aiLoading) return;
    setAiLoading(true);
    setAiReport("");
    aiReportRef.current = "";
    const dims = health.dimensions.map(d => d.name + ":" + d.score.toFixed(0)).join(",");
    const prompt = "项目健康分析 -- 评分:" + health.overall_score + " 等级:" + health.risk_level + " 维度:" + dims;
    try {
      const sid = await reactChat(prompt, "ERP风险专家。基于数据给出简要分析：1)主要风险 2)建议措施 3)沟通策略�?);
      aiSessionRef.current = sid;
    } catch(e) { console.warn("[Risk] AI分析失败:", e); setAiLoading(false); }
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
          {/* 总评�?*/}
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
              }`}>{health.risk_level === "critical" ? "危�? : health.risk_level === "high" ? "高风�? :
                 health.risk_level === "medium" ? "关注" : "健康"}</span>
            </div>
            <p className="text-xs text-neutral-600">{health.trend}</p>
            {health.alert_count > 0 && (
              <p className="mt-1 text-xs font-medium text-red-600">�?{health.alert_count} 项指标需要关�?/p>
            )}
          </div>

          {/* 各维�?*/}
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
            {aiLoading ? "分析�?.." : "AI 风险分析"}
          </button>
          {aiReport && (
            <div className="mt-2 space-y-2">
              <div className="rounded-lg border border-amber-100 bg-amber-50 p-3 text-xs leading-relaxed text-neutral-700 whitespace-pre-wrap">{aiReport}</div>
              <button type="button" onClick={async () => {
                try {
                  const { save } = await import("@tauri-apps/plugin-dialog");
                  const path = await save({ filters: [{ name: "Markdown", extensions: ["md"] }] });
                  if (path) await exportReport(aiReport, path);
                } catch(e) { console.warn("[Risk] 导出AI报告失败:", e); alert("导出失败: " + String(e)); }
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
          placeholder="输入需求列表，每行一条，如：&#10;1. 总账模块支持多币�?#10;2. 需要定制化报表引擎"
          className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500" />
        <button type="button" onClick={async () => {
          if (!fitGapInput.trim()) return;
          setFitGapLoading(true);
          try { setFitGapResult(await analyzeFitGap(fitGapInput)); }
          catch (e) { console.warn("[Risk] 差异分析失败:", e); setFitGapResult("分析失败: " + String(e)); }
          setFitGapLoading(false);
        }} disabled={fitGapLoading || !fitGapInput.trim()}
          className="mt-2 flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50">
          {fitGapLoading ? <Loader2 className="h-3 w-3 animate-spin" /> : null}
          {fitGapLoading ? "分析�?.." : "开始分�?}
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
                } catch(e) { console.warn("[Risk] 导出差异分析失败:", e); alert("导出失败: " + String(e)); }
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
    } catch (e) { console.warn("[Risk] 生成应对话术失败:", e); alert(String(e)); }
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
            <label className="text-[10px] font-medium text-neutral-500">沟通基�?/label>
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
                } catch(e) { console.warn("[Risk] 导出版本话术失败:", e); alert("导出失败: " + String(e)); }
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

// ─── Analysis Tab: ReAct 深度分析对话 ──────────────────────────────────

interface ChatMsg {
  id: string;
  role: "user" | "assistant";
  content: string;
  loading?: boolean;
}

function AnalysisTab() {
  const [messages, setMessages] = useState<ChatMsg[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const msgRef = useRef("");
  const sessionRef = useRef<string | null>(null);
  const chatEndRef = useRef<HTMLDivElement>(null);

  // Listen for ReAct streaming events
  useEffect(() => {
    const p = listenReActEvents((event) => {
      if (event.session_id !== sessionRef.current) return;
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
    });
    return () => { p.then((fn) => fn()); };
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
      const sid = await reactChat(
        text,
        "你是�?KingdeeKB 双轨风险把控舱中的风控专家。分析以下问题时，你可以：\n" +
        "1) 使用 search_knowledge 搜索知识库中的风险案例和最佳实践\n" +
        "2) 使用 check_scope_creep 检查新需求是否超范围\n" +
        "3) 使用 get_project_health 获取项目健康评分\n" +
        "4) 使用 analyze_fit_gap 做差异分析\n" +
        "5) 使用 generate_defense_script 生成应对话术\n" +
        "给出专业、简洁、可执行的回答�?
      );
      sessionRef.current = sid;
    } catch {
      console.warn("[Risk] AI分析失败");
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
                <span className="flex items-center gap-1"><Loader2 className="h-3 w-3 animate-spin" />分析�?/span>
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
          发�?
        </button>
      </div>
    </div>
  );
}