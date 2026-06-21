import { Loader2 } from "lucide-react"
import type { ComponentPropsWithoutRef } from "react"

export default function Spinner(props: ComponentPropsWithoutRef<"svg">) {
  return (
    <Loader2
      {...props}
      className={`h-4 w-4 animate-spin${
        props.className ? ` ${props.className}` : ''
      }`}
    />
  )
}
