import { useCallback, useEffect, useRef } from "react"
import { useToast } from "@/components/Toast"
import { useAppError } from "@/contexts/AppErrorContext"

/**
 * Shared state and helpers for all RiskControl tabs.
 * Eliminates duplicated toast, appError, activeProjectRef, and projectId-cleanup patterns.
 */
export function useRiskControlTab(projectId: number | null) {
  const toast = useToast()
  const { showLlmKeyError } = useAppError()
  const activeProjectRef = useRef(projectId)

  useEffect(() => {
    activeProjectRef.current = projectId
  }, [projectId])

  /** Guard: early-return helper for null projectId operations. */
  const guard = useCallback((): boolean => {
    return projectId !== null
  }, [projectId])

  /** Guard with activeProjectRef check — call after async ops to verify we're still on the right project. */
  const isActive = useCallback((): boolean => {
    return activeProjectRef.current === projectId
  }, [projectId])

  return { toast, showLlmKeyError, activeProjectRef, guard, isActive }
}
