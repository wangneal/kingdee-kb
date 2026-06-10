import {
  BookOpen,
  ClipboardList,
  FolderKanban,
  LayoutDashboard,
  MessageSquare,
  Network,
  Package,
  Search,
  Settings,
  ShieldAlert,
  Zap,
} from "lucide-react"
import { useEffect, useRef, useState } from "react"
import { NavLink, Outlet, useNavigate } from "react-router-dom"
import { useProject } from "../contexts/ProjectContext"
import {
  agentChat,
  getModelStatus,
  getStats,
  isLLMConfigured,
  listenReActEvents,
} from "../lib/tauri-commands"
import ProjectSwitcher from "./ProjectSwitcher"
import Spotlight from "./Spotlight"

const LS_KEY_QUESTION = "kb_sidebar_question"
const LS_KEY_ANSWER = "kb_sidebar_answer"

const navItems = [
  { to: "/", icon: LayoutDashboard, label: "概览" },
  { to: "/browse", icon: BookOpen, label: "知识库" },
  { to: "/search", icon: Search, label: "检索" },
  { to: "/chat", icon: MessageSquare, label: "AI 对话" },
  { to: "/research", icon: ClipboardList, label: "调研助手" },
  { to: "/risk", icon: ShieldAlert, label: "风险把控" },
  { to: "/products", icon: Package, label: "产物管理" },
  { to: "/projects", icon: FolderKanban, label: "项目管理" },
  // { to: "/import", icon: Upload, label: "导入" }, // 入口已移至知识库右键菜单，配置在设置页
  { to: "/skills", icon: Zap, label: "技能体系" },
  { to: "/graph", icon: Network, label: "知识图谱" },
  { to: "/settings", icon: Settings, label: "设置" },
]

type StatusLevel = "ok" | "warn" | "error" | "loading"

interface StatusItem {
  label: string
  level: StatusLevel
  detail?: string
  section: string
}

function StatusBar({ onNavigate }: { onNavigate: (path: string) => void }) {
  const { currentProjectId } = useProject()
  const [items, setItems] = useState<StatusItem[]>([
    { label: "LLM", level: "loading", section: "llm" },
    { label: "Embedding", level: "loading", section: "embedding" },
    { label: "知识库", level: "loading", section: "kb" },
  ])

  useEffect(() => {
    let cancelled = false

    async function check() {
      const results: StatusItem[] = []

      // LLM 状态
      try {
        const configured = await isLLMConfigured()
        results.push({
          label: "LLM",
          level: configured ? "ok" : "error",
          detail: configured ? "已配置" : "未配置",
          section: "llm",
        })
      } catch {
        results.push({ label: "LLM", level: "error", detail: "检测失败", section: "llm" })
      }

      // Embedding 状态
      try {
        const loaded = await getModelStatus()
        results.push({
          label: "Embedding",
          level: loaded ? "ok" : "warn",
          detail: loaded ? "已加载" : "未加载",
          section: "embedding",
        })
      } catch {
        results.push({
          label: "Embedding",
          level: "error",
          detail: "检测失败",
          section: "embedding",
        })
      }

      // 知识库状态
      try {
        const stats = await getStats(currentProjectId)
        results.push({
          label: "知识库",
          level: stats.document_count > 0 ? "ok" : "warn",
          detail: `${stats.document_count} 篇文档`,
          section: "kb",
        })
      } catch {
        results.push({ label: "知识库", level: "error", detail: "检测失败", section: "kb" })
      }

      if (!cancelled) setItems(results)
    }

    check()
    const interval = setInterval(check, 60_000)
    return () => {
      cancelled = true
      clearInterval(interval)
    }
  }, [currentProjectId])

  const dotColor: Record<StatusLevel, string> = {
    ok: "bg-green-500",
    warn: "bg-yellow-500",
    error: "bg-red-500",
    loading: "bg-neutral-300 animate-pulse",
  }

  return (
    <div className="border-t border-neutral-200 px-3 py-2.5 space-y-1">
      {items.map((item) => (
        <button
          key={item.label}
          type="button"
          onClick={() => onNavigate(`/settings?section=${item.section}`)}
          className="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-[11px] text-neutral-500 hover:bg-neutral-50 hover:text-neutral-700 transition-colors"
          title={`${item.label}: ${item.detail ?? ""}`}
        >
          <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${dotColor[item.level]}`} />
          <span className="font-medium">{item.label}</span>
          {item.detail && <span className="ml-auto truncate text-neutral-400">{item.detail}</span>}
        </button>
      ))}
    </div>
  )
}

export default function Layout() {
  const { currentProjectId } = useProject()
  const sideAnswerRef = useRef("")
  const sideSessionRef = useRef<string | null>(null)
  const sideQuestionIdRef = useRef<string | null>(null)
  const navigate = useNavigate()

  // 侧边栏 localStorage 桥接：定期检查腾讯会议侧边栏发来的问题
  useEffect(() => {
    let cancelled = false
    let unsub: (() => void) | null = null

    listenReActEvents((event) => {
      // 同时支持 snake_case 和 camelCase（Tauri v2 可能转换）
      const eventSessionId = event.session_id || event.sessionId
      if (eventSessionId !== sideSessionRef.current) return
      if (event.type === "text_delta") {
        sideAnswerRef.current += event.content
      }
      if (event.type === "error") {
        sideAnswerRef.current = event.message || "AI 回答失败"
      }
      if (event.type === "done" || event.type === "error") {
        const answer = sideAnswerRef.current || "AI 未返回有效内容"
        try {
          if (sideQuestionIdRef.current) {
            localStorage.setItem(
              LS_KEY_ANSWER,
              JSON.stringify({ id: sideQuestionIdRef.current, text: answer }),
            )
          }
        } catch {
          /* localStorage 不可用 */
        }
        sideAnswerRef.current = ""
        sideSessionRef.current = null
        sideQuestionIdRef.current = null
      }
    }).then((fn) => {
      if (cancelled) {
        fn()
        return
      }
      unsub = fn
    })

    const interval = setInterval(() => {
      try {
        const raw = localStorage.getItem(LS_KEY_QUESTION)
        if (!raw) return
        const q = JSON.parse(raw)
        if (!q.text || !q.id) return
        localStorage.removeItem(LS_KEY_QUESTION)
        sideAnswerRef.current = ""
        sideQuestionIdRef.current = q.id
        const sid = `layout_${Date.now()}`
        sideSessionRef.current = sid
        agentChat(q.text, sid, currentProjectId).catch((error) => {
          try {
            localStorage.setItem(
              LS_KEY_ANSWER,
              JSON.stringify({ id: q.id, text: `AI 回答失败：${String(error)}` }),
            )
          } catch {
            /* localStorage 不可用 */
          }
          sideAnswerRef.current = ""
          sideSessionRef.current = null
          sideQuestionIdRef.current = null
        })
      } catch {
        /* 轮询异常 */
      }
    }, 5000)

    return () => {
      cancelled = true
      unsub?.()
      clearInterval(interval)
    }
  }, [currentProjectId])

  return (
    <div className="flex h-screen bg-neutral-50">
      {/* 侧边栏 */}
      <aside className="flex w-56 flex-col border-r border-neutral-200 bg-white">
        {/* 标识 */}
        <div className="flex h-14 items-center gap-2 border-b border-neutral-200 px-4">
          <div className="h-7 w-7 rounded-lg bg-[#1A6BD8] flex items-center justify-center">
            <BookOpen className="h-4 w-4 text-white" />
          </div>
          <span className="text-sm font-semibold text-neutral-800">实施顾问AI助手</span>
        </div>

        <ProjectSwitcher />

        {/* 导航 */}
        <nav className="flex-1 space-y-1 p-3">
          {navItems.map(({ to, icon: Icon, label }) => (
            <NavLink
              key={to}
              to={to}
              end={to === "/"}
              className={({ isActive }) =>
                `flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                  isActive
                    ? "bg-[#1A6BD8]/10 text-[#1A6BD8]"
                    : "text-neutral-600 hover:bg-neutral-100 hover:text-neutral-800"
                }`
              }
            >
              <Icon className="h-4 w-4" />
              {label}
            </NavLink>
          ))}
        </nav>

        {/* 状态指示器 */}
        <StatusBar onNavigate={navigate} />
      </aside>

      {/* 主内容 */}
      <main className="flex min-h-0 flex-1 flex-col overflow-auto">
        <Outlet />
      </main>

      {/* 全局 Spotlight 浮层 */}
      <Spotlight />
    </div>
  )
}
