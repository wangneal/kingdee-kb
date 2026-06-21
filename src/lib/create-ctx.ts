import { createContext, useContext } from "react"

export function createCtxTyped<Value>(displayName: string) {
  const Ctx = createContext<Value | null>(null)

  function useCtx(): Value {
    const ctx = useContext(Ctx)
    if (ctx == null) throw new Error(`use${displayName} must be used within ${displayName}Provider`)
    return ctx
  }

  return [Ctx, useCtx] as const
}
