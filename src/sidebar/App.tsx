import { useState, useEffect, useRef } from "react";

const LS_KEY_QUESTION = "kb_sidebar_question";
const LS_KEY_ANSWER = "kb_sidebar_answer";

type Theme = "light" | "dark";

interface Message {
  id: number;
  role: "user" | "assistant";
  content: string;
}

let msgId = 0;

export default function SidebarApp() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [theme, setTheme] = useState<Theme>("light");
  const chatEndRef = useRef<HTMLDivElement>(null);

  // Detect Tencent Meeting dark mode
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    setTheme(mq.matches ? "dark" : "light");
    const handler = (e: MediaQueryListEvent) => setTheme(e.matches ? "dark" : "light");
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);

  // Poll for answers from desktop app via localStorage bridge
  useEffect(() => {
    const interval = setInterval(() => {
      try {
        const raw = localStorage.getItem(LS_KEY_ANSWER);
        if (!raw) return;
        const answer = JSON.parse(raw);
        localStorage.removeItem(LS_KEY_ANSWER);
        if (answer.text) {
          setMessages((prev) => [...prev, { id: ++msgId, role: "assistant", content: answer.text }]);
        }
      } catch { /* ignore */ }
    }, 1000);
    return () => clearInterval(interval);
  }, []);

  // Auto-scroll
  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const handleSend = () => {
    const text = input.trim();
    if (!text || loading) return;
    setInput("");

    // Add user message
    setMessages((prev) => [...prev, { id: ++msgId, role: "user", content: text }]);

    // Write question to localStorage for desktop app to process
    try {
      localStorage.setItem(LS_KEY_QUESTION, JSON.stringify({ id: msgId, text }));
      setLoading(true);
    } catch { /* localStorage unavailable */ }

    // Poll for answer (the interval above handles this)
    // If no answer within 30s, show timeout
    const timeout = setTimeout(() => {
      setLoading(false);
      setMessages((prev) => [...prev, { id: ++msgId, role: "assistant", content: "⚠️ 请求超时，请确保桌面端 KingdeeKB 正在运行" }]);
    }, 30000);

    // Listen for this specific answer
    const check = setInterval(() => {
      try {
        const raw = localStorage.getItem(LS_KEY_ANSWER);
        if (!raw) return;
        const answer = JSON.parse(raw);
        if (answer.id === msgId) {
          clearTimeout(timeout);
          clearInterval(check);
          localStorage.removeItem(LS_KEY_ANSWER);
          setLoading(false);
          setMessages((prev) => [...prev, { id: ++msgId, role: "assistant", content: answer.text }]);
        }
      } catch { /* ignore */ }
    }, 500);

    // Cleanup
    setTimeout(() => { clearInterval(check); clearTimeout(timeout); setLoading(false); }, 35000);
  };

  const isDark = theme === "dark";
  const bg = isDark ? "#1a1a2e" : "#ffffff";
  const fg = isDark ? "#e0e0e0" : "#404040";
  const inputBg = isDark ? "#16213e" : "#f5f5f5";
  const border = isDark ? "#2a2a4a" : "#e5e5e5";
  const accent = "#1A6BD8";
  const userBubble = isDark ? "#1A6BD8" : "#1A6BD8";
  const assistantBubble = isDark ? "#16213e" : "#f0f0f0";

  return (
    <div style={{ height: "100vh", display: "flex", flexDirection: "column", background: bg, color: fg }}>
      {/* Header */}
      <div style={{ padding: "12px 16px", borderBottom: `1px solid ${border}`, display: "flex", alignItems: "center", gap: 8 }}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke={accent} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z" />
          <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
        </svg>
        <span style={{ fontSize: 14, fontWeight: 600, color: accent }}>KingdeeKB 实施助手</span>
      </div>

      {/* Messages */}
      <div style={{ flex: 1, overflowY: "auto", padding: "12px 12px 8px" }}>
        {messages.length === 0 && (
          <div style={{ textAlign: "center", marginTop: 40, opacity: 0.5 }}>
            <p style={{ fontSize: 13, fontWeight: 500 }}>金蝶ERP实施助手</p>
            <p style={{ fontSize: 11, marginTop: 4 }}>输入问题，快速查找知识库和方案建议</p>
          </div>
        )}
        {messages.map((msg) => (
          <div key={msg.id} style={{ display: "flex", justifyContent: msg.role === "user" ? "flex-end" : "flex-start", marginBottom: 10 }}>
            <div style={{
              maxWidth: "85%",
              padding: "8px 12px",
              borderRadius: 12,
              fontSize: 13,
              lineHeight: 1.5,
              background: msg.role === "user" ? userBubble : assistantBubble,
              color: msg.role === "user" ? "#fff" : fg,
              borderTopRightRadius: msg.role === "user" ? 4 : 12,
              borderTopLeftRadius: msg.role === "user" ? 12 : 4,
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
            }}>
              {msg.content}
            </div>
          </div>
        ))}
        <div ref={chatEndRef} />
      </div>

      {/* Input */}
      <div style={{ padding: "8px 12px 12px", borderTop: `1px solid ${border}` }}>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); } }}
            placeholder="搜索知识库或提问..."
            disabled={loading}
            style={{
              flex: 1,
              padding: "8px 12px",
              borderRadius: 20,
              border: `1px solid ${border}`,
              background: inputBg,
              color: fg,
              fontSize: 13,
              outline: "none",
            }}
          />
          <button
            onClick={handleSend}
            disabled={loading || !input.trim()}
            style={{
              width: 36,
              height: 36,
              borderRadius: "50%",
              border: "none",
              background: loading ? "#ccc" : accent,
              color: "#fff",
              cursor: loading ? "not-allowed" : "pointer",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              opacity: loading || !input.trim() ? 0.5 : 1,
            }}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              {loading ? (
                <circle cx="12" cy="12" r="10" strokeDasharray="30 30" />
              ) : (
                <><line x1="22" y1="2" x2="11" y2="13" /><polygon points="22 2 15 22 11 13 2 9 22 2" /></>
              )}
            </svg>
          </button>
        </div>
      </div>
    </div>
  );
}
