import { NavLink, Outlet } from "react-router-dom";
import { useEffect, useRef } from "react";
import { BookOpen, Search, Upload, Settings, LayoutDashboard, MessageSquare, FileEdit, Package, ClipboardList, ShieldAlert } from "lucide-react";
import Spotlight from "./Spotlight";
import { reactChat, listenReActEvents } from "../lib/tauri-commands";

const LS_KEY_QUESTION = "kb_sidebar_question";
const LS_KEY_ANSWER = "kb_sidebar_answer";

const navItems = [
  { to: "/", icon: LayoutDashboard, label: "概览" },
  { to: "/browse", icon: BookOpen, label: "知识浏览" },
  { to: "/search", icon: Search, label: "检索" },
  { to: "/chat", icon: MessageSquare, label: "AI 对话" },
  { to: "/research", icon: ClipboardList, label: "调研助手" },
  { to: "/risk", icon: ShieldAlert, label: "风险把控" },
  { to: "/templates", icon: FileEdit, label: "文档生成" },
  { to: "/products", icon: Package, label: "产物管理" },
  { to: "/import", icon: Upload, label: "导入" },
  { to: "/settings", icon: Settings, label: "设置" },
];

export default function Layout() {
  const sideAnswerRef = useRef("");
  const sideSessionRef = useRef<string | null>(null);

  // Sidebar localStorage bridge: poll for questions from Tencent Meeting sidebar
  useEffect(() => {
    let cancelled = false;
    let unsub: (() => void) | null = null;

    listenReActEvents((event) => {
      // Support both snake_case and camelCase (Tauri v2 may convert)
      const eventSessionId = event.session_id || (event as any).sessionId;
      if (eventSessionId !== sideSessionRef.current) return;
      if (event.type === "text_delta") {
        sideAnswerRef.current += event.content;
      }
      if (event.type === "done") {
        const answer = sideAnswerRef.current;
        try {
          const raw = localStorage.getItem(LS_KEY_QUESTION);
          if (raw) {
            const q = JSON.parse(raw);
            localStorage.setItem(LS_KEY_ANSWER, JSON.stringify({ id: q.id, text: answer }));
          }
        } catch(e) { /* localStorage unavailable */ }
        sideAnswerRef.current = "";
        sideSessionRef.current = null;
      }
    }).then((fn) => {
      if (cancelled) { fn(); return; }
      unsub = fn;
    });

    const interval = setInterval(() => {
      try {
        const raw = localStorage.getItem(LS_KEY_QUESTION);
        if (!raw) return;
        const q = JSON.parse(raw);
        if (!q.text || !q.id) return;
        localStorage.removeItem(LS_KEY_QUESTION);
        sideAnswerRef.current = "";
        // Generate session ID first before calling reactChat
        const sid = `layout_${Date.now()}`;
        sideSessionRef.current = sid;
        reactChat(q.text, "你是一个金蝶ERP实施顾问。请给出专业、简洁的回答。", sid);
      } catch(e) { /* poll error */ }
    }, 2000);

    return () => {
      cancelled = true;
      unsub?.();
      clearInterval(interval);
    };
  }, []);

  return (
    <div className="flex h-screen bg-neutral-50">
      {/* Sidebar */}
      <aside className="flex w-56 flex-col border-r border-neutral-200 bg-white">
        {/* Logo */}
        <div className="flex h-14 items-center gap-2 border-b border-neutral-200 px-4">
          <div className="h-7 w-7 rounded-lg bg-[#1A6BD8] flex items-center justify-center">
            <BookOpen className="h-4 w-4 text-white" />
          </div>
          <span className="text-sm font-semibold text-neutral-800">实施顾问AI助手</span>
        </div>

        {/* Navigation */}
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
      </aside>

      {/* Main content */}
      <main className="flex-1 overflow-auto">
        <Outlet />
      </main>

      {/* Global Spotlight overlay */}
      <Spotlight />
    </div>
  );
}
