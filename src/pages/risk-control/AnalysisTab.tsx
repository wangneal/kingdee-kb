import { Brain, Loader2, Send } from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { DEFAULT_SLOT, useAgent } from "@/contexts/AgentContext"
import { PRODUCT_NAME } from "@/lib/constants"

export default function AnalysisTab({ projectId }: { projectId: number | null }) {
  const agent = useAgent()
  const slotId = `risk-analysis:${projectId ?? "none"}`
  const slot = agent.slots.get(slotId) ?? DEFAULT_SLOT
  const { messages, loading, currentTrace } = slot
  const [input, setInput] = useState("")
  const chatEndRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    void projectId
    setInput("")
  }, [projectId])

  // 自动滚动
  useEffect(() => {
    void messages.length
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" })
  }, [messages.length])

  const handleSend = useCallback(async () => {
    const text = input.trim()
    if (!text || loading || projectId === null) return
    setInput("")

    const prompt =
      `请作为 ${PRODUCT_NAME} 双轨风险把控舱中的风控专家分析以下问题，必要时使用知识库搜索、范围蔓延检查、项目健康评分、差异分析或防身话术工具，并给出专业、简洁、可执行的回答。\n\n问题：` +
      text
    await agent.sendMessage(slotId, prompt, {
      projectId,
    })
  }, [input, loading, projectId, agent, slotId])

  return (
    <div className="flex flex-col" style={{ height: "calc(100vh - 12rem)" }}>
      {/* 聊天消息 */}
      <div className="flex-1 space-y-3 overflow-y-auto rounded-lg border border-neutral-200 bg-white p-4">
        {messages.length === 0 && (
          <div className="flex h-full items-center justify-center">
            <div className="text-center">
              <Brain className="mx-auto mb-2 h-8 w-8 text-amber-400" />
              <p className="text-sm font-medium text-neutral-500">AI 深度风险分析</p>
              <p className="mt-1 text-xs text-neutral-400">
                输入问题开始分析项目风险、范围蔓延、客户沟通策略等
              </p>
            </div>
          </div>
        )}
        {messages.map((msg) => (
          <div
            key={msg.id}
            className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}
          >
            <div
              className={`max-w-[80%] rounded-lg px-3 py-2 text-xs leading-relaxed ${
                msg.role === "user" ? "bg-amber-600 text-white" : "bg-neutral-100 text-neutral-700"
              }`}
            >
              {msg.streaming && !msg.content ? (
                <span className="flex items-center gap-1">
                  <Loader2 className="h-3 w-3 animate-spin" />
                  分析中
                </span>
              ) : (
                <span className="whitespace-pre-wrap">{msg.content}</span>
              )}
            </div>
          </div>
        ))}

        {/* ReAct 轨迹：思考和工具调用 */}
        {loading && (currentTrace.thinking || currentTrace.toolCalls.length > 0) && (
          <div className="space-y-2 rounded-lg border border-amber-200 bg-amber-50/50 p-3">
            {currentTrace.thinking && (
              <details className="text-xs">
                <summary className="cursor-pointer font-medium text-amber-700 select-none">
                  🤔 思考过程
                </summary>
                <div className="mt-1 whitespace-pre-wrap leading-relaxed text-amber-800 italic">
                  {currentTrace.thinking}
                </div>
              </details>
            )}
            {currentTrace.toolCalls.map((tc) => (
              <div
                key={`${tc.name}-${tc.args}-${tc.result ?? ""}`}
                className="flex items-center gap-2 rounded-md border border-neutral-200 bg-white px-3 py-1.5 text-xs"
              >
                <span className="text-amber-600">🔧</span>
                <span className="font-medium text-neutral-700">{tc.name}</span>
                {tc.args && (
                  <span className="truncate text-neutral-400 max-w-[200px]" title={tc.args}>
                    {tc.args.length > 60 ? `${tc.args.slice(0, 60)}…` : tc.args}
                  </span>
                )}
                <span
                  className={`ml-auto rounded px-1.5 py-0.5 text-[10px] font-medium ${
                    tc.result ? "bg-green-100 text-green-700" : "bg-amber-100 text-amber-700"
                  }`}
                >
                  {tc.result ? "完成" : "执行中…"}
                </span>
              </div>
            ))}
          </div>
        )}

        <div ref={chatEndRef} />
      </div>

      {/* 输入 */}
      <div className="mt-3 flex gap-2">
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault()
              handleSend()
            }
          }}
          placeholder="输入风险分析问题..."
          className="flex-1 rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
          disabled={loading}
        />
        <button
          type="button"
          onClick={handleSend}
          disabled={loading || !input.trim() || projectId === null}
          className="flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
        >
          {loading ? <Loader2 className="h-3 w-3 animate-spin" /> : <Send className="h-3 w-3" />}
          发送
        </button>
      </div>
    </div>
  )
}
