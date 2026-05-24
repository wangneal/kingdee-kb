import { useState, useEffect, useCallback, useRef } from "react";
import {
  ClipboardList,
  Plus,
  Mic,
  Square,
  Loader2,
  Download,
  Trash2,
  FileText,
  BookOpen,
  ChevronLeft,
  Edit3,
  MessageSquare,
  Send,
  AlertCircle,
  Brain,
} from "lucide-react";
import {
  type ResearchSession,
  type SessionDetail,
  listResearchSessions,
  createResearchSession,
  getResearchSession,
  deleteResearchSession,
  addQARecord,
  updateQARecord,
  deleteQARecord,
  exportSessionCsv,
  exportSessionMarkdown,
  extractBlueprint,
  loadWhisperModel,
  startWhisperRecording,
  stopWhisperRecording,
  getWhisperStatus,
  reactChat,
  listenReActEvents,
  type WhisperStatus,
} from "../lib/tauri-commands";

export default function ResearchAssistant() {
  const [mode, setMode] = useState<"list" | "detail" | "new">("list");
  const [sessions, setSessions] = useState<ResearchSession[]>([]);
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Load sessions on mount
  useEffect(() => {
    refreshList();
  }, []);

  const refreshList = useCallback(async () => {
    setLoading(true);
    try {
      const list = await listResearchSessions();
      setSessions(list);
    } catch (err) {
      setError(String(err));
    }
    setLoading(false);
  }, []);

  const openSession = useCallback(async (id: number) => {
    setLoading(true);
    try {
      const d = await getResearchSession(id);
      setDetail(d);
      setMode("detail");
    } catch (err) {
      setError(String(err));
    }
    setLoading(false);
  }, []);

  const handleDelete = useCallback(async (id: number) => {
    if (!confirm("确认删除此调研会话？所有记录将被永久删除。")) return;
    try {
      await deleteResearchSession(id);
      refreshList();
    } catch (err) {
      setError(String(err));
    }
  }, [refreshList]);

  // ── List View ──
  if (mode === "list") {
    return (
      <div className="flex h-full flex-col">
        <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
          <div className="flex items-center gap-2">
            <ClipboardList className="h-5 w-5 text-[#1A6BD8]" />
            <h1 className="text-base font-semibold text-neutral-800">调研助手</h1>
            <span className="text-xs text-neutral-400">{sessions.length} 个会话</span>
          </div>
          <button
            type="button"
            onClick={() => setMode("new")}
            className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#1558B0] transition-colors"
          >
            <Plus className="h-3.5 w-3.5" />
            新建调研
          </button>
        </div>

        {error && (
          <div className="mx-6 mt-3 flex items-center gap-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-red-600">
            <AlertCircle className="h-3.5 w-3.5" />
            {error}
          </div>
        )}

        <div className="flex-1 overflow-y-auto p-6">
          {loading ? (
            <div className="flex items-center justify-center pt-20">
              <Loader2 className="h-5 w-5 animate-spin text-neutral-400" />
            </div>
          ) : sessions.length === 0 ? (
            <div className="flex flex-col items-center justify-center pt-20 text-center">
              <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-neutral-100">
                <ClipboardList className="h-8 w-8 text-neutral-300" />
              </div>
              <p className="text-sm font-medium text-neutral-500">暂无调研会话</p>
              <p className="mt-1 text-xs text-neutral-400">点击"新建调研"开始</p>
            </div>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {sessions.map((s) => (
                <div
                  key={s.id}
                  className="group cursor-pointer rounded-lg border border-neutral-200 bg-white p-4 hover:border-[#1A6BD8]/30 hover:shadow-sm transition-all"
                  onClick={() => openSession(s.id)}
                >
                  <div className="mb-2 flex items-start justify-between">
                    <h3 className="text-sm font-medium text-neutral-800 line-clamp-1">{s.title}</h3>
                    <button
                      type="button"
                      onClick={(e) => { e.stopPropagation(); handleDelete(s.id); }}
                      className="shrink-0 rounded p-1 text-neutral-300 opacity-0 group-hover:opacity-100 hover:text-red-500 transition-all"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  </div>
                  <div className="flex flex-wrap gap-1.5 text-[10px] text-neutral-500">
                    <span className="rounded bg-neutral-100 px-1.5 py-0.5">{s.edition}</span>
                    <span className="rounded bg-neutral-100 px-1.5 py-0.5">{s.module_code}</span>
                    {s.status === "completed" && (
                      <span className="rounded bg-green-100 px-1.5 py-0.5 text-green-700">已完成</span>
                    )}
                  </div>
                  <p className="mt-2 text-xs text-neutral-400">
                    {s.interviewee || "未填受访人"} · {s.session_date || "未填日期"}
                  </p>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    );
  }

  // ── New Session Form ──
  if (mode === "new") {
    return <NewSessionForm onCreated={(id) => { refreshList(); openSession(id); }} onCancel={() => setMode("list")} />;
  }

  // ── Session Detail ──
  if (mode === "detail" && detail) {
    return (
      <SessionDetailView
        detail={detail}
        onBack={() => { setMode("list"); setDetail(null); refreshList(); }}
        onUpdated={() => {
          if (detail) getResearchSession(detail.session.id).then(setDetail);
        }}
      />
    );
  }

  return null;
}

// ── New Session Form ────────────────────────────────────────────────────────

function NewSessionForm({ onCreated, onCancel }: { onCreated: (id: number) => void; onCancel: () => void }) {
  const [title, setTitle] = useState("");
  const [edition, setEdition] = useState("enterprise");
  const [moduleCode, setModuleCode] = useState("");
  const [interviewee, setInterviewee] = useState("");
  const [sessionDate, setSessionDate] = useState(new Date().toISOString().slice(0, 10));
  const [saving, setSaving] = useState(false);

  const handleSubmit = async () => {
    if (!title.trim()) return;
    setSaving(true);
    try {
      const id = await createResearchSession(title.trim(), edition, moduleCode.trim(), interviewee.trim(), sessionDate);
      onCreated(id);
    } catch (err) {
      alert(String(err));
    }
    setSaving(false);
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center gap-2 border-b border-neutral-200 px-6">
        <button type="button" onClick={onCancel} className="flex items-center gap-1 text-xs text-neutral-500 hover:text-neutral-700 transition-colors">
          <ChevronLeft className="h-4 w-4" />
          返回
        </button>
        <span className="text-sm text-neutral-300">|</span>
        <h1 className="text-base font-semibold text-neutral-800">新建调研会话</h1>
      </div>
      <div className="flex-1 overflow-y-auto p-6">
        <div className="mx-auto max-w-lg space-y-4">
          <div>
            <label className="mb-1 block text-xs font-medium text-neutral-600">会话标题 *</label>
            <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="如：BOS 基础平台调研" className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20" />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="mb-1 block text-xs font-medium text-neutral-600">版本</label>
              <select value={edition} onChange={(e) => setEdition(e.target.value)} className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8]">
                <option value="enterprise">企业版</option>
                <option value="flagship">旗舰版</option>
              </select>
            </div>
            <div>
              <label className="mb-1 block text-xs font-medium text-neutral-600">模块编码</label>
              <input value={moduleCode} onChange={(e) => setModuleCode(e.target.value)} placeholder="如：BOS" className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20" />
            </div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="mb-1 block text-xs font-medium text-neutral-600">受访人</label>
              <input value={interviewee} onChange={(e) => setInterviewee(e.target.value)} placeholder="姓名" className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20" />
            </div>
            <div>
              <label className="mb-1 block text-xs font-medium text-neutral-600">调研日期</label>
              <input type="date" value={sessionDate} onChange={(e) => setSessionDate(e.target.value)} className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-[#1A6BD8]" />
            </div>
          </div>
          <div className="flex justify-end gap-2 pt-2">
            <button type="button" onClick={onCancel} className="rounded-lg border border-neutral-200 px-4 py-2 text-xs font-medium text-neutral-600 hover:bg-neutral-50 transition-colors">取消</button>
            <button type="button" onClick={handleSubmit} disabled={saving || !title.trim()} className="flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-xs font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors">
              {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Plus className="h-3.5 w-3.5" />}
              创建
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ── Session Detail View ─────────────────────────────────────────────────────

function SessionDetailView({ detail, onBack, onUpdated }: { detail: SessionDetail; onBack: () => void; onUpdated: () => void }) {
  const { session, records } = detail;
  const [recording, setRecording] = useState(false);
  const [whisperStatus, setWhisperStatus] = useState<WhisperStatus | null>(null);
  const [loadingWhisper, setLoadingWhisper] = useState(false);
  const [newQuestion, setNewQuestion] = useState("");
  const [newAnswer, setNewAnswer] = useState("");
  const [editingRecord, setEditingRecord] = useState<number | null>(null);
  const [editAnswer, setEditAnswer] = useState("");
  const [aiLoading, setAiLoading] = useState(false);
  const aiAnswerRef = useRef("");

  useEffect(() => {
    getWhisperStatus().then(setWhisperStatus).catch(() => {});
  }, []);

  // Subscribe to ReAct events for AI assist
  useEffect(() => {
    const unlisten = listenReActEvents((event) => {
      if (event.type === "text_delta") {
        aiAnswerRef.current += event.content;
        setNewAnswer(aiAnswerRef.current);
      }
      if (event.type === "done") {
        setAiLoading(false);
      }
      if (event.type === "error") {
        setAiLoading(false);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  const handleAIAssist = async () => {
    if (!newQuestion.trim() || aiLoading) return;
    setAiLoading(true);
    aiAnswerRef.current = "";
    setNewAnswer("");
    const context = `当前调研：${session.title}（${session.edition}/${session.module_code}）\n已有记录：${records.map((r) => `Q: ${r.question_text}`).join("\n")}`;
    try {
      await reactChat(`请回答以下调研问题，基于知识库中的金蝶ERP实施经验：\n\n问题：${newQuestion}\n\n背景：${context}`, `你是一个金蝶ERP实施顾问，正在辅助一个调研访谈。请基于知识库给出专业的回答。回答要具体、可操作，包含系统配置路径或单据类型。不确定的写[待确认]。`);
    } catch (err) {
      setAiLoading(false);
    }
  };

  const handleStartRecording = async () => {
    if (!whisperStatus?.model_loaded) {
      setLoadingWhisper(true);
      try {
        await loadWhisperModel("tiny");
        const status = await getWhisperStatus();
        setWhisperStatus(status);
      } catch (err) {
        alert("加载语音模型失败: " + String(err));
        setLoadingWhisper(false);
        return;
      }
      setLoadingWhisper(false);
    }
    try {
      await startWhisperRecording();
      setRecording(true);
    } catch (err) {
      alert("启动录音失败: " + String(err));
    }
  };

  const handleStopRecording = async () => {
    try {
      const result = await stopWhisperRecording();
      setRecording(false);
      if (result.text.trim()) {
        setNewQuestion(result.text.trim());
      }
    } catch (err) {
      setRecording(false);
      alert("停止录音失败: " + String(err));
    }
  };

  const handleAddRecord = async () => {
    if (!newQuestion.trim()) return;
    try {
      await addQARecord(session.id, null, newQuestion.trim(), newAnswer.trim(), "", records.length);
      setNewQuestion("");
      setNewAnswer("");
      onUpdated();
    } catch (err) {
      alert(String(err));
    }
  };

  const handleUpdateRecord = async (recordId: number) => {
    try {
      await updateQARecord(recordId, editAnswer, "");
      setEditingRecord(null);
      onUpdated();
    } catch (err) {
      alert(String(err));
    }
  };

  const handleDeleteRecord = async (recordId: number) => {
    if (!confirm("确认删除此记录？")) return;
    try {
      await deleteQARecord(recordId);
      onUpdated();
    } catch (err) {
      alert(String(err));
    }
  };

  const handleExportCsv = async () => {
    try {
      const csv = await exportSessionCsv(session.id);
      const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `调研记录_${session.title}_${session.session_date}.csv`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      alert(String(err));
    }
  };

  const handleExportMd = async () => {
    try {
      const md = await exportSessionMarkdown(session.id);
      const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `调研记录_${session.title}_${session.session_date}.md`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      alert(String(err));
    }
  };

  const handleExtractBlueprint = async () => {
    const qaText = records.map((r, i) => `Q${i + 1}: ${r.question_text}\nA: ${r.answer_text}`).join("\n\n");
    if (!qaText.trim()) { alert("暂无记录，无法提炼蓝图"); return; }
    try {
      const blueprint = await extractBlueprint(qaText);
      const blob = new Blob([blueprint], { type: "text/markdown;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `蓝图_${session.title}.md`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      alert("蓝图提炼失败: " + String(err));
    }
  };

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
        <div className="flex items-center gap-2">
          <button type="button" onClick={onBack} className="flex items-center gap-1 text-xs text-neutral-500 hover:text-neutral-700 transition-colors">
            <ChevronLeft className="h-4 w-4" />
            返回
          </button>
          <span className="text-sm text-neutral-300">|</span>
          <BookOpen className="h-5 w-5 text-[#1A6BD8]" />
          <h1 className="text-base font-semibold text-neutral-800">{session.title}</h1>
          <span className="rounded bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">{session.edition}/{session.module_code}</span>
          {session.status === "completed" && <span className="rounded bg-green-100 px-1.5 py-0.5 text-[10px] text-green-700">已完成</span>}
        </div>
        <div className="flex items-center gap-2">
          <button type="button" onClick={handleExportCsv} className="flex items-center gap-1 rounded-lg border border-neutral-200 px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50 transition-colors">
            <Download className="h-3.5 w-3.5" /> CSV
          </button>
          <button type="button" onClick={handleExportMd} className="flex items-center gap-1 rounded-lg border border-neutral-200 px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50 transition-colors">
            <FileText className="h-3.5 w-3.5" /> MD
          </button>
          <button type="button" onClick={handleExtractBlueprint} className="flex items-center gap-1 rounded-lg border border-blue-200 px-3 py-1.5 text-xs text-blue-600 hover:bg-blue-50 transition-colors">
            <FileText className="h-3.5 w-3.5" /> 提炼蓝图
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex flex-1 overflow-hidden">
        {/* Q&A Records */}
        <div className="flex-1 overflow-y-auto p-6">
          {records.length === 0 ? (
            <div className="flex flex-col items-center justify-center pt-16 text-center">
              <MessageSquare className="mb-3 h-10 w-10 text-neutral-200" />
              <p className="text-sm text-neutral-400">暂无记录，使用录音或手动添加问题</p>
            </div>
          ) : (
            <div className="space-y-3">
              {records.map((r, i) => (
                <div key={r.id} className="rounded-lg border border-neutral-200 bg-white p-4">
                  <div className="mb-2 flex items-start justify-between">
                    <span className="text-xs font-medium text-[#1A6BD8]">Q{i + 1}</span>
                    <div className="flex gap-1">
                      <button type="button" onClick={() => { setEditingRecord(r.id); setEditAnswer(r.answer_text); }} className="rounded p-1 text-neutral-300 hover:text-[#1A6BD8] transition-colors">
                        <Edit3 className="h-3.5 w-3.5" />
                      </button>
                      <button type="button" onClick={() => handleDeleteRecord(r.id)} className="rounded p-1 text-neutral-300 hover:text-red-500 transition-colors">
                        <Trash2 className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  </div>
                  <p className="mb-2 text-sm font-medium text-neutral-800">{r.question_text}</p>
                  {editingRecord === r.id ? (
                    <div className="space-y-2">
                      <textarea value={editAnswer} onChange={(e) => setEditAnswer(e.target.value)} rows={2} className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-[#1A6BD8]" />
                      <div className="flex gap-2">
                        <button type="button" onClick={() => handleUpdateRecord(r.id)} className="rounded bg-[#1A6BD8] px-3 py-1 text-xs text-white hover:bg-[#1558B0]">保存</button>
                        <button type="button" onClick={() => setEditingRecord(null)} className="rounded border border-neutral-200 px-3 py-1 text-xs text-neutral-600 hover:bg-neutral-50">取消</button>
                      </div>
                    </div>
                  ) : (
                    <p className="text-xs leading-relaxed text-neutral-600">{r.answer_text || <span className="italic text-neutral-300">未填写回答</span>}</p>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Input Panel */}
        <div className="flex w-80 flex-col border-l border-neutral-200 bg-neutral-50 p-4">
          {/* Voice Recording */}
          <div className="mb-4 rounded-lg border border-neutral-200 bg-white p-4">
            <div className="mb-2 flex items-center gap-2">
              <Mic className="h-4 w-4 text-[#1A6BD8]" />
              <span className="text-xs font-semibold text-neutral-700">语音输入</span>
              {whisperStatus && !whisperStatus.model_loaded && (
                <span className="text-[10px] text-amber-600">（模型未加载）</span>
              )}
            </div>
            {loadingWhisper ? (
              <div className="flex items-center gap-2 text-xs text-neutral-500">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                加载语音模型...
              </div>
            ) : recording ? (
              <button type="button" onClick={handleStopRecording} className="flex w-full items-center justify-center gap-2 rounded-lg bg-red-500 px-3 py-2 text-xs font-medium text-white hover:bg-red-600 transition-colors">
                <Square className="h-3.5 w-3.5" />
                停止录音
              </button>
            ) : (
              <button type="button" onClick={handleStartRecording} className="flex w-full items-center justify-center gap-2 rounded-lg border border-[#1A6BD8] px-3 py-2 text-xs font-medium text-[#1A6BD8] hover:bg-[#1A6BD8]/5 transition-colors">
                <Mic className="h-3.5 w-3.5" />
                开始录音
              </button>
            )}
          </div>

          {/* Manual Input */}
          <div className="flex-1 space-y-2">
            <span className="text-xs font-semibold text-neutral-700">添加问题</span>
            <textarea
              value={newQuestion}
              onChange={(e) => setNewQuestion(e.target.value)}
              placeholder="输入或通过语音录入问题..."
              rows={3}
              className="w-full resize-none rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20"
            />
            <textarea
              value={newAnswer}
              onChange={(e) => setNewAnswer(e.target.value)}
              placeholder="回答内容..."
              rows={2}
              className="w-full resize-none rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20"
            />
            <button
              type="button"
              onClick={handleAddRecord}
              disabled={!newQuestion.trim()}
              className="flex w-full items-center justify-center gap-1.5 rounded-lg bg-[#1A6BD8] px-3 py-2 text-xs font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 transition-colors"
            >
              <Send className="h-3.5 w-3.5" />
              添加记录
            </button>
            <button
              type="button"
              onClick={handleAIAssist}
              disabled={!newQuestion.trim() || aiLoading}
              className="flex w-full items-center justify-center gap-1.5 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50 transition-colors"
            >
              {aiLoading ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Brain className="h-3.5 w-3.5" />
              )}
              {aiLoading ? "搜索知识库..." : "AI 辅助"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
