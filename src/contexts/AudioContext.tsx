/**
 * 录音状态管理上下文
 *
 * 提供语音录音的完整生命周期管理：开始录音 → 转录中 → 返回文本。
 * 仅在研究助手页面（research 路由）内使用。
 */
import { createContext, type ReactNode, useCallback, useContext, useEffect, useState } from "react"
import { formatAppError } from "@/lib/app-error"
import { getWhisperStatus, loadWhisperModel, startRecording, stopRecording } from "@/lib/audio"

// ── 类型定义 ──────────────────────────────────────────────────────────────

/** 录音状态机：空闲 → 录音中 → 转录中 → 空闲（或错误） */
type RecordingStatus = "idle" | "recording" | "transcribing" | "error"

/** 上下文值接口 */
interface AudioContextValue {
  /** 当前录音状态 */
  status: RecordingStatus
  /** Whisper 模型是否已加载 */
  isModelLoaded: boolean
  /** 最近一次错误信息（成功操作后自动清除） */
  error: string | null
  /** 开始录音 */
  startAudioRecording: () => Promise<void>
  /** 停止录音并返回转录文本 */
  stopAudioRecording: () => Promise<string>
  /** 检查模型加载状态 */
  checkModelStatus: () => Promise<void>
  /** 加载 Whisper 模型 */
  loadModel: (size?: string) => Promise<void>
}

// ── 上下文 ────────────────────────────────────────────────────────────────

const AudioContext = createContext<AudioContextValue | null>(null)

/** 录音上下文 Hook */
export function useAudio(): AudioContextValue {
  const ctx = useContext(AudioContext)
  if (!ctx) throw new Error("useAudio must be used within AudioProvider")
  return ctx
}

// ── Provider ──────────────────────────────────────────────────────────────

export function AudioProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<RecordingStatus>("idle")
  const [isModelLoaded, setIsModelLoaded] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // ── 检查模型状态 ──
  const checkModelStatus = useCallback(async () => {
    try {
      const whisperStatus = await getWhisperStatus()
      setIsModelLoaded(whisperStatus.model_loaded)
    } catch (err) {
      // 首次检查可能失败（模型未下载），不设为错误状态
      console.warn("[AudioContext] 检查模型状态失败:", err)
    }
  }, [])

  // ── 加载模型 ──
  const loadModel = useCallback(
    async (size: string = "base") => {
      setError(null)
      try {
        await loadWhisperModel(size)
        await checkModelStatus()
      } catch (err) {
        const msg = formatAppError(err)
        setError(msg)
        console.error("[AudioContext] 加载模型失败:", msg)
      }
    },
    [checkModelStatus],
  )

  // ── 开始录音 ──
  const startAudioRecording = useCallback(async () => {
    setError(null)
    try {
      await startRecording()
      setStatus("recording")
    } catch (err) {
      const msg = formatAppError(err)
      setError(msg)
      setStatus("error")
      console.error("[AudioContext] 开始录音失败:", msg)
    }
  }, [])

  // ── 停止录音并转录 ──
  const stopAudioRecording = useCallback(async (): Promise<string> => {
    setStatus("transcribing")
    setError(null)
    try {
      const result = await stopRecording()
      setStatus("idle")
      return result.text
    } catch (err) {
      const msg = formatAppError(err)
      setError(msg)
      setStatus("error")
      console.error("[AudioContext] 转录失败:", msg)
      return ""
    }
  }, [])

  // ── 组件挂载时检查模型状态 ──
  useEffect(() => {
    checkModelStatus()
  }, [checkModelStatus])

  // ── 上下文值 ──
  const ctx: AudioContextValue = {
    status,
    isModelLoaded,
    error,
    startAudioRecording,
    stopAudioRecording,
    checkModelStatus,
    loadModel,
  }

  return <AudioContext.Provider value={ctx}>{children}</AudioContext.Provider>
}
