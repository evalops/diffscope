import { useMemo } from 'react'
import {
  AreaChart, Area, BarChart, Bar,
  ResponsiveContainer, XAxis, YAxis, Tooltip, CartesianGrid,
} from 'recharts'
import { useReviews } from '../api/hooks'
import { Loader2 } from 'lucide-react'
import { scoreColorClass } from '../lib/scores'
import { SEV_COLORS, CHART_THEME } from '../lib/constants'
import type { ReviewSession, Severity } from '../api/types'

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

  // Aggregate stats
  const totalFindings = completed.reduce((s, r) => s + r.summary!.total_comments, 0)
  const avgFindings = completed.length > 0 ? totalFindings / completed.length : 0
  const avgScore = completed.length > 0
    ? completed.reduce((s, r) => s + r.summary!.overall_score, 0) / completed.length : 0
  const totalFiles = completed.reduce((s, r) => s + r.files_reviewed, 0)

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
    stats: {
      totalReviews: completed.length,
      avgScore,
      totalFindings,
      avgFindings,
      totalFiles,
      sevTotals,
      criticalRate,
    },
  }
}

const tooltipStyle = {
  contentStyle: { background: CHART_THEME.tooltipBg, border: `1px solid ${CHART_THEME.tooltipBorder}`, borderRadius: 6, fontSize: 11 },
  labelStyle: { color: CHART_THEME.tooltipText },
}
const axisTick = { fontSize: 10, fill: CHART_THEME.tick }
const gridProps = { strokeDasharray: '3 3' as const, stroke: CHART_THEME.grid }

export function Analytics() {
  const { data: reviews, isLoading } = useReviews()

  const analytics = useMemo(() => {
    return computeAnalytics(reviews || [])
  }, [reviews])

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="animate-spin text-accent" size={32} />
      </div>
    )
  }

  const { scoreOverTime, severityOverTime, categoryData, stats } = analytics

  if (stats.totalReviews === 0) {
    return (
      <div className="p-6 max-w-5xl mx-auto">
        <h1 className="text-xl font-semibold text-text-primary mb-6">Analytics</h1>
        <div className="bg-surface-1 border border-border rounded-lg p-12 text-center text-text-muted text-sm">
          No completed reviews yet. Run some reviews to see analytics.
        </div>
      </div>
    )
  }

  return (
    <div className="p-6 max-w-5xl mx-auto">
      <h1 className="text-xl font-semibold text-text-primary mb-6">Analytics</h1>

      {/* Top stats row */}
      <div className="grid grid-cols-2 gap-3 mb-6">
        {/* Findings per review chart */}
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
            <ResponsiveContainer width="100%" height="99%"  minWidth={50} minHeight={50}>
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

        {/* Score trend */}
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
            <ResponsiveContainer width="100%" height="99%"  minWidth={50} minHeight={50}>
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

      {/* Bottom row */}
      <div className="grid grid-cols-2 gap-3">
        {/* Severity stacked area */}
        <div className="bg-surface-1 border border-border rounded-lg p-4">
          <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
            SEVERITY BREAKDOWN
          </div>
          <div className="h-44">
            <ResponsiveContainer width="100%" height="99%"  minWidth={50} minHeight={50}>
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

        {/* Category bar chart */}
        <div className="bg-surface-1 border border-border rounded-lg p-4">
          <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">
            FINDINGS BY CATEGORY
          </div>
          {categoryData.length > 0 ? (
            <div className="h-44">
              <ResponsiveContainer width="100%" height="99%"  minWidth={50} minHeight={50}>
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
    </div>
  )
}
