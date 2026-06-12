/**
 * 数据清洗模块
 * 提供数据格式检测、清洗、去重、转换等功能
 *
 * 原始实现：data-cleaner/SKILL.md
 */

// ─── 类型定义 ──────────────────────────────────────────────

export interface CleaningRule {
  id: string
  name: string
  description: string
  category: "format" | "content" | "structure" | "encoding"
  enabled: boolean
}

export interface CleaningResult {
  original: string
  cleaned: string
  changes: {
    rule: string
    count: number
    examples: string[]
  }[]
  stats: {
    originalLength: number
    cleanedLength: number
    linesRemoved: number
    duplicatesRemoved: number
    encodingFixed: number
  }
}

export interface DataQualityReport {
  totalItems: number
  validItems: number
  invalidItems: number
  duplicates: number
  issues: {
    type: string
    count: number
    examples: string[]
  }[]
  score: number // 0-100
}

const CONTROL_CHARS_PATTERN = "[\\u0000-\\u0008\\u000B\\u000C\\u000E-\\u001F\\u007F]"

// ─── 清洗规则 ──────────────────────────────────────────────

export const CLEANING_RULES: CleaningRule[] = [
  // 格式规则
  {
    id: "trim_whitespace",
    name: "去除首尾空白",
    description: "去除每行首尾的空白字符",
    category: "format",
    enabled: true,
  },
  {
    id: "normalize_line_endings",
    name: "统一换行符",
    description: "将 CRLF 和 CR 统一为 LF",
    category: "format",
    enabled: true,
  },
  {
    id: "remove_empty_lines",
    name: "删除空行",
    description: "删除连续的空行，保留单个空行",
    category: "format",
    enabled: true,
  },
  {
    id: "normalize_spaces",
    name: "规范化空格",
    description: "将多个连续空格合并为一个",
    category: "format",
    enabled: true,
  },
  {
    id: "fix_indentation",
    name: "修复缩进",
    description: "统一使用空格缩进，移除 Tab",
    category: "format",
    enabled: true,
  },

  // 内容规则
  {
    id: "remove_html_tags",
    name: "移除HTML标签",
    description: "移除内容中的 HTML 标签",
    category: "content",
    enabled: false,
  },
  {
    id: "remove_markdown_links",
    name: "简化Markdown链接",
    description: "将 Markdown 链接转换为纯文本",
    category: "content",
    enabled: false,
  },
  {
    id: "remove_urls",
    name: "移除URL",
    description: "移除内容中的 URL",
    category: "content",
    enabled: false,
  },
  {
    id: "remove_email_addresses",
    name: "移除邮箱地址",
    description: "移除内容中的邮箱地址",
    category: "content",
    enabled: false,
  },
  {
    id: "remove_special_chars",
    name: "移除特殊字符",
    description: "移除不可打印的特殊字符",
    category: "content",
    enabled: true,
  },

  // 结构规则
  {
    id: "normalize_headers",
    name: "规范化标题",
    description: "确保标题格式一致",
    category: "structure",
    enabled: true,
  },
  {
    id: "fix_list_format",
    name: "修复列表格式",
    description: "统一列表符号和缩进",
    category: "structure",
    enabled: true,
  },
  {
    id: "normalize_code_blocks",
    name: "规范化代码块",
    description: "确保代码块格式一致",
    category: "structure",
    enabled: true,
  },

  // 编码规则
  {
    id: "fix_encoding",
    name: "修复编码",
    description: "修复常见的编码问题",
    category: "encoding",
    enabled: true,
  },
  {
    id: "normalize_unicode",
    name: "规范化Unicode",
    description: "统一 Unicode 字符表示",
    category: "encoding",
    enabled: true,
  },
]

// ─── 清洗函数 ──────────────────────────────────────────────

export function cleanText(text: string, rules?: CleaningRule[]): CleaningResult {
  const enabledRules = rules || CLEANING_RULES.filter((r) => r.enabled)
  let cleaned = text
  const changes: CleaningResult["changes"] = []

  // 1. 去除首尾空白
  if (enabledRules.some((r) => r.id === "trim_whitespace")) {
    const before = cleaned
    cleaned = cleaned
      .split("\n")
      .map((line) => line.trim())
      .join("\n")
    const count = before.length - cleaned.length
    if (count > 0) {
      changes.push({
        rule: "trim_whitespace",
        count: 1,
        examples: [`移除了 ${count} 个空白字符`],
      })
    }
  }

  // 2. 统一换行符
  if (enabledRules.some((r) => r.id === "normalize_line_endings")) {
    const before = cleaned
    cleaned = cleaned.replace(/\r\n/g, "\n").replace(/\r/g, "\n")
    const count = (before.match(/\r/g) || []).length
    if (count > 0) {
      changes.push({
        rule: "normalize_line_endings",
        count,
        examples: [`统一了 ${count} 个换行符`],
      })
    }
  }

  // 3. 删除空行
  if (enabledRules.some((r) => r.id === "remove_empty_lines")) {
    const before = cleaned
    cleaned = cleaned.replace(/\n{3,}/g, "\n\n")
    const beforeLines = before.split("\n").length
    const afterLines = cleaned.split("\n").length
    const count = beforeLines - afterLines
    if (count > 0) {
      changes.push({
        rule: "remove_empty_lines",
        count,
        examples: [`删除了 ${count} 个空行`],
      })
    }
  }

  // 4. 规范化空格
  if (enabledRules.some((r) => r.id === "normalize_spaces")) {
    const before = cleaned
    cleaned = cleaned.replace(/[^\S\n]+/g, " ")
    const count = before.length - cleaned.length
    if (count > 0) {
      changes.push({
        rule: "normalize_spaces",
        count: 1,
        examples: [`合并了 ${count} 个多余空格`],
      })
    }
  }

  // 5. 修复缩进
  if (enabledRules.some((r) => r.id === "fix_indentation")) {
    const before = cleaned
    cleaned = cleaned.replace(/\t/g, "  ")
    const count = (before.match(/\t/g) || []).length
    if (count > 0) {
      changes.push({
        rule: "fix_indentation",
        count,
        examples: [`替换了 ${count} 个 Tab 字符`],
      })
    }
  }

  // 6. 移除HTML标签
  if (enabledRules.some((r) => r.id === "remove_html_tags")) {
    const before = cleaned
    cleaned = cleaned.replace(/<[^>]+>/g, "")
    const count = (before.match(/<[^>]+>/g) || []).length
    if (count > 0) {
      changes.push({
        rule: "remove_html_tags",
        count,
        examples: [`移除了 ${count} 个 HTML 标签`],
      })
    }
  }

  // 7. 移除特殊字符
  if (enabledRules.some((r) => r.id === "remove_special_chars")) {
    const before = cleaned
    cleaned = cleaned.replace(new RegExp(CONTROL_CHARS_PATTERN, "g"), "")
    const count = before.length - cleaned.length
    if (count > 0) {
      changes.push({
        rule: "remove_special_chars",
        count,
        examples: [`移除了 ${count} 个特殊字符`],
      })
    }
  }

  // 8. 修复编码
  if (enabledRules.some((r) => r.id === "fix_encoding")) {
    const before = cleaned
    // 修复常见的编码问题
    cleaned = cleaned
      .replace(/â€™/g, "'")
      .replace(/â€œ/g, '"')
      .replace(/â€/g, '"')
      .replace(/â€"/g, "—")
      .replace(/â€"/g, "–")
      .replace(/Ã©/g, "é")
      .replace(/Ã¨/g, "è")
      .replace(/Ã /g, "à")
      .replace(/Ã¢/g, "â")
      .replace(/Ãª/g, "ê")
      .replace(/Ã®/g, "î")
      .replace(/Ã´/g, "ô")
      .replace(/Ã»/g, "û")
      .replace(/Ã±/g, "ñ")
      .replace(/Ã¼/g, "ü")
      .replace(/Ã¶/g, "ö")
      .replace(/Ã¤/g, "ä")
      .replace(/ÃŸ/g, "ß")
    const count = before.length - cleaned.length
    if (count > 0) {
      changes.push({
        rule: "fix_encoding",
        count,
        examples: [`修复了 ${count} 个编码问题`],
      })
    }
  }

  // 计算统计信息
  const originalLines = text.split("\n")
  const cleanedLines = cleaned.split("\n")

  return {
    original: text,
    cleaned,
    changes,
    stats: {
      originalLength: text.length,
      cleanedLength: cleaned.length,
      linesRemoved: originalLines.length - cleanedLines.length,
      duplicatesRemoved: 0,
      encodingFixed: changes.find((c) => c.rule === "fix_encoding")?.count || 0,
    },
  }
}

// ─── 去重函数 ──────────────────────────────────────────────

export function removeDuplicates<T>(
  items: T[],
  keyFn: (item: T) => string,
): { unique: T[]; duplicates: number } {
  const seen = new Set<string>()
  const unique: T[] = []
  let duplicates = 0

  for (const item of items) {
    const key = keyFn(item)
    if (seen.has(key)) {
      duplicates++
    } else {
      seen.add(key)
      unique.push(item)
    }
  }

  return { unique, duplicates }
}

export function removeDuplicateLines(text: string): { text: string; duplicates: number } {
  const lines = text.split("\n")
  const { unique, duplicates } = removeDuplicates(lines, (line) => line.trim())
  return {
    text: unique.join("\n"),
    duplicates,
  }
}

// ─── 格式转换 ──────────────────────────────────────────────

export function convertToMarkdown(text: string, sourceFormat: "html" | "plain" | "json"): string {
  switch (sourceFormat) {
    case "html":
      return htmlToMarkdown(text)
    case "json":
      return jsonToMarkdown(text)
    default:
      return text
  }
}

function htmlToMarkdown(html: string): string {
  let md = html

  // 标题
  md = md.replace(/<h1[^>]*>(.*?)<\/h1>/gi, "# $1\n\n")
  md = md.replace(/<h2[^>]*>(.*?)<\/h2>/gi, "## $1\n\n")
  md = md.replace(/<h3[^>]*>(.*?)<\/h3>/gi, "### $1\n\n")
  md = md.replace(/<h4[^>]*>(.*?)<\/h4>/gi, "#### $1\n\n")
  md = md.replace(/<h5[^>]*>(.*?)<\/h5>/gi, "##### $1\n\n")
  md = md.replace(/<h6[^>]*>(.*?)<\/h6>/gi, "###### $1\n\n")

  // 粗体和斜体
  md = md.replace(/<strong[^>]*>(.*?)<\/strong>/gi, "**$1**")
  md = md.replace(/<b[^>]*>(.*?)<\/b>/gi, "**$1**")
  md = md.replace(/<em[^>]*>(.*?)<\/em>/gi, "*$1*")
  md = md.replace(/<i[^>]*>(.*?)<\/i>/gi, "*$1*")

  // 链接
  md = md.replace(/<a[^>]*href="([^"]*)"[^>]*>(.*?)<\/a>/gi, "[$2]($1)")

  // 图片
  md = md.replace(/<img[^>]*src="([^"]*)"[^>]*alt="([^"]*)"[^>]*\/?>/gi, "![$2]($1)")
  md = md.replace(/<img[^>]*src="([^"]*)"[^>]*\/?>/gi, "![]($1)")

  // 列表
  md = md.replace(/<ul[^>]*>([\s\S]*?)<\/ul>/gi, (_, content) => {
    return content.replace(/<li[^>]*>(.*?)<\/li>/gi, "- $1\n")
  })
  md = md.replace(/<ol[^>]*>([\s\S]*?)<\/ol>/gi, (_outer: string, content: string) => {
    let i = 0
    return content.replace(/<li[^>]*>(.*?)<\/li>/gi, (_m: string, inner: string) => {
      i++
      return `${i}. ${inner}`
    })
  })

  // 段落
  md = md.replace(/<p[^>]*>(.*?)<\/p>/gi, "$1\n\n")

  // 换行
  md = md.replace(/<br\s*\/?>/gi, "\n")

  // 代码
  md = md.replace(/<code[^>]*>(.*?)<\/code>/gi, "`$1`")
  md = md.replace(/<pre[^>]*>([\s\S]*?)<\/pre>/gi, "```\n$1\n```")

  // 移除其他标签
  md = md.replace(/<[^>]+>/g, "")

  // 清理多余空行
  md = md.replace(/\n{3,}/g, "\n\n")

  return md.trim()
}

function jsonToMarkdown(json: string): string {
  try {
    const data = JSON.parse(json)
    return JSON.stringify(data, null, 2)
  } catch {
    return json
  }
}

// ─── 数据质量检查 ──────────────────────────────────────────────

export function checkDataQuality(text: string): DataQualityReport {
  const lines = text.split("\n")
  const issues: DataQualityReport["issues"] = []

  // 检查空行
  const emptyLines = lines.filter((line) => line.trim() === "").length
  if (emptyLines > 0) {
    issues.push({
      type: "empty_lines",
      count: emptyLines,
      examples: [`发现 ${emptyLines} 个空行`],
    })
  }

  // 检查尾随空格
  const trailingSpaces = lines.filter((line) => line !== line.trimEnd()).length
  if (trailingSpaces > 0) {
    issues.push({
      type: "trailing_spaces",
      count: trailingSpaces,
      examples: [`${trailingSpaces} 行有尾随空格`],
    })
  }

  // 检查 Tab 字符
  const tabLines = lines.filter((line) => line.includes("\t")).length
  if (tabLines > 0) {
    issues.push({
      type: "tab_characters",
      count: tabLines,
      examples: [`${tabLines} 行包含 Tab 字符`],
    })
  }

  // 检查重复行
  const { duplicates } = removeDuplicateLines(text)
  if (duplicates > 0) {
    issues.push({
      type: "duplicate_lines",
      count: duplicates,
      examples: [`发现 ${duplicates} 行重复内容`],
    })
  }

  // 检查特殊字符
  const specialChars = text.match(new RegExp(CONTROL_CHARS_PATTERN, "g"))
  if (specialChars && specialChars.length > 0) {
    issues.push({
      type: "special_characters",
      count: specialChars.length,
      examples: [`发现 ${specialChars.length} 个特殊字符`],
    })
  }

  // 检查编码问题
  const encodingIssues = text.match(/â€™|â€œ|â€|â€"|â€"|Ã©|Ã¨|Ã |Ã¢|Ãª|Ã®|Ã´|Ã»|Ã±|Ã¼|Ã¶|Ã¤|ÃŸ/g)
  if (encodingIssues && encodingIssues.length > 0) {
    issues.push({
      type: "encoding_issues",
      count: encodingIssues.length,
      examples: [`发现 ${encodingIssues.length} 个编码问题`],
    })
  }

  // 计算质量分数
  const totalIssues = issues.reduce((sum, issue) => sum + issue.count, 0)
  const maxIssues = lines.length * 2 // 假设每行最多2个问题
  const score = Math.max(0, Math.min(100, Math.round((1 - totalIssues / maxIssues) * 100)))

  return {
    totalItems: lines.length,
    validItems: lines.length - issues.reduce((sum, issue) => sum + issue.count, 0),
    invalidItems: issues.reduce((sum, issue) => sum + issue.count, 0),
    duplicates,
    issues,
    score,
  }
}

// ─── 批量处理 ──────────────────────────────────────────────

export function batchClean(
  texts: string[],
  rules?: CleaningRule[],
): { results: CleaningResult[]; summary: { total: number; cleaned: number; issues: number } } {
  const results = texts.map((text) => cleanText(text, rules))
  const summary = {
    total: texts.length,
    cleaned: results.filter((r) => r.changes.length > 0).length,
    issues: results.reduce((sum, r) => sum + r.changes.length, 0),
  }
  return { results, summary }
}
