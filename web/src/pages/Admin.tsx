import { useMemo, useState } from 'react'
import {
  AreaChart, Area, BarChart, Bar, PieChart, Pie, Cell,
  ResponsiveContainer, XAxis, YAxis, Tooltip, CartesianGrid,
} from 'recharts'
import { useEvents } from '../api/hooks'
import { Loader2, AlertTriangle, ArrowUpRight, ArrowDownRight, DollarSign, ToggleLeft, ToggleRight } from 'lucide-react'
import { CHART_THEME, SEV_COLORS } from '../lib/constants'
import { estimateCost, formatCost, totalCost } from '../lib/cost'
import type { ReviewEvent } from '../api/types'

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  return `${(ms / 60_000).toFixed(1)}m`
}

const fmtTokens = (n: number) => n.toLocaleString()

const tooltipStyle = {
  contentStyle: { background: CHART_THEME.tooltipBg, border: `1px solid ${CHART_THEME.tooltipBorder}`, borderRadius: 6, fontSize: 11 },
  labelStyle: { color: CHART_THEME.tooltipText },
}
const axisTick = { fontSize: 10, fill: CHART_THEME.tick }
const gridProps = { strokeDasharray: '3 3' as const, stroke: CHART_THEME.grid }

const SOURCE_COLORS = ['#4ade80', '#3b82f6', '#f59e0b', '#a78bfa', '#f472b6']

// Time range filter configuration
type TimeRange = '1h' | '24h' | '7d' | '30d' | 'All'
const TIME_RANGES: { key: TimeRange; label: string; fraction: number }[] = [
  { key: '1h', label: '1h', fraction: 0.10 },
  { key: '24h', label: '24h', fraction: 0.25 },
  { key: '7d', label: '7d', fraction: 0.50 },
  { key: '30d', label: '30d', fraction: 0.75 },
  { key: 'All', label: 'All', fraction: 1.0 },
]

function sliceByTimeRange(events: ReviewEvent[], range: TimeRange): ReviewEvent[] {
  if (range === 'All' || events.length === 0) return events
  const config = TIME_RANGES.find(t => t.key === range)!
  const count = Math.max(1, Math.ceil(events.length * config.fraction))
  // Take the most recent N events (events are sorted by time, most recent last)
  return events.slice(-count)
}

const CATEGORIES = ['Bug', 'Security', 'Performance', 'Style', 'Documentation', 'BestPractice', 'Maintainability', 'Testing', 'Architecture'] as const

function computeAdmin(events: ReviewEvent[]) {
  const completed = events.filter(e => e.event_type === 'review.completed')
  const failed = events.filter(e => e.event_type !== 'review.completed')

  // === Usage Analytics ===
  const totalReviews = events.length
  const totalTokens = completed.reduce((s, e) => s + (e.tokens_total ?? 0), 0)
  const totalDuration = completed.reduce((s, e) => s + e.duration_ms, 0)
  const totalFiles = completed.reduce((s, e) => s + e.diff_files_reviewed, 0)
  const avgDuration = completed.length > 0 ? totalDuration / completed.length : 0
  const errorRate = totalReviews > 0 ? (failed.length / totalReviews) * 100 : 0

  // Reviews by source
  const bySource: Record<string, number> = {}
  for (const e of events) bySource[e.diff_source] = (bySource[e.diff_source] ?? 0) + 1
  const sourceData = Object.entries(bySource)
    .sort((a, b) => b[1] - a[1])
    .map(([name, value]) => ({ name, value }))

  // Reviews over time (last 20 completed, as area chart)
  const timeline = [...completed].reverse().slice(-20).map((e, i) => ({
    idx: i + 1,
    label: `#${i + 1}`,
    duration: e.duration_ms / 1000,
    tokens: e.tokens_total ?? 0,
    files: e.diff_files_reviewed,
    comments: e.comments_total,
  }))

  // === Model Breakdown ===
  const modelStats: Record<string, { count: number; tokens: number; totalScore: number; scoreCount: number; totalDuration: number; cost: number }> = {}
  for (const e of completed) {
    const m = modelStats[e.model] ?? { count: 0, tokens: 0, totalScore: 0, scoreCount: 0, totalDuration: 0, cost: 0 }
    m.count++
    m.tokens += e.tokens_total ?? 0
    m.totalDuration += e.duration_ms
    m.cost += estimateCost(e)
    if (e.overall_score != null) { m.totalScore += e.overall_score; m.scoreCount++ }
    modelStats[e.model] = m
  }
  const modelData = Object.entries(modelStats)
    .sort((a, b) => b[1].count - a[1].count)
    .map(([model, s]) => ({
      model,
      reviews: s.count,
      tokens: s.tokens,
      avgScore: s.scoreCount > 0 ? s.totalScore / s.scoreCount : 0,
      avgDuration: s.count > 0 ? s.totalDuration / s.count : 0,
      cost: s.cost,
    }))

  // === Repo/PR Activity ===
  const repoStats: Record<string, { count: number; posted: number }> = {}
  for (const e of events) {
    const repo = e.github_repo || '(local)'
    const r = repoStats[repo] ?? { count: 0, posted: 0 }
    r.count++
    if (e.github_posted) r.posted++
    repoStats[repo] = r
  }
  const repoData = Object.entries(repoStats)
    .sort((a, b) => b[1].count - a[1].count)
    .map(([repo, s]) => ({ repo, reviews: s.count, posted: s.posted }))

  // === Error & Health ===
  const failedEvents = failed.map(e => ({
    id: e.review_id.slice(0, 8),
    source: e.title || e.diff_source,
    error: e.error || 'Unknown error',
    model: e.model,
  }))

  // Latency percentiles
  const durations = completed.map(e => e.duration_ms).sort((a, b) => a - b)
  const p50 = durations.length > 0 ? durations[Math.floor(durations.length * 0.5)] : 0
  const p95 = durations.length > 0 ? durations[Math.floor(durations.length * 0.95)] : 0
  const p99 = durations.length > 0 ? durations[Math.floor(durations.length * 0.99)] : 0

  // Slow files (from file_metrics)
  const fileLatencies: Record<string, number[]> = {}
  for (const e of completed) {
    for (const f of e.file_metrics ?? []) {
      ;(fileLatencies[f.file_path] ??= []).push(f.latency_ms)
    }
  }
  const slowFiles = Object.entries(fileLatencies)
    .map(([path, lats]) => ({
      path,
      avgLatency: lats.reduce((s, l) => s + l, 0) / lats.length,
      maxLatency: Math.max(...lats),
      occurrences: lats.length,
    }))
    .sort((a, b) => b.avgLatency - a.avgLatency)
    .slice(0, 10)

  // === Token Cost Dashboard ===
  const estTotalCost = totalCost(completed)
  const costTimeline = completed.map((e, i) => {
    const runningSum = completed.slice(0, i + 1).reduce((s, ev) => s + estimateCost(ev), 0)
    return {
      idx: i + 1,
      label: `#${i + 1}`,
      cost: runningSum,
      eventCost: estimateCost(e),
    }
  })
  const projectedMonthlyBurn = completed.length > 3
    ? (estTotalCost / completed.length) * 30
    : null

  // === Severity Trends ===
  const severityOverTime = completed.map((e, i) => ({
    idx: i + 1,
    label: `#${i + 1}`,
    Error: e.comments_by_severity['Error'] || 0,
    Warning: e.comments_by_severity['Warning'] || 0,
    Info: e.comments_by_severity['Info'] || 0,
    Suggestion: e.comments_by_severity['Suggestion'] || 0,
  }))

  // === Category Heatmap ===
  // Build category vs repo matrix
  const categoryRepoMatrix: Record<string, Record<string, number>> = {}
  const allRepos = new Set<string>()
  for (const e of completed) {
    const repo = e.github_repo || '(local)'
    allRepos.add(repo)
    for (const [cat, count] of Object.entries(e.comments_by_category)) {
      if (!categoryRepoMatrix[cat]) categoryRepoMatrix[cat] = {}
      categoryRepoMatrix[cat][repo] = (categoryRepoMatrix[cat][repo] ?? 0) + count
    }
  }
  const heatmapRepos = Array.from(allRepos).sort()
  const heatmapCategories = CATEGORIES.filter(c => categoryRepoMatrix[c])
  // Find max for color intensity scaling
  let heatmapMax = 0
  for (const cat of heatmapCategories) {
    for (const repo of heatmapRepos) {
      const val = categoryRepoMatrix[cat]?.[repo] ?? 0
      if (val > heatmapMax) heatmapMax = val
    }
  }

  // === Comparison Mode Data ===
  const halfIdx = Math.floor(completed.length / 2)
  const firstHalf = completed.slice(0, halfIdx)
  const secondHalf = completed.slice(halfIdx)

  function computePeriodStats(evts: ReviewEvent[]) {
    const dur = evts.reduce((s, e) => s + e.duration_ms, 0)
    const tok = evts.reduce((s, e) => s + (e.tokens_total ?? 0), 0)
    const scores = evts.filter(e => e.overall_score != null)
    const avgScore = scores.length > 0 ? scores.reduce((s, e) => s + e.overall_score!, 0) / scores.length : 0
    const avgDur = evts.length > 0 ? dur / evts.length : 0
    const errs = evts.filter(e => e.event_type !== 'review.completed').length
    const errRate = evts.length > 0 ? (errs / evts.length) * 100 : 0
    return { avgDuration: avgDur, totalTokens: tok, avgScore, errorRate: errRate, count: evts.length }
  }

  const comparison = {
    first: computePeriodStats(firstHalf),
    second: computePeriodStats(secondHalf),
  }

  return {
    totalReviews, totalTokens, totalDuration, totalFiles, avgDuration, errorRate,
    sourceData, timeline, modelData, repoData, failedEvents,
    p50, p95, p99, slowFiles,
    completedCount: completed.length, failedCount: failed.length,
    estTotalCost, costTimeline, projectedMonthlyBurn,
    severityOverTime,
    categoryRepoMatrix, heatmapRepos, heatmapCategories, heatmapMax,
    comparison,
  }
}

export function Admin() {
  const { data: events, isLoading } = useEvents()
  const [timeRange, setTimeRange] = useState<TimeRange>('All')
  const [compareMode, setCompareMode] = useState(false)

  const allEvents = useMemo(() => events ?? [], [events])
  const filteredEvents = useMemo(() => sliceByTimeRange(allEvents, timeRange), [allEvents, timeRange])
  const admin = useMemo(() => computeAdmin(filteredEvents), [filteredEvents])

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="animate-spin text-accent" size={32} />
      </div>
    )
  }

  if (admin.totalReviews === 0 && allEvents.length === 0) {
    return (
      <div className="p-6 max-w-6xl mx-auto">
        <h1 className="text-xl font-semibold text-text-primary mb-6">System Admin</h1>
        <div className="bg-surface-1 border border-border rounded-lg p-12 text-center text-text-muted text-sm">
          No review data yet. Complete some reviews to see system analytics.
        </div>
      </div>
    )
  }

  return (
    <div className="p-6 max-w-6xl mx-auto">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-xl font-semibold text-text-primary">System Admin</h1>

        {/* Time Range Selector */}
        <div className="flex items-center gap-1 bg-surface-1 border border-border rounded-lg p-0.5">
          {TIME_RANGES.map(t => (
            <button
              key={t.key}
              onClick={() => setTimeRange(t.key)}
              className={`px-3 py-1 text-[11px] font-code font-semibold rounded-md transition-colors ${
                timeRange === t.key
                  ? 'bg-accent text-surface-0'
                  : 'text-text-muted hover:text-text-primary hover:bg-surface-2'
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>
      </div>

      {admin.totalReviews === 0 && (
        <div className="bg-surface-1 border border-border rounded-lg p-12 text-center text-text-muted text-sm mb-8">
          No review data in this time range. Try selecting a wider range.
        </div>
      )}

      {admin.totalReviews > 0 && (
        <>
          {/* === Usage Analytics Section === */}
          <SectionHeader title="Usage Analytics" />
          <div className="grid grid-cols-2 md:grid-cols-7 gap-3 mb-4">
            {[
              { label: 'TOTAL REVIEWS', value: String(admin.totalReviews) },
              { label: 'COMPLETED', value: String(admin.completedCount), valueColor: 'text-sev-suggestion' },
              { label: 'FAILED', value: String(admin.failedCount), valueColor: admin.failedCount > 0 ? 'text-sev-error' : 'text-text-primary' },
              { label: 'TOTAL TOKENS', value: fmtTokens(admin.totalTokens) },
              { label: 'AVG DURATION', value: formatDuration(admin.avgDuration) },
              { label: 'FILES REVIEWED', value: String(admin.totalFiles) },
              { label: 'EST. TOTAL COST', value: formatCost(admin.estTotalCost), valueColor: 'text-accent' },
            ].map(c => (
              <div key={c.label} className="bg-surface-1 border border-border rounded-lg p-3">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">{c.label}</div>
                <div className={`text-lg font-bold font-code mt-1 ${c.valueColor || 'text-text-primary'}`}>{c.value}</div>
              </div>
            ))}
          </div>

          {/* Projected monthly burn rate */}
          {admin.projectedMonthlyBurn !== null && (
            <div className="bg-surface-1 border border-accent/20 rounded-lg p-3 mb-4 flex items-center gap-3">
              <DollarSign size={14} className="text-accent" />
              <div className="text-[11px] text-text-secondary font-code">
                <span className="text-text-muted">Projected monthly burn rate:</span>{' '}
                <span className="text-accent font-bold">{formatCost(admin.projectedMonthlyBurn)}/mo</span>
                <span className="text-text-muted ml-2">(based on {admin.completedCount} reviews)</span>
              </div>
            </div>
          )}

          <div className="grid grid-cols-1 md:grid-cols-3 gap-3 mb-8">
            {/* Reviews over time */}
            {admin.timeline.length > 1 && (
              <div className="bg-surface-1 border border-border rounded-lg p-4 md:col-span-2">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">ACTIVITY OVER TIME</div>
                <div className="h-32">
                  <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                    <AreaChart data={admin.timeline}>
                      <defs>
                        <linearGradient id="actGrad" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor={CHART_THEME.accent} stopOpacity={0.3} />
                          <stop offset="95%" stopColor={CHART_THEME.accent} stopOpacity={0.02} />
                        </linearGradient>
                      </defs>
                      <CartesianGrid {...gridProps} />
                      <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                      <YAxis tick={axisTick} axisLine={false} tickLine={false} />
                      <Tooltip {...tooltipStyle} />
                      <Area type="monotone" dataKey="comments" stroke={CHART_THEME.accent} fill="url(#actGrad)" strokeWidth={1.5} dot={false} name="Findings" />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
              </div>
            )}

            {/* Reviews by source pie */}
            {admin.sourceData.length > 0 && (
              <div className="bg-surface-1 border border-border rounded-lg p-4">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">REVIEWS BY SOURCE</div>
                <div className="h-32">
                  <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                    <PieChart>
                      <Pie data={admin.sourceData} dataKey="value" nameKey="name" cx="50%" cy="50%" outerRadius={50} innerRadius={25} strokeWidth={0}>
                        {admin.sourceData.map((_, i) => (
                          <Cell key={i} fill={SOURCE_COLORS[i % SOURCE_COLORS.length]} />
                        ))}
                      </Pie>
                      <Tooltip {...tooltipStyle} />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
                <div className="flex flex-wrap gap-x-3 gap-y-1 mt-2">
                  {admin.sourceData.map((d, i) => (
                    <div key={d.name} className="flex items-center gap-1.5 text-[10px] text-text-secondary">
                      <span className="w-2 h-2 rounded-full" style={{ backgroundColor: SOURCE_COLORS[i % SOURCE_COLORS.length] }} />
                      {d.name} ({d.value})
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>

          {/* === Comparison Mode === */}
          <div className="flex items-center gap-3 mb-4">
            <button
              onClick={() => setCompareMode(!compareMode)}
              className="flex items-center gap-2 text-[11px] font-code font-semibold text-text-muted hover:text-text-primary transition-colors"
            >
              {compareMode ? <ToggleRight size={16} className="text-accent" /> : <ToggleLeft size={16} />}
              Compare periods
            </button>
          </div>

          {compareMode && admin.completedCount >= 4 && (
            <>
              <SectionHeader title="Period Comparison (First Half vs Second Half)" />
              <div className="grid grid-cols-1 md:grid-cols-4 gap-3 mb-8">
                <ComparisonCard
                  label="AVG DURATION"
                  first={formatDuration(admin.comparison.first.avgDuration)}
                  second={formatDuration(admin.comparison.second.avgDuration)}
                  firstRaw={admin.comparison.first.avgDuration}
                  secondRaw={admin.comparison.second.avgDuration}
                  lowerIsBetter
                />
                <ComparisonCard
                  label="TOTAL TOKENS"
                  first={fmtTokens(admin.comparison.first.totalTokens)}
                  second={fmtTokens(admin.comparison.second.totalTokens)}
                  firstRaw={admin.comparison.first.totalTokens}
                  secondRaw={admin.comparison.second.totalTokens}
                  lowerIsBetter
                />
                <ComparisonCard
                  label="AVG SCORE"
                  first={admin.comparison.first.avgScore.toFixed(1)}
                  second={admin.comparison.second.avgScore.toFixed(1)}
                  firstRaw={admin.comparison.first.avgScore}
                  secondRaw={admin.comparison.second.avgScore}
                  lowerIsBetter={false}
                />
                <ComparisonCard
                  label="ERROR RATE"
                  first={`${admin.comparison.first.errorRate.toFixed(1)}%`}
                  second={`${admin.comparison.second.errorRate.toFixed(1)}%`}
                  firstRaw={admin.comparison.first.errorRate}
                  secondRaw={admin.comparison.second.errorRate}
                  lowerIsBetter
                />
              </div>
            </>
          )}

          {compareMode && admin.completedCount < 4 && (
            <div className="bg-surface-1 border border-border rounded-lg p-6 text-center text-text-muted text-[12px] mb-8">
              Need at least 4 completed reviews to compare periods.
            </div>
          )}

          {/* === Severity Trends Section === */}
          {admin.severityOverTime.length > 1 && (
            <>
              <SectionHeader title="Severity Trends" />
              <div className="bg-surface-1 border border-border rounded-lg p-4 mb-8">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">SEVERITY BREAKDOWN OVER TIME</div>
                <div className="h-44">
                  <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                    <AreaChart data={admin.severityOverTime}>
                      <defs>
                        {Object.entries(SEV_COLORS).map(([key, color]) => (
                          <linearGradient key={key} id={`adminSev${key}`} x1="0" y1="0" x2="0" y2="1">
                            <stop offset="5%" stopColor={color} stopOpacity={0.4} />
                            <stop offset="95%" stopColor={color} stopOpacity={0.05} />
                          </linearGradient>
                        ))}
                      </defs>
                      <CartesianGrid {...gridProps} />
                      <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                      <YAxis tick={axisTick} axisLine={false} tickLine={false} />
                      <Tooltip {...tooltipStyle} />
                      <Area type="monotone" dataKey="Error" stackId="1" stroke={SEV_COLORS.Error} fill="url(#adminSevError)" strokeWidth={1} name="Error" />
                      <Area type="monotone" dataKey="Warning" stackId="1" stroke={SEV_COLORS.Warning} fill="url(#adminSevWarning)" strokeWidth={1} name="Warning" />
                      <Area type="monotone" dataKey="Info" stackId="1" stroke={SEV_COLORS.Info} fill="url(#adminSevInfo)" strokeWidth={1} name="Info" />
                      <Area type="monotone" dataKey="Suggestion" stackId="1" stroke={SEV_COLORS.Suggestion} fill="url(#adminSevSuggestion)" strokeWidth={1} name="Suggestion" />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
                <div className="flex items-center gap-4 mt-3">
                  {(['Error', 'Warning', 'Info', 'Suggestion'] as const).map(sev => (
                    <div key={sev} className="flex items-center gap-1.5 text-[10px] text-text-secondary">
                      <span className="w-2 h-2 rounded-full" style={{ backgroundColor: SEV_COLORS[sev] }} />
                      {sev}
                    </div>
                  ))}
                </div>
              </div>
            </>
          )}

          {/* === Token Cost Dashboard Section === */}
          {admin.costTimeline.length > 1 && (
            <>
              <SectionHeader title="Token Cost Dashboard" />
              <div className="bg-surface-1 border border-border rounded-lg p-4 mb-8">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">CUMULATIVE COST OVER TIME</div>
                <div className="h-40">
                  <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                    <AreaChart data={admin.costTimeline}>
                      <defs>
                        <linearGradient id="costGrad" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor={CHART_THEME.accent} stopOpacity={0.3} />
                          <stop offset="95%" stopColor={CHART_THEME.accent} stopOpacity={0.02} />
                        </linearGradient>
                      </defs>
                      <CartesianGrid {...gridProps} />
                      <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                      <YAxis
                        tick={axisTick}
                        axisLine={false}
                        tickLine={false}
                        tickFormatter={(v: number) => formatCost(v)}
                      />
                      <Tooltip
                        {...tooltipStyle}
                        formatter={(value) => [formatCost(Number(value)), 'Cumulative Cost']}
                      />
                      <Area type="monotone" dataKey="cost" stroke={CHART_THEME.accent} fill="url(#costGrad)" strokeWidth={1.5} dot={false} name="Cumulative Cost" />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
              </div>
            </>
          )}

          {/* === Model Breakdown Section === */}
          <SectionHeader title="Model Breakdown" />
          {admin.modelData.length > 0 && (
            <div className="bg-surface-1 border border-border rounded-lg overflow-hidden mb-8">
              <table className="w-full text-[12px]">
                <thead>
                  <tr className="border-b border-border">
                    {['MODEL', 'REVIEWS', 'TOKENS', 'AVG SCORE', 'AVG DURATION', 'EST. COST'].map(h => (
                      <th key={h} className="text-left px-4 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px]">{h}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {admin.modelData.map(m => (
                    <tr key={m.model} className="border-b border-border-subtle">
                      <td className="px-4 py-2.5 font-code text-text-primary font-medium">{m.model}</td>
                      <td className="px-4 py-2.5 font-code text-text-secondary">{m.reviews}</td>
                      <td className="px-4 py-2.5 font-code text-text-secondary">{fmtTokens(m.tokens)}</td>
                      <td className="px-4 py-2.5">
                        <span className={`font-code font-bold ${m.avgScore >= 7 ? 'text-sev-suggestion' : m.avgScore >= 4 ? 'text-sev-warning' : 'text-sev-error'}`}>
                          {m.avgScore.toFixed(1)}
                        </span>
                      </td>
                      <td className="px-4 py-2.5 font-code text-text-secondary">{formatDuration(m.avgDuration)}</td>
                      <td className="px-4 py-2.5 font-code text-accent">{formatCost(m.cost)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {/* === Category Heatmap Section === */}
          {admin.heatmapCategories.length > 0 && admin.heatmapRepos.length > 0 && (
            <>
              <SectionHeader title="Category Heatmap" />
              <div className="bg-surface-1 border border-border rounded-lg overflow-hidden mb-8">
                <div className="overflow-x-auto">
                  <table className="w-full text-[11px]">
                    <thead>
                      <tr className="border-b border-border">
                        <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px] sticky left-0 bg-surface-1">CATEGORY</th>
                        {admin.heatmapRepos.map(repo => (
                          <th key={repo} className="text-center px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px] whitespace-nowrap">
                            {repo.length > 20 ? `...${repo.slice(-18)}` : repo}
                          </th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {admin.heatmapCategories.map(cat => (
                        <tr key={cat} className="border-b border-border-subtle">
                          <td className="px-3 py-2 font-code text-text-primary font-medium sticky left-0 bg-surface-1">{cat}</td>
                          {admin.heatmapRepos.map(repo => {
                            const val = admin.categoryRepoMatrix[cat]?.[repo] ?? 0
                            const intensity = admin.heatmapMax > 0 ? val / admin.heatmapMax : 0
                            return (
                              <td key={repo} className="px-3 py-2 text-center font-code">
                                {val > 0 ? (
                                  <span
                                    className="inline-block rounded px-2 py-0.5 font-semibold text-[10px]"
                                    style={{
                                      backgroundColor: `rgba(74, 222, 128, ${Math.max(0.08, intensity * 0.5)})`,
                                      color: intensity > 0.5 ? '#1a2117' : '#9ca898',
                                    }}
                                  >
                                    {val}
                                  </span>
                                ) : (
                                  <span className="text-text-muted/40">{'\u2014'}</span>
                                )}
                              </td>
                            )
                          })}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            </>
          )}

          {/* === Repo/PR Activity Section === */}
          <SectionHeader title="Repository Activity" />
          {admin.repoData.length > 0 ? (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3 mb-8">
              <div className="bg-surface-1 border border-border rounded-lg p-4">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">REVIEWS BY REPOSITORY</div>
                <div className="h-36">
                  <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                    <BarChart data={admin.repoData} layout="vertical">
                      <CartesianGrid {...gridProps} horizontal={false} />
                      <XAxis type="number" tick={axisTick} axisLine={false} tickLine={false} />
                      <YAxis type="category" dataKey="repo" tick={{ fontSize: 10, fill: CHART_THEME.tooltipText }} axisLine={false} tickLine={false} width={120} />
                      <Tooltip {...tooltipStyle} />
                      <Bar dataKey="reviews" fill={CHART_THEME.accent} radius={[0, 4, 4, 0]} barSize={14} name="Reviews" />
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              </div>

              <div className="bg-surface-1 border border-border rounded-lg overflow-hidden">
                <table className="w-full text-[12px]">
                  <thead>
                    <tr className="border-b border-border">
                      {['REPOSITORY', 'REVIEWS', 'POSTED TO PR'].map(h => (
                        <th key={h} className="text-left px-4 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px]">{h}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {admin.repoData.map(r => (
                      <tr key={r.repo} className="border-b border-border-subtle">
                        <td className="px-4 py-2.5 font-code text-text-primary">{r.repo}</td>
                        <td className="px-4 py-2.5 font-code text-text-secondary">{r.reviews}</td>
                        <td className="px-4 py-2.5 font-code text-text-secondary">
                          {r.posted > 0 ? <span className="text-accent">{r.posted}</span> : '\u2014'}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          ) : (
            <div className="bg-surface-1 border border-border rounded-lg p-6 text-center text-text-muted text-[12px] mb-8">
              No repository data. Connect GitHub and review PRs to see repository activity.
            </div>
          )}

          {/* === Error & Health Section === */}
          <SectionHeader title="Error & Health Monitoring" />
          <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-4">
            {[
              { label: 'ERROR RATE', value: `${admin.errorRate.toFixed(1)}%`, valueColor: admin.errorRate > 10 ? 'text-sev-error' : admin.errorRate > 0 ? 'text-sev-warning' : 'text-sev-suggestion' },
              { label: 'P50 LATENCY', value: formatDuration(admin.p50) },
              { label: 'P95 LATENCY', value: formatDuration(admin.p95), valueColor: admin.p95 > 60_000 ? 'text-sev-warning' : undefined },
              { label: 'P99 LATENCY', value: formatDuration(admin.p99), valueColor: admin.p99 > 120_000 ? 'text-sev-error' : undefined },
            ].map(c => (
              <div key={c.label} className="bg-surface-1 border border-border rounded-lg p-3">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">{c.label}</div>
                <div className={`text-lg font-bold font-code mt-1 ${c.valueColor || 'text-text-primary'}`}>{c.value}</div>
              </div>
            ))}
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-3 mb-8">
            {/* Failed reviews */}
            {admin.failedEvents.length > 0 && (
              <div className="bg-surface-1 border border-border rounded-lg p-4">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3 flex items-center gap-1.5">
                  <AlertTriangle size={12} className="text-sev-error" />
                  FAILED REVIEWS ({admin.failedEvents.length})
                </div>
                <div className="max-h-48 overflow-auto space-y-2">
                  {admin.failedEvents.map(f => (
                    <div key={f.id} className="bg-sev-error/5 border border-sev-error/20 rounded px-3 py-2 text-[11px]">
                      <div className="flex items-center gap-2">
                        <span className="font-code text-text-muted">{f.id}</span>
                        <span className="text-text-primary">{f.source}</span>
                        <span className="text-text-muted ml-auto font-code text-[10px]">{f.model}</span>
                      </div>
                      <div className="text-sev-error font-code mt-0.5 truncate">{f.error}</div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Slow files */}
            {admin.slowFiles.length > 0 && (
              <div className="bg-surface-1 border border-border rounded-lg p-4">
                <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">SLOWEST FILES (avg latency)</div>
                <div className="max-h-48 overflow-auto">
                  <table className="w-full text-[11px]">
                    <thead>
                      <tr className="text-text-muted text-left">
                        <th className="font-medium pr-3 py-0.5">File</th>
                        <th className="font-medium pr-3 py-0.5 text-right">Avg</th>
                        <th className="font-medium pr-3 py-0.5 text-right">Max</th>
                        <th className="font-medium py-0.5 text-right">Seen</th>
                      </tr>
                    </thead>
                    <tbody>
                      {admin.slowFiles.map(f => (
                        <tr key={f.path} className="text-text-secondary">
                          <td className="font-code text-text-primary truncate max-w-40 pr-3 py-0.5" title={f.path}>{f.path.split('/').pop()}</td>
                          <td className="font-code text-right pr-3 py-0.5">{formatDuration(f.avgLatency)}</td>
                          <td className="font-code text-right pr-3 py-0.5">{formatDuration(f.maxLatency)}</td>
                          <td className="font-code text-right py-0.5">{f.occurrences}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
          </div>
        </>
      )}
    </div>
  )
}

// === Comparison Card Component ===
function ComparisonCard({
  label,
  first,
  second,
  firstRaw,
  secondRaw,
  lowerIsBetter,
}: {
  label: string
  first: string
  second: string
  firstRaw: number
  secondRaw: number
  lowerIsBetter: boolean
}) {
  const delta = secondRaw - firstRaw
  const pctChange = firstRaw !== 0 ? ((delta / firstRaw) * 100) : 0
  const improved = lowerIsBetter ? delta < 0 : delta > 0
  const unchanged = Math.abs(pctChange) < 0.5

  return (
    <div className="bg-surface-1 border border-border rounded-lg p-3">
      <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-2">{label}</div>
      <div className="grid grid-cols-2 gap-2">
        <div>
          <div className="text-[9px] text-text-muted font-code mb-0.5">FIRST HALF</div>
          <div className="text-sm font-bold font-code text-text-secondary">{first}</div>
        </div>
        <div>
          <div className="text-[9px] text-text-muted font-code mb-0.5">SECOND HALF</div>
          <div className="text-sm font-bold font-code text-text-primary">{second}</div>
        </div>
      </div>
      {!unchanged && (
        <div className={`flex items-center gap-1 mt-2 text-[10px] font-code font-semibold ${improved ? 'text-sev-suggestion' : 'text-sev-error'}`}>
          {improved ? <ArrowDownRight size={12} /> : <ArrowUpRight size={12} />}
          {Math.abs(pctChange).toFixed(1)}% {improved ? 'improvement' : 'regression'}
        </div>
      )}
      {unchanged && (
        <div className="flex items-center gap-1 mt-2 text-[10px] font-code text-text-muted">
          No significant change
        </div>
      )}
    </div>
  )
}

function SectionHeader({ title }: { title: string }) {
  return (
    <div className="flex items-center gap-2 mb-3">
      <h2 className="text-sm font-semibold text-text-primary">{title}</h2>
      <div className="flex-1 h-px bg-border" />
    </div>
  )
}
