import { createContext, useContext, useState, useCallback, useRef, useEffect, type ReactNode } from "react";
import {
  listenReActEvents,
  agentChat,
  cancelAgentStream,
  answerQuestion,
  type ClarificationPayload,
  type ChatMessage,
} from "../lib/tauri-commands";

// ── Exported Types ──────────────────────────────────────────────────────────

export interface RAGSource {
  title: string;
  section_path?: string;
  content_snippet?: string;
  score: number;
}

export interface AgentMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  streaming?: boolean;
  error?: boolean;
  hiddenContext?: string;
  clarification?: ClarificationPayload;
  clarificationAnswered?: boolean;
  sources?: RAGSource[];
}

export interface ReActTrace {
  thinking: string;
  toolCalls: { name: string; args: string; result: string }[];
}

export interface AgentSlot {
  messages: AgentMessage[];
  loading: boolean;
  currentTrace: ReActTrace;
  sessionId: string | null;
}

export function createDefaultSlot(): AgentSlot {
  return {
    messages: [],
    loading: false,
    currentTrace: { thinking: "", toolCalls: [] },
    sessionId: null,
  };
}

/** @deprecated Use createDefaultSlot() instead to avoid shared mutable state */
export const DEFAULT_SLOT: AgentSlot = createDefaultSlot();

export interface SendMessageOptions {
  projectId?: string;
  riskProjectId?: number | null;
  providerId?: string;
  history?: ChatMessage[];
  /** Override the text shown as user message (defaults to outbound text) */
  displayText?: string;
}

export interface AgentContextValue {
  /** Reactive map of all agent slots (keyed by slot ID). */
  slots: ReadonlyMap<string, AgentSlot>;

  /** Send a message in a slot. Creates user + placeholder assistant messages and calls agentChat. */
  sendMessage: (slotId: string, text: string, options?: SendMessageOptions) => Promise<void>;

  /** Answer a clarification question for a slot. */
  answerClarification: (slotId: string, questionId: string, answer: string) => Promise<void>;

  /** Cancel the active agent stream for a slot. */
  cancelSession: (slotId: string) => Promise<void>;

  /** Clear all messages and reset a slot. */
  clearSlot: (slotId: string) => void;

  /** Update messages in a slot using a functional updater. */
  updateMessages: (slotId: string, updater: (prev: AgentMessage[]) => AgentMessage[]) => void;
}

// ── Helpers ─────────────────────────────────────────────────────────────────

function nextId(): string {
  return crypto.randomUUID();
}

function summarizeToolResult(toolName: string, result: string): string {
  if (!result.trim()) return "";
  const important = result
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) =>
      /输出目录|沙箱目录|生成|文件|路径|output|sandbox|\.pptx|\.docx|\.xlsx|\.html|\.pdf/i.test(line)
    )
    .join("\n");
  const text = important || result;
  const clipped = text.length > 2000 ? `${text.slice(0, 2000)}\n...[truncated]` : text;
  return `工具 ${toolName} 结果摘要:\n${clipped}`;
}

/** Try to extract RAG sources from a search-knowledge tool result */
function extractSourcesFromToolResult(toolName: string, result: string): RAGSource[] | undefined {
  const name = (toolName || "").toLowerCase().replace(/[-_\s]/g, "");
  if (!name.includes("searchknowledge") && !name.includes("hybridsearch") && !name.includes("knowledge")) {
    return undefined;
  }
  
  const sources: RAGSource[] = [];
  
  // 解析格式: 【1】title (相关度: 0.xxx, 来源: xxx)\ncontent
  const regex = /【\d+】(.+?)\s*\(相关度:\s*([\d.]+),\s*来源:\s*(.+?)\)\n([\s\S]*?)(?=【\d+】|$)/g;
  let match;
  
  while ((match = regex.exec(result)) !== null) {
    sources.push({
      title: match[1].trim(),
      section_path: match[3].trim(),
      content_snippet: match[4].trim().slice(0, 200),
      score: parseFloat(match[2]) || 0,
    });
  }
  
  // 如果上面的格式没匹配，尝试简单的格式
  if (sources.length === 0) {
    const simpleRegex = /【\d+】(.+?)\s*\(相关度:\s*([\d.]+)\)/g;
    while ((match = simpleRegex.exec(result)) !== null) {
      sources.push({
        title: match[1].trim(),
        score: parseFloat(match[2]) || 0,
      });
    }
  }
  
  return sources.length > 0 ? sources.slice(0, 8) : undefined;
}

/** Build conversation history for multi-turn agent context. */
export function buildAgentHistory(messages: AgentMessage[]): ChatMessage[] {
  return messages
    .filter((m) => !m.streaming && !m.error && !m.clarification)
    .map((m) => {
      const hidden = m.hiddenContext ? `\n\n【上一轮工具上下文】\n${m.hiddenContext}` : "";
      return { role: m.role, content: `${m.content}${hidden}`.trim() };
    })
    .filter((m) => m.content.length > 0)
    .slice(-12);
}

// ── Context ─────────────────────────────────────────────────────────────────

const AgentContext = createContext<AgentContextValue | null>(null);

export function useAgent(): AgentContextValue {
  const ctx = useContext(AgentContext);
  if (!ctx) throw new Error("useAgent must be used within AgentProvider");
  return ctx;
}

// ── Internal Types ──────────────────────────────────────────────────────────

interface SlotInternal {
  slot: AgentSlot;
  latestToolName: string;
}

// ── Provider ────────────────────────────────────────────────────────────────

export function AgentProvider({ children }: { children: ReactNode }) {
  const [slots, setSlots] = useState<Map<string, SlotInternal>>(new Map());
  const sessionToSlot = useRef<Map<string, string>>(new Map());
  const latestSlots = useRef(slots);
  latestSlots.current = slots;

  // Update slots helper (also keeps ref in sync)
  const updateSlots = useCallback(
    (updater: (prev: Map<string, SlotInternal>) => Map<string, SlotInternal>) => {
      setSlots((prev) => {
        const next = updater(prev);
        latestSlots.current = next;
        return next;
      });
    },
    [],
  );

  // ── Global ReAct event listener ─────────────────────────────────────────
  useEffect(() => {
    let unsub: (() => void) | null = null;
    let cancelled = false;
    listenReActEvents((event) => {
      const eventSessionId = event.session_id || event.sessionId;
      if (!eventSessionId) return;
      const slotId = sessionToSlot.current.get(eventSessionId);
      if (!slotId) return;

      updateSlots((prev) => {
        const next = new Map(prev);
        const internal = next.get(slotId);
        if (!internal) return prev;

        const slot: AgentSlot = { ...internal.slot, currentTrace: { ...internal.slot.currentTrace }, messages: [...internal.slot.messages] };
        let toolName = internal.latestToolName;

        switch (event.type) {
          case "thinking":
            slot.currentTrace = {
              ...slot.currentTrace,
              thinking: slot.currentTrace.thinking + event.content,
            };
            break;

          case "tool_call":
            toolName = event.name;
            slot.currentTrace = {
              ...slot.currentTrace,
              toolCalls: [
                ...slot.currentTrace.toolCalls,
                { name: event.name, args: event.args, result: "" },
              ],
            };
            break;

          case "tool_result": {
            // Update trace
            const calls = [...slot.currentTrace.toolCalls];
            const last = calls[calls.length - 1];
            if (last && !last.result) {
              calls[calls.length - 1] = { ...last, result: event.result };
            }
            slot.currentTrace = { ...slot.currentTrace, toolCalls: calls };

            // Try to extract RAG sources from search tool results
            const newSources = extractSourcesFromToolResult(event.name || toolName || "tool", event.result);

            // Build hidden context for the last streaming assistant message
            const lastMsg = slot.messages[slot.messages.length - 1];
            if (lastMsg && lastMsg.role === "assistant" && lastMsg.streaming) {
              const summary = summarizeToolResult(event.name || toolName || "tool", event.result);
              const existingSources = lastMsg.sources ?? [];
              slot.messages[slot.messages.length - 1] = {
                ...lastMsg,
                hiddenContext: [lastMsg.hiddenContext, summary].filter(Boolean).join("\n\n"),
                sources: newSources ? [...existingSources, ...newSources] : existingSources.length > 0 ? existingSources : undefined,
              };
            }
            break;
          }

          case "text_delta": {
            const last = slot.messages[slot.messages.length - 1];
            if (last && last.role === "assistant" && last.streaming) {
              slot.messages[slot.messages.length - 1] = { ...last, content: last.content + event.content };
            } else {
              slot.messages = [...slot.messages, { id: nextId(), role: "assistant", content: event.content, streaming: true }];
            }
            break;
          }

          case "done": {
            slot.messages = slot.messages.map((m) =>
              m.streaming ? { ...m, streaming: false } : m,
            );
            slot.loading = false;
            slot.currentTrace = { thinking: "", toolCalls: [] };
            sessionToSlot.current.delete(eventSessionId);
            slot.sessionId = null;
            break;
          }

          case "clarification": {
            const payload = event.payload;
            const last = slot.messages[slot.messages.length - 1];
            const clarMsg: AgentMessage = {
              id: nextId(),
              role: "assistant",
              content: payload.prompt,
              clarification: payload,
            };
            if (last && last.role === "assistant" && last.streaming) {
              slot.messages[slot.messages.length - 1] = { ...last, streaming: false, ...clarMsg };
            } else {
              slot.messages = [...slot.messages, clarMsg];
            }
            slot.loading = false;
            slot.currentTrace = { thinking: "", toolCalls: [] };
            break;
          }

          case "error": {
            slot.messages = slot.messages.map((m) =>
              m.streaming
                ? { ...m, content: m.content || "请求失败：" + event.message, streaming: false, error: true }
                : m,
            );
            slot.loading = false;
            slot.currentTrace = { thinking: "", toolCalls: [] };
            sessionToSlot.current.delete(eventSessionId);
            slot.sessionId = null;
            break;
          }
        }

        next.set(slotId, { slot, latestToolName: toolName });
        return next;
      });
    }).then((unsubFn) => {
      if (cancelled) {
        unsubFn();
      } else {
        unsub = unsubFn;
      }
    });
    return () => {
      cancelled = true;
      unsub?.();
    };
  }, [updateSlots]);

  // ── Actions ────────────────────────────────────────────────────────────

  const sendMessage = useCallback(
    async (slotId: string, text: string, options?: SendMessageOptions) => {
      const sid = nextId();
      const displayText = options?.displayText ?? text;
      const userMsg: AgentMessage = { id: nextId(), role: "user", content: displayText };
      const assistantMsg: AgentMessage = { id: nextId(), role: "assistant", content: "", streaming: true };

      sessionToSlot.current.set(sid, slotId);

      updateSlots((prev) => {
        const next = new Map(prev);
        const internal = next.get(slotId) ?? { slot: createDefaultSlot(), latestToolName: "" };
        next.set(slotId, {
          slot: {
            messages: [...internal.slot.messages, userMsg, assistantMsg],
            loading: true,
            currentTrace: { thinking: "", toolCalls: [] },
            sessionId: sid,
          },
          latestToolName: "",
        });
        return next;
      });

      try {
        await agentChat(text, sid, options?.projectId, options?.riskProjectId, options?.history, options?.providerId);
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : String(err);
        updateSlots((prev) => {
          const next = new Map(prev);
          const internal = next.get(slotId);
          if (!internal) return prev;
          const msgs = internal.slot.messages.map((m) =>
            m.id === assistantMsg.id
              ? { ...m, content: "请求失败：" + errorMsg, streaming: false, error: true }
              : m,
          );
          next.set(slotId, { ...internal, slot: { ...internal.slot, messages: msgs, loading: false } });
          return next;
        });
      }
    },
    [updateSlots],
  );

  const answerClarification = useCallback(
    async (slotId: string, questionId: string, answer: string) => {
      // 1. Mark as answered
      updateSlots((prev) => {
        const next = new Map(prev);
        const internal = next.get(slotId);
        if (!internal) return prev;
        const msgs = internal.slot.messages.map((m) =>
          m.clarification?.question_id === questionId ? { ...m, clarificationAnswered: true } : m,
        );
        next.set(slotId, { ...internal, slot: { ...internal.slot, messages: msgs } });
        return next;
      });

      // 2. Add answer + placeholder
      const answerMsg: AgentMessage = { id: nextId(), role: "user", content: answer };
      const assistantMsg: AgentMessage = { id: nextId(), role: "assistant", content: "", streaming: true };

      updateSlots((prev) => {
        const next = new Map(prev);
        const internal = next.get(slotId);
        if (!internal) return prev;
        next.set(slotId, {
          ...internal,
          slot: {
            ...internal.slot,
            messages: [...internal.slot.messages, answerMsg, assistantMsg],
            loading: true,
            currentTrace: { thinking: "", toolCalls: [] },
          },
        });
        return next;
      });

      // 3. Send to backend
      try {
        await answerQuestion(questionId, answer);
      } catch (err) {
        console.error("[AgentContext] Failed to answer question:", err);
        updateSlots((prev) => {
          const next = new Map(prev);
          const internal = next.get(slotId);
          if (!internal) return prev;
          next.set(slotId, { ...internal, slot: { ...internal.slot, loading: false } });
          return next;
        });
      }
    },
    [updateSlots],
  );

  const cancelSession = useCallback(async (slotId: string) => {
    const internal = latestSlots.current.get(slotId);
    if (!internal?.slot.sessionId) return;
    try {
      await cancelAgentStream(internal.slot.sessionId);
    } catch (err) {
      console.warn("[AgentContext] Failed to cancel:", err);
    }
  }, []);

  const clearSlot = useCallback(
    (slotId: string) => {
      updateSlots((prev) => {
        const next = new Map(prev);
        const internal = next.get(slotId);
        if (internal?.slot.sessionId) {
          sessionToSlot.current.delete(internal.slot.sessionId);
        }
        next.delete(slotId);
        return next;
      });
    },
    [updateSlots],
  );

  const updateMessages = useCallback(
    (slotId: string, updater: (prev: AgentMessage[]) => AgentMessage[]) => {
      updateSlots((prev) => {
        const next = new Map(prev);
        const internal = next.get(slotId) ?? { slot: createDefaultSlot(), latestToolName: "" };
        next.set(slotId, {
          ...internal,
          slot: { ...internal.slot, messages: updater(internal.slot.messages) },
        });
        return next;
      });
    },
    [updateSlots],
  );

  // ── Context value ──────────────────────────────────────────────────────

  // Derive a reactive ReadonlyMap of AgentSlot from internal Map
  const reactiveSlots: ReadonlyMap<string, AgentSlot> = new Map(
    Array.from(slots.entries()).map(([k, v]) => [k, v.slot]),
  );

  const ctx: AgentContextValue = {
    slots: reactiveSlots,
    sendMessage,
    answerClarification,
    cancelSession,
    clearSlot,
    updateMessages,
  };

  return <AgentContext.Provider value={ctx}>{children}</AgentContext.Provider>;
}
