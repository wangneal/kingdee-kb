interface PageContainerProps {
  children: React.ReactNode
  className?: string
}

/**
 * Shared page content container.
 * Wraps `flex-1 overflow-y-auto p-6` used across 4+ pages.
 */
export default function PageContainer({ children, className }: PageContainerProps) {
  return (
    <div className={`flex-1 overflow-y-auto p-6${className ? ` ${className}` : ''}`}>
      {children}
    </div>
  )
}
