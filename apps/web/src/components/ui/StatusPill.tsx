export type StatusTone = 'info' | 'planned' | 'quiet'

interface StatusPillProps {
  readonly children: string
  readonly tone?: StatusTone
}

export function StatusPill({ children, tone = 'quiet' }: StatusPillProps) {
  return <span className={`status-pill status-pill--${tone}`}>{children}</span>
}
