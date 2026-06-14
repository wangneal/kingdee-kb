// 覆盖：null / 0边 / wikilink=0 / 完全正常
import { render } from "@testing-library/react"
import { describe, expect, it } from "vitest"
import type { GraphStats } from "@/lib/tauri-commands"
import GraphStatsBanner, { getBannerContent } from "./GraphStatsBanner"

function makeStats(overrides: Partial<GraphStats> = {}): GraphStats {
  return {
    total_edges: 10,
    total_nodes: 5,
    signal_breakdown: { tag: 8, source: 2 },
    avg_degree: 1.6,
    ...overrides,
  }
}

describe("getBannerContent", () => {
  it("stats 为 null 时不显示 banner", () => {
    expect(getBannerContent(null)).toBeNull()
  })

  it("total_edges === 0 时显示 info banner（提示构建图谱）", () => {
    const stats = makeStats({ total_edges: 0, total_nodes: 0, signal_breakdown: {} })
    const content = getBannerContent(stats)
    expect(content).not.toBeNull()
    if (!content) return
    expect(content.variant).toBe("info")
    expect(content.title).toContain("图谱尚未构建")
  })

  it("total_edges > 0 但 wikilink=0 时显示 warning banner", () => {
    const stats = makeStats({
      total_edges: 10,
      signal_breakdown: { tag: 8, source: 2 }, // 没有 wikilink
    })
    const content = getBannerContent(stats)
    expect(content).not.toBeNull()
    if (!content) return
    expect(content.variant).toBe("warning")
    expect(content.title).toContain("wikilink")
    expect(content.body).toContain("[[slug]]")
  })

  it("total_edges > 0 且 wikilink > 0 时不显示 banner（正常工作）", () => {
    const stats = makeStats({
      total_edges: 10,
      signal_breakdown: { wikilink: 3, tag: 5, source: 2 },
    })
    expect(getBannerContent(stats)).toBeNull()
  })

  it("signal_breakdown 完全为空对象时按 wikilink=0 处理（warning）", () => {
    const stats = makeStats({ total_edges: 5, signal_breakdown: {} })
    const content = getBannerContent(stats)
    expect(content).not.toBeNull()
    if (!content) return
    expect(content.variant).toBe("warning")
  })

  it("wikilink 字段缺失时按 0 处理", () => {
    const stats = makeStats({ total_edges: 5, signal_breakdown: { tag: 5 } })
    const content = getBannerContent(stats)
    expect(content).not.toBeNull()
    if (!content) return
    expect(content.variant).toBe("warning")
  })
})

describe("GraphStatsBanner 组件", () => {
  it("stats=null 时渲染空（无 DOM 元素）", () => {
    const { container } = render(<GraphStatsBanner stats={null} />)
    expect(container.firstChild).toBeNull()
  })

  it("info banner 渲染蓝色样式", () => {
    const { container } = render(
      <GraphStatsBanner stats={makeStats({ total_edges: 0, total_nodes: 0 })} />,
    )
    // 用 querySelector 避开 React 19 strict mode 双渲染 + getByTestId 多元素报错
    const banner = container.querySelector('[data-variant="info"]')
    expect(banner).not.toBeNull()
    if (!banner) return
    expect(banner.className).toContain("border-sky-200")
    expect(banner.className).toContain("bg-sky-50")
  })

  it("warning banner 渲染黄色样式 + 包含关键提示词", () => {
    const { container } = render(
      <GraphStatsBanner stats={makeStats({ total_edges: 5, signal_breakdown: { tag: 5 } })} />,
    )
    const banner = container.querySelector('[data-variant="warning"]')
    expect(banner).not.toBeNull()
    if (!banner) return
    expect(banner.className).toContain("border-amber-200")
    expect(banner.className).toContain("bg-amber-50")
    expect(banner.textContent).toMatch(/wikilink/i)
    expect(banner.textContent).toMatch(/\[\[slug\]\]/)
  })

  it("正常 stats 不渲染 banner", () => {
    const { container } = render(
      <GraphStatsBanner
        stats={makeStats({ total_edges: 10, signal_breakdown: { wikilink: 3 } })}
      />,
    )
    expect(container.firstChild).toBeNull()
  })
})
