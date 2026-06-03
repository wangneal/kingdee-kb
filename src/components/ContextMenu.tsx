/**
 * 通用右键菜单组件
 *
 * 支持在指定屏幕坐标弹出菜单，包含图标、分隔线、危险操作样式。
 * 自动检测视口边界，点击外部或按 Esc 关闭。
 */

import { type ReactNode, useCallback, useEffect, useRef, useState } from "react"
import { createPortal } from "react-dom"

/** 菜单项接口 */
export interface ContextMenuItem {
  /** 唯一标识（可选，用于 key） */
  id?: string
  /** 显示文本 */
  label: string
  /** 左侧图标 */
  icon?: ReactNode
  /** 点击回调 */
  onClick: () => void
  /** 危险操作（红色样式） */
  danger?: boolean
  /** 禁用状态 */
  disabled?: boolean
  /** 分隔线（独占一行，忽略其他字段） */
  type?: "separator"
}

/** 组件 Props */
export interface ContextMenuProps {
  /** 弹出位置 x 坐标 */
  x: number
  /** 弹出位置 y 坐标 */
  y: number
  /** 菜单项列表 */
  items: ContextMenuItem[]
  /** 关闭回调 */
  onClose: () => void
}

/**
 * 通用右键菜单
 *
 * 使用 React Portal 渲染到 document.body，确保不被父容器裁剪。
 */
export default function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ x, y })

  // ── 计算菜单位置，确保不超出视口 ──
  const adjustPosition = useCallback(() => {
    const menu = menuRef.current
    if (!menu) return

    const rect = menu.getBoundingClientRect()
    const viewportWidth = window.innerWidth
    const viewportHeight = window.innerHeight
    const padding = 8 // 边界留白

    let adjustedX = x
    let adjustedY = y

    // 右边界检测
    if (x + rect.width > viewportWidth - padding) {
      adjustedX = viewportWidth - rect.width - padding
    }

    // 下边界检测
    if (y + rect.height > viewportHeight - padding) {
      adjustedY = viewportHeight - rect.height - padding
    }

    // 左边界检测
    if (adjustedX < padding) {
      adjustedX = padding
    }

    // 上边界检测
    if (adjustedY < padding) {
      adjustedY = padding
    }

    setPosition({ x: adjustedX, y: adjustedY })
  }, [x, y])

  // ── 监听外部点击和键盘事件关闭菜单 ──
  useEffect(() => {
    const handleMouseDown = (e: MouseEvent) => {
      // 点击菜单外部时关闭
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose()
      }
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose()
      }
    }

    // 使用 mousedown 而非 click，确保在 click 事件触发前就关闭
    document.addEventListener("mousedown", handleMouseDown)
    document.addEventListener("keydown", handleKeyDown)

    return () => {
      document.removeEventListener("mousedown", handleMouseDown)
      document.removeEventListener("keydown", handleKeyDown)
    }
  }, [onClose])

  // ── 菜单渲染后调整位置 ──
  useEffect(() => {
    adjustPosition()
  }, [adjustPosition])

  // ── 点击菜单项后关闭 ──
  const handleItemClick = useCallback(
    (item: ContextMenuItem) => {
      if (item.disabled || item.type === "separator") return
      item.onClick()
      onClose()
    },
    [onClose],
  )

  const menu = (
    <div
      ref={menuRef}
      role="menu"
      className="fixed z-[9999] min-w-[160px] rounded-md border border-neutral-200 bg-white py-1 shadow-lg"
      style={{
        left: `${position.x}px`,
        top: `${position.y}px`,
      }}
    >
      {items.map((item, index) => {
        // 分隔线
        if (item.type === "separator") {
          return <hr key={item.id ?? `separator-${index}`} className="my-1 border-neutral-100" />
        }

        return (
          <button
            key={item.id ?? item.label ?? index}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            onClick={() => handleItemClick(item)}
            className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm transition-colors outline-none ${
              item.disabled
                ? "cursor-not-allowed text-neutral-300"
                : item.danger
                  ? "text-red-600 hover:bg-red-50"
                  : "text-neutral-700 hover:bg-neutral-100"
            }`}
          >
            {/* 图标区域 */}
            {item.icon && (
              <span className="flex h-4 w-4 shrink-0 items-center justify-center">{item.icon}</span>
            )}
            {/* 标签 */}
            <span className="flex-1">{item.label}</span>
          </button>
        )
      })}
    </div>
  )

  return createPortal(menu, document.body)
}
