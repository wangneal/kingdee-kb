interface PageContainerProps {
  children: React.ReactNode
  className?: string
}

export default function PageContainer({ children, className }: PageContainerProps) {
  return (
    <div className={`flex-1 overflow-y-auto p-6${className ? ` ${className}` : ''}`}>
      {children}
    </div>
  )
}
