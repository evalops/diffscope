import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ReviewView } from '../ReviewView'
import type { Comment, PrReadinessReview, PrReadinessSnapshot, ReviewSession, ReviewSummary } from '../../api/types'

let currentRouteReviewId = 'review-1'
let currentSearchParams = new URLSearchParams()
const useReviewMock = vi.fn()
const useGhPrReadinessMock = vi.fn()
const feedbackMutate = vi.fn()
const lifecycleMutate = vi.fn()

vi.mock('react-router-dom', () => ({
  useParams: () => ({ id: currentRouteReviewId }),
  useSearchParams: () => [
    currentSearchParams,
    () => {},
  ],
}))

vi.mock('../../api/hooks', () => ({
  useReview: (id: string | undefined) => useReviewMock(id),
  useGhPrReadiness: (repo: string | undefined, prNumber: number | undefined) => useGhPrReadinessMock(repo, prNumber),
  useSubmitFeedback: () => ({ mutate: feedbackMutate }),
  useUpdateCommentLifecycle: () => ({ mutate: lifecycleMutate }),
}))

const DIFF_CONTENT = `diff --git a/src/a.ts b/src/a.ts
@@ -1 +1 @@
-old
+new
diff --git a/src/b.ts b/src/b.ts
@@ -1 +1 @@
-old
+new
`

function makeSummary(overrides: Partial<ReviewSummary> = {}): ReviewSummary {
  return {
    total_comments: 3,
    by_severity: { Error: 1, Warning: 1, Info: 1 },
    by_category: { Bug: 2, Style: 1 },
    critical_issues: 1,
    files_reviewed: 2,
    overall_score: 7.5,
    recommendations: [],
    open_comments: 2,
    open_by_severity: { Error: 1, Info: 1 },
    open_blocking_comments: 1,
    open_informational_comments: 1,
    resolved_comments: 1,
    dismissed_comments: 0,
    open_blockers: 1,
    completeness: {
      total_findings: 3,
      acknowledged_findings: 1,
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
    readiness_reasons: ['1 blocking finding remains open.'],
    ...overrides,
  }
}

function makeComment(overrides: Partial<Comment> = {}): Comment {
  return {
    id: 'comment-1',
    file_path: 'src/a.ts',
    line_number: 1,
    content: 'Blocking regression',
    severity: 'Error',
    category: 'Bug',
    confidence: 0.92,
    tags: [],
    fix_effort: 'Medium',
    status: 'Open',
    ...overrides,
  }
}

function makeReview(overrides: Partial<ReviewSession> = {}): ReviewSession {
  return {
    id: 'review-1',
    status: 'Complete',
    diff_source: 'branch',
    started_at: 1,
    completed_at: 2,
    comments: [
      makeComment(),
      makeComment({
        id: 'comment-2',
        file_path: 'src/b.ts',
        content: 'Informational note',
        severity: 'Info',
        category: 'Style',
      }),
      makeComment({
        id: 'comment-3',
        file_path: 'src/b.ts',
        content: 'Resolved blocker',
        severity: 'Warning',
        status: 'Resolved',
      }),
    ],
    summary: makeSummary(),
    files_reviewed: 2,
    diff_content: DIFF_CONTENT,
    ...overrides,
  }
}

function makePrReadinessReview(review: ReviewSession, overrides: Partial<PrReadinessReview> = {}): PrReadinessReview {
  return {
    id: review.id,
    status: review.status,
    started_at: review.started_at,
    completed_at: review.completed_at,
    summary: review.summary,
    files_reviewed: review.files_reviewed,
    comment_count: review.comments.length,
    error: review.error,
    ...overrides,
  }
}

function makePrReadinessSnapshot(overrides: Partial<PrReadinessSnapshot> = {}): PrReadinessSnapshot {
  return {
    repo: 'owner/repo',
    pr_number: 42,
    diff_source: 'pr:owner/repo#42',
    ...overrides,
  }
}

describe('ReviewView blocker mode', () => {
  beforeEach(() => {
    currentRouteReviewId = 'review-1'
    currentSearchParams = new URLSearchParams()
    useReviewMock.mockReset()
    useGhPrReadinessMock.mockReset()
    feedbackMutate.mockReset()
    lifecycleMutate.mockReset()
    useGhPrReadinessMock.mockReturnValue({ data: undefined, isLoading: false })
  })

  it.skip('shows only open blockers and hides non-blocking files when enabled', async () => {
    useReviewMock.mockReturnValue({ data: makeReview(), isLoading: false })

    render(<ReviewView />)

    expect(screen.getAllByText('Blocking regression').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Informational note').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Resolved blocker').length).toBeGreaterThan(0)
    expect(screen.getAllByText('b.ts').length).toBeGreaterThan(0)

    fireEvent.click(screen.getByRole('button', { name: /Blockers only/i }))

    await waitFor(() => {
      expect(screen.getByText('Open Error + Warning')).toBeInTheDocument()
    })
    expect(screen.getAllByText('Blocking regression').length).toBeGreaterThan(0)
    expect(screen.queryAllByText('Informational note')).toHaveLength(0)
    expect(screen.queryByText('Resolved blocker')).not.toBeInTheDocument()
    expect(screen.queryAllByText('b.ts')).toHaveLength(0)
  })

  it.skip('shows a clear empty state when a review has no open blockers', async () => {
    const user = userEvent.setup()
    useReviewMock.mockReturnValue({
      data: makeReview({
        comments: [
          makeComment({
            id: 'comment-4',
            content: 'Resolved blocker',
            severity: 'Warning',
            status: 'Resolved',
          }),
          makeComment({
            id: 'comment-5',
            content: 'Informational note',
            severity: 'Info',
            category: 'Style',
          }),
        ],
        summary: makeSummary({
          total_comments: 2,
          by_severity: { Warning: 1, Info: 1 },
          by_category: { Bug: 1, Style: 1 },
          critical_issues: 0,
          open_comments: 1,
          open_by_severity: { Info: 1 },
          open_blocking_comments: 0,
          open_informational_comments: 1,
          resolved_comments: 1,
          open_blockers: 0,
          completeness: {
            total_findings: 2,
            acknowledged_findings: 1,
            fixed_findings: 1,
            stale_findings: 0,
          },
          merge_readiness: 'Ready',
          readiness_reasons: [],
        }),
        diff_content: undefined,
      }),
      isLoading: false,
    })

    render(<ReviewView />)

    await user.click(screen.getByRole('button', { name: /Blockers only/i }))

    await waitFor(() => {
      expect(screen.getByText(/No open blockers remain in this review\.?/)).toBeInTheDocument()
    })
  })

  it.skip('groups list view comments into unresolved, informational, and fixed sections', async () => {
    useReviewMock.mockReturnValue({ data: makeReview(), isLoading: false })

    render(<ReviewView />)

    fireEvent.click(screen.getByRole('button', { name: 'List' }))

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Unresolved' })).toBeInTheDocument()
      expect(screen.getByRole('heading', { name: 'Informational' })).toBeInTheDocument()
      expect(screen.getByRole('heading', { name: 'Fixed' })).toBeInTheDocument()
    })
    expect(screen.queryByRole('heading', { name: 'Stale' })).not.toBeInTheDocument()
  })

  it.skip('groups open comments into a stale section when the review needs re-review', async () => {
    const user = userEvent.setup()
    useReviewMock.mockReturnValue({
      data: makeReview({
        summary: makeSummary({
          merge_readiness: 'NeedsReReview',
          completeness: {
            total_findings: 3,
            acknowledged_findings: 1,
            fixed_findings: 1,
            stale_findings: 2,
          },
          readiness_reasons: ['New commits landed after the latest completed review.'],
        }),
      }),
      isLoading: false,
    })

    render(<ReviewView />)

    await user.click(screen.getByRole('button', { name: 'List' }))

    expect(screen.getByRole('heading', { name: 'Stale' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Unresolved' })).not.toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Informational' })).not.toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Fixed' })).toBeInTheDocument()
  })

  it('groups comments into a stale section when stale outcomes are present', async () => {
    const user = userEvent.setup()
    useReviewMock.mockReturnValue({
      data: makeReview({
        comments: [
          makeComment({ id: 'comment-1', outcomes: ['stale'] }),
          makeComment({
            id: 'comment-2',
            file_path: 'src/b.ts',
            content: 'Resolved blocker',
            severity: 'Warning',
            status: 'Resolved',
          }),
        ],
        summary: makeSummary({
          total_comments: 2,
          by_severity: { Error: 1, Warning: 1 },
          by_category: { Bug: 2 },
          critical_issues: 1,
          open_comments: 1,
          open_by_severity: { Error: 1 },
          open_blocking_comments: 1,
          open_informational_comments: 0,
          resolved_comments: 1,
          open_blockers: 1,
          completeness: {
            total_findings: 2,
            acknowledged_findings: 1,
            fixed_findings: 1,
            stale_findings: 1,
          },
          merge_readiness: 'NeedsReReview',
          readiness_reasons: ['New commits landed after the latest completed review.'],
        }),
      }),
      isLoading: false,
    })

    render(<ReviewView />)

    await user.click(screen.getByRole('button', { name: 'List' }))

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Stale' })).toBeInTheDocument()
    })
    expect(screen.queryByRole('heading', { name: 'Unresolved' })).not.toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Fixed' })).toBeInTheDocument()
  })

  it('keeps open findings unresolved when re-review is caused by verification rather than stale commits', async () => {
    const user = userEvent.setup()
    useReviewMock.mockReturnValue({
      data: makeReview({
        summary: makeSummary({
          merge_readiness: 'NeedsReReview',
          completeness: {
            total_findings: 3,
            acknowledged_findings: 1,
            fixed_findings: 1,
            stale_findings: 0,
          },
          readiness_reasons: ['verification was inconclusive or fail-open; rerun this review'],
          verification: {
            state: 'Inconclusive',
            judge_count: 2,
            required_votes: 2,
            warning_count: 1,
            filtered_comments: 0,
            abstained_comments: 0,
          },
        }),
      }),
      isLoading: false,
    })

    render(<ReviewView />)

    await user.click(screen.getByRole('button', { name: 'List' }))

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Unresolved' })).toBeInTheDocument()
    })
    expect(screen.queryByRole('heading', { name: 'Stale' })).not.toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Informational' })).toBeInTheDocument()
  })

  it('treats open comments with addressed outcomes as fixed in list view', async () => {
    const user = userEvent.setup()
    useReviewMock.mockReturnValue({
      data: makeReview({
        comments: [
          makeComment({
            content: 'Handled in a follow-up commit',
            outcomes: ['addressed'],
          }),
        ],
        summary: makeSummary({
          total_comments: 1,
          by_severity: { Error: 1 },
          by_category: { Bug: 1 },
          critical_issues: 1,
          open_comments: 1,
          open_by_severity: { Error: 1 },
          open_blocking_comments: 1,
          open_informational_comments: 0,
          resolved_comments: 0,
          open_blockers: 1,
          completeness: {
            total_findings: 1,
            acknowledged_findings: 0,
            fixed_findings: 0,
            stale_findings: 0,
          },
        }),
      }),
      isLoading: false,
    })

    render(<ReviewView />)

    await user.click(screen.getByRole('button', { name: 'List' }))

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Fixed' })).toBeInTheDocument()
    })
    expect(screen.queryByRole('heading', { name: 'Unresolved' })).not.toBeInTheDocument()
    expect(screen.getByText('Handled in a follow-up commit')).toBeInTheDocument()
  })

  it.skip('supports keyboard finding workflows for next, thumbs, and resolve actions', async () => {
    const user = userEvent.setup()
    useReviewMock.mockReturnValue({ data: makeReview(), isLoading: false })

    const { container } = render(<ReviewView />)

    await user.keyboard('n')
    expect(container.querySelector('[data-comment-id="comment-1"]')).toHaveFocus()

    await user.keyboard('a')
    expect(feedbackMutate).toHaveBeenNthCalledWith(1, { commentId: 'comment-1', action: 'accept' })
    await waitFor(() => expect(container.querySelector('[data-comment-id="comment-2"]')).toHaveFocus())

    await user.keyboard('e')
    expect(lifecycleMutate).toHaveBeenCalledWith({ commentId: 'comment-2', status: 'resolved' })
    await waitFor(() => expect(container.querySelector('[data-comment-id="comment-3"]')).toHaveFocus())

    await user.keyboard('r')
    expect(feedbackMutate).toHaveBeenNthCalledWith(2, { commentId: 'comment-3', action: 'reject' })
  })

  it('shows a train-the-reviewer callout when thumbs coverage is low', () => {
    useReviewMock.mockReturnValue({ data: makeReview(), isLoading: false })

    render(<ReviewView />)

    expect(screen.getByText('Train the reviewer')).toBeInTheDocument()
    expect(screen.getByText('No thumbs recorded yet. Label findings below to train the reviewer.')).toBeInTheDocument()
    expect(screen.getByText('0%')).toBeInTheDocument()
  })

  it('hides the train-the-reviewer callout when enough findings are labeled', () => {
    useReviewMock.mockReturnValue({
      data: makeReview({
        comments: [
          makeComment({ feedback: 'accept' }),
          makeComment({
            id: 'comment-2',
            file_path: 'src/b.ts',
            content: 'Informational note',
            severity: 'Info',
            category: 'Style',
            feedback: 'reject',
          }),
          makeComment({
            id: 'comment-3',
            file_path: 'src/b.ts',
            content: 'Resolved blocker',
            severity: 'Warning',
            status: 'Resolved',
          }),
        ],
      }),
      isLoading: false,
    })

    render(<ReviewView />)

    expect(screen.queryByText('Train the reviewer')).not.toBeInTheDocument()
  })

  it('shows review-change comparisons for the previous PR run', () => {
    currentRouteReviewId = 'review-2'

    const previousReview = makeReview({
      id: 'review-1',
      diff_source: 'pr:owner/repo#42',
      started_at: 10,
      completed_at: 11,
      comments: [
        makeComment({ id: 'shared-1', content: 'Persistent blocker', severity: 'Error' }),
        makeComment({ id: 'old-1', file_path: 'src/old.ts', line_number: 12, content: 'Old blocker', severity: 'Warning' }),
      ],
      summary: makeSummary({
        total_comments: 2,
        by_severity: { Error: 1, Warning: 1 },
        by_category: { Bug: 2 },
        critical_issues: 1,
        overall_score: 6.5,
        open_comments: 2,
        open_by_severity: { Error: 1, Warning: 1 },
        open_blocking_comments: 2,
        open_informational_comments: 0,
        resolved_comments: 0,
        dismissed_comments: 0,
        open_blockers: 2,
        completeness: {
          total_findings: 2,
          acknowledged_findings: 0,
          fixed_findings: 0,
          stale_findings: 0,
        },
        readiness_reasons: ['2 blocking findings remain open.'],
      }),
    })

    const currentReview = makeReview({
      id: 'review-2',
      diff_source: 'pr:owner/repo#42',
      started_at: 20,
      completed_at: 21,
      comments: [
        makeComment({ id: 'shared-1', content: 'Persistent blocker', severity: 'Error' }),
        makeComment({ id: 'new-1', file_path: 'src/new.ts', line_number: 18, content: 'New regression', severity: 'Warning' }),
      ],
      summary: makeSummary({
        total_comments: 2,
        by_severity: { Error: 1, Warning: 1 },
        by_category: { Bug: 2 },
        critical_issues: 1,
        overall_score: 7.5,
        open_comments: 2,
        open_by_severity: { Error: 1, Warning: 1 },
        open_blocking_comments: 2,
        open_informational_comments: 0,
        resolved_comments: 0,
        dismissed_comments: 0,
        open_blockers: 2,
        completeness: {
          total_findings: 2,
          acknowledged_findings: 1,
          fixed_findings: 0,
          stale_findings: 0,
        },
        readiness_reasons: ['2 blocking findings remain open.'],
      }),
    })

    useReviewMock.mockImplementation((reviewId: string | undefined) => ({
      data: reviewId === 'review-2'
        ? currentReview
        : reviewId === 'review-1'
          ? previousReview
          : undefined,
      isLoading: false,
      error: undefined,
    }))
    useGhPrReadinessMock.mockReturnValue({
      data: makePrReadinessSnapshot({
        timeline: [
          makePrReadinessReview(previousReview),
          makePrReadinessReview(currentReview),
        ],
      }),
      isLoading: false,
    })

    render(<ReviewView />)

    const comparison = screen.getByRole('region', { name: 'Changes since previous run' })
    expect(comparison).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Compare previous/i })).toBeInTheDocument()
    expect(comparison).toHaveTextContent('Compared with review review-1')
    expect(comparison).toHaveTextContent('Score 6.5 → 7.5 (+1.0)')
    expect(comparison).toHaveTextContent('New regression')
    expect(comparison).toHaveTextContent('Old blocker')
  })

  it('compares historical PR reviews against the immediately previous run', () => {
    currentRouteReviewId = 'review-2'

    const firstReview = makeReview({
      id: 'review-1',
      diff_source: 'pr:owner/repo#42',
      started_at: 10,
      completed_at: 11,
      comments: [
        makeComment({ id: 'first-1', content: 'First pass blocker', severity: 'Error' }),
      ],
      summary: makeSummary({
        total_comments: 1,
        by_severity: { Error: 1 },
        by_category: { Bug: 1 },
        critical_issues: 1,
        overall_score: 6.0,
        open_comments: 1,
        open_by_severity: { Error: 1 },
        open_blocking_comments: 1,
        open_informational_comments: 0,
        resolved_comments: 0,
        dismissed_comments: 0,
        open_blockers: 1,
        completeness: {
          total_findings: 1,
          acknowledged_findings: 0,
          fixed_findings: 0,
          stale_findings: 0,
        },
      }),
    })

    const middleReview = makeReview({
      id: 'review-2',
      diff_source: 'pr:owner/repo#42',
      started_at: 20,
      completed_at: 21,
      comments: [
        makeComment({ id: 'middle-1', content: 'Middle run blocker', severity: 'Warning' }),
      ],
      summary: makeSummary({
        total_comments: 1,
        by_severity: { Warning: 1 },
        by_category: { Bug: 1 },
        critical_issues: 0,
        overall_score: 7.0,
        open_comments: 1,
        open_by_severity: { Warning: 1 },
        open_blocking_comments: 1,
        open_informational_comments: 0,
        resolved_comments: 0,
        dismissed_comments: 0,
        open_blockers: 1,
        completeness: {
          total_findings: 1,
          acknowledged_findings: 0,
          fixed_findings: 0,
          stale_findings: 0,
        },
      }),
    })

    const latestReview = makeReview({
      id: 'review-3',
      diff_source: 'pr:owner/repo#42',
      started_at: 30,
      completed_at: 31,
      comments: [
        makeComment({ id: 'latest-1', content: 'Latest run blocker', severity: 'Info', category: 'Style' }),
      ],
      summary: makeSummary({
        total_comments: 1,
        by_severity: { Info: 1 },
        by_category: { Style: 1 },
        critical_issues: 0,
        overall_score: 8.5,
        open_comments: 1,
        open_by_severity: { Info: 1 },
        open_blocking_comments: 0,
        open_informational_comments: 1,
        resolved_comments: 0,
        dismissed_comments: 0,
        open_blockers: 0,
        completeness: {
          total_findings: 1,
          acknowledged_findings: 0,
          fixed_findings: 0,
          stale_findings: 0,
        },
      }),
    })

    useReviewMock.mockImplementation((reviewId: string | undefined) => ({
      data: reviewId === 'review-2'
        ? middleReview
        : reviewId === 'review-1'
          ? firstReview
          : reviewId === 'review-3'
            ? latestReview
            : undefined,
      isLoading: false,
      error: undefined,
    }))
    useGhPrReadinessMock.mockReturnValue({
      data: makePrReadinessSnapshot({
        timeline: [
          makePrReadinessReview(firstReview),
          makePrReadinessReview(middleReview),
          makePrReadinessReview(latestReview),
        ],
      }),
      isLoading: false,
    })

    render(<ReviewView />)

    const comparison = screen.getByRole('region', { name: 'Changes since previous run' })
    expect(comparison).toHaveTextContent('Compared with review review-1')
    expect(comparison).toHaveTextContent('First pass blocker')
    expect(comparison).not.toHaveTextContent('Latest run blocker')
  })
})
