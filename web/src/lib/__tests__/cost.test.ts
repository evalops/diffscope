import { describe, expect, it } from 'vitest'
import { estimateCost, formatCost, totalCost } from '../cost'
import type { ReviewEvent } from '../../api/types'

function makeEvent(overrides: Partial<ReviewEvent> = {}): ReviewEvent {
  return {
    review_id: 'e1',
    event_type: 'review.completed',
    model: 'gpt-4o',
    diff_source: 'head',
    duration_ms: 1000,
    diff_bytes: 0,
    diff_files_total: 0,
    diff_files_reviewed: 0,
    diff_files_skipped: 0,
    comments_total: 0,
    comments_by_severity: {},
    comments_by_category: {},
    hotspots_detected: 0,
    high_risk_files: 0,
    github_posted: false,
    tokens_total: 100_000,
    ...overrides,
  }
}

describe('formatCost', () => {
  it('formats zero as $0', () => {
    expect(formatCost(0)).toBe('$0')
  })
  it('formats small values with enough precision', () => {
    expect(formatCost(0.0001)).toBe('<$0.001')
    expect(formatCost(0.005)).toBe('$0.0050')
    expect(formatCost(0.5)).toBe('$0.500')
  })
  it('formats dollars with two decimals', () => {
    expect(formatCost(1.5)).toBe('$1.50')
    expect(formatCost(10)).toBe('$10.00')
  })
})

describe('estimateCost', () => {
  it('prefers server cost_estimate_usd when present', () => {
    const e = makeEvent({ cost_estimate_usd: 0.42 })
    expect(estimateCost(e)).toBe(0.42)
  })
  it('uses client estimate when cost_estimate_usd is absent', () => {
    const e = makeEvent({ cost_estimate_usd: undefined, tokens_total: 1_000_000 })
    expect(estimateCost(e)).toBeGreaterThan(0)
  })
  it('returns 0 when tokens_total is 0 and no server cost', () => {
    const e = makeEvent({ tokens_total: 0, cost_estimate_usd: undefined })
    expect(estimateCost(e)).toBe(0)
  })
})

describe('totalCost', () => {
  it('sums server cost when present', () => {
    const events = [
      makeEvent({ cost_estimate_usd: 0.1 }),
      makeEvent({ cost_estimate_usd: 0.2 }),
    ]
    expect(totalCost(events)).toBeCloseTo(0.3)
  })
  it('returns 0 for empty list', () => {
    expect(totalCost([])).toBe(0)
  })
})
