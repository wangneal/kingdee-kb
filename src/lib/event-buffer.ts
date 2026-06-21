/**
 * Event buffer & dispatcher for the AgentContext streaming event system.
 *
 * Extracted from AgentContext.tsx to reduce complexity:
 *  - rAF-based text_delta / thinking accumulation (streaming batch)
 *  - Typed event handler map replacing the switch/case block
 *  - Early-return guards to flatten deep nesting
 *
 * The buffer/scheduler API is intentionally minimal — AgentContext owns
 * the slot map, the updateSlots callback, and all business helpers.
 */

import type { AgentMessage, AgentSlot, FileAttachment, RAGSource } from "@/contexts/AgentContext"
import type { AgentEvent } from "@/lib/tauri-commands"
import type { AppErrorPayload } from "@/lib/app-error"

// ── Constants ───────────────────────────────────────────────────────────

const DEFAULT_TRACE: AgentSlot["currentTrace"] = {
  thinking: "",
  toolCalls: [],
  plan: null,
  currentStepIndex: null,
  totalSteps: 0,
  stepResults: {},
  replanReason: null,
  plannerTimeoutMessage: null,
}

// ── Buffer / rAF Scheduler ──────────────────────────────────────────────

/** Per-slot accumulated buffer text */
interface BufferEntry {
  text: string
  thinking: string
}

/**
 * Create a buffered event dispatcher.
 *
 * Returns a scheduler object with:
 *  - `buffer` — the accumulated entries
 *  - `schedule()` — enqueue an rAF flush
 *  - `flush(applyFn)` — immediately flush entries via applyFn
 *  - `dispose()` — cancel any pending rAF and flush
 *  - `rafScheduled` — whether an rAF is pending
 */
export function createEventBuffer(flushApply: (entries: Map<string, BufferEntry>) => void) {
  const buffer = new Map<string, BufferEntry>()
  let rafScheduled = false
  let rafId: number | null = null

  const flush = (applyFn: (entries: Map<string, BufferEntry>) => void) => {
    rafScheduled = false
    if (buffer.size === 0) return
    if (rafId != null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
    const snapshot = new Map(buffer)
    buffer.clear()
    applyFn(snapshot)
  }

  const schedule = () => {
    if (!rafScheduled) {
      rafScheduled = true
      rafId = requestAnimationFrame(() => flush(flushApply))
    }
  }

  const dispose = () => {
    if (rafId != null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
    if (rafScheduled) {
      rafScheduled = false
      buffer.clear()
    }
  }

  return {
    buffer,
    schedule,
    flush,
    dispose,
    get rafScheduled() {
      return rafScheduled
    },
  }
}

// ── Slot copy helpers ───────────────────────────────────────────────────

/** Shallow-copy a slot with immutable traces & messages arrays. */
export function copySlot(slot: AgentSlot): AgentSlot {
  return {
    ...slot,
    currentTrace: { ...slot.currentTrace },
    messages: [...slot.messages],
  }
}

/** Mark the last streaming assistant message as non-streaming. */
export function markLastNonStreaming(slot: AgentSlot): AgentSlot {
  const msgs = slot.messages
  for (let i = msgs.length - 1; i >= 0; i--) {
    if (msgs[i].streaming) {
      const updated = [...msgs]
      updated[i] = { ...msgs[i], streaming: false, statusText: undefined }
      return { ...slot, messages: updated }
    }
  }
  return slot
}

// ── Typed event handler map ─────────────────────────────────────────────


/** Result of dispatching a single event against a slot copy. */
export interface DispatchResult {
  slot: AgentSlot
  toolName: string
}

/**
 * Create a typed event handler function.
 *
 * Each handler receives a copy of the slot and the event, returns the
 * updated slot plus the (possibly new) tool name.  The outer updateSlots
 * wrapper handles immutability and Map persistence.
 *
 * This replaces the ~110-line switch/case block inside listenAgentEvents.
 */
export function createEventHandlerMap(opts: {
  nextId: () => string
  extractSources: (toolName: string, result: string) => RAGSource[] | undefined
  summarizeTool: (toolName: string, result: string) => string
  extractFiles: (toolName: string, result: string) => FileAttachment[]
  showLlmKeyErrorRef: { current: (payload: AppErrorPayload) => void }
}): (slot: AgentSlot, event: AgentEvent, toolName: string) => DispatchResult {
  const { nextId, extractSources, summarizeTool, extractFiles, showLlmKeyErrorRef } = opts

  const handlers: Record<
    string,
    (slot: AgentSlot, event: AgentEvent, toolName: string) => DispatchResult
  > = {
    tool_call(slot, event) {
      if (event.type !== "tool_call") return { slot, toolName: "" }
      const name = event.name
      const trace = {
        ...slot.currentTrace,
        toolCalls: [
          ...slot.currentTrace.toolCalls,
          { name: event.name, args: event.args, result: "" },
        ],
      }
      const msgs = [...slot.messages]
      const last = msgs[msgs.length - 1]
      if (last && last.role === "assistant" && last.streaming && !last.content) {
        msgs[msgs.length - 1] = { ...last, statusText: `正在使用工具：${event.name}` }
      }
      return { slot: { ...slot, currentTrace: trace, messages: msgs }, toolName: name }
    },

    tool_result(slot, event, toolName) {
      if (event.type !== "tool_result") return { slot, toolName }
      const name = event.name || toolName || "tool"
      const calls = [...slot.currentTrace.toolCalls]
      const lastCall = calls[calls.length - 1]
      if (lastCall && !lastCall.result) {
        calls[calls.length - 1] = { ...lastCall, result: event.result }
      }
      const trace = { ...slot.currentTrace, toolCalls: calls }
      const sources = extractSources(name, event.result)
      const msgs = [...slot.messages]
      const lastMsg = msgs[msgs.length - 1]
      if (lastMsg && lastMsg.role === "assistant" && lastMsg.streaming) {
        const summary = summarizeTool(name, event.result)
        const existingSources = lastMsg.sources ?? []
        const newFiles = extractFiles(name, event.result)
        const existingFiles = lastMsg.attachments ?? []
        msgs[msgs.length - 1] = {
          ...lastMsg,
          statusText: lastMsg.content ? undefined : "正在整理结果并生成回答...",
          hiddenContext: [lastMsg.hiddenContext, summary].filter(Boolean).join("\n\n"),
          sources: sources
            ? [...existingSources, ...sources]
            : existingSources.length > 0
              ? existingSources
              : undefined,
          attachments: [...existingFiles, ...newFiles],
        }
      }
      return { slot: { ...slot, currentTrace: trace, messages: msgs }, toolName }
    },

    done(slot, event) {
      if (event.type !== "done") return { slot, toolName: "" }
      const updated = markLastNonStreaming(slot)
      return { slot: { ...updated, loading: false }, toolName: "" }
    },

    error(slot, event) {
      if (event.type !== "error") return { slot, toolName: "" }
      if (event.error_code === "LLM_INVALID_KEY") {
        showLlmKeyErrorRef.current({
          code: "LLM_INVALID_KEY",
          message: event.message,
          provider_id: event.provider_id,
        })
      }
      const msgs = slot.messages.map((m) =>
        m.streaming
          ? {
              ...m,
              content: m.content || `请求失败：${event.message}`,
              streaming: false,
              statusText: undefined,
              error: true,
            }
          : m,
      )
      return {
        slot: { ...slot, messages: msgs, loading: false, currentTrace: { ...DEFAULT_TRACE } },
        toolName: "",
      }
    },

    plan_generated(slot, event) {
      if (event.type !== "plan_generated") return { slot, toolName: "" }
      const trace = {
        ...slot.currentTrace,
        plan: event.steps,
        totalSteps: event.steps.length,
        currentStepIndex: 0,
      }
      return { slot: { ...slot, currentTrace: trace }, toolName: "" }
    },

    step_start(slot, event) {
      if (event.type !== "step_start") return { slot, toolName: "" }
      const trace = { ...slot.currentTrace, currentStepIndex: event.step_index }
      return { slot: { ...slot, currentTrace: trace }, toolName: "" }
    },

    step_result(slot, event) {
      if (event.type !== "step_result") return { slot, toolName: "" }
      const stepResults = {
        ...slot.currentTrace.stepResults,
        [event.step_index]: { result: event.result, success: event.success },
      }
      const trace = { ...slot.currentTrace, stepResults }
      return { slot: { ...slot, currentTrace: trace }, toolName: "" }
    },

    replan(slot, event) {
      if (event.type !== "replan") return { slot, toolName: "" }
      const trace = { ...slot.currentTrace, replanReason: event.reason }
      return { slot: { ...slot, currentTrace: trace }, toolName: "" }
    },

    planner_timeout(slot, event) {
      if (event.type !== "planner_timeout") return { slot, toolName: "" }
      const trace = { ...slot.currentTrace, plannerTimeoutMessage: event.message }
      return { slot: { ...slot, currentTrace: trace }, toolName: "" }
    },

    clarification(slot, event) {
      if (event.type !== "clarification") return { slot, toolName: "" }
      const payload = event.payload
      const last = slot.messages[slot.messages.length - 1]
      const clarMsg: AgentMessage = {
        id: nextId(),
        role: "assistant",
        content: payload.prompt,
        clarification: payload,
      }
      const msgs =
        last && last.role === "assistant" && last.streaming
          ? [...slot.messages.slice(0, -1), { ...last, streaming: false, ...clarMsg }]
          : [...slot.messages, clarMsg]
      return {
        slot: { ...slot, messages: msgs, loading: false, currentTrace: { ...DEFAULT_TRACE } },
        toolName: "",
      }
    },
  }

  // Return a dispatcher function
  return (slot: AgentSlot, event: AgentEvent, toolName: string): DispatchResult => {
    const h = handlers[event.type]
    if (h) return h(slot, event, toolName)
    return { slot, toolName }
  }
}

// ── Guard helpers ───────────────────────────────────────────────────────

/**
 * Early-return guard: extract session_id → slot_id mapping.
 * Returns null if the event should be ignored.
 */
export function resolveSlotId(
  event: AgentEvent,
  sessionToSlot: { current: Map<string, string> },
  cancelledSlots: { current: Set<string> },
): string | null {
  const eventSessionId = event.session_id || event.sessionId
  if (!eventSessionId) return null
  const slotId = sessionToSlot.current.get(eventSessionId)
  if (!slotId) return null
  if (cancelledSlots.current.has(slotId)) return null
  return slotId
}
