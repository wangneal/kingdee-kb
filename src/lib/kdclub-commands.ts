/**
 * 金蝶云社区 PAT (Personal Access Token) 命令封装
 *
 * 后端走系统钥匙串（tauri-plugin-keyring-store），不存 localStorage，
 * 避免敏感凭据明文留在用户目录的浏览器存储里。
 */
import { invoke } from "@tauri-apps/api/core"

/** 保存 PAT。空 token 等同于删除。 */
export async function saveKdclubToken(token: string): Promise<void> {
  return invoke("save_kdclub_token", { token: token.trim() || null })
}

/** 读取 PAT。未配置返回 null。 */
export async function getKdclubToken(): Promise<string | null> {
  return invoke<string | null>("get_kdclub_token")
}
