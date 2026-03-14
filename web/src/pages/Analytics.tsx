import { useMemo } from 'react'
import {
  AreaChart, Area, BarChart, Bar,
  ResponsiveContainer, XAxis, YAxis, Tooltip, CartesianGrid,
} from 'recharts'
import { useAnalyticsTrends, useReviews } from '../api/hooks'
import { AlertTriangle, Loader2 } from 'lucide-react'
import { scoreColorClass } from '../lib/scores'
import { SEV_COLORS, CHART_THEME } from '../lib/constants'
import type {
  AnalyticsTrendsResponse,
  FeedbackEvalTrendGap,
  ReviewSession,
  Severity,
} from '../api/types'

function computeAnalytics(reviews: ReviewSession[]) {
  const completed = reviews
    .filter(r => r.status === 'Complete' && r.summary)
    .sort((a, b) => a.started_at.localeCompare(b.started_at))

  // Score over time
  const scoreOverTime = completed.map((r, i) => ({
    idx: i + 1,
    label: `#${i + 1}`,
    score: r.summary!.overall_score,
    findings: r.summary!.total_comments,
    files: r.files_reviewed,
  }))

  // Findings by severity over time
  const severityOverTime = completed.map((r, i) => ({
    idx: i + 1,
    label: `#${i + 1}`,
    Error: r.summary!.by_severity['Error'] || 0,
    Warning: r.summary!.by_severity['Warning'] || 0,
    Info: r.summary!.by_severity['Info'] || 0,
    Suggestion: r.summary!.by_severity['Suggestion'] || 0,
  }))

  // Category distribution
  const catTotals: Record<string, number> = {}
  for (const r of completed) {
    for (const [cat, count] of Object.entries(r.summary!.by_category)) {
      catTotals[cat] = (catTotals[cat] || 0) + count
    }
  }
  const categoryData = Object.entries(catTotals)
    .sort((a, b) => b[1] - a[1])
    .map(([name, value]) => ({ name, value }))

  const feedbackTotalsByCategory: Record<string, { accepted: number; rejected: number }> = {}
  const feedbackTotalsByRule: Record<string, { accepted: number; rejected: number }> = {}
  const feedbackCoverageSeries = completed.map((r, i) => {
    let accepted = 0
    let rejected = 0

    for (const comment of r.comments) {
      if (comment.feedback === 'accept') {
        accepted += 1
      } else if (comment.feedback === 'reject') {
        rejected += 1
      } else {
        continue
      }

      const current = feedbackTotalsByCategory[comment.category] ?? { accepted: 0, rejected: 0 }
      if (comment.feedback === 'accept') {
        current.accepted += 1
      } else {
        current.rejected += 1
      }
      feedbackTotalsByCategory[comment.category] = current

      const ruleId = comment.rule_id?.trim()
      if (ruleId) {
        const currentRule = feedbackTotalsByRule[ruleId] ?? { accepted: 0, rejected: 0 }
        if (comment.feedback === 'accept') {
          currentRule.accepted += 1
        } else {
          currentRule.rejected += 1
        }
        feedbackTotalsByRule[ruleId] = currentRule
      }
    }

    const totalComments = r.comments.length
    const labeled = accepted + rejected

    return {
      idx: i + 1,
      label: `#${i + 1}`,
      coverage: totalComments > 0 ? labeled / totalComments : 0,
      acceptanceRate: labeled > 0 ? accepted / labeled : 0,
      labeled,
      accepted,
      rejected,
      totalComments,
    }
  })

  const feedbackCategoryData = Object.entries(feedbackTotalsByCategory)
    .map(([name, totals]) => {
      const total = totals.accepted + totals.rejected
      return {
        name,
        accepted: totals.accepted,
        rejected: totals.rejected,
        total,
        acceptanceRate: total > 0 ? totals.accepted / total : 0,
      }
    })
    .sort((left, right) => right.total - left.total || right.accepted - left.accepted)

  const feedbackRuleData = Object.entries(feedbackTotalsByRule)
    .map(([name, totals]) => {
      const total = totals.accepted + totals.rejected
      return {
        name,
        accepted: totals.accepted,
        rejected: totals.rejected,
        total,
        acceptanceRate: total > 0 ? totals.accepted / total : 0,
      }
    })
    .sort((left, right) => right.total - left.total || right.accepted - left.accepted)

  const topAcceptedCategories = feedbackCategoryData
    .filter(item => item.accepted > 0)
    .sort((left, right) => right.accepted - left.accepted || right.total - left.total)
    .slice(0, 5)

  const topRejectedCategories = feedbackCategoryData
    .filter(item => item.rejected > 0)
    .sort((left, right) => right.rejected - left.rejected || right.total - left.total)
    .slice(0, 5)

  const topAcceptedRules = feedbackRuleData
    .filter(item => item.accepted > 0)
    .sort((left, right) => right.accepted - left.accepted || right.total - left.total)
    .slice(0, 5)

  const topRejectedRules = feedbackRuleData
    .filter(item => item.rejected > 0)
    .sort((left, right) => right.rejected - left.rejected || right.total - left.total)
    .slice(0, 5)

  // Aggregate stats
  const totalFindings = completed.reduce((s, r) => s + r.summary!.total_comments, 0)
  const avgFindings = completed.length > 0 ? totalFindings / completed.length : 0
  const avgScore = completed.length > 0
    ? completed.reduce((s, r) => s + r.summary!.overall_score, 0) / completed.length : 0
  const totalFiles = completed.reduce((s, r) => s + r.files_reviewed, 0)
  const labeledFeedbackTotal = feedbackCoverageSeries.reduce((sum, point) => sum + point.labeled, 0)
  const acceptedFeedbackTotal = feedbackCoverageSeries.reduce((sum, point) => sum + point.accepted, 0)
  const rejectedFeedbackTotal = feedbackCoverageSeries.reduce((sum, point) => sum + point.rejected, 0)
  const totalCommentCount = completed.reduce((sum, r) => sum + r.comments.length, 0)
  const reviewsWithFeedback = feedbackCoverageSeries.filter(point => point.labeled > 0).length

  const sevTotals: Record<Severity, number> = { Error: 0, Warning: 0, Info: 0, Suggestion: 0 }
  for (const r of completed) {
    for (const [sev, count] of Object.entries(r.summary!.by_severity)) {
      sevTotals[sev as Severity] = (sevTotals[sev as Severity] || 0) + count
    }
  }

  // Critical ratio
  const criticalReviews = completed.filter(r => r.summary!.critical_issues > 0).length
  const criticalRate = completed.length > 0 ? (criticalReviews / completed.length * 100) : 0

  return {
    scoreOverTime,
    severityOverTime,
    categoryData,
    feedbackCoverageSeries,
    topAcceptedCategories,
    topRejectedCategories,
    topAcceptedRules,
    topRejectedRules,
    stats: {
      totalReviews: completed.length,
      avgScore,
      totalFindings,
      avgFindings,
      totalFiles,
      sevTotals,
      criticalRate,
      labeledFeedbackTotal,
      acceptedFeedbackTotal,
      rejectedFeedbackTotal,
      feedbackCoverageRate: totalCommentCount > 0 ? labeledFeedbackTotal / totalCommentCount : 0,
      feedbackAcceptanceRate: labeledFeedbackTotal > 0 ? acceptedFeedbackTotal / labeledFeedbackTotal : 0,
      reviewsWithFeedback,
    },
  }
}

const tooltipStyle = {
  contentStyle: { background: CHART_THEME.tooltipBg, border: `1px solid ${CHART_THEME.tooltipBorder}`, borderRadius: 6, fontSize: 11 },
  labelStyle: { color: CHART_THEME.tooltipText },
}
const axisTick = { fontSize: 10, fill: CHART_THEME.tick }
const gridProps = { strokeDasharray: '3 3' as const, stroke: CHART_THEME.grid }

function formatTrendLabel(timestamp: string, index: number): string {
  const parsed = new Date(timestamp)
  if (Number.isNaN(parsed.getTime())) return `#${index + 1}`
  return `${parsed.getMonth() + 1}/${parsed.getDate()}`
}

function formatPercent(value: number | undefined): string {
  return value == null ? 'n/a' : `${(value * 100).toFixed(0)}%`
}

function computeTrendAnalytics(trends: AnalyticsTrendsResponse | undefined) {
  const evalEntries = trends?.eval_trend.entries ?? []
  const feedbackEntries = trends?.feedback_eval_trend.entries ?? []

  const evalSeries = evalEntries.map((entry, index) => ({
    idx: index + 1,
    label: formatTrendLabel(entry.timestamp, index),
    microF1: entry.micro_f1,
    weightedScore: entry.weighted_score ?? entry.micro_f1,
    fixtures: entry.fixture_count,
    warnings: entry.verification_warning_count ?? 0,
    parseFailures: entry.verification_parse_failure_count ?? 0,
    requestFailures: entry.verification_request_failure_count ?? 0,
  }))

  const feedbackSeries = feedbackEntries.map((entry, index) => ({
    idx: index + 1,
    label: formatTrendLabel(entry.timestamp, index),
    acceptanceRate: entry.acceptance_rate,
    confidenceF1: entry.confidence_f1 ?? 0,
    confidenceAgreement: entry.confidence_agreement_rate ?? 0,
    labeledComments: entry.labeled_comments,
  }))

  return {
    warnings: trends?.warnings ?? [],
    evalEntries,
    feedbackEntries,
    evalSeries,
    feedbackSeries,
    latestEval: evalEntries[evalEntries.length - 1],
    latestFeedback: feedbackEntries[feedbackEntries.length - 1],
    evalTrendPath: trends?.eval_trend_path ?? '.diffscope.eval-trend.json',
    feedbackTrendPath: trends?.feedback_eval_trend_path ?? '.diffscope.feedback-eval-trend.json',
  }
}

function TrendList({ items, emptyLabel }: { items: FeedbackEvalTrendGap[]; emptyLabel: string }) {
  if (items.length === 0) {
    return (
      <div className="text-[11px] text-text-muted">
        {emptyLabel}
      </div>
    )
  }

  return (
    <div className="space-y-2">
      {items.map(item => (
        <div key={item.name} className="flex items-center justify-between gap-3 text-[11px]">
          <div className="min-w-0">
            <div className="text-text-primary font-medium truncate">{item.name}</div>
            <div className="text-text-muted">
              {item.feedback_total} labels · {item.high_confidence_total} high-confidence
            </div>
          </div>
          <div className="text-right shrink-0">
            <div className="font-code text-sev-warning">{formatPercent(item.gap)}</div>
            <div className="text-text-muted">gap</div>
          </div>
        </div>
      ))}
    </div>
  )
}

function FeedbackBreakdownList({
  items,
  mode,
  emptyLabel,
}: {
  items: Array<{ name: string; accepted: number; rejected: number; total: number; acceptanceRate: number }>
  mode: 'accepted' | 'rejected'
  emptyLabel: string
}) {
  if (items.length === 0) {
    return <div className="text-[11px] text-text-muted">{emptyLabel}</div>
  }

  return (
    <div className="space-y-2">
      {items.map(item => {
        const count = mode === 'accepted' ? item.accepted : item.rejected
        return (
          <div key={`${mode}-${item.name}`} className="flex items-center justify-between gap-3 text-[11px]">
            <div className="min-w-0">
              <div className="truncate font-medium text-text-primary">{item.name}</div>
              <div className="text-text-muted">
                {item.total} labeled · {formatPercent(item.acceptanceRate)} accepted
              </div>
            </div>
            <div className={`shrink-0 font-code ${mode === 'accepted' ? 'text-sev-suggestion' : 'text-sev-error'}`}>
              {count}
            </div>
          </div>
        )
      })}
    </div>
  )
}

export function Analytics() {
  const { data: reviews, isLoading } = useReviews()
  const { data: trends, isLoading: trendsLoading } = useAnalyticsTrends()

  const analytics = useMemo(() => {
    return computeAnalytics(reviews || [])
  }, [reviews])
  const trendAnalytics = useMemo(() => computeTrendAnalytics(trends), [trends])

  if (isLoading || trendsLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="animate-spin text-accent" size={32} />
      </div>
    )
  }

  const {
    scoreOverTime,
    severityOverTime,
    categoryData,
    feedbackCoverageSeries,
    topAcceptedCategories,
    topRejectedCategories,
    topAcceptedRules,
    topRejectedRules,
    stats,
  } = analytics
  const {
    warnings,
    evalEntries,
    feedbackEntries,
    evalSeries,
    feedbackSeries,
    latestEval,
    latestFeedback,
    evalTrendPath,
    feedbackTrendPath,
  } = trendAnalytics
  const hasReviewAnalytics = stats.totalReviews > 0
  const hasTrendAnalytics = evalEntries.length > 0 || feedbackEntries.length > 0

  if (!hasReviewAnalytics && !hasTrendAnalytics) {
    return (
      <div className="p-6 max-w-6xl mx-auto">
        <h1 className="text-xl font-semibold text-text-primary mb-6">Analytics</h1>
        <div className="bg-surface-1 border border-border rounded-lg p-12 text-center text-text-muted text-sm">
          No completed reviews or eval trend history yet. Run reviews, `diffscope eval`, or `diffscope feedback-eval` to start building analytics.
        </div>
        <div className="mt-3 text-[10px] text-text-muted font-code">
          eval: {evalTrendPath} · feedback: {feedbackTrendPath}
        </div>
      </div>
    )
  }

  return (
    <div className="p-6 max-w-6xl mx-auto">
      <h1 className="text-xl font-semibold text-text-primary mb-6">Analytics</h1>

      {warnings.length > 0 && (
        <div className="mb-6 flex items-start gap-3 rounded-lg border border-sev-warning/30 bg-sev-warning/10 px-4 py-3 text-[12px] text-text-secondary">
          <AlertTriangle size={14} className="mt-0.5 shrink-0 text-sev-warning" />
          <div>{warnings.join(' ')}</div>
        </div>
      )}

      {hasReviewAnalytics && (
        <>
          <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
            REVIEW ANALYTICS
          </div>

          <div className="grid grid-cols-2 gap-3 mb-6">
            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                FINDINGS PER REVIEW
              </div>
              <div className="flex items-baseline gap-2 mb-1">
                <span className="text-2xl font-bold font-code text-text-primary">
                  {stats.avgFindings.toFixed(1)}
                </span>
                <span className="text-[11px] text-text-muted">avg</span>
              </div>
              <div className="h-32 mt-2">
                <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                  <AreaChart data={scoreOverTime}>
                    <defs>
                      <linearGradient id="findingsGrad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={CHART_THEME.accent} stopOpacity={0.3} />
                        <stop offset="95%" stopColor={CHART_THEME.accent} stopOpacity={0.02} />
                      </linearGradient>
                    </defs>
                    <CartesianGrid {...gridProps} />
                    <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                    <YAxis tick={axisTick} axisLine={false} tickLine={false} />
                    <Tooltip {...tooltipStyle} />
                    <Area type="monotone" dataKey="findings" stroke={CHART_THEME.accent} fill="url(#findingsGrad)" strokeWidth={1.5} dot={false} />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
              <div className="flex items-center gap-6 mt-3 pt-3 border-t border-border-subtle">
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-text-primary">{stats.totalFindings}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">TOTAL</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-sev-error">{stats.sevTotals.Error}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">ERRORS</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-sev-warning">{stats.sevTotals.Warning}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">WARNINGS</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-sev-suggestion">{stats.sevTotals.Suggestion}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">SUGGESTIONS</div>
                </div>
              </div>
            </div>

            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                SCORE TREND
              </div>
              <div className="flex items-baseline gap-2 mb-1">
                <span className={`text-2xl font-bold font-code ${scoreColorClass(stats.avgScore)}`}>
                  {stats.avgScore.toFixed(1)}
                </span>
                <span className="text-[11px] text-text-muted">avg score</span>
              </div>
              <div className="h-32 mt-2">
                <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                  <AreaChart data={scoreOverTime}>
                    <defs>
                      <linearGradient id="scoreGradA" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={CHART_THEME.accent} stopOpacity={0.3} />
                        <stop offset="95%" stopColor={CHART_THEME.accent} stopOpacity={0.02} />
                      </linearGradient>
                    </defs>
                    <CartesianGrid {...gridProps} />
                    <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                    <YAxis domain={[0, 10]} tick={axisTick} axisLine={false} tickLine={false} />
                    <Tooltip {...tooltipStyle} />
                    <Area type="monotone" dataKey="score" stroke={CHART_THEME.accent} fill="url(#scoreGradA)" strokeWidth={1.5} dot={false} />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
              <div className="flex items-center gap-6 mt-3 pt-3 border-t border-border-subtle">
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-text-primary">{stats.totalReviews}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">REVIEWS</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-text-primary">{stats.totalFiles}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">FILES</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-sev-error">{stats.criticalRate.toFixed(0)}%</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">CRITICAL RATE</div>
                </div>
              </div>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                SEVERITY BREAKDOWN
              </div>
              <div className="h-44">
                <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                  <AreaChart data={severityOverTime}>
                    <defs>
                      {Object.entries(SEV_COLORS).map(([key, color]) => (
                        <linearGradient key={key} id={`sev${key}`} x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor={color} stopOpacity={0.4} />
                          <stop offset="95%" stopColor={color} stopOpacity={0.05} />
                        </linearGradient>
                      ))}
                    </defs>
                    <CartesianGrid {...gridProps} />
                    <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                    <YAxis tick={axisTick} axisLine={false} tickLine={false} />
                    <Tooltip {...tooltipStyle} />
                    <Area type="monotone" dataKey="Error" stackId="1" stroke={SEV_COLORS.Error} fill="url(#sevError)" strokeWidth={1} />
                    <Area type="monotone" dataKey="Warning" stackId="1" stroke={SEV_COLORS.Warning} fill="url(#sevWarning)" strokeWidth={1} />
                    <Area type="monotone" dataKey="Info" stackId="1" stroke={SEV_COLORS.Info} fill="url(#sevInfo)" strokeWidth={1} />
                    <Area type="monotone" dataKey="Suggestion" stackId="1" stroke={SEV_COLORS.Suggestion} fill="url(#sevSuggestion)" strokeWidth={1} />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
            </div>

            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                FINDINGS BY CATEGORY
              </div>
              {categoryData.length > 0 ? (
                <div className="h-44">
                  <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                    <BarChart data={categoryData} layout="vertical">
                      <CartesianGrid {...gridProps} horizontal={false} />
                      <XAxis type="number" tick={axisTick} axisLine={false} tickLine={false} />
                      <YAxis type="category" dataKey="name" tick={{ fontSize: 10, fill: CHART_THEME.tooltipText }} axisLine={false} tickLine={false} width={90} />
                      <Tooltip {...tooltipStyle} />
                      <Bar dataKey="value" fill={CHART_THEME.accent} radius={[0, 4, 4, 0]} barSize={14} />
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              ) : (
                <div className="h-44 flex items-center justify-center text-text-muted text-sm">
                  No category data yet
                </div>
              )}
            </div>
          </div>

          <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mt-8 mb-3">
            LEARNING LOOP
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-3">
            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                FEEDBACK COVERAGE
              </div>
              <div className="flex items-baseline gap-2 mb-1">
                <span className="text-2xl font-bold font-code text-accent">
                  {formatPercent(stats.feedbackCoverageRate)}
                </span>
                <span className="text-[11px] text-text-muted">of findings labeled</span>
              </div>
              <div className="h-32 mt-2">
                <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                  <AreaChart data={feedbackCoverageSeries}>
                    <defs>
                      <linearGradient id="feedbackCoverageGrad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={CHART_THEME.accent} stopOpacity={0.35} />
                        <stop offset="95%" stopColor={CHART_THEME.accent} stopOpacity={0.05} />
                      </linearGradient>
                    </defs>
                    <CartesianGrid {...gridProps} />
                    <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                    <YAxis domain={[0, 1]} tick={axisTick} axisLine={false} tickLine={false} />
                    <Tooltip {...tooltipStyle} />
                    <Area type="monotone" dataKey="coverage" stroke={CHART_THEME.accent} fill="url(#feedbackCoverageGrad)" strokeWidth={1.5} dot={false} name="Coverage" />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
              <div className="flex items-center gap-6 mt-3 pt-3 border-t border-border-subtle">
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-text-primary">{stats.labeledFeedbackTotal}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">LABELED</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-text-primary">{stats.reviewsWithFeedback}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">REVIEWS</div>
                </div>
              </div>
            </div>

            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                ACCEPTANCE TREND
              </div>
              <div className="flex items-baseline gap-2 mb-1">
                <span className="text-2xl font-bold font-code text-sev-suggestion">
                  {formatPercent(stats.feedbackAcceptanceRate)}
                </span>
                <span className="text-[11px] text-text-muted">accepted when labeled</span>
              </div>
              <div className="h-32 mt-2">
                <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                  <AreaChart data={feedbackCoverageSeries}>
                    <defs>
                      <linearGradient id="feedbackAcceptanceGrad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={SEV_COLORS.Suggestion} stopOpacity={0.35} />
                        <stop offset="95%" stopColor={SEV_COLORS.Suggestion} stopOpacity={0.05} />
                      </linearGradient>
                    </defs>
                    <CartesianGrid {...gridProps} />
                    <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                    <YAxis domain={[0, 1]} tick={axisTick} axisLine={false} tickLine={false} />
                    <Tooltip {...tooltipStyle} />
                    <Area type="monotone" dataKey="acceptanceRate" stroke={SEV_COLORS.Suggestion} fill="url(#feedbackAcceptanceGrad)" strokeWidth={1.5} dot={false} name="Acceptance rate" />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
              <div className="flex items-center gap-6 mt-3 pt-3 border-t border-border-subtle">
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-sev-suggestion">{stats.acceptedFeedbackTotal}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">ACCEPTED</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-sev-error">{stats.rejectedFeedbackTotal}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">REJECTED</div>
                </div>
              </div>
            </div>

            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                TOP LABELED CATEGORIES
              </div>
              {stats.labeledFeedbackTotal > 0 ? (
                <div className="grid grid-cols-1 gap-4">
                  <div>
                    <div className="text-[10px] font-semibold text-text-muted tracking-[0.05em] font-code mb-2">
                      MOST ACCEPTED
                    </div>
                    <FeedbackBreakdownList
                      items={topAcceptedCategories}
                      mode="accepted"
                      emptyLabel="No accepted categories yet"
                    />
                  </div>
                  <div className="pt-3 border-t border-border-subtle">
                    <div className="text-[10px] font-semibold text-text-muted tracking-[0.05em] font-code mb-2">
                      MOST REJECTED
                    </div>
                    <FeedbackBreakdownList
                      items={topRejectedCategories}
                      mode="rejected"
                      emptyLabel="No rejected categories yet"
                    />
                  </div>
                </div>
              ) : (
                <div className="h-32 flex items-center justify-center text-center text-text-muted text-sm px-6">
                  No thumbs recorded yet. Label findings in review detail to train the reviewer.
                </div>
              )}
            </div>

            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                TOP LABELED RULES
              </div>
              {stats.labeledFeedbackTotal > 0 ? (
                <div className="grid grid-cols-1 gap-4">
                  <div>
                    <div className="text-[10px] font-semibold text-text-muted tracking-[0.05em] font-code mb-2">
                      MOST ACCEPTED
                    </div>
                    <FeedbackBreakdownList
                      items={topAcceptedRules}
                      mode="accepted"
                      emptyLabel="No accepted rules yet"
                    />
                  </div>
                  <div className="pt-3 border-t border-border-subtle">
                    <div className="text-[10px] font-semibold text-text-muted tracking-[0.05em] font-code mb-2">
                      MOST REJECTED
                    </div>
                    <FeedbackBreakdownList
                      items={topRejectedRules}
                      mode="rejected"
                      emptyLabel="No rejected rules yet"
                    />
                  </div>
                </div>
              ) : (
                <div className="h-32 flex items-center justify-center text-center text-text-muted text-sm px-6">
                  Rule-level learning appears once findings with rule IDs receive thumbs.
                </div>
              )}
            </div>
          </div>
        </>
      )}

      <div className={`${hasReviewAnalytics ? 'mt-8' : ''}`}>
        <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
          EVAL QUALITY & FEEDBACK LOOP
        </div>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-6">
          {[
            { label: 'LATEST MICRO F1', value: formatPercent(latestEval?.micro_f1), valueColor: 'text-accent' },
            { label: 'LATEST WEIGHTED SCORE', value: formatPercent(latestEval?.weighted_score), valueColor: 'text-text-primary' },
            { label: 'LATEST ACCEPTANCE RATE', value: formatPercent(latestFeedback?.acceptance_rate), valueColor: 'text-sev-suggestion' },
            { label: 'LATEST CONFIDENCE F1', value: formatPercent(latestFeedback?.confidence_f1), valueColor: 'text-sev-warning' },
          ].map(card => (
            <div key={card.label} className="bg-surface-1 border border-border rounded-lg p-3">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">{card.label}</div>
              <div className={`text-lg font-bold font-code mt-1 ${card.valueColor}`}>{card.value}</div>
            </div>
          ))}
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          <div className="bg-surface-1 border border-border rounded-lg p-4">
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
              EVAL QUALITY TREND
            </div>
            {evalSeries.length > 0 ? (
              <div className="h-44">
                <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                  <AreaChart data={evalSeries}>
                    <defs>
                      <linearGradient id="evalMicroF1Grad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={CHART_THEME.accent} stopOpacity={0.35} />
                        <stop offset="95%" stopColor={CHART_THEME.accent} stopOpacity={0.05} />
                      </linearGradient>
                      <linearGradient id="evalWeightedGrad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={SEV_COLORS.Info} stopOpacity={0.25} />
                        <stop offset="95%" stopColor={SEV_COLORS.Info} stopOpacity={0.04} />
                      </linearGradient>
                    </defs>
                    <CartesianGrid {...gridProps} />
                    <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                    <YAxis domain={[0, 1]} tick={axisTick} axisLine={false} tickLine={false} />
                    <Tooltip {...tooltipStyle} />
                    <Area type="monotone" dataKey="microF1" stroke={CHART_THEME.accent} fill="url(#evalMicroF1Grad)" strokeWidth={1.5} dot={false} name="Micro F1" />
                    <Area type="monotone" dataKey="weightedScore" stroke={SEV_COLORS.Info} fill="url(#evalWeightedGrad)" strokeWidth={1.5} dot={false} name="Weighted score" />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
            ) : (
              <div className="h-44 flex items-center justify-center text-text-muted text-sm text-center px-6">
                No eval trend data yet. `diffscope eval` will append to {evalTrendPath}.
              </div>
            )}
          </div>

          <div className="bg-surface-1 border border-border rounded-lg p-4">
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
              VERIFICATION HEALTH TREND
            </div>
            {evalSeries.length > 0 ? (
              <div className="h-44">
                <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                  <BarChart data={evalSeries}>
                    <CartesianGrid {...gridProps} />
                    <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                    <YAxis tick={axisTick} axisLine={false} tickLine={false} />
                    <Tooltip {...tooltipStyle} />
                    <Bar dataKey="warnings" fill={SEV_COLORS.Warning} radius={[2, 2, 0, 0]} name="Warnings" />
                    <Bar dataKey="parseFailures" fill={SEV_COLORS.Error} radius={[2, 2, 0, 0]} name="Parse failures" />
                    <Bar dataKey="requestFailures" fill={SEV_COLORS.Info} radius={[2, 2, 0, 0]} name="Request failures" />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            ) : (
              <div className="h-44 flex items-center justify-center text-text-muted text-sm text-center px-6">
                Verification health appears once eval trend history exists.
              </div>
            )}
          </div>

          <div className="bg-surface-1 border border-border rounded-lg p-4">
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
              FEEDBACK CALIBRATION TREND
            </div>
            {feedbackSeries.length > 0 ? (
              <div className="h-44">
                <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                  <AreaChart data={feedbackSeries}>
                    <defs>
                      <linearGradient id="feedbackAcceptGrad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={SEV_COLORS.Suggestion} stopOpacity={0.35} />
                        <stop offset="95%" stopColor={SEV_COLORS.Suggestion} stopOpacity={0.05} />
                      </linearGradient>
                      <linearGradient id="feedbackF1Grad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={SEV_COLORS.Warning} stopOpacity={0.25} />
                        <stop offset="95%" stopColor={SEV_COLORS.Warning} stopOpacity={0.04} />
                      </linearGradient>
                    </defs>
                    <CartesianGrid {...gridProps} />
                    <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                    <YAxis domain={[0, 1]} tick={axisTick} axisLine={false} tickLine={false} />
                    <Tooltip {...tooltipStyle} />
                    <Area type="monotone" dataKey="acceptanceRate" stroke={SEV_COLORS.Suggestion} fill="url(#feedbackAcceptGrad)" strokeWidth={1.5} dot={false} name="Acceptance rate" />
                    <Area type="monotone" dataKey="confidenceF1" stroke={SEV_COLORS.Warning} fill="url(#feedbackF1Grad)" strokeWidth={1.5} dot={false} name="Confidence F1" />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
            ) : (
              <div className="h-44 flex items-center justify-center text-text-muted text-sm text-center px-6">
                No feedback-eval trend data yet. `diffscope feedback-eval` will append to {feedbackTrendPath}.
              </div>
            )}
          </div>

          <div className="bg-surface-1 border border-border rounded-lg p-4">
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
              LATEST ATTENTION GAPS
            </div>
            <div className="grid grid-cols-1 gap-4">
              <div>
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.05em] font-code mb-2">
                  BY CATEGORY
                </div>
                <TrendList
                  items={latestFeedback?.attention_by_category ?? []}
                  emptyLabel="No category gaps recorded yet"
                />
              </div>
              <div className="pt-3 border-t border-border-subtle">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.05em] font-code mb-2">
                  BY RULE
                </div>
                <TrendList
                  items={latestFeedback?.attention_by_rule ?? []}
                  emptyLabel="No rule gaps recorded yet"
                />
              </div>
            </div>
          </div>
        </div>

        <div className="mt-3 text-[10px] text-text-muted font-code">
          eval: {evalTrendPath} · feedback: {feedbackTrendPath}
        </div>
      </div>
    </div>
  )
}
