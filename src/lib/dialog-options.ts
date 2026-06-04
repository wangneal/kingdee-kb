import { documentDir, homeDir } from "@tauri-apps/api/path"

export async function getImportDialogDefaultPath(): Promise<string | undefined> {
  try {
    return await documentDir()
  } catch (documentError) {
    console.warn("读取文档目录失败，改用用户主目录", documentError)
  }

  try {
    return await homeDir()
  } catch (homeError) {
    console.warn("读取用户主目录失败，将使用系统默认目录", homeError)
    return undefined
  }
}
