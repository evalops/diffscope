import { MODEL_PRESETS } from './models'
import type { CostBreakdownRow, ReviewEvent } from '../api/types'

// Parse price string like "$3", "$0.25", "free" into per-million-token cost
function parsePricePerMillion(price: string): number {
  if (price === 'free') return 0
  const match = price.match(/\$?([\d.]+)/)
  return match ? parseFloat(match[1]) : 0
}

// Build a lookup: normalized model fragment -> price per million tokens
const priceLookup: [string[], number][] = MODEL_PRESETS.map(p => {
  // Extract recognizable fragments from the preset ID
  // e.g. "anthropic/claude-sonnet-4.5" -> ["claude-sonnet-4.5", "claude-sonnet", "sonnet-4.5"]
  const parts = p.id.split('/')
  const modelPart = parts[parts.length - 1].toLowerCase()
  const fragments = [modelPart]
  // Also store without version suffixes
  const noVersion = modelPart.replace(/[-.]?\d+(\.\d+)*$/, '')
  if (noVersion && noVersion !== modelPart) fragments.push(noVersion)
  return [fragments, parsePricePerMillion(p.price)]
})

/** Estimate cost in USD for a review event. Prefer server-side cost_estimate_usd when present. */
export function estimateCost(event: ReviewEvent): number {
  if (event.cost_estimate_usd != null && event.cost_estimate_usd >= 0) return event.cost_estimate_usd
  const tokens = event.tokens_total ?? 0
  if (tokens === 0) return 0

  const modelLower = event.model.toLowerCase()
  // Try to find a matching preset
  for (const [fragments, pricePerM] of priceLookup) {
    for (const frag of fragments) {
      if (modelLower.includes(frag) || frag.includes(modelLower)) {
        return (tokens / 1_000_000) * pricePerM
      }
    }
  }

  // Fallback: assume $1/M tokens (conservative middle ground)
  return (tokens / 1_000_000) * 1
}

/** Format cost as a readable string */
export function formatCost(usd: number): string {
  if (usd === 0) return '$0'
  if (usd < 0.001) return '<$0.001'
  if (usd < 0.01) return `$${usd.toFixed(4)}`
  if (usd < 1) return `$${usd.toFixed(3)}`
  return `$${usd.toFixed(2)}`
}

/** Estimate total cost across multiple events */
export function totalCost(events: ReviewEvent[]): number {
  return events.reduce((sum, e) => sum + estimateCost(e), 0)
}

export function aggregateCostBreakdowns(rows: CostBreakdownRow[]): CostBreakdownRow[] {
  const aggregated = new Map<string, CostBreakdownRow>()

  rows.forEach(row => {
    const key = [row.workload, row.role, row.provider ?? '', row.model].join('::')
    const current = aggregated.get(key)
    if (current) {
      current.prompt_tokens += row.prompt_tokens
      current.completion_tokens += row.completion_tokens
      current.total_tokens += row.total_tokens
      current.cost_estimate_usd += row.cost_estimate_usd
      return
    }

    aggregated.set(key, { ...row })
  })

  return Array.from(aggregated.values()).sort((left, right) => {
    if (right.cost_estimate_usd !== left.cost_estimate_usd) {
      return right.cost_estimate_usd - left.cost_estimate_usd
    }
    if (right.total_tokens !== left.total_tokens) {
      return right.total_tokens - left.total_tokens
    }
    return left.model.localeCompare(right.model)
  })
}

export function formatCostWorkload(workload: string): string {
  switch (workload) {
    case 'review_generation':
      return 'Review generation'
    case 'review_verification':
      return 'Review verification'
    case 'eval_generation':
      return 'Eval generation'
    case 'eval_verification':
      return 'Eval verification'
    case 'eval_auditing':
      return 'Eval auditing'
    default:
      return workload.replace(/_/g, ' ')
  }
}

export function formatCostRole(role: string): string {
  return role ? `${role.charAt(0).toUpperCase()}${role.slice(1)}` : 'Unknown'
}
