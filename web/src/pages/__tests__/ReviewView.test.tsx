import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ReviewView } from '../ReviewView'
import type { Comment, ReviewSession, ReviewSummary } from '../../api/types'

const useReviewMock = vi.fn()
const feedbackMutate = vi.fn()
const lifecycleMutate = vi.fn()

vi.mock('react-router-dom', () => ({
  useParams: () => ({ id: 'review-1' }),
  useSearchParams: () => [
    { get: () => null },
    () => {},
  ],
}))

vi.mock('../../api/hooks', () => ({
  useReview: (id: string | undefined) => useReviewMock(id),
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

describe('ReviewView blocker mode', () => {
  beforeEach(() => {
    useReviewMock.mockReset()
    feedbackMutate.mockReset()
    lifecycleMutate.mockReset()
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
})
