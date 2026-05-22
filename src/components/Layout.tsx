import { NavLink, Outlet } from "react-router-dom";
import { BookOpen, Search, Upload, Settings, LayoutDashboard, MessageSquare } from "lucide-react";

const navItems = [
  { to: "/", icon: LayoutDashboard, label: "概览" },
  { to: "/browse", icon: BookOpen, label: "知识浏览" },
  { to: "/search", icon: Search, label: "检索" },
  { to: "/chat", icon: MessageSquare, label: "AI 对话" },
  { to: "/import", icon: Upload, label: "导入" },
  { to: "/settings", icon: Settings, label: "设置" },
];

export default function Layout() {
  return (
    <div className="flex h-screen bg-neutral-50">
      {/* Sidebar */}
      <aside className="flex w-56 flex-col border-r border-neutral-200 bg-white">
        {/* Logo */}
        <div className="flex h-14 items-center gap-2 border-b border-neutral-200 px-4">
          <div className="h-7 w-7 rounded-lg bg-[#1A6BD8] flex items-center justify-center">
            <BookOpen className="h-4 w-4 text-white" />
          </div>
          <span className="text-sm font-semibold text-neutral-800">KingdeeKB</span>
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
    </div>
  );
}
