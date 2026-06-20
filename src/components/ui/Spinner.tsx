import { Loader2 } from "lucide-react"
import type { ComponentPropsWithoutRef } from "react"

/**
 * Shared spinner component.
 * Wraps `<Loader2 className="... animate-spin" />` used 58+ times across 17 files.
 * Supports all standard SVG/img props for size/color customization via className.
 */
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
