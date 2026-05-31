/**
 * 剪贴板和拖拽文件提取工具
 *
 * 从 ClipboardEvent / DragEvent 提取文件，优先用 Tauri WebView2 的 File.path，
 * 无路径时写临时文件。
 */

import { tempDir } from '@tauri-apps/api/path';
import { mkdir, writeFile } from '@tauri-apps/plugin-fs';

export interface PastedFile {
  path: string;
  isTemp: boolean;
  mimeType: string;
  name: string;
  /** data URL for frontend preview (avoids asset protocol 403 on temp files) */
  previewUrl?: string;
}

const MIME_TO_EXT: Record<string, string> = {
  'image/png': '.png',
  'image/jpeg': '.jpg',
  'image/webp': '.webp',
  'image/gif': '.gif',
  'image/bmp': '.bmp',
};

const TEMP_DIR_NAME = 'kingdee-kb';

async function ensureTempDir(): Promise<string> {
  const base = await tempDir();
  const dir = `${base}${TEMP_DIR_NAME}`;
  await mkdir(dir, { recursive: true });
  return dir;
}

function getExt(file: File): string {
  if (file.type.startsWith('image/')) {
    return MIME_TO_EXT[file.type] ?? '.png';
  }
  const idx = file.name.lastIndexOf('.');
  if (idx > 0 && idx < file.name.length - 1) {
    return file.name.slice(idx).toLowerCase();
  }
  return MIME_TO_EXT[file.type] ?? '.png';
}

async function writeTempFile(file: File): Promise<PastedFile> {
  const dir = await ensureTempDir();
  const fileName = `paste-${Date.now()}-${Math.random().toString(36).slice(2, 8)}${getExt(file)}`;
  const filePath = `${dir}/${fileName}`;

  const buffer = await file.arrayBuffer();
  await writeFile(filePath, new Uint8Array(buffer));

  // Build data URL for preview so frontend avoids asset protocol 403
  let previewUrl: string | undefined;
  if (file.type.startsWith('image/')) {
    const base64 = btoa(String.fromCodePoint(...new Uint8Array(buffer)));
    previewUrl = `data:${file.type};base64,${base64}`;
  }

  return {
    path: filePath,
    isTemp: true,
    mimeType: file.type || 'application/octet-stream',
    name: file.name || fileName,
    previewUrl,
  };
}

async function processFile(file: File): Promise<PastedFile> {
  const tauriPath = (file as { path?: string }).path;
  if (typeof tauriPath === 'string' && tauriPath.length > 0) {
    return {
      path: tauriPath,
      isTemp: false,
      mimeType: file.type || 'application/octet-stream',
      name: file.name,
    };
  }
  return writeTempFile(file);
}

function extractFileList(files: FileList | null): File[] {
  if (!files || files.length === 0) return [];
  const result: File[] = [];
  for (let i = 0; i < files.length; i++) {
    if (files[i]) result.push(files[i]);
  }
  return result;
}

/** 从 paste 事件提取文件。不调用 preventDefault，由调用方决定。 */
export async function extractFilesFromPasteEvent(e: ClipboardEvent): Promise<PastedFile[]> {
  return Promise.all(extractFileList(e.clipboardData?.files ?? null).map(processFile));
}

/** 从 drop 事件提取文件。不调用 preventDefault，由调用方决定。 */
export async function extractFilesFromDropEvent(e: DragEvent): Promise<PastedFile[]> {
  return Promise.all(extractFileList(e.dataTransfer?.files ?? null).map(processFile));
}
