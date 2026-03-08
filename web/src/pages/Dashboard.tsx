import { useNavigate } from 'react-router-dom'
import { Play, GitBranch, GitCommit, Loader2, ArrowRight, AlertCircle, ArrowUpRight } from 'lucide-react'
import { AreaChart, Area, ResponsiveContainer } from 'recharts'
import { useStatus, useReviews, useStartReview } from '../api/hooks'
import { SeverityDot } from '../components/SeverityBadge'
import { scoreColorClass } from '../lib/scores'
import { SEVERITIES, STATUS_STYLES } from '../lib/constants'
import type { Severity, ReviewSession } from '../api/types'

function getGreeting() {
  const h = new Date().getHours()
  if (h < 12) return 'Good Morning'
  if (h < 18) return 'Good Afternoon'
  return 'Good Evening'
}

function buildScoreHistory(reviews: ReviewSession[]) {
  return reviews
    .filter(r => r.status === 'Complete' && r.summary)
    .slice(0, 10)
    .reverse()
    .map((r, i) => ({ idx: i, score: r.summary!.overall_score, findings: r.summary!.total_comments }))
}

function buildSeverityStats(reviews: ReviewSession[]) {
  const totals: Record<string, number> = Object.fromEntries(SEVERITIES.map(s => [s, 0]))
  for (const r of reviews) {
    if (r.status !== 'Complete' || !r.summary) continue
    for (const [sev, count] of Object.entries(r.summary.by_severity)) {
      totals[sev] = (totals[sev] || 0) + count
    }
  }
  return totals
}

export function Dashboard() {
  const navigate = useNavigate()
  const { data: status } = useStatus()
  const { data: reviews } = useReviews()
  const startReview = useStartReview()

  const handleReview = (source: 'head' | 'staged' | 'branch') => {
    startReview.mutate(
      { diff_source: source, base_branch: source === 'branch' ? 'main' : undefined },
      { onSuccess: (data) => navigate(`/review/${data.id}`) }
    )
  }

  const allReviews = reviews || []
  const completedReviews = allReviews.filter(r => r.status === 'Complete')
  const scoreHistory = buildScoreHistory(allReviews)
  const sevStats = buildSeverityStats(allReviews)
  const avgScore = completedReviews.length > 0
    ? completedReviews.reduce((s, r) => s + (r.summary?.overall_score || 0), 0) / completedReviews.length
    : 0
  const totalFindings = completedReviews.reduce((s, r) => s + (r.summary?.total_comments || 0), 0)
  const recentReviews = allReviews.slice(0, 5)

  return (
    <div className="p-6 max-w-5xl mx-auto">
      <h1 className="text-xl font-semibold text-text-primary mb-6">{getGreeting()}</h1>

      {/* Quick actions */}
      <div className="grid grid-cols-3 gap-3 mb-8">
        {[
          { source: 'head' as const, icon: GitCommit, label: 'Review HEAD', desc: 'Last commit diff' },
          { source: 'staged' as const, icon: Play, label: 'Review Staged', desc: 'Staged changes' },
          { source: 'branch' as const, icon: GitBranch, label: 'Review Branch', desc: 'vs main' },
        ].map(({ source, icon: Icon, label, desc }) => (
          <button
            key={source}
            onClick={() => handleReview(source)}
            disabled={startReview.isPending}
            className="group flex items-center gap-3 p-3.5 bg-surface-1 border border-border rounded-lg hover:border-accent/30 hover:bg-surface-2 transition-all disabled:opacity-40 text-left"
          >
            <Icon className="text-accent" size={17} />
            <div className="flex-1">
              <div className="text-[13px] font-medium text-text-primary">{label}</div>
              <div className="text-[11px] text-text-muted">{desc}</div>
            </div>
            <ArrowRight size={13} className="text-text-muted opacity-0 group-hover:opacity-100 transition-opacity" />
          </button>
        ))}
      </div>

      {startReview.isPending && (
        <div className="flex items-center gap-2 mb-4 text-[12px] text-accent">
          <Loader2 size={14} className="animate-spin" />
          Starting review...
        </div>
      )}

      {startReview.isError && (
        <div className="flex items-center gap-2 mb-4 text-[12px] text-sev-error bg-sev-error/5 border border-sev-error/20 rounded px-3 py-2">
          <AlertCircle size={14} />
          {startReview.error.message}
        </div>
      )}

      {/* Metric cards */}
      <div className="grid grid-cols-4 gap-3 mb-8">
        <MetricCard label="AVERAGE SCORE">
          <span className={`text-2xl font-bold font-code ${scoreColorClass(avgScore)}`}>
            {avgScore > 0 ? avgScore.toFixed(1) : '\u2014'}
          </span>
          {scoreHistory.length > 1 && (
            <div className="mt-2" style={{ width: '100%', height: 40, minWidth: 50 }}>
              <ResponsiveContainer width="100%" height={40} minWidth={50}>
                <AreaChart data={scoreHistory}>
                  <defs>
                    <linearGradient id="scoreGrad" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="#4ade80" stopOpacity={0.3} />
                      <stop offset="95%" stopColor="#4ade80" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <Area type="monotone" dataKey="score" stroke="#4ade80" fill="url(#scoreGrad)" strokeWidth={1.5} dot={false} />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          )}
        </MetricCard>

        <MetricCard label="TOTAL FINDINGS">
          <div className="text-2xl font-bold font-code text-text-primary">{totalFindings}</div>
          <div className="flex items-center gap-3 mt-3">
            {Object.entries(sevStats).map(([sev, count]) => (
              count > 0 && (
                <span key={sev} className="flex items-center gap-1 text-[11px] text-text-muted">
                  <SeverityDot severity={sev as Severity} />
                  {count}
                </span>
              )
            ))}
          </div>
        </MetricCard>

        <MetricCard label="REVIEWS">
          <div className="text-2xl font-bold font-code text-text-primary">{completedReviews.length}</div>
          <div className="text-[11px] text-text-muted mt-1">completed</div>
        </MetricCard>

        <MetricCard label="FILES REVIEWED">
          <div className="text-2xl font-bold font-code text-text-primary">
            {completedReviews.reduce((s, r) => s + r.files_reviewed, 0)}
          </div>
          <div className="text-[11px] text-text-muted mt-1">across all reviews</div>
        </MetricCard>
      </div>

      {/* Status bar */}
      {status && (
        <div className="bg-surface-1 border border-border rounded-lg p-3.5 mb-8">
          <div className="flex items-center gap-6 text-[12px]">
            <div className="flex items-center gap-2">
              <span className="w-1.5 h-1.5 rounded-full bg-accent" />
              <span className="font-code text-text-primary">{status.repo_path.split('/').pop()}</span>
            </div>
            {status.branch && (
              <div className="text-text-muted">
                <span className="text-text-muted/50">branch:</span>{' '}
                <span className="font-code text-text-secondary">{status.branch}</span>
              </div>
            )}
            <div className="text-text-muted">
              <span className="text-text-muted/50">model:</span>{' '}
              <span className="font-code text-text-secondary truncate">{status.model}</span>
            </div>
            {status.active_reviews > 0 && (
              <div className="flex items-center gap-1 text-accent">
                <ArrowUpRight size={12} />
                {status.active_reviews} active
              </div>
            )}
          </div>
        </div>
      )}

      {/* Recent activity */}
      <div className="flex items-center gap-1.5 mb-3">
        <span className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">RECENT ACTIVITY</span>
      </div>
      {recentReviews.length === 0 ? (
        <div className="bg-surface-1 border border-border rounded-lg p-10 text-center text-text-muted text-sm">
          No reviews yet. Click a button above to start one.
        </div>
      ) : (
        <div className="bg-surface-1 border border-border rounded-lg overflow-hidden">
          {recentReviews.map((review, i) => (
            <button
              key={review.id}
              onClick={() => navigate(`/review/${review.id}`)}
              className={`w-full flex items-center gap-3 px-4 py-2.5 hover:bg-surface-2 transition-colors text-left ${
                i < recentReviews.length - 1 ? 'border-b border-border-subtle' : ''
              }`}
            >
              {review.status === 'Running' || review.status === 'Pending' ? (
                <Loader2 size={13} className="text-accent animate-spin shrink-0" />
              ) : review.status === 'Failed' ? (
                <span className="w-2 h-2 rounded-full bg-sev-error shrink-0" />
              ) : (
                <span className="w-2 h-2 rounded-full bg-accent shrink-0" />
              )}

              <span className="text-[12px] font-medium text-text-primary w-16">{review.diff_source}</span>
              <span className="text-[11px] font-code text-text-muted">{review.id.slice(0, 8)}</span>

              {review.summary && (
                <div className="flex items-center gap-3 ml-4">
                  <span className={`font-code text-[12px] font-bold ${scoreColorClass(review.summary.overall_score)}`}>
                    {review.summary.overall_score.toFixed(1)}
                  </span>
                  <div className="flex items-center gap-2">
                    {Object.entries(review.summary.by_severity).map(([sev, count]) => (
                      count > 0 && (
                        <span key={sev} className="flex items-center gap-1 text-[11px] text-text-muted">
                          <SeverityDot severity={sev as Severity} />
                          {count}
                        </span>
                      )
                    ))}
                  </div>
                </div>
              )}

              <span className={`ml-auto text-[10px] px-2 py-0.5 rounded font-code ${
                STATUS_STYLES[review.status] || 'text-text-muted bg-surface-3'
              }`}>
                {review.status}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

function MetricCard({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="bg-surface-1 border border-border rounded-lg p-4">
      <div className="flex items-center gap-1.5 mb-3">
        <span className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">{label}</span>
      </div>
      {children}
    </div>
  )
}
