/**
 * ImportTabs — Tab 栏定义与切换
 */

import { ClipboardPaste, FileText, FolderOpen } from "lucide-react"
import type { LucideIcon } from "lucide-react"

export type TabKey = "text" | "file" | "folder"

export interface TabDefinition {
  key: TabKey
  label: string
  icon: LucideIcon
}

export const TABS: TabDefinition[] = [
  { key: "text", label: "粘贴文本", icon: ClipboardPaste },
  { key: "file", label: "选择文件", icon: FileText },
  { key: "folder", label: "选择文件夹", icon: FolderOpen },
]

export interface ImportTabsProps {
  activeTab: TabKey
  onChange: (key: TabKey) => void
}

export default function ImportTabs({ activeTab, onChange }: ImportTabsProps) {
  return (
    <div className="flex border-b border-neutral-100">
      {TABS.map((tab) => {
        const Icon = tab.icon
        return (
          <button
            key={tab.key}
            type="button"
            onClick={() => onChange(tab.key)}
            className={`flex flex-1 items-center justify-center gap-1.5 px-4 py-2.5 text-xs font-medium transition-colors ${
              activeTab === tab.key
                ? "border-b-2 border-[#1A6BD8] text-[#1A6BD8]"
                : "text-neutral-500 hover:text-neutral-700"
            }`}
          >
            <Icon className="h-3.5 w-3.5" />
            {tab.label}
          </button>
        )
      })}
    </div>
  )
}
