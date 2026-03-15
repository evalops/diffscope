import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  AreaChart, Area, BarChart, Bar,
  ResponsiveContainer, XAxis, YAxis, Tooltip, CartesianGrid,
} from 'recharts'
import { useAnalyticsTrends, useEventStats, useReviews } from '../api/hooks'
import { AlertTriangle, Download, Loader2 } from 'lucide-react'
import type { CostBreakdownRow, FeedbackEvalTrendGap } from '../api/types'
import {
  buildAnalyticsDrilldown,
  buildAnalyticsExportReport,
  computeAnalytics,
  computeTrendAnalytics,
  exportAnalyticsCsv,
  exportAnalyticsJson,
  formatDurationHours,
  formatPercent,
} from '../lib/analytics'
import {
  aggregateCostBreakdowns,
  formatCost,
  formatCostRole,
  formatCostWorkload,
} from '../lib/cost'
import { scoreColorClass } from '../lib/scores'
import { SEV_COLORS, CHART_THEME } from '../lib/constants'

const tooltipStyle = {
  contentStyle: { background: CHART_THEME.tooltipBg, border: `1px solid ${CHART_THEME.tooltipBorder}`, borderRadius: 6, fontSize: 11 },
  labelStyle: { color: CHART_THEME.tooltipText },
}
const axisTick = { fontSize: 10, fill: CHART_THEME.tick }
const gridProps = { strokeDasharray: '3 3' as const, stroke: CHART_THEME.grid }

function getActivePayloadValue<T>(state: unknown): T | undefined {
  return (state as { activePayload?: Array<{ payload?: T }> } | undefined)?.activePayload?.[0]?.payload
}

function formatSignedPercent(value: number | undefined): string {
  if (value == null) {
    return 'n/a'
  }

  const percent = `${(value * 100).toFixed(0)}%`
  return value > 0 ? `+${percent}` : percent
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
  onSelectItem,
}: {
  items: Array<{ name: string; accepted: number; rejected: number; total: number; acceptanceRate: number }>
  mode: 'accepted' | 'rejected'
  emptyLabel: string
  onSelectItem?: (name: string) => void
}) {
  if (items.length === 0) {
    return <div className="text-[11px] text-text-muted">{emptyLabel}</div>
  }

  return (
    <div className="space-y-2">
      {items.map(item => {
        const count = mode === 'accepted' ? item.accepted : item.rejected
        const content = (
          <>
            <div className="min-w-0 text-left">
              <div className="truncate font-medium text-text-primary">{item.name}</div>
              <div className="text-text-muted">
                {item.total} labeled · {formatPercent(item.acceptanceRate)} accepted
              </div>
            </div>
            <div className={`shrink-0 font-code ${mode === 'accepted' ? 'text-sev-suggestion' : 'text-sev-error'}`}>
              {count}
            </div>
          </>
        )

        if (onSelectItem) {
          return (
            <button
              key={`${mode}-${item.name}`}
              type="button"
              onClick={() => onSelectItem(item.name)}
              className="flex w-full items-center justify-between gap-3 rounded px-1 py-1 text-[11px] hover:bg-surface-2"
            >
              {content}
            </button>
          )
        }

        return (
          <div key={`${mode}-${item.name}`} className="flex items-center justify-between gap-3 text-[11px]">
            {content}
          </div>
        )
      })}
    </div>
  )
}

function ContextSourceList({
  items,
  emptyLabel,
  onSelectItem,
}: {
  items: Array<{
    name: string
    label: string
    total: number
    labeled: number
    accepted: number
    rejected: number
    resolved: number
    reviewCount: number
    acceptanceRate: number
    fixRate: number
  }>
  emptyLabel: string
  onSelectItem?: (name: string) => void
}) {
  if (items.length === 0) {
    return <div className="text-[11px] text-text-muted">{emptyLabel}</div>
  }

  return (
    <div className="space-y-2">
      {items.map(item => {
        const content = (
          <>
            <div className="min-w-0 text-left">
              <div className="truncate font-medium text-text-primary">{item.label}</div>
              <div className="text-text-muted">
                {item.total} finding{item.total === 1 ? '' : 's'} · {item.reviewCount} review{item.reviewCount === 1 ? '' : 's'} · {formatPercent(item.acceptanceRate)} accepted · {formatPercent(item.fixRate)} fixed
              </div>
            </div>
            <div className="shrink-0 font-code text-text-primary">{item.total}</div>
          </>
        )

        if (onSelectItem) {
          return (
            <button
              key={item.name}
              type="button"
              onClick={() => onSelectItem(item.name)}
              className="flex w-full items-center justify-between gap-3 rounded px-1 py-1 text-[11px] hover:bg-surface-2"
            >
              {content}
            </button>
          )
        }

        return (
          <div key={item.name} className="flex items-center justify-between gap-3 text-[11px]">
            {content}
          </div>
        )
      })}
    </div>
  )
}

function CostBreakdownList({
  items,
  emptyLabel,
}: {
  items: CostBreakdownRow[]
  emptyLabel: string
}) {
  if (items.length === 0) {
    return <div className="text-[11px] text-text-muted">{emptyLabel}</div>
  }

  return (
    <div className="space-y-2">
      {items.map(item => (
        <div
          key={`${item.workload}-${item.role}-${item.provider ?? 'unknown'}-${item.model}`}
          className="flex items-center justify-between gap-3 text-[11px]"
        >
          <div className="min-w-0">
            <div className="truncate font-medium text-text-primary">
              {formatCostWorkload(item.workload)} · {formatCostRole(item.role)}
            </div>
            <div className="truncate text-text-muted">
              {[item.provider, item.model, `${item.total_tokens.toLocaleString()} tokens`]
                .filter(Boolean)
                .join(' · ')}
            </div>
          </div>
          <div className="shrink-0 text-right">
            <div className="font-code text-text-primary">{formatCost(item.cost_estimate_usd)}</div>
            <div className="text-text-muted">est.</div>
          </div>
        </div>
      ))}
    </div>
  )
}

export function Analytics() {
  const navigate = useNavigate()
  const { data: reviews, isLoading } = useReviews()
  const { data: trends, isLoading: trendsLoading } = useAnalyticsTrends()
  const { data: eventStats } = useEventStats()
  const [drilldownSelection, setDrilldownSelection] = useState<
    { type: 'review'; reviewId: string }
    | { type: 'category'; category: string }
    | { type: 'rule'; ruleId: string }
    | { type: 'contextSource'; source: string }
    | { type: 'patternRepositorySource'; source: string }
    | null
  >(null)

  const analytics = useMemo(() => {
    return computeAnalytics(reviews || [])
  }, [reviews])
  const trendAnalytics = useMemo(() => computeTrendAnalytics(trends), [trends])
  const drilldown = useMemo(
    () => (drilldownSelection ? buildAnalyticsDrilldown(reviews || [], drilldownSelection) : null),
    [drilldownSelection, reviews],
  )
  const exportReport = useMemo(
    () => buildAnalyticsExportReport(reviews || [], trends),
    [reviews, trends],
  )
  const reviewCostBreakdowns = useMemo(
    () => eventStats?.cost_breakdowns ?? [],
    [eventStats],
  )
  const evalCostBreakdowns = useMemo(
    () => aggregateCostBreakdowns(
      (trends?.eval_trend?.entries ?? []).flatMap(entry => entry.cost_breakdowns ?? []),
    ),
    [trends],
  )
  const reviewCostTotal = useMemo(
    () => reviewCostBreakdowns.reduce((sum, item) => sum + item.cost_estimate_usd, 0)
      || eventStats?.total_cost_estimate
      || 0,
    [eventStats?.total_cost_estimate, reviewCostBreakdowns],
  )
  const evalCostTotal = useMemo(
    () => evalCostBreakdowns.reduce((sum, item) => sum + item.cost_estimate_usd, 0),
    [evalCostBreakdowns],
  )

  const selectReviewDrilldown = (reviewId?: string) => {
    if (reviewId) {
      setDrilldownSelection({ type: 'review', reviewId })
    }
  }

  const selectCategoryDrilldown = (category?: string) => {
    if (category) {
      setDrilldownSelection({ type: 'category', category })
    }
  }

  const selectRuleDrilldown = (ruleId?: string) => {
    if (ruleId) {
      setDrilldownSelection({ type: 'rule', ruleId })
    }
  }

  const selectContextSourceDrilldown = (source?: string) => {
    if (source) {
      setDrilldownSelection({ type: 'contextSource', source })
    }
  }

  const openReviewTarget = (reviewId: string, params?: Record<string, string | undefined>) => {
    const searchParams = new URLSearchParams()
    Object.entries(params ?? {}).forEach(([key, value]) => {
      if (value) {
        searchParams.set(key, value)
      }
    })

    const search = searchParams.toString()
    navigate(search ? `/review/${reviewId}?${search}` : `/review/${reviewId}`)
  }

  const openDrilldownReview = (reviewId: string) => {
    if (!drilldownSelection || drilldownSelection.type === 'review') {
      openReviewTarget(reviewId)
      return
    }

    if (drilldownSelection.type === 'category') {
      openReviewTarget(reviewId, { view: 'list', category: drilldownSelection.category })
      return
    }

    if (
      drilldownSelection.type === 'contextSource'
      || drilldownSelection.type === 'patternRepositorySource'
    ) {
      openReviewTarget(reviewId)
      return
    }

    openReviewTarget(reviewId, { view: 'list', rule: drilldownSelection.ruleId })
  }

  const openDrilldownComment = (comment: NonNullable<typeof drilldown>['comments'][number]) => {
    openReviewTarget(comment.reviewId, {
      view: 'list',
      file: comment.filePath,
      comment: comment.id,
      category: comment.category,
      rule: comment.ruleId,
    })
  }

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
    completenessSeries,
    meanTimeToResolutionSeries,
    feedbackCoverageSeries,
    topAcceptedCategories,
    topRejectedCategories,
    topAcceptedRules,
    topRejectedRules,
    contextSourceSeries,
    contextSourceData,
    stats,
  } = analytics
  const {
    warnings,
    evalEntries,
    feedbackEntries,
    evalSeries,
    feedbackSeries,
    independentAuditorStory,
    latestEval,
    latestFeedback,
    evalTrendPath,
    feedbackTrendPath,
  } = trendAnalytics
  const hasReviewAnalytics = stats.totalReviews > 0
  const hasTrendAnalytics = evalEntries.length > 0 || feedbackEntries.length > 0
  const hasCostAnalytics = reviewCostBreakdowns.length > 0 || evalCostBreakdowns.length > 0

  if (!hasReviewAnalytics && !hasTrendAnalytics) {
    return (
      <div className="p-6 max-w-6xl mx-auto">
        <div className="mb-6 flex items-center justify-between gap-3">
          <h1 className="text-xl font-semibold text-text-primary">Analytics</h1>
          <div className="flex items-center gap-2">
            <button
              onClick={() => exportAnalyticsCsv(exportReport)}
              className="inline-flex items-center gap-1.5 bg-surface-1 border border-border rounded px-2.5 py-1.5 text-[12px] text-text-secondary hover:text-text-primary font-code transition-colors focus-visible:border-accent/50"
            >
              <Download size={13} />
              Export CSV
            </button>
            <button
              onClick={() => exportAnalyticsJson(exportReport)}
              className="inline-flex items-center gap-1.5 bg-surface-1 border border-border rounded px-2.5 py-1.5 text-[12px] text-text-secondary hover:text-text-primary font-code transition-colors focus-visible:border-accent/50"
            >
              Export JSON
            </button>
          </div>
        </div>
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
      <div className="mb-6 flex items-center justify-between gap-3">
        <h1 className="text-xl font-semibold text-text-primary">Analytics</h1>
        <div className="flex items-center gap-2">
          <button
            onClick={() => exportAnalyticsCsv(exportReport)}
            className="inline-flex items-center gap-1.5 bg-surface-1 border border-border rounded px-2.5 py-1.5 text-[12px] text-text-secondary hover:text-text-primary font-code transition-colors"
          >
            <Download size={13} />
            Export CSV
          </button>
          <button
            onClick={() => exportAnalyticsJson(exportReport)}
            className="inline-flex items-center gap-1.5 bg-surface-1 border border-border rounded px-2.5 py-1.5 text-[12px] text-text-secondary hover:text-text-primary font-code transition-colors focus-visible:border-accent/50"
          >
            Export JSON
          </button>
        </div>
      </div>

      {warnings.length > 0 && (
        <div className="mb-6 flex items-start gap-3 rounded-lg border border-sev-warning/30 bg-sev-warning/10 px-4 py-3 text-[12px] text-text-secondary">
          <AlertTriangle size={14} className="mt-0.5 shrink-0 text-sev-warning" />
          <div>{warnings.join(' ')}</div>
        </div>
      )}

      {hasCostAnalytics && (
        <>
          <div className="mb-3 text-[10px] font-semibold tracking-[0.08em] text-text-muted font-code">
            COST ROUTING
          </div>
          <div className="mb-6 grid grid-cols-2 gap-3">
            <div className="rounded-lg border border-border bg-surface-1 p-4">
              <div className="mb-1 text-[10px] font-semibold tracking-[0.08em] text-text-muted font-code">
                REVIEW WORKLOADS
              </div>
              <div className="mb-3 flex items-baseline gap-2">
                <span className="text-2xl font-bold font-code text-text-primary">
                  {formatCost(reviewCostTotal)}
                </span>
                <span className="text-[11px] text-text-muted">tracked</span>
              </div>
              <CostBreakdownList
                items={reviewCostBreakdowns.slice(0, 6)}
                emptyLabel="No review cost routing data yet."
              />
            </div>
            <div className="rounded-lg border border-border bg-surface-1 p-4">
              <div className="mb-1 text-[10px] font-semibold tracking-[0.08em] text-text-muted font-code">
                EVAL WORKLOADS
              </div>
              <div className="mb-3 flex items-baseline gap-2">
                <span className="text-2xl font-bold font-code text-text-primary">
                  {formatCost(evalCostTotal)}
                </span>
                <span className="text-[11px] text-text-muted">tracked</span>
              </div>
              <CostBreakdownList
                items={evalCostBreakdowns.slice(0, 6)}
                emptyLabel="No eval cost routing data yet."
              />
            </div>
          </div>
        </>
      )}

      {hasReviewAnalytics && (
        <>
          <div className="flex items-center gap-4 mb-3">
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">
              REVIEW ANALYTICS
            </div>
            {eventStats != null && (
              <div className="text-[11px] text-text-muted font-code">
                Est. runtime cost: <span className="text-text-primary font-medium">{formatCost(eventStats.total_cost_estimate)}</span>
              </div>
            )}
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
                  <AreaChart
                    data={scoreOverTime}
                    onClick={state => selectReviewDrilldown(
                      getActivePayloadValue<{ reviewId?: string }>(state)?.reviewId,
                    )}
                  >
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
                  <AreaChart
                    data={scoreOverTime}
                    onClick={state => selectReviewDrilldown(
                      getActivePayloadValue<{ reviewId?: string }>(state)?.reviewId,
                    )}
                  >
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
                    <BarChart
                      data={categoryData}
                      layout="vertical"
                      onClick={state => selectCategoryDrilldown(getActivePayloadValue<{ name?: string }>(state)?.name)}
                    >
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
            LIFECYCLE FOLLOW-THROUGH
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-3 mb-6">
            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                COMPLETENESS TREND
              </div>
              <div className="flex items-baseline gap-2 mb-1">
                <span className="text-2xl font-bold font-code text-accent">
                  {formatPercent(stats.completenessRate)}
                </span>
                <span className="text-[11px] text-text-muted">acknowledged of findings</span>
              </div>
              <div className="h-32 mt-2">
                <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                  <AreaChart
                    data={completenessSeries}
                    onClick={state => selectReviewDrilldown(
                      getActivePayloadValue<{ reviewId?: string }>(state)?.reviewId,
                    )}
                  >
                    <defs>
                      <linearGradient id="completenessAckGrad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={CHART_THEME.accent} stopOpacity={0.35} />
                        <stop offset="95%" stopColor={CHART_THEME.accent} stopOpacity={0.05} />
                      </linearGradient>
                      <linearGradient id="completenessFixedGrad" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={SEV_COLORS.Suggestion} stopOpacity={0.25} />
                        <stop offset="95%" stopColor={SEV_COLORS.Suggestion} stopOpacity={0.04} />
                      </linearGradient>
                    </defs>
                    <CartesianGrid {...gridProps} />
                    <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                    <YAxis domain={[0, 1]} tick={axisTick} axisLine={false} tickLine={false} />
                    <Tooltip {...tooltipStyle} />
                    <Area type="monotone" dataKey="acknowledgedRate" stroke={CHART_THEME.accent} fill="url(#completenessAckGrad)" strokeWidth={1.5} dot={false} name="Acknowledged rate" />
                    <Area type="monotone" dataKey="fixedRate" stroke={SEV_COLORS.Suggestion} fill="url(#completenessFixedGrad)" strokeWidth={1.5} dot={false} name="Fixed rate" />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
              <div className="flex items-center gap-6 mt-3 pt-3 border-t border-border-subtle">
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-text-primary">{stats.totalAcknowledgedFindings}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">ACKNOWLEDGED</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-sev-suggestion">{stats.totalFixedFindings}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">FIXED</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-sev-warning">{stats.totalStaleFindings}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">STALE</div>
                </div>
              </div>
            </div>

            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                MEAN TIME TO RESOLUTION
              </div>
              <div className="flex items-baseline gap-2 mb-1">
                <span className="text-2xl font-bold font-code text-sev-suggestion">
                  {formatDurationHours(stats.meanTimeToResolutionHours)}
                </span>
                <span className="text-[11px] text-text-muted">avg for fixed findings</span>
              </div>
              {stats.resolvedWithTimestampCount > 0 ? (
                <div className="h-32 mt-2">
                  <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                    <AreaChart
                      data={meanTimeToResolutionSeries}
                      onClick={state => selectReviewDrilldown(
                        getActivePayloadValue<{ reviewId?: string }>(state)?.reviewId,
                      )}
                    >
                      <defs>
                        <linearGradient id="mttrGrad" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor={SEV_COLORS.Suggestion} stopOpacity={0.35} />
                          <stop offset="95%" stopColor={SEV_COLORS.Suggestion} stopOpacity={0.05} />
                        </linearGradient>
                      </defs>
                      <CartesianGrid {...gridProps} />
                      <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                      <YAxis tick={axisTick} axisLine={false} tickLine={false} />
                      <Tooltip {...tooltipStyle} />
                      <Area type="monotone" dataKey="meanHours" stroke={SEV_COLORS.Suggestion} fill="url(#mttrGrad)" strokeWidth={1.5} dot={false} name="Mean hours" />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
              ) : (
                <div className="h-32 mt-2 flex items-center justify-center text-center text-text-muted text-sm px-6">
                  Resolution timing appears once resolved findings include timestamps. Older reviews without tracked resolution times are skipped.
                </div>
              )}
              <div className="flex items-center gap-6 mt-3 pt-3 border-t border-border-subtle">
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-sev-suggestion">{stats.totalFixedFindings}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">FIXED</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-text-primary">{stats.resolvedWithTimestampCount}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">TIMED</div>
                </div>
                <div className="text-center">
                  <div className="text-sm font-bold font-code text-text-primary">{stats.reviewsWithTimedResolutions}</div>
                  <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">REVIEWS</div>
                </div>
              </div>
            </div>
          </div>

          <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mt-8 mb-3">
            LEARNING LOOP
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 2xl:grid-cols-5 gap-3">
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
                  <AreaChart
                    data={feedbackCoverageSeries}
                    onClick={state => selectReviewDrilldown(
                      getActivePayloadValue<{ reviewId?: string }>(state)?.reviewId,
                    )}
                  >
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
                  <AreaChart
                    data={feedbackCoverageSeries}
                    onClick={state => selectReviewDrilldown(
                      getActivePayloadValue<{ reviewId?: string }>(state)?.reviewId,
                    )}
                  >
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
                LEARNING EFFECTIVENESS
              </div>
              {stats.feedbackLearningLabeledTotal > 0 ? (
                <>
                  <div className="flex items-baseline gap-2 mb-3">
                    <span className={`text-2xl font-bold font-code ${
                      (stats.feedbackLearningAcceptanceLift ?? 0) >= 0
                        ? 'text-sev-suggestion'
                        : 'text-sev-warning'
                    }`}>
                      {stats.feedbackLearningAcceptanceLift != null
                        ? formatSignedPercent(stats.feedbackLearningAcceptanceLift)
                        : formatPercent(stats.feedbackLearningAcceptanceRate)}
                    </span>
                    <span className="text-[11px] text-text-muted">
                      {stats.feedbackLearningAcceptanceLift != null ? 'lift vs baseline' : 'accepted when tuned'}
                    </span>
                  </div>

                  <div className="grid grid-cols-2 gap-2 text-[11px]">
                    <div className="rounded border border-border-subtle bg-surface px-2 py-1.5">
                      <div className="text-text-muted">Tuned</div>
                      <div className="mt-0.5 font-code text-text-primary">
                        {formatPercent(stats.feedbackLearningAcceptanceRate)}
                      </div>
                    </div>
                    <div className="rounded border border-border-subtle bg-surface px-2 py-1.5">
                      <div className="text-text-muted">Baseline</div>
                      <div className="mt-0.5 font-code text-text-primary">
                        {stats.feedbackLearningBaselineAcceptanceRate != null
                          ? formatPercent(stats.feedbackLearningBaselineAcceptanceRate)
                          : 'n/a'}
                      </div>
                    </div>
                    <div className="rounded border border-border-subtle bg-surface px-2 py-1.5">
                      <div className="text-text-muted">Tuned labeled</div>
                      <div className="mt-0.5 font-code text-text-primary">{stats.feedbackLearningLabeledTotal}</div>
                    </div>
                    <div className="rounded border border-border-subtle bg-surface px-2 py-1.5">
                      <div className="text-text-muted">Reviews</div>
                      <div className="mt-0.5 font-code text-text-primary">{stats.feedbackLearningReviewCount}</div>
                    </div>
                  </div>

                  <div className="flex items-center gap-6 mt-3 pt-3 border-t border-border-subtle">
                    <div className="text-center">
                      <div className="text-sm font-bold font-code text-sev-suggestion">{stats.feedbackLearningBoostedAcceptedTotal}</div>
                      <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">BOOSTED OK</div>
                    </div>
                    <div className="text-center">
                      <div className="text-sm font-bold font-code text-sev-warning">{stats.feedbackLearningDemotedRejectedTotal}</div>
                      <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">DEMOTED REJECTED</div>
                    </div>
                  </div>
                </>
              ) : (
                <div className="h-32 flex items-center justify-center text-center text-text-muted text-sm px-6">
                  No feedback-tuned findings have been labeled yet. Thumbs on learning-tagged findings will show lift here.
                </div>
              )}
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
                      onSelectItem={selectCategoryDrilldown}
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
                      onSelectItem={selectCategoryDrilldown}
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
                      onSelectItem={selectRuleDrilldown}
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
                      onSelectItem={selectRuleDrilldown}
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

          <div className="grid grid-cols-1 xl:grid-cols-2 gap-3 mt-3">
            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                CONTEXT SOURCE IMPACT
              </div>
              {stats.contextSourceFindingTotal > 0 ? (
                <>
                  <div className="flex items-baseline gap-2 mb-1">
                    <span className={`text-2xl font-bold font-code ${
                      (stats.contextSourceAcceptanceLift ?? 0) >= 0
                        ? 'text-sev-suggestion'
                        : 'text-sev-warning'
                    }`}>
                      {stats.contextSourceAcceptanceLift != null
                        ? formatSignedPercent(stats.contextSourceAcceptanceLift)
                        : formatPercent(stats.contextSourceAcceptanceRate)}
                    </span>
                    <span className="text-[11px] text-text-muted">
                      {stats.contextSourceAcceptanceLift != null ? 'acceptance lift vs baseline' : 'accepted when labeled'}
                    </span>
                  </div>
                  <div className="text-[11px] text-text-muted">
                    {formatPercent(stats.contextSourceAcceptanceRate)} accepted across {stats.contextSourceLabeledTotal} labeled context-backed findings · {formatPercent(stats.contextSourceFixRate)} fixed overall.
                  </div>
                  <div className="h-32 mt-2">
                    <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                      <AreaChart
                        data={contextSourceSeries}
                        onClick={state => selectReviewDrilldown(
                          getActivePayloadValue<{ reviewId?: string }>(state)?.reviewId,
                        )}
                      >
                        <defs>
                          <linearGradient id="contextSourceGrad" x1="0" y1="0" x2="0" y2="1">
                            <stop offset="5%" stopColor={CHART_THEME.accent} stopOpacity={0.35} />
                            <stop offset="95%" stopColor={CHART_THEME.accent} stopOpacity={0.05} />
                          </linearGradient>
                        </defs>
                        <CartesianGrid {...gridProps} />
                        <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                        <YAxis tick={axisTick} axisLine={false} tickLine={false} />
                        <Tooltip {...tooltipStyle} />
                        <Area type="monotone" dataKey="findings" stroke={CHART_THEME.accent} fill="url(#contextSourceGrad)" strokeWidth={1.5} dot={false} name="Context-backed findings" />
                      </AreaChart>
                    </ResponsiveContainer>
                  </div>
                  <div className="flex items-center gap-6 mt-3 pt-3 border-t border-border-subtle">
                    <div className="text-center">
                      <div className="text-sm font-bold font-code text-text-primary">{stats.contextSourceFindingTotal}</div>
                      <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">FINDINGS</div>
                    </div>
                    <div className="text-center">
                      <div className="text-sm font-bold font-code text-text-primary">{stats.contextSourceReviewCount}</div>
                      <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">REVIEWS</div>
                    </div>
                    <div className="text-center">
                      <div className="text-sm font-bold font-code text-text-primary">{stats.contextSourceSourceCount}</div>
                      <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">SOURCES</div>
                    </div>
                    <div className="text-center">
                      <div className="text-sm font-bold font-code text-accent">{formatPercent(stats.contextSourceFixRate)}</div>
                      <div className="text-[10px] text-text-muted tracking-[0.05em] font-code">FIX RATE</div>
                    </div>
                  </div>
                </>
              ) : (
                <div className="h-32 flex items-center justify-center text-center text-text-muted text-sm px-6">
                  No context-backed findings yet. Jira, Linear, custom context, and pattern-repository evidence will appear here once they influence completed reviews.
                </div>
              )}
            </div>

            <div className="bg-surface-1 border border-border rounded-lg p-4">
              <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
                TOP CONTEXT SOURCES
              </div>
              <ContextSourceList
                items={contextSourceData.slice(0, 8)}
                emptyLabel="No context sources have influenced findings yet"
                onSelectItem={selectContextSourceDrilldown}
              />
            </div>
          </div>

          {drilldown && (
            <div className="mt-8 rounded-lg border border-border bg-surface-1 p-4">
              <div className="mb-4 flex items-start justify-between gap-3">
                <div>
                  <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">
                    ANALYTICS DRILL-DOWN
                  </div>
                  <div className="mt-1 text-sm font-semibold text-text-primary">{drilldown.title}</div>
                  <div className="mt-1 text-[12px] text-text-secondary">
                    {drilldown.description} Jump straight into the matching review or finding below.
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => setDrilldownSelection(null)}
                  className="rounded border border-border px-2 py-1 text-[11px] text-text-secondary hover:text-text-primary"
                >
                  Clear
                </button>
              </div>

              <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
                <div className="rounded border border-border-subtle bg-surface p-3">
                  <div className="mb-2 text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">
                    REVIEWS
                  </div>
                  <div className="space-y-2">
                    {drilldown.reviews.map(review => (
                      <div key={review.id} className="flex items-center justify-between gap-3 rounded bg-surface-2 px-3 py-2 text-[11px]">
                        <div className="min-w-0">
                          <div className="font-medium text-text-primary">Review {review.label}</div>
                          <div className="text-text-muted">
                            {review.findingCount} finding{review.findingCount === 1 ? '' : 's'}
                            {review.overallScore != null ? ` · score ${review.overallScore.toFixed(1)}` : ''}
                          </div>
                        </div>
                        <button
                          type="button"
                          onClick={() => openDrilldownReview(review.id)}
                          className="shrink-0 rounded border border-border px-2 py-1 text-[10px] text-text-secondary hover:text-text-primary"
                        >
                          Open review
                        </button>
                      </div>
                    ))}
                  </div>
                </div>

                <div className="rounded border border-border-subtle bg-surface p-3 xl:col-span-2">
                  <div className="mb-2 text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">
                    FINDINGS
                  </div>
                  <div className="max-h-72 space-y-2 overflow-y-auto pr-1">
                    {drilldown.comments.map(comment => (
                      <div key={comment.id} className="rounded bg-surface-2 px-3 py-2 text-[11px]">
                        <div className="flex items-center justify-between gap-3">
                          <button
                            type="button"
                            onClick={() => navigate(`/review/${comment.reviewId}`)}
                            className="font-code text-accent hover:underline"
                          >
                            Review {comment.reviewLabel}
                          </button>
                          <div className="text-text-muted">
                            {comment.filePath}:{comment.lineNumber}
                          </div>
                        </div>
                        <div className="mt-1 text-text-primary">{comment.content}</div>
                        <div className="mt-1 flex flex-wrap gap-2 text-[10px] text-text-muted">
                          <span>{comment.category}</span>
                          <button
                            type="button"
                            onClick={() => openDrilldownComment(comment)}
                            className="rounded border border-border px-1.5 py-0.5 hover:text-text-primary"
                          >
                            Open finding
                          </button>
                          {comment.ruleId && (
                            <button
                              type="button"
                              onClick={() => selectRuleDrilldown(comment.ruleId)}
                              className="rounded border border-border px-1.5 py-0.5 font-code hover:text-text-primary"
                            >
                              {comment.ruleId}
                            </button>
                          )}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>

                <div className="rounded border border-border-subtle bg-surface p-3 xl:col-span-3">
                  <div className="mb-2 text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">
                    RELATED RULES
                  </div>
                  {drilldown.relatedRules.length > 0 ? (
                    <div className="flex flex-wrap gap-2">
                      {drilldown.relatedRules.map(ruleId => (
                        <button
                          key={ruleId}
                          type="button"
                          onClick={() => selectRuleDrilldown(ruleId)}
                          className="rounded border border-border px-2 py-1 text-[11px] font-code text-text-secondary hover:text-text-primary"
                        >
                          {ruleId}
                        </button>
                      ))}
                    </div>
                  ) : (
                    <div className="text-[11px] text-text-muted">No rule IDs are attached to this selection yet.</div>
                  )}
                </div>
              </div>
            </div>
          )}
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

        <div className="bg-surface-1 border border-border rounded-lg p-4 mb-6">
          <div className="flex items-center justify-between gap-3 mb-3">
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">
              INDEPENDENT AUDITOR BENCHMARK
            </div>
            {independentAuditorStory?.winnerReviewMode && (
              <div className="text-[10px] font-code text-accent">
                {independentAuditorStory.winnerReviewMode}
              </div>
            )}
          </div>
          {independentAuditorStory ? (
            <div className="grid grid-cols-1 md:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)] gap-4">
              <div>
                <div className="text-sm font-medium text-text-primary">{independentAuditorStory.benchmarkLabel}</div>
                <div className="text-[11px] text-text-secondary mt-1">{independentAuditorStory.winnerReviewer}</div>
                <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mt-4">
                  {[
                    { label: 'USEFULNESS', value: formatPercent(independentAuditorStory.winnerUsefulnessScore), valueColor: 'text-accent' },
                    { label: 'WEIGHTED', value: formatPercent(independentAuditorStory.winnerWeightedScore), valueColor: 'text-text-primary' },
                    { label: 'VERIFY', value: formatPercent(independentAuditorStory.winnerVerificationHealth), valueColor: 'text-sev-warning' },
                    { label: 'LIFECYCLE', value: formatPercent(independentAuditorStory.winnerLifecycleAccuracy), valueColor: 'text-sev-suggestion' },
                  ].map(card => (
                    <div key={card.label} className="rounded border border-border-subtle bg-surface-0 px-3 py-2">
                      <div className="text-[10px] font-semibold text-text-muted tracking-[0.05em] font-code">{card.label}</div>
                      <div className={`mt-1 text-sm font-bold font-code ${card.valueColor}`}>{card.value}</div>
                    </div>
                  ))}
                </div>
              </div>

              <div className="rounded border border-border-subtle bg-surface-0 px-3 py-3">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.05em] font-code mb-3">
                  REVIEW MODE DELTA
                </div>
                {independentAuditorStory.comparison ? (
                  <div className="space-y-2 text-[11px]">
                    <div className="text-text-secondary">
                      {independentAuditorStory.comparison.compareReviewMode} vs {independentAuditorStory.comparison.baselineReviewMode}
                    </div>
                    {[
                      { label: 'USEFULNESS', value: formatSignedPercent(independentAuditorStory.comparison.usefulnessScoreDelta), valueColor: 'text-accent' },
                      { label: 'WEIGHTED', value: formatSignedPercent(independentAuditorStory.comparison.weightedScoreDelta), valueColor: 'text-text-primary' },
                      { label: 'MICRO F1', value: formatSignedPercent(independentAuditorStory.comparison.microF1Delta), valueColor: 'text-sev-warning' },
                      { label: 'PASS RATE', value: formatSignedPercent(independentAuditorStory.comparison.passRateDelta), valueColor: 'text-sev-suggestion' },
                    ].map(item => (
                      <div key={item.label} className="flex items-center justify-between gap-3">
                        <div className="text-text-muted font-code">{item.label}</div>
                        <div className={`font-code ${item.valueColor}`}>{item.value}</div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="text-[11px] text-text-muted">
                    Run `diffscope eval --compare-agent-loop` to append a side-by-side benchmark story.
                  </div>
                )}
              </div>
            </div>
          ) : (
            <div className="text-[11px] text-text-muted">
              No independent-auditor benchmark published yet. Eval trend history will surface it here.
            </div>
          )}
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
