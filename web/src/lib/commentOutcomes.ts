import type { Comment, CommentOutcome, ReviewSummary } from '../api/types'

export function isStaleReviewSummary(summary?: Pick<ReviewSummary, 'completeness'>): boolean {
  return Boolean(summary?.completeness.stale_findings && summary.completeness.stale_findings > 0)
}

export function getCommentOutcomes(
  comment: Comment,
  options: { staleReview?: boolean } = {},
): CommentOutcome[] {
  if (Array.isArray(comment.outcomes)) {
    return comment.outcomes
  }

  const outcomes: CommentOutcome[] = []
  if (comment.feedback === 'accept') {
    outcomes.push('accepted')
  } else if (comment.feedback === 'reject') {
    outcomes.push('rejected')
  }

  if (comment.status === 'Resolved') {
    outcomes.push('addressed')
  }

  if ((comment.status ?? 'Open') === 'Open' && options.staleReview) {
    outcomes.push('stale')
  }

  if (outcomes.length === 0 && (comment.status ?? 'Open') === 'Open') {
    outcomes.push('new')
  }

  return outcomes
}
