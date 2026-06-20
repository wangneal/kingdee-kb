import {
  createContext,
  useContext,
} from "react"

/**
 * Result of `createCtxProvider`: `[Provider, useXxx]` tuple.
 *
 * Provider wraps children in a React Context.
 * The generic hook throws if called outside the Provider.
 */
export function createCtxProvider<Value>() {
  const Ctx = createContext<Value | null>(null)

  function useCtx(): Value {
    const ctx = useContext(Ctx)
    if (ctx == null) {
      throw new Error(
        `useCtx must be used within a Provider wrapper`,
      )
    }
    return ctx
  }

  return [Ctx, useCtx] as const
}

/**
 * Typed variant — accepts a display name for clearer error messages.
 *
 * @example
 * ```ts
 * const [ProjectContext, useProject] = createCtxWithDisplayName<ProjectContextValue>()
 * // error: "useProject must be used within ProjectProvider"
 * ```
 */
export function createCtxTyped<Value>(displayName: string) {
  const Ctx = createContext<Value | null>(null)

  function useCtx(): Value {
    const ctx = useContext(Ctx)
    if (ctx == null) {
      throw new Error(`use${displayName} must be used within ${displayName}Provider`)
    }
    return ctx
  }

  return [Ctx, useCtx] as const
}
