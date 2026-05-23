import { useState, useCallback, useRef, useEffect } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  MessageSquare,
  Send,
  Trash2,
  Loader2,
  FileText,
  AlertCircle,
  ChevronDown,
} from "lucide-react";
import {
  isLLMConfigured,
  startChatStream,
  listenChatEvents,
  saveChatMemory,
  type ChatMessage,
  type RAGSource,
} from "../lib/tauri-commands";

interface DisplayMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  sources?: RAGSource[];
  streaming?: boolean;
  error?: boolean;
  thinking?: string;
}

let msgIdCounter = 0;
function nextId(): string {
  return `msg_${++msgIdCounter}_${Date.now()}`;
}

const CHAT_STORAGE_KEY = "kingdee_kb_chat_history";
const MAX_CONTEXT_MESSAGES = 3; // Number of recent messages to inject as context

// Load persisted chat history from localStorage
function loadChatHistory(): DisplayMessage[] {
  try {
    const raw = localStorage.getItem(CHAT_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as DisplayMessage[];
    // Ensure IDs are valid
    return parsed.filter((m) => m.id && m.role);
  } catch {
    return [];
  }
}

// Save chat history to localStorage (throttled via requestAnimationFrame)
function saveChatHistory(messages: DisplayMessage[]) {
  try {
    // Only store messages that have content (skip empty streaming placeholders)
    const clean = messages.map((m) => ({
      ...m,
      streaming: false, // Never persist streaming state
    }));
    localStorage.setItem(CHAT_STORAGE_KEY, JSON.stringify(clean));
  } catch {
    // localStorage quota exceeded or unavailable
  }
}

export default function Chat() {
  const [messages, setMessages] = useState<DisplayMessage[]>(loadChatHistory);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [llmReady, setLlmReady] = useState<boolean | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const currentAssistantId = useRef<string | null>(null);
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Debounced save to localStorage whenever messages change
  useEffect(() => {
    if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
    saveTimeoutRef.current = setTimeout(() => {
      saveChatHistory(messages);
    }, 500);
    return () => {
      if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
    };
  }, [messages]);

  // Check LLM availability on mount
  useEffect(() => {
    isLLMConfigured()
      .then(setLlmReady)
      .catch(() => setLlmReady(false));
  }, []);

  // Save chat memory when a conversation completes (loading→false with content)
  // NOTE: Initial value is `false` (not `true`) to avoid spurious save on page
  // load when localStorage already has messages and loading starts as false.
  const prevLoadingRef = useRef(false);
  useEffect(() => {
    if (prevLoadingRef.current && !loading && messages.length >= 2) {
      const history: ChatMessage[] = messages
        .filter((m) => !m.error && m.content)
        .map((m) => ({ role: m.role, content: m.content }));
      if (history.length >= 2) {
        saveChatMemory(history);
      }
    }
    prevLoadingRef.current = loading;
  }, [loading, messages]);

  // Subscribe to real-time streaming events (EchoBird pattern)
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    listenChatEvents((event) => {
      if (cancelled) return;

      switch (event.type) {
        case "text_delta":
          setMessages((prev) => {
            const targetId = currentAssistantId.current;
            if (targetId) {
              // Append to the specific message by ID (avoids race with placeholder)
              return prev.map((m) =>
                m.id === targetId
                  ? { ...m, content: m.content + (event.content ?? "") }
                  : m
              );
            }
            // No placeholder yet — event arrived before handleSend committed.
            // Find any streaming assistant message as fallback
            const last = prev[prev.length - 1];
            if (last && last.role === "assistant" && last.streaming) {
              return [
                ...prev.slice(0, -1),
                { ...last, content: last.content + (event.content ?? "") },
              ];
            }
            // If neither works, drop this event (next one will catch up)
            return prev;
          });
          break;

        case "thinking":
          setMessages((prev) => {
            const targetId = currentAssistantId.current;
            if (targetId) {
              return prev.map((m) =>
                m.id === targetId
                  ? { ...m, thinking: (m.thinking ?? "") + (event.content ?? "") }
                  : m
              );
            }
            const last = prev[prev.length - 1];
            if (last && last.role === "assistant" && last.streaming) {
              return [
                ...prev.slice(0, -1),
                { ...last, thinking: (last.thinking ?? "") + (event.content ?? "") },
              ];
            }
            return prev;
          });
          break;

        case "done":
          setMessages((prev) =>
            prev.map((m) =>
              m.streaming ? { ...m, streaming: false } : m
            )
          );
          currentAssistantId.current = null;
          setLoading(false);
          break;

        case "sources":
          if (event.sources && event.sources.length > 0) {
            setMessages((prev) =>
              prev.map((m) =>
                !m.streaming && m.role === "assistant" && !m.sources
                  ? { ...m, sources: event.sources }
                  : m
              )
            );
          }
          break;

        case "error":
          setMessages((prev) =>
            prev.map((m) =>
              m.streaming
                ? {
                    ...m,
                    content: `请求失败：${event.message ?? ""}`,
                    streaming: false,
                    error: true,
                  }
                : m
            )
          );
          currentAssistantId.current = null;
          setLoading(false);
          break;
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Auto-scroll to bottom on new messages
  // biome-ignore lint/correctness/useExhaustiveDependencies: need messages.length to trigger scroll on new messages
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages.length]);

  // Auto-focus input after loading finishes
  useEffect(() => {
    if (!loading && inputRef.current) {
      inputRef.current.focus();
    }
  }, [loading]);

  const handleSend = useCallback(async () => {
    const text = input.trim();
    if (!text || loading) return;

    const userMsg: DisplayMessage = {
      id: nextId(),
      role: "user",
      content: text,
    };

    // Build conversation history for multi-turn (last N messages only)
    const history: ChatMessage[] = messages
      .filter((m) => !m.error)
      .slice(-MAX_CONTEXT_MESSAGES * 2) // Last N user/assistant pairs
      .map((m) => ({ role: m.role, content: m.content }));

    setMessages((prev) => [...prev, userMsg]);
    setInput("");
    setLoading(true);

    // Add placeholder assistant message for streaming
    const assistantId = nextId();
    currentAssistantId.current = assistantId;
    setMessages((prev) => [
      ...prev,
      {
        id: assistantId,
        role: "assistant",
        content: "",
        streaming: true,
      },
    ]);

    // Start the streaming background task
    try {
      await startChatStream(text, undefined, history);
      // Note: loading is set to false by the event listener on "done"/"error"
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      setMessages((prev) =>
        prev.map((m) =>
          m.id === assistantId
            ? {
                ...m,
                content: `请求失败：${errorMsg}`,
                streaming: false,
                error: true,
              }
            : m
        )
      );
      setLoading(false);
    }
  }, [input, loading, messages]);

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

  // Empty state
  if (messages.length === 0) {
    return (
      <div className="flex h-full flex-col">
        {/* Header */}
        <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
          <div className="flex items-center gap-2">
            <MessageSquare className="h-5 w-5 text-[#1A6BD8]" />
            <h1 className="text-base font-semibold text-neutral-800">
              AI 对话
            </h1>
          </div>
        </div>

        {/* Empty state center */}
        <div className="flex flex-1 items-center justify-center">
          <div className="text-center">
            <div className="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-neutral-100">
              <MessageSquare className="h-8 w-8 text-neutral-300" />
            </div>
            <p className="text-sm font-medium text-neutral-500">
              输入问题开始对话
            </p>
            <p className="mt-1 text-xs text-neutral-400">
              基于知识库的 RAG 智能问答
            </p>
            {llmReady === false && (
              <div className="mt-4 inline-flex items-center gap-1.5 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700">
                <AlertCircle className="h-3.5 w-3.5" />
                LLM 未配置，请先在设置中填写 API Key
              </div>
            )}
          </div>
        </div>

        {/* Input bar */}
        <div className="border-t border-neutral-200 bg-white p-4">
          <div className="w-full">
          <div className="flex items-end gap-2">
            <textarea
              ref={inputRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入问题..."
              rows={1}
              disabled={loading}
              className="flex-1 resize-none rounded-lg border border-neutral-200 bg-white px-4 py-2.5 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20 disabled:opacity-50"
            />
            <button
              type="button"
              onClick={handleSend}
              disabled={loading || !input.trim()}
              className="flex h-10 w-10 items-center justify-center rounded-lg bg-[#1A6BD8] text-white hover:bg-[#1558B0] disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
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

  // ── Chat with messages ────────────────────────────────────────────────
  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
        <div className="flex items-center gap-2">
          <MessageSquare className="h-5 w-5 text-[#1A6BD8]" />
          <h1 className="text-base font-semibold text-neutral-800">
            AI 对话
          </h1>
          <span className="text-xs text-neutral-400">
            {messages.filter((m) => m.role === "user").length} 轮对话
          </span>
        </div>
        <button
          type="button"
          onClick={handleClear}
          className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700 transition-colors"
        >
          <Trash2 className="h-3.5 w-3.5" />
          清空对话
        </button>
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-4">
        <div className="w-full space-y-4">
          {messages.map((msg) => (
            <MessageBubble key={msg.id} message={msg} />
          ))}
        </div>
      </div>

      {/* Input bar */}
      <div className="border-t border-neutral-200 bg-white p-4">
        <div className="w-full">
          <div className="flex items-end gap-2">
            <textarea
              ref={inputRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入问题..."
              rows={1}
              disabled={loading}
              className="flex-1 resize-none rounded-lg border border-neutral-200 bg-white px-4 py-2.5 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20 disabled:opacity-50"
            />
            <button
              type="button"
              onClick={handleSend}
              disabled={loading || !input.trim()}
              className="flex h-10 w-10 items-center justify-center rounded-lg bg-[#1A6BD8] text-white hover:bg-[#1558B0] disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
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

function MessageBubble({ message }: { message: DisplayMessage }) {
  const isUser = message.role === "user";
  const [showSources, setShowSources] = useState(false);
  const [showThinking, setShowThinking] = useState(false);

  return (
    <div className={`flex ${isUser ? "justify-end" : "justify-start"}`}>
      <div
        className={`max-w-[80%] ${
          isUser ? "" : "w-full"
        }`}
      >
        {/* Avatar + name */}
        <div
          className={`mb-1 flex items-center gap-1.5 text-xs ${
            isUser ? "justify-end text-neutral-400" : "text-[#1A6BD8]"
          }`}
        >
          <span className="flex h-5 w-5 items-center justify-center rounded-full bg-neutral-100 text-[10px]">
            {isUser ? "👤" : "🤖"}
          </span>
          <span className="font-medium">
            {isUser ? "你" : "AI 助手"}
          </span>
        </div>

        {/* Thinking section (collapsible) */}
        {!isUser && message.thinking && (
          <div className="mb-2">
            <button
              type="button"
              onClick={() => setShowThinking((v) => !v)}
              className="flex items-center gap-1 text-xs text-neutral-400 hover:text-neutral-600 transition-colors"
            >
              <ChevronDown
                className={`h-3 w-3 transition-transform ${
                  showThinking ? "rotate-180" : ""
                }`}
              />
              思考过程
            </button>
            {showThinking && (
              <div className="mt-1 rounded-md bg-neutral-50 border border-neutral-100 px-3 py-2 text-xs text-neutral-500 italic whitespace-pre-wrap">
                {message.thinking}
              </div>
            )}
          </div>
        )}

        {/* Message bubble */}
        <div
          className={`rounded-2xl px-4 py-3 text-sm leading-relaxed ${
            isUser
              ? "bg-[#1A6BD8] text-white rounded-tr-md"
              : message.error
              ? "bg-red-50 text-red-700 border border-red-200 rounded-tl-md"
              : "bg-white text-neutral-700 border border-neutral-200 rounded-tl-md shadow-sm"
          }`}
        >
          {isUser ? (
            <div className="whitespace-pre-wrap">{message.content}</div>
          ) : (
            <div className="prose prose-sm max-w-none prose-headings:text-neutral-800 prose-a:text-[#1A6BD8] prose-code:bg-neutral-100 prose-code:px-1 prose-code:rounded prose-pre:bg-neutral-900 prose-pre:text-neutral-100">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {message.content.replace(/^\n+/, "")}
              </ReactMarkdown>
              {message.streaming && (
                <span className="ml-1 inline-block h-3.5 w-1.5 animate-pulse bg-[#1A6BD8] rounded-sm align-middle" />
              )}
            </div>
          )}
        </div>

        {/* Sources panel */}
        {!isUser && message.sources && message.sources.length > 0 && (
          <div className="mt-2">
            <button
              type="button"
              onClick={() => setShowSources((v) => !v)}
              className="flex items-center gap-1 text-xs text-neutral-500 hover:text-[#1A6BD8] transition-colors"
            >
              <ChevronDown
                className={`h-3.5 w-3.5 transition-transform ${
                  showSources ? "rotate-180" : ""
                }`}
              />
              {message.sources.length} 个参考来源
            </button>
            {showSources && (
              <div className="mt-2 space-y-2">
                {message.sources.map((src, i) => (
                  <SourceCard key={`${src.title}-${src.score}`} source={src} index={i + 1} />
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Source Card Component ─────────────────────────────────────────────────

function SourceCard({
  source,
  index,
}: {
  source: RAGSource;
  index: number;
}) {
  return (
    <div className="rounded-lg border border-neutral-100 bg-neutral-50 px-3 py-2.5">
      <div className="mb-1 flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5 min-w-0">
          <FileText className="h-3.5 w-3.5 shrink-0 text-[#1A6BD8]" />
          <span className="text-xs font-medium text-neutral-700 truncate">
            [{index}] {source.title}
          </span>
        </div>
        <span
          className={`shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-medium ${
            source.score >= 0.8
              ? "bg-green-100 text-green-700"
              : source.score >= 0.5
              ? "bg-yellow-100 text-yellow-700"
              : "bg-neutral-100 text-neutral-500"
          }`}
        >
          {(source.score * 100).toFixed(0)}%
        </span>
      </div>
      {source.section_path && (
        <p className="mb-1 text-[10px] text-neutral-400">
          {source.section_path}
        </p>
      )}
      <p className="text-xs leading-relaxed text-neutral-600 line-clamp-3">
        {source.content_snippet}
      </p>
    </div>
  );
}
