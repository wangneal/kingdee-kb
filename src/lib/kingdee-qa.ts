/**
 * 金蝶产品智能问答模块
 * 基于金蝶云社区 COSMIC API 实现
 *
 * 原始实现：kdclub-ai-product-qa/scripts/cosmic_qa.py
 */

// ─── 类型定义 ──────────────────────────────────────────────

export interface KingdeeProduct {
  productId: number
  name: string
}

export interface QASource {
  title: string
  url: string
  snippet?: string
}

export interface QAResponse {
  type: "start" | "think" | "answer" | "end" | "error"
  content?: string
  sessionId?: string
  fullAnswer?: string
  answerFormat?: "html" | "markdown"
  thinkContent?: string
  searchSources?: QASource[]
  error?: string
  errorCode?: string
}

export interface TokenStatus {
  valid: boolean
  tokenPreview?: string
  tokenLength?: number
  file?: string
  error?: string
}

export interface QAConfig {
  token: string
  productId: number
  sessionId?: string
  deepThink?: boolean
}

// ─── 产品列表 ──────────────────────────────────────────────

export const KINGDEE_PRODUCTS: KingdeeProduct[] = [
  { productId: 3, name: "金蝶AI星瀚" },
  { productId: 93, name: "金蝶AI套件" },
  { productId: 9, name: "金蝶AI星辰" },
  { productId: 87, name: "金蝶AI苍穹" },
  { productId: 1, name: "金蝶AI星空企业版/标准版" },
  { productId: 11, name: "EAS Cloud" },
  { productId: 16, name: "S-HR Cloud" },
  { productId: 15, name: "精斗云-云会计" },
  { productId: 98, name: "精斗云-云进销存" },
  { productId: 38, name: "账无忧" },
]

// ─── Token 管理 ──────────────────────────────────────────────

const TOKEN_STORAGE_KEY = "kingdee_kb_pat_token"

export function saveToken(token: string): void {
  const data = {
    token: token.trim(),
    domain: "vip.kingdee.com",
    lastUpdated: new Date().toISOString(),
  }
  localStorage.setItem(TOKEN_STORAGE_KEY, JSON.stringify(data))
}

export function loadToken(): string | null {
  // 1. 从本地存储加载
  const raw = localStorage.getItem(TOKEN_STORAGE_KEY)
  if (raw) {
    try {
      const data = JSON.parse(raw)
      if (data.token) {
        return data.token.trim()
      }
    } catch {
      // ignore
    }
  }
  return null
}

export function getTokenStatus(): TokenStatus {
  const token = loadToken()
  if (!token) {
    return {
      valid: false,
      error: "未找到有效的 PAT Token。请在设置中配置。",
    }
  }

  const preview =
    token.length > 12 ? `${token.substring(0, 8)}...${token.substring(token.length - 4)}` : "***"

  return {
    valid: true,
    tokenPreview: preview,
    tokenLength: token.length,
  }
}

// ─── 图片URL修复 ──────────────────────────────────────────────

function fixImageUrls(content: string): string {
  const base = "https://vip.kingdee.com"

  // 1. 补全相对路径（/xxx → https://vip.kingdee.com/xxx）
  content = content.replace(/src="\/(?!\/)([^"]+)"/g, `src="${base}/$1"`)
  content = content.replace(/src='\/(?!\/)([^']+)'/g, `src='${base}/$1'`)
  content = content.replace(/data-src="\/(?!\/)([^"]+)"/g, `data-src="${base}/$1"`)
  content = content.replace(/data-src='\/(?!\/)([^']+)'/g, `data-src='${base}/$1'`)

  // 2. 懒加载 data-src → src 提升
  content = content.replace(/<img\s[^>]*?data-src=["']([^"']+)["'][^>]*?\/?>/gi, (match, url) => {
    if (
      match.includes('src="') &&
      !match.includes('src="data:') &&
      !match.includes("placeholder")
    ) {
      return match
    }
    return match.replace(/src="[^"]*"/, `src="${url}"`).replace(/src='[^']*'/, `src='${url}'`)
  })

  // 3. Markdown 图片相对路径补全
  content = content.replace(/!\[([^\]]*)\]\(\/(?!\/)([^)]+)\)/g, `![$1](${base}/$2)`)

  return content
}

// ─── 流式问答 ──────────────────────────────────────────────

export async function* streamKingdeeQA(
  question: string,
  config: QAConfig,
  signal?: AbortSignal,
): AsyncGenerator<QAResponse> {
  const { token, productId, sessionId, deepThink } = config

  const params = new URLSearchParams({
    scene: "1",
    searchText: question,
    productId: String(productId),
    useDeepThink: deepThink ? "true" : "false",
    useClarification: "false",
    productLineId: "35",
    channel_level: "Agent Skill",
  })

  if (sessionId) {
    params.set("sessionId", sessionId)
  }

  const url = `https://vip.kingdee.com/aisapi/ai-search?${params.toString()}`

  try {
    const response = await fetch(url, {
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: "text/event-stream",
      },
      signal,
    })

    if (!response.ok) {
      if (response.status === 401 || response.status === 403) {
        yield {
          type: "error",
          errorCode: "UNAUTHORIZED",
          error: "未授权操作，PAT Token 可能已过期或无效。请重新配置。",
        }
        return
      }
      const body = await response.text()
      yield {
        type: "error",
        error: `HTTP ${response.status}: ${response.statusText}。${body}`,
      }
      return
    }

    const reader = response.body?.getReader()
    if (!reader) {
      yield { type: "error", error: "无法读取响应流" }
      return
    }

    const decoder = new TextDecoder()
    let buffer = ""
    let fullMessage = ""
    let thinkContent = ""
    let finalSessionId = ""
    let searchSources: QASource[] = []

    yield { type: "start" }

    while (true) {
      const { done, value } = await reader.read()
      if (done) break

      buffer += decoder.decode(value, { stream: true })
      const lines = buffer.split("\n")
      buffer = lines.pop() || ""

      for (const line of lines) {
        if (!line.startsWith("data:")) continue

        try {
          const data = JSON.parse(line.slice(5).trim())

          if (data.message === "未授权操作") {
            yield {
              type: "error",
              errorCode: "UNAUTHORIZED",
              error: "未授权操作，PAT Token 可能已过期或无效。",
            }
            return
          }

          if (data.isThink && data.message) {
            thinkContent += data.message
            yield {
              type: "think",
              content: data.message,
            }
          } else if (data.message) {
            const fixedMsg = fixImageUrls(data.message)
            fullMessage += fixedMsg
            yield {
              type: "answer",
              content: fixedMsg,
            }
          }

          if (data.aiSearchSessionId) {
            finalSessionId = String(data.aiSearchSessionId)
          }

          if (
            data.searchSources &&
            Array.isArray(data.searchSources) &&
            data.searchSources.length > 0
          ) {
            searchSources = data.searchSources
          }

          if (data.answerEnd) {
            fullMessage = fixImageUrls(fullMessage)
            yield {
              type: "end",
              sessionId: finalSessionId,
              fullAnswer: fullMessage,
              answerFormat: fullMessage.trim().startsWith("<") ? "html" : "markdown",
              thinkContent,
              searchSources,
            }
            return
          }
        } catch {
          // skip invalid JSON
        }
      }
    }

    // 没有 answerEnd 也输出结果
    fullMessage = fixImageUrls(fullMessage)
    yield {
      type: "end",
      sessionId: finalSessionId,
      fullAnswer: fullMessage,
      answerFormat: fullMessage.trim().startsWith("<") ? "html" : "markdown",
      thinkContent,
      searchSources,
    }
  } catch (err: unknown) {
    if (err instanceof Error && err.name === "AbortError") {
      return
    }
    yield {
      type: "error",
      error: `请求异常: ${err instanceof Error ? err.message : String(err)}`,
    }
  }
}

// ─── 便捷函数 ──────────────────────────────────────────────

export async function askKingdeeQuestion(
  question: string,
  productId: number,
  options?: {
    sessionId?: string
    deepThink?: boolean
    onThink?: (content: string) => void
    onAnswer?: (content: string) => void
    onSources?: (sources: QASource[]) => void
    signal?: AbortSignal
  },
): Promise<{ answer: string; sessionId?: string; sources?: QASource[] }> {
  const token = loadToken()
  if (!token) {
    throw new Error("未配置 PAT Token，请在设置中配置。")
  }

  let answer = ""
  let sessionId = ""
  let sources: QASource[] = []

  for await (const event of streamKingdeeQA(
    question,
    {
      token,
      productId,
      sessionId: options?.sessionId,
      deepThink: options?.deepThink,
    },
    options?.signal,
  )) {
    switch (event.type) {
      case "think":
        options?.onThink?.(event.content || "")
        break
      case "answer":
        answer += event.content || ""
        options?.onAnswer?.(event.content || "")
        break
      case "end":
        answer = event.fullAnswer || answer
        sessionId = event.sessionId || ""
        sources = event.searchSources || []
        options?.onSources?.(sources)
        break
      case "error":
        throw new Error(event.error)
    }
  }

  return { answer, sessionId, sources }
}
