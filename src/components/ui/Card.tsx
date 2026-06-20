interface CardProps {
  children: React.ReactNode
  className?: string
}

/**
 * Shared card wrapper.
 * Wraps `rounded-lg border border-neutral-200 bg-white p-4` used 67+ times across 16 files.
 */
export default function Card({ children, className }: CardProps) {
  return (
    <div className={`rounded-lg border border-neutral-200 bg-white p-4${className ? ` ${className}` : ''}`}>
      {children}
    </div>
  )
}
