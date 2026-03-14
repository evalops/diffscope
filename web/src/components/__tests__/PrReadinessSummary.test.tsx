import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

import { PrReadinessSummary } from '../PrReadinessSummary'
import type { PrReadinessSnapshot, ReviewSummary } from '../../api/types'

function makeSummary(overrides: Partial<ReviewSummary> = {}): ReviewSummary {
  return {
    total_comments: 4,
    by_severity: { Warning: 2, Info: 2 },
    by_category: { Bug: 2, Style: 2 },
    critical_issues: 0,
    files_reviewed: 2,
    overall_score: 8.4,
    recommendations: [],
    open_comments: 2,
    open_by_severity: { Warning: 2 },
    open_blocking_comments: 2,
    open_informational_comments: 0,
    resolved_comments: 1,
    dismissed_comments: 1,
    open_blockers: 2,
    completeness: {
      total_findings: 4,
      acknowledged_findings: 2,
      fixed_findings: 1,
      stale_findings: 0,
    },
    merge_readiness: 'NeedsAttention',
    verification: {
      state: 'Verified',
      judge_count: 2,
      required_votes: 2,
      warning_count: 0,
      filtered_comments: 0,
      abstained_comments: 0,
    },
    readiness_reasons: [],
    ...overrides,
  }
}

function makeSnapshot(overrides: Partial<PrReadinessSnapshot> = {}): PrReadinessSnapshot {
  return {
    repo: 'owner/repo',
    pr_number: 42,
    diff_source: 'pr:owner/repo#42',
    current_head_sha: '0123456789abcdef',
    latest_review: {
      id: 'review-12345678',
      status: 'Complete',
      started_at: 1,
      completed_at: 2,
      reviewed_head_sha: 'fedcba9876543210',
      summary: makeSummary(),
      files_reviewed: 2,
      comment_count: 4,
    },
    ...overrides,
  }
}

describe('PrReadinessSummary', () => {
  it('renders a no-review state when readiness is empty', () => {
    render(<PrReadinessSummary readiness={{ repo: 'owner/repo', pr_number: 42, diff_source: 'pr:owner/repo#42' }} />)

    expect(screen.getByText('Latest DiffScope readiness')).toBeInTheDocument()
    expect(screen.getByText(/No DiffScope review has been recorded/i)).toBeInTheDocument()
  })

  it('renders lifecycle-aware merge blockers for the latest review', () => {
    render(<PrReadinessSummary readiness={makeSnapshot()} />)

    expect(screen.getByText('Needs Attention')).toBeInTheDocument()
    expect(screen.getByText('Incremental review coverage')).toBeInTheDocument()
    expect(screen.getByText(/does not include the newer delta yet/i)).toBeInTheDocument()
    expect(screen.getByText('Open blockers')).toBeInTheDocument()
    expect(screen.getByText('Completeness')).toBeInTheDocument()
    expect(screen.getByText('2/4 acknowledged')).toBeInTheDocument()
    expect(screen.getByText('1 fixed · 0 stale')).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
    expect(screen.getByText('2 blocking findings remain open.')).toBeInTheDocument()
    expect(screen.getAllByText('0123456789ab')).toHaveLength(2)
    expect(screen.getAllByText('fedcba987654')).toHaveLength(2)
  })

  it('does not show the incremental callout when the latest review matches the current head', () => {
    render(<PrReadinessSummary readiness={makeSnapshot({ current_head_sha: 'fedcba9876543210' })} />)

    expect(screen.queryByText('Incremental review coverage')).not.toBeInTheDocument()
  })

  it('opens the latest review when requested', async () => {
    const user = userEvent.setup()
    const onOpenReview = vi.fn()
    render(<PrReadinessSummary readiness={makeSnapshot()} onOpenReview={onOpenReview} />)

    await user.click(screen.getByRole('button', { name: /Open latest review/i }))
    expect(onOpenReview).toHaveBeenCalledWith('review-12345678')
  })
})
