/**
 * data-cleaner.ts 的单元测试
 *
 * 覆盖：removeDuplicates / removeDuplicateLines / convertToMarkdown / checkDataQuality
 *
 * 这是项目第二个 vitest demo，验证：
 * 1. vitest 能跑多个 test file
 * 2. 泛型函数 + 异步无关的纯逻辑可单测
 */
import { describe, expect, it } from "vitest"
import {
  batchClean,
  checkDataQuality,
  convertToMarkdown,
  removeDuplicateLines,
  removeDuplicates,
} from "./data-cleaner"

describe("removeDuplicates", () => {
  it("应按 keyFn 去重并保持首次出现顺序", () => {
    const items = [
      { id: "a", name: "Alice" },
      { id: "b", name: "Bob" },
      { id: "a", name: "Alice2" }, // id 重复，应被去重
      { id: "c", name: "Charlie" },
    ]
    const result = removeDuplicates(items, (x) => x.id)
    expect(result.unique).toEqual([
      { id: "a", name: "Alice" },
      { id: "b", name: "Bob" },
      { id: "c", name: "Charlie" },
    ])
    expect(result.duplicates).toBe(1)
  })

  it("空数组应返回 unique=[] duplicates=0", () => {
    const result = removeDuplicates([] as number[], (x) => String(x))
    expect(result.unique).toEqual([])
    expect(result.duplicates).toBe(0)
  })

  it("全唯一应 duplicates=0", () => {
    const result = removeDuplicates([1, 2, 3], (x) => String(x))
    expect(result.unique).toEqual([1, 2, 3])
    expect(result.duplicates).toBe(0)
  })

  it("keyFn 决定是否重复：相同数字不同 keyFn 结果", () => {
    const items = [1, 2, 3, 4]
    const byValue = removeDuplicates(items, (x) => String(x))
    const alwaysUnique = removeDuplicates(items, () => "same")
    expect(byValue.duplicates).toBe(0)
    expect(alwaysUnique.unique.length).toBe(1)
    expect(alwaysUnique.duplicates).toBe(3)
  })
})

describe("removeDuplicateLines", () => {
  it("应去重完全相同的行（trim 后比较）", () => {
    const text = "line1\nline2\nline1\nline3\nline2"
    const result = removeDuplicateLines(text)
    expect(result.text).toBe("line1\nline2\nline3")
    expect(result.duplicates).toBe(2)
  })

  it("应忽略首尾空格差异", () => {
    const text = "hello\n  hello  \nworld"
    const result = removeDuplicateLines(text)
    expect(result.duplicates).toBe(1)
    // 保留首次出现的形式（"hello"）
    expect(result.text).toBe("hello\nworld")
  })

  it("空文本应返回空", () => {
    const result = removeDuplicateLines("")
    expect(result.text).toBe("")
    expect(result.duplicates).toBe(0)
  })
})

describe("convertToMarkdown", () => {
  it("HTML 应转 Markdown：标题 + 链接 + 列表", () => {
    const html = `
      <h1>标题一</h1>
      <p>这是 <strong>粗体</strong> 和 <em>斜体</em></p>
      <ul><li>项目1</li><li>项目2</li></ul>
      <a href="https://example.com">链接</a>
    `
    const md = convertToMarkdown(html, "html")
    expect(md).toContain("# 标题一")
    expect(md).toContain("**粗体**")
    expect(md).toContain("*斜体*")
    expect(md).toContain("- 项目1")
    expect(md).toContain("- 项目2")
    expect(md).toContain("[链接](https://example.com)")
  })

  it("HTML 列表应为有序列表当 <ol>", () => {
    const html = "<ol><li>一</li><li>二</li><li>三</li></ol>"
    const md = convertToMarkdown(html, "html")
    expect(md).toContain("1. 一")
    expect(md).toContain("2. 二")
    expect(md).toContain("3. 三")
  })

  it("HTML 代码块应被正确包裹", () => {
    const html = "<pre><code>const x = 1;</code></pre>"
    const md = convertToMarkdown(html, "html")
    expect(md).toContain("```")
    expect(md).toContain("const x = 1;")
  })

  it("JSON 应 pretty-print", () => {
    const json = '{"a":1,"b":[1,2,3]}'
    const md = convertToMarkdown(json, "json")
    // JSON 解析后被 stringify(2 缩进)
    expect(md).toContain('"a": 1')
    expect(md).toContain('"b": [')
  })

  it("非法 JSON 应原样返回（不抛错）", () => {
    const bad = "{not valid json"
    const md = convertToMarkdown(bad, "json")
    expect(md).toBe(bad)
  })

  it("plain 格式应原样返回", () => {
    const text = "plain text\nwith newline"
    expect(convertToMarkdown(text, "plain")).toBe(text)
  })
})

describe("checkDataQuality", () => {
  it("干净文本应得 100 分、0 issue", () => {
    const clean = "line1\nline2\nline3"
    const report = checkDataQuality(clean)
    expect(report.score).toBe(100)
    expect(report.issues).toEqual([])
    expect(report.duplicates).toBe(0)
  })

  it("应检测尾随空格", () => {
    const text = "line1 \nline2\nline3   "
    const report = checkDataQuality(text)
    const ts = report.issues.find((i) => i.type === "trailing_spaces")
    expect(ts).toBeDefined()
    if (!ts) return
    expect(ts.count).toBe(2)
  })

  it("应检测 Tab 字符", () => {
    const text = "line1\nline2\tindented\nline3"
    const report = checkDataQuality(text)
    const tab = report.issues.find((i) => i.type === "tab_characters")
    expect(tab).toBeDefined()
    if (!tab) return
    expect(tab.count).toBe(1)
  })

  it("应检测重复行", () => {
    const text = "a\nb\na\nc\nb"
    const report = checkDataQuality(text)
    const dup = report.issues.find((i) => i.type === "duplicate_lines")
    expect(dup).toBeDefined()
    if (!dup) return
    expect(dup.count).toBe(2)
    expect(report.duplicates).toBe(2)
  })

  it("应检测 mojibake 编码问题", () => {
    const text = "正常文本 â€™ 错误符号 Ã© 更多 Ã¼"
    const report = checkDataQuality(text)
    const enc = report.issues.find((i) => i.type === "encoding_issues")
    expect(enc).toBeDefined()
    if (!enc) return
    expect(enc.count).toBeGreaterThanOrEqual(3)
  })

  it("得分应随问题增多而下降", () => {
    const clean = "a\nb\nc"
    const dirty = "a \nb\na\nc\nb"
    expect(checkDataQuality(clean).score).toBeGreaterThan(checkDataQuality(dirty).score)
  })
})

describe("batchClean", () => {
  it("应返回 results 数组 + summary", () => {
    const texts = ["hello   world", "  leading", "trailing  ", "ok"]
    const out = batchClean(texts)
    expect(out.results).toHaveLength(4)
    expect(out.summary.total).toBe(4)
    expect(out.summary.cleaned).toBeGreaterThanOrEqual(3) // 至少 3 个被改了
    expect(out.summary.issues).toBeGreaterThan(0)
  })

  it("全空数组应 summary=0", () => {
    const out = batchClean([])
    expect(out.summary.total).toBe(0)
    expect(out.summary.cleaned).toBe(0)
  })
})
