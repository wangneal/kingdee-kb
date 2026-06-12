/**
 * ASR（自动语音识别）Tauri 命令封装
 *
 * 对应 Rust 后端 commands::media 模块中的 5 个语音相关命令。
 * 所有类型定义严格匹配 Rust 结构体字段。
 */
import { invoke } from "@tauri-apps/api/core"

// ── 类型定义（匹配 Rust 结构体） ──────────────────────────────────────────

/** 转录片段（带时间戳） */
export interface TranscriptionSegment {
  /** 起始时间（毫秒） */
  start_ms: number
  /** 结束时间（毫秒） */
  end_ms: number
  /** 片段文本 */
  text: string
}

/** 完整转录结果 */
export interface TranscriptionResult {
  /** 合并后的完整文本 */
  text: string
  /** 带时间戳的分段结果 */
  segments: TranscriptionSegment[]
  /** 平均置信度（0-1） */
  confidence: number
  /** 处理耗时（毫秒） */
  processing_time_ms: number
}

/** Whisper 服务状态 */
export interface WhisperStatus {
  /** 模型是否已加载就绪 */
  model_loaded: boolean
  /** 当前模型大小标识（tiny/base/small） */
  model_size: string
  /** 语言代码（zh/en） */
  language: string
}

/** ASR 服务商信息 */
export interface AsrProviderInfo {
  /** 服务商类型标识 */
  type: string
  /** 显示名称 */
  name: string
  /** 功能描述 */
  description: string
  /** 是否支持流式识别 */
  supports_streaming: boolean
  /** 是否支持文件识别 */
  supports_file: boolean
}

/** 麦克风输入设备信息 */
export interface AudioInputDeviceInfo {
  /** 跨 Host 的设备标识 */
  id: string
  /** 系统设备名称 */
  name: string
  /** cpal Host 名称 */
  host: string
  /** 是否为系统默认输入设备 */
  is_default: boolean
}

// ── 命令封装 ─────────────────────────────────────────────────────────────

/**
 * 开始麦克风录音
 *
 * 调用后进入录音状态，需配合 stopRecording 停止并获取转录结果。
 */
export async function startRecording(deviceName?: string): Promise<void> {
  return invoke("start_whisper_recording", { deviceName: deviceName ?? null })
}

/**
 * 停止录音并转录音频
 *
 * @param provider - 可选 ASR 服务商（"tencent"），为空则使用本地 Whisper
 * @returns 转录结果，包含文本、分段、置信度和处理耗时
 */
export async function stopRecording(provider?: "tencent"): Promise<TranscriptionResult> {
  return invoke("stop_whisper_recording", { provider: provider ?? null })
}

/**
 * 加载 Whisper 模型
 *
 * @param modelSize - 模型大小，可选 "tiny" / "base" / "small"，默认 "base"
 */
export async function loadWhisperModel(modelSize: string = "base"): Promise<void> {
  return invoke("load_whisper_model", { modelSize })
}

/**
 * 获取 Whisper 服务当前状态
 *
 * @returns 包含模型加载状态、模型大小、语言等信息
 */
export async function getWhisperStatus(): Promise<WhisperStatus> {
  return invoke("get_whisper_status")
}

/**
 * 列出系统可见的麦克风输入设备
 *
 * @returns 输入设备信息列表
 */
export async function listAudioInputDevices(): Promise<AudioInputDeviceInfo[]> {
  return invoke("list_audio_input_devices")
}

/**
 * 列出所有可用的 ASR 服务商
 *
 * @returns 服务商信息列表（本地 Whisper、腾讯云等）
 */
export async function listAsrProviders(): Promise<AsrProviderInfo[]> {
  return invoke("list_asr_providers")
}
