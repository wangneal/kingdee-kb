import { createContext, useContext, useState, useCallback, useRef, useEffect, type ReactNode } from "react";
import {
  listenReActEvents,
  agentChat,
  cancelAgentStream,
  answerQuestion,
  runVerification,
  type ClarificationPayload,
  type ChatMessage,
  type PlanStep,
  type AttachmentInfo,
} from "../lib/tauri-commands";

// ── 导出类型 ──────────────────────────────────────────────────────────

export interface RAGSource {
  title: string;
  section_path?: string;
  content_snippet?: string;
  score: number;
}

/** 验证报告（与 backend VerificationReport 对应） */
export interface VerificationReport {
  level: "Confirmed" | "NeedsReview" | "Suspected" | "Failed";
  overall_confidence: number;
  checks: {
    check_name: string;
    passed: boolean;
    confidence: number;
    detail: string;
    evidence: string[];
  }[];
  suggested_labels: string[];
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
  /** 文件附件（用户发送的文件、Agent 生成的文件） */
  attachments?: FileAttachment[];
  /** 验证层报告 */
  verificationReport?: VerificationReport;
}

/** 文件附件类型 */
export interface FileAttachment {
  id: string;
  path: string;
  name: string;
  kind: "document" | "image" | "generated";
  size?: number;
  mimeType?: string;
}

export interface ReActTrace {
  thinking: string;
  toolCalls: { name: string; args: string; result: string }[];
  plan: PlanStep[] | null;
  currentStepIndex: number | null;
  totalSteps: number;
  stepResults: Record<number, { result: string; success: boolean }>;
  replanReason: string | null;
  plannerTimeoutMessage: string | null;
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
    currentTrace: { thinking: "", toolCalls: [], plan: null, currentStepIndex: null, totalSteps: 0, stepResults: {}, replanReason: null, plannerTimeoutMessage: null },
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
  attachments?: AttachmentInfo[];
  /** 文件附件（用于在消息中显示） */
  fileAttachments?: FileAttachment[];
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

// ── 辅助函数 ─────────────────────────────────────────────────────────────────

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

/** 从工具结果中提取生成的文件路径 */
function extractFilesFromToolResult(_toolName: string, result: string): FileAttachment[] {
  const files: FileAttachment[] = [];
  const extPattern = /\.(pptx?|docx?|xlsx?|pdf|html?|md|txt|csv|png|jpg|jpeg|gif|svg|json|xml|yaml|zip|mp4|webm)\b/i;

  // 匹配"输出/生成/文件: /path/to/file.ext" 模式
  const pathRegex = /(?:输出目录[:：\s]*|生成[:：\s]*|文件[:：\s]*|output[:：\s]*|sandbox[:：\s]*)(.+?\.\w+)/gi;
  let match;
  while ((match = pathRegex.exec(result)) !== null) {
    const path = match[1].trim();
    if (extPattern.test(path)) {
      const name = path.split(/[\\/]/).pop() || path;
      const isImage = /\.(png|jpg|jpeg|gif|svg|webp|bmp)$/i.test(path);
      files.push({
        id: crypto.randomUUID(),
        path,
        name,
        kind: isImage ? "image" : "generated",
      });
    }
  }

  // 匹配 file:///path 或 /absolute/path 模式
  if (files.length === 0) {
    const absPathRegex = /(?:file:\/\/\/|output\s*(?:dir|directory)?[\s:]*)(\/[^\s,;]+\.\w+)/gi;
    while ((match = absPathRegex.exec(result)) !== null) {
      const path = match[1];
      const name = path.split(/[\\/]/).pop() || path;
      const isImage = /\.(png|jpg|jpeg|gif|svg|webp|bmp)$/i.test(path);
      if (!files.some((f) => f.path === path)) {
        files.push({
          id: crypto.randomUUID(),
          path,
          name,
          kind: isImage ? "image" : "generated",
        });
      }
    }
  }

  return files.slice(0, 5);
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

// ── 上下文 ─────────────────────────────────────────────────────────────────

const AgentContext = createContext<AgentContextValue | null>(null);

export function useAgent(): AgentContextValue {
  const ctx = useContext(AgentContext);
  if (!ctx) throw new Error("useAgent must be used within AgentProvider");
  return ctx;
}

// ── 内部类型 ──────────────────────────────────────────────────────────

interface SlotInternal {
  slot: AgentSlot;
  latestToolName: string;
}

// ── 提供者 ────────────────────────────────────────────────────────────────

export function AgentProvider({ children }: { children: ReactNode }) {
  const [slots, setSlots] = useState<Map<string, SlotInternal>>(new Map());
  const sessionToSlot = useRef<Map<string, string>>(new Map());
  const latestSlots = useRef(slots);
  latestSlots.current = slots;

  // 更新 slots 辅助函数（同步保持 ref）
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

  // ── 全局 ReAct 事件监听 ─────────────────────────────────────────
  useEffect(() => {
    let unsub: (() => void) | null = null;
    let cancelled = false;

    // 流式事件缓冲：text_delta/thinking 积累后 rAF 批量刷新，避免逐 token 触发 React 重渲染
    const eventBuffer = new Map<string, { text: string; thinking: string }>();
    let rafScheduled = false;

    const flushBuffer = () => {
      rafScheduled = false;
      if (eventBuffer.size === 0) return;
      const snapshot = new Map(eventBuffer);
      eventBuffer.clear();

      updateSlots((prev) => {
        const next = new Map(prev);
        for (const [sid, buf] of snapshot) {
          const internal = next.get(sid);
          if (!internal) continue;
          const slot: AgentSlot = {
            ...internal.slot,
            currentTrace: { ...internal.slot.currentTrace },
            messages: [...internal.slot.messages],
          };
          if (buf.text) {
            const last = slot.messages[slot.messages.length - 1];
            if (last && last.role === "assistant" && last.streaming) {
              slot.messages[slot.messages.length - 1] = { ...last, content: last.content + buf.text };
            } else {
              slot.messages = [...slot.messages, { id: nextId(), role: "assistant", content: buf.text, streaming: true }];
            }
          }
          if (buf.thinking) {
            slot.currentTrace = {
              ...slot.currentTrace,
              thinking: slot.currentTrace.thinking + buf.thinking,
            };
          }
          next.set(sid, { slot, latestToolName: internal.latestToolName });
        }
        return next;
      });
    };

    let rafId: number | null = null;
    const scheduleFlush = () => {
      if (!rafScheduled) {
        rafScheduled = true;
        rafId = requestAnimationFrame(() => flushBuffer());
      }
    };

    listenReActEvents((event) => {
      const eventSessionId = event.session_id || event.sessionId;
      if (!eventSessionId) return;
      const slotId = sessionToSlot.current.get(eventSessionId);
      if (!slotId) return;

      // text_delta / thinking → 入缓冲，rAF 批量 flush
      if (event.type === "text_delta") {
        const buf = eventBuffer.get(slotId) ?? { text: "", thinking: "" };
        buf.text += event.content;
        eventBuffer.set(slotId, buf);
        scheduleFlush();
        return;
      }

      if (event.type === "thinking") {
        const buf = eventBuffer.get(slotId) ?? { text: "", thinking: "" };
        buf.thinking += event.content;
        eventBuffer.set(slotId, buf);
        scheduleFlush();
        return;
      }

      // 非流式事件 → 先 flush 缓冲，再立即处理
      if (rafScheduled) {
        rafScheduled = false;
        flushBuffer();
      }

      updateSlots((prev) => {
        const next = new Map(prev);
        const internal = next.get(slotId);
        if (!internal) return prev;

        const slot: AgentSlot = { ...internal.slot, currentTrace: { ...internal.slot.currentTrace }, messages: [...internal.slot.messages] };
        let toolName = internal.latestToolName;

        switch (event.type) {
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
            // 更新轨迹
            const calls = [...slot.currentTrace.toolCalls];
            const last = calls[calls.length - 1];
            if (last && !last.result) {
              calls[calls.length - 1] = { ...last, result: event.result };
            }
            slot.currentTrace = { ...slot.currentTrace, toolCalls: calls };

            // 从搜索工具结果中提取 RAG 来源
            const newSources = extractSourcesFromToolResult(event.name || toolName || "tool", event.result);

            // 为最后一条流式助理消息构建隐藏上下文
            const lastMsg = slot.messages[slot.messages.length - 1];
            if (lastMsg && lastMsg.role === "assistant" && lastMsg.streaming) {
              const summary = summarizeToolResult(event.name || toolName || "tool", event.result);
              const existingSources = lastMsg.sources ?? [];
              // 从工具结果中提取生成的文件
              const newFiles = extractFilesFromToolResult(event.name || toolName || "tool", event.result);
              const existingFiles = lastMsg.attachments ?? [];
              slot.messages[slot.messages.length - 1] = {
                ...lastMsg,
                hiddenContext: [lastMsg.hiddenContext, summary].filter(Boolean).join("\n\n"),
                sources: newSources ? [...existingSources, ...newSources] : existingSources.length > 0 ? existingSources : undefined,
                attachments: [...existingFiles, ...newFiles],
              };
            }
            break;
          }

          case "done": {
            slot.messages = slot.messages.map((m) =>
              m.streaming ? { ...m, streaming: false } : m,
            );
            slot.loading = false;
            slot.currentTrace = { thinking: "", toolCalls: [], plan: null, currentStepIndex: null, totalSteps: 0, stepResults: {}, replanReason: null, plannerTimeoutMessage: null };

            // 验证层：对最后一条 assistant 消息执行验证
            const lastMsg = slot.messages[slot.messages.length - 1];
            if (lastMsg && lastMsg.role === "assistant" && lastMsg.content) {
              runVerification(lastMsg.content, "chat").then((res) => {
                updateMessages(slotId, (prev) =>
                  prev.map((m) =>
                    m.id === lastMsg.id ? { ...m, verificationReport: res.report } : m,
                  ),
                );
              }).catch(() => {});
            }
            break;
          }

          case "error": {
            slot.messages = slot.messages.map((m) =>
              m.streaming
                ? { ...m, content: m.content || "请求失败：" + event.message, streaming: false, error: true }
                : m,
            );
            slot.loading = false;
            slot.currentTrace = { thinking: "", toolCalls: [], plan: null, currentStepIndex: null, totalSteps: 0, stepResults: {}, replanReason: null, plannerTimeoutMessage: null };
            sessionToSlot.current.delete(eventSessionId);
            slot.sessionId = null;
            break;
          }

          case "plan_generated":
            slot.currentTrace = {
              ...slot.currentTrace,
              plan: event.steps,
              totalSteps: event.steps.length,
              currentStepIndex: 0,
            };
            break;

          case "step_start":
            slot.currentTrace = {
              ...slot.currentTrace,
              currentStepIndex: event.step_index,
            };
            break;

          case "step_result":
            slot.currentTrace = {
              ...slot.currentTrace,
              stepResults: {
                ...slot.currentTrace.stepResults,
                [event.step_index]: { result: event.result, success: event.success },
              },
            };
            break;

          case "replan":
            slot.currentTrace = {
              ...slot.currentTrace,
              replanReason: event.reason,
            };
            break;

          case "planner_timeout":
            slot.currentTrace = {
              ...slot.currentTrace,
              plannerTimeoutMessage: event.message,
            };
            break;

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
            slot.currentTrace = { thinking: "", toolCalls: [], plan: null, currentStepIndex: null, totalSteps: 0, stepResults: {}, replanReason: null, plannerTimeoutMessage: null };
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
      if (rafId != null) cancelAnimationFrame(rafId);
      unsub?.();
    };
  }, [updateSlots]);

  // ── 操作 ────────────────────────────────────────────────────────────

  const sendMessage = useCallback(
    async (slotId: string, text: string, options?: SendMessageOptions) => {
      const sid = nextId();
      const displayText = options?.displayText ?? text;
      const userMsg: AgentMessage = {
        id: nextId(),
        role: "user",
        content: displayText,
        attachments: options?.fileAttachments,
      };
      const assistantMsg: AgentMessage = { id: nextId(), role: "assistant", content: "", streaming: true };

      sessionToSlot.current.set(sid, slotId);

      updateSlots((prev) => {
        const next = new Map(prev);
        const internal = next.get(slotId) ?? { slot: createDefaultSlot(), latestToolName: "" };
        next.set(slotId, {
          slot: {
            messages: [...internal.slot.messages, userMsg, assistantMsg],
            loading: true,
            currentTrace: { thinking: "", toolCalls: [], plan: null, currentStepIndex: null, totalSteps: 0, stepResults: {}, replanReason: null, plannerTimeoutMessage: null },
            sessionId: sid,
          },
          latestToolName: "",
        });
        return next;
      });

      try {
        await agentChat(
          text,
          sid,
          options?.projectId,
          options?.riskProjectId,
          options?.history,
          options?.providerId,
          options?.attachments,
        );
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
      // 1. 标记为已回答
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

      // 2. 添加答案 + 占位消息
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
            currentTrace: { thinking: "", toolCalls: [], plan: null, currentStepIndex: null, totalSteps: 0, stepResults: {}, replanReason: null, plannerTimeoutMessage: null },
          },
        });
        return next;
      });

      // 3. 发送到后端
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

  // ── 上下文值 ──────────────────────────────────────────────────────

  // 从内部状态派生出响应式的 AgentSlot 只读映射 Map
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
