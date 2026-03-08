import type { Severity } from '../api/types'

// Severity definitions
export const SEVERITIES: Severity[] = ['Error', 'Warning', 'Info', 'Suggestion']

export const SEV_COLORS: Record<Severity, string> = {
  Error: '#ef4444',
  Warning: '#f59e0b',
  Info: '#3b82f6',
  Suggestion: '#4ade80',
}

// Status badge styles (used in Dashboard + History)
export const STATUS_STYLES: Record<string, string> = {
  Complete: 'text-badge-completed bg-badge-completed/10',
  Failed: 'text-badge-failed bg-badge-failed/10',
  Running: 'text-accent bg-accent-bg',
  Pending: 'text-accent bg-accent-bg',
  Skipped: 'text-badge-skipped bg-badge-skipped/10',
}

// Chart theme
export const CHART_THEME = {
  accent: '#4ade80',
  grid: '#2e3a28',
  tick: '#5e6b58',
  tooltipBg: '#1a2117',
  tooltipBorder: '#2e3a28',
  tooltipText: '#9ca898',
} as const

// Refetch intervals (ms)
export const REFETCH = {
  status: 30_000,
  reviews: 5_000,
  activeReview: 2_000,
} as const

export const PAGE_SIZE = 10
