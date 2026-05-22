import { invoke } from "@tauri-apps/api/core"

/**
 * Securely store an API key in the OS credential store (Windows Credential Manager).
 * The key never touches disk as plaintext JSON.
 *
 * @param service - Service identifier (e.g., "openai", "anthropic")
 * @param key - The API key to store
 */
export async function setApiKey(service: string, key: string): Promise<void> {
  await invoke("set_api_key", { service, key })
}

/**
 * Retrieve an API key from the OS credential store.
 *
 * @param service - Service identifier (e.g., "openai", "anthropic")
 * @returns The stored API key, or null if not found
 */
export async function getApiKey(service: string): Promise<string | null> {
  return invoke<string | null>("get_api_key", { service })
}

/**
 * Delete an API key from the OS credential store.
 *
 * @param service - Service identifier (e.g., "openai", "anthropic")
 */
export async function deleteApiKey(service: string): Promise<void> {
  await invoke("delete_api_key", { service })
}
