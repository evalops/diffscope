import { ExternalLink, Loader2 } from 'lucide-react'

import type { MergeReadiness, PrReadinessSnapshot, ReviewSummary } from '../api/types'

type Props = {
  readiness?: PrReadinessSnapshot
  isLoading?: boolean
  error?: unknown
  onOpenReview?: (reviewId: string) => void
}

const READINESS_STYLES: Record<MergeReadiness, string> = {
  Ready: 'bg-sev-suggestion/10 text-sev-suggestion border border-sev-suggestion/20',
  NeedsAttention: 'bg-sev-warning/10 text-sev-warning border border-sev-warning/20',
  NeedsReReview: 'bg-accent/10 text-accent border border-accent/20',
}

const READINESS_LABELS: Record<MergeReadiness, string> = {
  Ready: 'Merge Ready',
  NeedsAttention: 'Needs Attention',
  NeedsReReview: 'Needs Re-review',
}

export function PrReadinessSummary({ readiness, isLoading = false, error, onOpenReview }: Props) {
  const latestReview = readiness?.latest_review
  const summary = latestReview?.summary
  const timeline = readiness?.timeline?.length
    ? readiness.timeline.filter(review => review.summary)
    : latestReview?.summary
      ? [latestReview]
      : []
  const firstMergeableReview = timeline.find(review => review.summary?.merge_readiness === 'Ready')
  const latestTimelineReviewId = timeline.length > 0 ? timeline[timeline.length - 1].id : undefined
  const isIncrementalReview = Boolean(
    readiness?.current_head_sha
    && latestReview?.reviewed_head_sha
    && readiness.current_head_sha !== latestReview.reviewed_head_sha,
  )

  return (
    <div className="mb-4 rounded-lg border border-border-subtle bg-surface p-3">
      <div className="flex items-start justify-between gap-3 mb-3">
        <div>
          <div className="text-[13px] text-text-primary">Latest DiffScope readiness</div>
          <div className="text-[11px] text-text-muted mt-0.5">
            Merge guidance from the latest stored DiffScope review for this PR.
          </div>
        </div>
        {latestReview?.id && onOpenReview && (
          <button
            type="button"
            onClick={() => onOpenReview(latestReview.id)}
            className="inline-flex items-center gap-1 px-2 py-1 rounded text-[11px] font-medium bg-surface-2 border border-border text-text-secondary hover:text-text-primary hover:border-text-muted transition-colors"
          >
            Open latest review <ExternalLink size={12} />
          </button>
        )}
      </div>

      {isLoading ? (
        <div className="flex items-center gap-2 text-[12px] text-text-muted">
          <Loader2 size={14} className="animate-spin" />
          Loading readiness…
        </div>
      ) : error ? (
        <div className="text-[12px] text-sev-error">
          {error instanceof Error ? error.message : 'Failed to load readiness'}
        </div>
      ) : !latestReview ? (
        <div className="text-[12px] text-text-secondary">
          No DiffScope review has been recorded for this PR yet. Start a review below to populate merge guidance.
        </div>
      ) : !summary ? (
        <div className="text-[12px] text-text-secondary">
          The latest DiffScope review does not have a readiness summary yet.
        </div>
      ) : (
        <>
          {isIncrementalReview && readiness?.current_head_sha && latestReview?.reviewed_head_sha && (
            <div className="mb-3 rounded border border-accent/20 bg-accent/5 p-3">
              <div className="text-[11px] font-medium text-accent mb-1">Incremental review coverage</div>
              <div className="text-[11px] text-text-secondary">
                DiffScope last reviewed PR head <span className="font-code text-text-primary">{shortSha(latestReview.reviewed_head_sha)}</span>,
                but GitHub is now at <span className="font-code text-text-primary">{shortSha(readiness.current_head_sha)}</span>.
                This readiness summary does not include the newer delta yet.
              </div>
            </div>
          )}

          <div className="flex flex-wrap items-center gap-2 mb-3">
            <span className={`text-[10px] px-2 py-0.5 rounded font-code ${READINESS_STYLES[summary.merge_readiness]}`}>
              {READINESS_LABELS[summary.merge_readiness]}
            </span>
            <span className="text-[10px] text-text-muted font-code">
              Review {latestReview.id.slice(0, 8)}
            </span>
            <span className="text-[10px] text-text-muted font-code">{latestReview.status}</span>
          </div>

          <div className="grid grid-cols-2 md:grid-cols-3 gap-3 text-[11px] mb-3">
            <Metric label="Open blockers" value={String(summary.open_blockers)} tone={summary.open_blockers > 0 ? 'warning' : 'success'} />
            <Metric label="Verification" value={summary.verification.state} />
            <Metric label="Lifecycle" value={`${summary.open_comments} open`} hint={`${summary.resolved_comments} resolved · ${summary.dismissed_comments} dismissed`} />
            <Metric
              label="Completeness"
              value={`${summary.completeness.acknowledged_findings}/${summary.completeness.total_findings} acknowledged`}
              hint={`${summary.completeness.fixed_findings} fixed · ${summary.completeness.stale_findings} stale`}
            />
            <Metric label="Findings" value={String(summary.total_comments)} hint={`${latestReview.files_reviewed} file${latestReview.files_reviewed === 1 ? '' : 's'} reviewed`} />
          </div>

          {(readiness?.current_head_sha || latestReview.reviewed_head_sha) && (
            <div className="grid grid-cols-2 gap-3 text-[10px] font-code text-text-muted mb-3">
              {readiness?.current_head_sha && (
                <Metric label="Current head" value={shortSha(readiness.current_head_sha)} />
              )}
              {latestReview.reviewed_head_sha && (
                <Metric label="Reviewed head" value={shortSha(latestReview.reviewed_head_sha)} />
              )}
            </div>
          )}

          {timeline.length > 0 && (
            <div className="rounded border border-border-subtle bg-surface-1 p-3 mb-3">
              <div className="flex items-center justify-between gap-3 mb-2">
                <div className="text-[11px] font-medium text-text-primary">Readiness timeline</div>
                <div className="text-[10px] text-text-muted">
                  {firstMergeableReview
                    ? `Became mergeable on ${formatReviewTimestamp(firstMergeableReview.completed_at ?? firstMergeableReview.started_at)}`
                    : 'No merge-ready checkpoint yet'}
                </div>
              </div>
              <ol className="space-y-2">
                {timeline.map(review => {
                  const reviewSummary = review.summary
                  if (!reviewSummary) return null
                  const isFirstMergeable = review.id === firstMergeableReview?.id
                  const isSuperseded = isIncrementalReview && review.id === latestTimelineReviewId

                  return (
                    <li key={review.id} className="flex items-start gap-2">
                      <span className={`mt-1.5 h-2 w-2 rounded-full ${timelineDotClassName(reviewSummary.merge_readiness)}`} />
                      <div className="min-w-0">
                        <div className="flex items-center gap-2 flex-wrap">
                          <span className={`text-[10px] px-2 py-0.5 rounded font-code ${READINESS_STYLES[reviewSummary.merge_readiness]}`}>
                            {READINESS_LABELS[reviewSummary.merge_readiness]}
                          </span>
                          {isFirstMergeable && (
                            <span className="text-[10px] px-2 py-0.5 rounded border border-sev-suggestion/20 bg-sev-suggestion/10 text-sev-suggestion">
                              First mergeable
                            </span>
                          )}
                          {isSuperseded && (
                            <span className="text-[10px] px-2 py-0.5 rounded border border-accent/20 bg-accent/10 text-accent">
                              Superseded by newer commits
                            </span>
                          )}
                        </div>
                        <div className="text-[10px] text-text-muted mt-1">
                          {formatReviewTimestamp(review.completed_at ?? review.started_at)} · {reviewSummary.open_blockers} blocker{reviewSummary.open_blockers === 1 ? '' : 's'} · {reviewSummary.total_comments} finding{reviewSummary.total_comments === 1 ? '' : 's'} · Review {review.id.slice(0, 8)}
                        </div>
                      </div>
                    </li>
                  )
                })}
              </ol>
            </div>
          )}

          <div className="rounded border border-border-subtle bg-surface-1 p-3">
            <div className="text-[11px] font-medium text-text-primary mb-2">What still blocks merge</div>
            <ul className="space-y-1 text-[11px] text-text-secondary list-disc pl-4">
              {buildReadinessReasons(summary).map(reason => (
                <li key={reason}>{reason}</li>
              ))}
            </ul>
          </div>
        </>
      )}
    </div>
  )
}

function buildReadinessReasons(summary: ReviewSummary): string[] {
  const reasons: string[] = []
  if (summary.open_blockers > 0) {
    reasons.push(`${summary.open_blockers} blocking finding${summary.open_blockers === 1 ? '' : 's'} ${summary.open_blockers === 1 ? 'remains' : 'remain'} open.`)
  }
  reasons.push(...summary.readiness_reasons)

  if (reasons.length === 0) {
    reasons.push('No open blockers remain in the latest DiffScope review.')
  }

  return reasons
}

function timelineDotClassName(readiness: MergeReadiness): string {
  if (readiness === 'Ready') return 'bg-sev-suggestion'
  if (readiness === 'NeedsAttention') return 'bg-sev-warning'
  return 'bg-accent'
}

function formatReviewTimestamp(value: string | number): string {
  const date = toDate(value)
  return date
    ? date.toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
    })
    : 'Unknown time'
}

function toDate(value: string | number): Date | null {
  const date = typeof value === 'number'
    ? new Date(value * 1000)
    : new Date(value)
  return Number.isNaN(date.getTime()) ? null : date
}

function shortSha(sha: string): string {
  return sha.slice(0, 12)
}

function Metric({
  label,
  value,
  hint,
  tone = 'default',
}: {
  label: string
  value: string
  hint?: string
  tone?: 'default' | 'warning' | 'success'
}) {
  const toneClass = tone === 'warning'
    ? 'text-sev-warning'
    : tone === 'success'
      ? 'text-sev-suggestion'
      : 'text-text-primary'

  return (
    <div>
      <div className="text-text-muted">{label}</div>
      <div className={`mt-0.5 ${toneClass}`}>{value}</div>
      {hint && <div className="text-text-muted mt-0.5">{hint}</div>}
    </div>
  )
}
