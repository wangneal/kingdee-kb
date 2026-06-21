/**
 * 前端共享工具函数
 *
 * 从 Chat.tsx、AgentContext.tsx 等文件中提取的重复逻辑。
 */

/** 判断是否为图片文件 */
export function isImageFile(name: string): boolean {
  const ext = name.split(".").pop()?.toLowerCase() ?? ""
  return ["png", "jpg", "jpeg", "webp", "bmp", "gif", "svg"].includes(ext)
}

/** 格式化文件大小 */
export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
