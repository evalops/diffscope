import type { Severity } from '../api/types'

const config: Record<Severity, { dot: string; text: string }> = {
  Error: { dot: 'bg-sev-error', text: 'text-sev-error' },
  Warning: { dot: 'bg-sev-warning', text: 'text-sev-warning' },
  Info: { dot: 'bg-sev-info', text: 'text-sev-info' },
  Suggestion: { dot: 'bg-sev-suggestion', text: 'text-sev-suggestion' },
}

export function SeverityBadge({ severity }: { severity: Severity }) {
  const c = config[severity]
  return (
    <span className={`inline-flex items-center gap-1.5 text-[11px] font-medium ${c.text}`}>
      <span className={`w-1.5 h-1.5 rounded-full ${c.dot}`} />
      {severity}
    </span>
  )
}

export function SeverityDot({ severity }: { severity: Severity }) {
  return <span className={`w-2 h-2 rounded-full ${config[severity].dot}`} />
}
