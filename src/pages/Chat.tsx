import { useState, useCallback, useRef, useEffect } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Send,
  Trash2,
  Loader2,
  AlertCircle,
  Brain,
} from "lucide-react";
import {
  isLLMConfigured,
  reactChat,
  listenReActEvents,
  answerQuestion,
  saveChatMemory,
  type ReActEvent,
  type ClarificationPayload,
} from "../lib/tauri-commands";

interface DisplayMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  streaming?: boolean;
  error?: boolean;
  reactTrace?: ReActEvent[];
  clarification?: ClarificationPayload;
}

interface ReActTrace {
  thinking: string;
  toolCalls: { name: string; args: string; result: string }[];
}

let msgIdCounter = 0;
function nextId(): string {
  return `msg_${++msgIdCounter}_${Date.now()}`;
}

const CHAT_STORAGE_KEY = "kingdee_kb_chat_history";
const MAX_STORED_MESSAGES = 500;

function loadChatHistory(): DisplayMessage[] {
  try {
    const raw = localStorage.getItem(CHAT_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as DisplayMessage[];
    return parsed.filter((m) => m.id && m.role);
  } catch {
    return [];
  }
}

function saveChatHistory(messages: DisplayMessage[]) {
  try {
    const clean = messages.map((m) => ({ ...m, streaming: false }));
    const trimmed = clean.length > MAX_STORED_MESSAGES
      ? clean.slice(clean.length - MAX_STORED_MESSAGES)
      : clean;
    localStorage.setItem(CHAT_STORAGE_KEY, JSON.stringify(trimmed));
  } catch {
    // ignore
  }
}

export default function Chat() {
  const [messages, setMessages] = useState<DisplayMessage[]>(loadChatHistory);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [llmReady, setLlmReady] = useState<boolean | null>(null);
  const [currentTrace, setCurrentTrace] = useState<ReActTrace>({
    thinking: "",
    toolCalls: [],
  });
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const currentSessionId = useRef<string | null>(null);
  const unsubRef = useRef<(() => void) | null>(null);

  // Check LLM on mount
  useEffect(() => {
    isLLMConfigured()
      .then(setLlmReady)
      .catch(() => setLlmReady(false));
  }, []);

  // Subscribe to ReAct events (filtered by session)
  useEffect(() => {
    let cancelled = false;
    listenReActEvents((event) => {
      // Check session_id in both snake_case and camelCase (Tauri v2 may convert)
      const eventSessionId = event.session_id || (event as any).sessionId;
      if (eventSessionId !== currentSessionId.current) return;
      switch (event.type) {
        case "thinking":
          setCurrentTrace((prev) => ({ ...prev, thinking: prev.thinking + event.content }));
          break;
        case "tool_call":
          setCurrentTrace((prev) => ({
            ...prev,
            toolCalls: [
              ...prev.toolCalls,
              { name: event.name, args: event.args, result: "" },
            ],
          }));
          break;
        case "tool_result":
          setCurrentTrace((prev) => {
            const calls = [...prev.toolCalls];
            const last = calls[calls.length - 1];
            if (last && !last.result) {
              calls[calls.length - 1] = { ...last, result: event.result };
            }
            return { ...prev, toolCalls: calls };
          });
          break;
        case "text_delta":
          setMessages((prev) => {
            const last = prev[prev.length - 1];
            if (last && last.role === "assistant" && last.streaming) {
              return [
                ...prev.slice(0, -1),
                { ...last, content: last.content + event.content },
              ];
            }
            return prev;
          });
          break;
        case "done":
          setMessages((prev) => {
            const updated = prev.map((m) =>
              m.streaming ? { ...m, streaming: false, reactTrace: undefined } : m
            );
            // Save conversation to memory after chat completes (non-blocking)
            const conversation = updated
              .filter((m) => !m.error)
              .map((m) => ({ role: m.role, content: m.content }));
            saveChatMemory(conversation).catch((e) =>
              console.warn("[Chat] Failed to save chat memory:", e)
            );
            return updated;
          });
          setCurrentTrace({ thinking: "", toolCalls: [] });
          setLoading(false);
          break;
        case "clarification":
          setMessages((prev) => {
            const clarPayload = event.payload;
            const last = prev[prev.length - 1];
            const clarMsg: DisplayMessage = {
              id: nextId(),
              role: "assistant",
              content: clarPayload.prompt,
              clarification: clarPayload,
            };
            if (last && last.role === "assistant" && last.streaming) {
              return [
                ...prev.slice(0, -1),
                { ...last, streaming: false, ...clarMsg },
              ];
            }
            return [...prev, clarMsg];
          });
          setCurrentTrace({ thinking: "", toolCalls: [] });
          setLoading(false);
          break;
        case "error":
          setMessages((prev) =>
            prev.map((m) =>
              m.streaming
                ? { ...m, content: m.content || "请求失败：" + event.message, streaming: false, error: true }
                : m
            )
          );
          setCurrentTrace({ thinking: "", toolCalls: [] });
          setLoading(false);
          break;
      }
    }).then((unsub) => {
      if (cancelled) { unsub(); return; }
      unsubRef.current = unsub;
    });
    return () => {
      cancelled = true;
      currentSessionId.current = null;
      unsubRef.current?.();
    };
  }, []);

  // Auto-scroll
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, currentTrace]);

  const handleSend = useCallback(async () => {
    const text = input.trim();
    if (!text || loading) return;

    const userMsg: DisplayMessage = { id: nextId(), role: "user", content: text };
    const assistantId = nextId();
    const assistantMsg: DisplayMessage = {
      id: assistantId,
      role: "assistant",
      content: "",
      streaming: true,
    };

    setMessages((prev) => [...prev, userMsg, assistantMsg]);
    setInput("");
    setLoading(true);
    setCurrentTrace({ thinking: "", toolCalls: [] });

    try {
      // Generate session ID first and set it before calling reactChat
      // because events may arrive before reactChat returns
      const sid = nextId();
      currentSessionId.current = sid;
      await reactChat(text, undefined, sid);
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      setMessages((prev) =>
        prev.map((m) =>
          m.id === assistantId
            ? { ...m, content: "请求失败：" + errorMsg, streaming: false, error: true }
            : m
        )
      );
      setLoading(false);
    }
  }, [input, loading]);

  // Answer a pending clarification question (resolves the blocked question tool)
  const handleClarify = useCallback(async (questionId: string, answer: string) => {
    try {
      await answerQuestion(questionId, answer);
    } catch (err) {
      console.error("[Chat] Failed to answer question:", err);
    }
  }, []);

  // Save to localStorage debounced
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
    saveTimeoutRef.current = setTimeout(() => saveChatHistory(messages), 500);
    return () => { if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current); };
  }, [messages]);

  const handleClear = useCallback(() => {
    setMessages([]);
    localStorage.removeItem(CHAT_STORAGE_KEY);
  }, []);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend]
  );

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
        <div className="flex items-center gap-2">
          <Brain className="h-5 w-5 text-amber-600" />
          <h1 className="text-base font-semibold text-neutral-800">AI 助手</h1>
          <span className="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] font-medium text-amber-700">Agent</span>
          <span className="text-xs text-neutral-400">
            {messages.filter((m) => m.role === "user").length} 轮对话
          </span>
        </div>
        <button type="button" onClick={handleClear}
          className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium text-neutral-500 hover:bg-neutral-100 transition-colors">
          <Trash2 className="h-3.5 w-3.5" />
          清空对话
        </button>
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-4">
        <div className="mx-auto max-w-3xl space-y-4">
          {messages.length === 0 && !loading ? (
            <div className="flex flex-col items-center justify-center pt-20 text-center">
              <div className="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-amber-50">
                <Brain className="h-8 w-8 text-amber-300" />
              </div>
              <p className="text-sm font-medium text-neutral-500">输入问题开始对话</p>
              <p className="mt-1 text-xs text-neutral-400">Agent 可以搜索知识库、生成文档、分析风险</p>
              {llmReady === false && (
                <div className="mt-4 inline-flex items-center gap-1.5 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700">
                  <AlertCircle className="h-3.5 w-3.5" />
                  LLM 未配置，请先在设置中填写 API Key
                </div>
              )}
            </div>
          ) : (
            messages.map((msg) => (
              <MessageBubble key={msg.id} message={msg} onClarify={handleClarify} />
            ))
          )}

          {/* ReAct Trace (while loading) */}
          {loading && (currentTrace.thinking || currentTrace.toolCalls.length > 0) && (
            <div className="space-y-2 border-l-2 border-amber-200 pl-4">
              {currentTrace.thinking && (
                <div className="text-xs text-amber-700 italic leading-relaxed">
                  🤔 {currentTrace.thinking}
                </div>
              )}
              {currentTrace.toolCalls.map((tc, i) => (
                <div key={i}>
                  <div className="rounded-t-lg bg-amber-50 border border-amber-200 px-3 py-2 text-xs font-medium text-amber-800">
                    🔧 {tc.name}
                  </div>
                  {tc.result && (
                    <div className="rounded-b-lg bg-green-50 border border-green-200 px-3 py-2 text-xs text-green-700 border-t-0 whitespace-pre-wrap line-clamp-3">
                      {tc.result}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Input bar */}
      <div className="border-t border-neutral-200 bg-white p-4">
        <div className="mx-auto max-w-3xl">
          <div className="flex items-end gap-2">
            <textarea
              ref={inputRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入问题，Agent 将自动调用工具来回答..."
              rows={1}
              disabled={loading}
              className="flex-1 resize-none rounded-lg border border-neutral-200 bg-white px-4 py-2.5 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-amber-500 focus:ring-2 focus:ring-amber-500/20 disabled:opacity-50"
            />
            <button
              type="button"
              onClick={handleSend}
              disabled={loading || !input.trim()}
              className="flex h-10 w-10 items-center justify-center rounded-lg bg-amber-600 text-white hover:bg-amber-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {loading ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Send className="h-4 w-4" />
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ── Message Bubble Component ──────────────────────────────────────────────

function MessageBubble({ message, onClarify }: { message: DisplayMessage; onClarify: (questionId: string, answer: string) => void }) {
  const isUser = message.role === "user";
  const [freeInput, setFreeInput] = useState("");

  return (
    <div className={`flex ${isUser ? "justify-end" : "justify-start"}`}>
      <div className={`max-w-[80%] ${isUser ? "" : "w-full"}`}>
        {/* Avatar */}
        <div className={`mb-1 flex items-center gap-1.5 text-xs ${
          isUser ? "justify-end text-neutral-400" : "text-amber-600"
        }`}>
          <span className="flex h-5 w-5 items-center justify-center rounded-full bg-neutral-100 text-[10px]">
            {isUser ? "👤" : "🤖"}
          </span>
          <span className="font-medium">{isUser ? "你" : "AI 助手"}</span>
        </div>

        {/* Message */}
        <div className={`rounded-2xl px-4 py-3 text-sm leading-relaxed ${
          isUser
            ? "bg-amber-600 text-white rounded-tr-md"
            : message.error
            ? "bg-red-50 text-red-700 border border-red-200 rounded-tl-md"
            : "bg-white text-neutral-700 border border-neutral-200 rounded-tl-md shadow-sm"
        }`}>
          {isUser ? (
            <div className="whitespace-pre-wrap">{message.content}</div>
          ) : (
            <div className="prose prose-sm max-w-none prose-headings:text-neutral-800 prose-a:text-amber-600 prose-code:bg-neutral-100 prose-code:px-1 prose-code:rounded prose-pre:bg-neutral-900 prose-pre:text-neutral-100">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {message.content.replace(/^\n+/, "")}
              </ReactMarkdown>
              {message.streaming && (
                <span className="ml-1 inline-block h-3.5 w-1.5 animate-pulse bg-amber-500 rounded-sm align-middle" />
              )}
            </div>
          )}

          {/* Clarification options */}
          {message.clarification && (
            <div className="mt-3 border-t border-neutral-100 pt-3 space-y-2">
              {/* Single choice: clickable buttons */}
              {message.clarification.mode === "single_choice" && (
                <div className="flex flex-wrap gap-2">
                  {message.clarification.options.map((opt) => (
                    <button
                      key={opt}
                      type="button"
                      onClick={() => onClarify(message.clarification!.question_id, opt)}
                      className="rounded-lg border border-amber-200 bg-amber-50 px-3 py-1.5 text-xs font-medium text-amber-700 hover:bg-amber-100 transition-colors"
                    >
                      {opt}
                    </button>
                  ))}
                </div>
              )}

              {/* Multi choice: checkboxes + confirm */}
              {message.clarification.mode === "multi_choice" && (
                <MultiChoiceOptions
                  options={message.clarification.options}
                  questionId={message.clarification.question_id}
                  onConfirm={onClarify}
                />
              )}

              {/* Free input: text box + submit */}
              {message.clarification.mode === "free_input" && (
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={freeInput}
                    onChange={(e) => setFreeInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && freeInput.trim()) {
                        onClarify(message.clarification!.question_id, freeInput.trim());
                        setFreeInput("");
                      }
                    }}
                    placeholder="输入你的回答..."
                    className="flex-1 rounded-lg border border-neutral-200 bg-white px-3 py-1.5 text-xs outline-none focus:border-amber-500"
                  />
                  <button
                    type="button"
                    onClick={() => {
                      if (freeInput.trim()) {
                        onClarify(message.clarification!.question_id, freeInput.trim());
                        setFreeInput("");
                      }
                    }}
                    disabled={!freeInput.trim()}
                    className="rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50 transition-colors"
                  >
                    发送
                  </button>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/** Multi-choice checkbox group with confirm button */
function MultiChoiceOptions({
  options,
  questionId,
  onConfirm,
}: {
  options: string[];
  questionId: string;
  onConfirm: (questionId: string, answer: string) => void;
}) {
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const toggle = (opt: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(opt)) next.delete(opt);
      else next.add(opt);
      return next;
    });
  };

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-2">
        {options.map((opt) => (
          <button
            key={opt}
            type="button"
            onClick={() => toggle(opt)}
            className={`rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors ${
              selected.has(opt)
                ? "border-amber-500 bg-amber-100 text-amber-800"
                : "border-neutral-200 bg-white text-neutral-600 hover:border-amber-200"
            }`}
          >
            {selected.has(opt) ? "✓ " : ""}{opt}
          </button>
        ))}
      </div>
      <button
        type="button"
        onClick={() => onConfirm(questionId, Array.from(selected).join(", "))}
        disabled={selected.size === 0}
        className="rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50 transition-colors"
      >
        确认选择 ({selected.size})
      </button>
    </div>
  );
}
