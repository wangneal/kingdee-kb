import { useState, useCallback, useRef, useEffect } from "react";
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
  ragQueryStream,
  isLLMConfigured,
  type ChatMessage,
  type StreamChunk,
  type RAGSource,
} from "../lib/tauri-commands";

interface DisplayMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  sources?: RAGSource[];
  streaming?: boolean;
  error?: boolean;
}

let msgIdCounter = 0;
function nextId(): string {
  return `msg_${++msgIdCounter}_${Date.now()}`;
}

export default function Chat() {
  const [messages, setMessages] = useState<DisplayMessage[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [llmReady, setLlmReady] = useState<boolean | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Check LLM availability on mount
  useEffect(() => {
    isLLMConfigured()
      .then(setLlmReady)
      .catch(() => setLlmReady(false));
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

    // Build conversation history for multi-turn
    const history: ChatMessage[] = messages
      .filter((m) => !m.error)
      .map((m) => ({ role: m.role, content: m.content }));

    setMessages((prev) => [...prev, userMsg]);
    setInput("");
    setLoading(true);

    // Add placeholder assistant message for streaming
    const assistantId = nextId();
    setMessages((prev) => [
      ...prev,
      {
        id: assistantId,
        role: "assistant",
        content: "",
        streaming: true,
      },
    ]);

    try {
      const chunks: StreamChunk[] = await ragQueryStream(
        text,
        undefined,
        history
      );

      if (!chunks || chunks.length === 0) {
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId
              ? {
                  ...m,
                  content: "知识库中暂无相关内容，或 LLM 服务未配置。",
                  streaming: false,
                }
              : m
          )
        );
        return;
      }

      // Simulate streaming: progressively reveal content from chunks
      let accumulated = "";

      for (let i = 0; i < chunks.length; i++) {
        const chunk = chunks[i];
        accumulated += chunk.content;

        // Capture sources from the last chunk if it has metadata
        // (backend sends sources as part of the final response)

        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId
              ? { ...m, content: accumulated, streaming: i < chunks.length - 1 }
              : m
          )
        );

        // Small delay between chunks for streaming effect
        if (i < chunks.length - 1) {
          await new Promise((r) => setTimeout(r, 30));
        }
      }

      // After streaming completes, fetch sources via a non-streaming query
      // The sources are embedded in the RAG response — we re-fetch with ragQuery
      // to get structured sources. For now, we extract from the stream.
      // Actually, let's try to get sources from the final chunk metadata.
      // Since ragQueryStream returns StreamChunk[] without sources,
      // we do a separate non-streaming call just for sources.
      try {
        const { ragQuery } = await import("../lib/tauri-commands");
        const response = await ragQuery(text, undefined, history);
        if (response.sources && response.sources.length > 0) {
          setMessages((prev) =>
            prev.map((m) =>
              m.id === assistantId
                ? { ...m, sources: response.sources }
                : m
            )
          );
        }
      } catch {
        // Sources fetch is best-effort; ignore failures
      }
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
    } finally {
      setLoading(false);
    }
  }, [input, loading, messages]);

  const handleClear = useCallback(() => {
    setMessages([]);
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
          <div className="mx-auto max-w-3xl">
            <div className="flex items-end gap-2">
              <textarea
                ref={inputRef}
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="输入问题..."
                rows={1}
                className="flex-1 resize-none rounded-lg border border-neutral-200 bg-white px-4 py-2.5 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20"
              />
              <button
                type="button"
                onClick={handleSend}
                disabled={!input.trim()}
                className="flex h-10 w-10 items-center justify-center rounded-lg bg-[#1A6BD8] text-white hover:bg-[#1558B0] disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              >
                <Send className="h-4 w-4" />
              </button>
            </div>
          </div>
        </div>
      </div>
    );
  }

  // Chat with messages
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
        <div className="mx-auto max-w-3xl space-y-4">
          {messages.map((msg) => (
            <MessageBubble key={msg.id} message={msg} />
          ))}
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
          <div className="whitespace-pre-wrap">
            {message.content}
            {message.streaming && (
              <span className="ml-1 inline-block h-3.5 w-1.5 animate-pulse bg-[#1A6BD8] rounded-sm" />
            )}
          </div>
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
