/**
 * 类型定义
 */

export type ImportStatus = "idle" | "loading" | "success" | "error"

export interface Feedback {
  status: ImportStatus
  message: string
}
