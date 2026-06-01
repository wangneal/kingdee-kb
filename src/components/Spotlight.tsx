import { useState, useEffect, useRef, useCallback } from "react";
import { Search, Loader2, Send, X } from "lucide-react";
import { agentChat, listenReActEvents } from "../lib/tauri-commands";
import { useProject } from "../contexts/ProjectContext";

export default function Spotlight() {
  const { projectId } = useProject();
  const [visible, setVisible] = useState(false);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState("");
  const resultRef = useRef("");
  const inputRef = useRef<HTMLInputElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const spotSessionRef = useRef<string | null>(null);

  // Listen for Alt+Space via Tauri global-shortcut event
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen("spotlight-toggle", () => {
        setVisible((v) => !v);
        setInput("");
        setResult("");
        resultRef.current = "";
      });
    })();

    // Also keep local Escape handler for closing
    const escHandler = (e: KeyboardEvent) => {
      if (e.key === "Escape" && visible) {
        setVisible(false);
      }
    };
    window.addEventListener("keydown", escHandler);

    return () => {
      if (unlisten) unlisten();
      window.removeEventListener("keydown", escHandler);
    };
  }, [visible]);

  // Auto-focus input when visible
  useEffect(() => {
    if (visible && inputRef.current) {
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [visible]);

  // Listen for ReAct events (filtered by session)
  useEffect(() => {
    let cancelled = false;
    listenReActEvents((event) => {
      // Support both snake_case and camelCase (Tauri v2 may convert)
      const eventSessionId = event.session_id || (event as any).sessionId;
      if (eventSessionId !== spotSessionRef.current) return;
      if (event.type === "text_delta") {
        resultRef.current += event.content;
        setResult(resultRef.current);
      }
      if (event.type === "done" || event.type === "error") {
        setLoading(false);
        spotSessionRef.current = null;
      }
    }).then((fn) => {
      if (cancelled) { fn(); return; }
    });
    return () => { cancelled = true; };
  }, []);

  const handleSubmit = useCallback(async () => {
    const text = input.trim();
    if (!text || loading) return;
    setLoading(true);
    setResult("");
    resultRef.current = "";
    try {
      // Generate session ID first before calling agentChat
      const sid = `spot_${Date.now()}`;
      spotSessionRef.current = sid;
      await agentChat(text, sid, projectId);
    }
    catch { setLoading(false); }
  }, [input, loading]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  if (!visible) return null;

  return (
    <div
      ref={overlayRef}
      className="fixed inset-0 z-[9999] flex items-start justify-center bg-black/30 pt-[15vh]"
      onClick={(e) => { if (e.target === overlayRef.current) setVisible(false); }}
    >
      <div className="w-full max-w-xl rounded-xl bg-white shadow-2xl border border-neutral-200 overflow-hidden">
        {/* Search bar */}
        <div className="flex items-center gap-3 border-b border-neutral-100 px-4 py-3">
          <Search className="h-5 w-5 text-neutral-400 shrink-0" />
          <input
            ref={inputRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="向 AI 提问（搜索知识库、生成文档、分析风险）..."
            className="flex-1 text-sm text-neutral-700 placeholder-neutral-400 outline-none bg-transparent"
          />
          {loading ? (
            <Loader2 className="h-4 w-4 animate-spin text-amber-500 shrink-0" />
          ) : input.trim() ? (
            <button type="button" onClick={handleSubmit} className="p-1 text-amber-600 hover:text-amber-700">
              <Send className="h-4 w-4" />
            </button>
          ) : null}
          <button type="button" onClick={() => setVisible(false)} className="p-1 text-neutral-300 hover:text-neutral-500">
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Results */}
        {result && (
          <div className="max-h-60 overflow-y-auto px-4 py-3">
            <div className="text-xs leading-relaxed text-neutral-700 whitespace-pre-wrap">
              {result}
            </div>
          </div>
        )}

        {/* Hint */}
        {!input && !result && (
          <div className="px-4 py-3 text-xs text-neutral-400 flex items-center gap-3">
            <span>Alt+Space 切换</span>
            <span>Enter 发送</span>
            <span>Esc 关闭</span>
          </div>
        )}
      </div>
    </div>
  );
}
