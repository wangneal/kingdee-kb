import type { ComponentPropsWithoutRef } from "react"

export default function Input(props: ComponentPropsWithoutRef<"input">) {
  return (
    <input
      {...props}
      className={`w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20${
        props.className ? ` ${props.className}` : ''
      }`}
    />
  )
}
