import { useMemo } from 'react'
import {
  AreaChart, Area, BarChart, Bar, PieChart, Pie, Cell,
  ResponsiveContainer, XAxis, YAxis, Tooltip, CartesianGrid,
} from 'recharts'
import { useEvents } from '../api/hooks'
import { Loader2, AlertTriangle } from 'lucide-react'
import { CHART_THEME } from '../lib/constants'
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
  const modelStats: Record<string, { count: number; tokens: number; totalScore: number; scoreCount: number; totalDuration: number }> = {}
  for (const e of completed) {
    const m = modelStats[e.model] ?? { count: 0, tokens: 0, totalScore: 0, scoreCount: 0, totalDuration: 0 }
    m.count++
    m.tokens += e.tokens_total ?? 0
    m.totalDuration += e.duration_ms
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

  return {
    totalReviews, totalTokens, totalDuration, totalFiles, avgDuration, errorRate,
    sourceData, timeline, modelData, repoData, failedEvents,
    p50, p95, p99, slowFiles,
    completedCount: completed.length, failedCount: failed.length,
  }
}

export function Admin() {
  const { data: events, isLoading } = useEvents()
  const allEvents = events ?? []

  const admin = useMemo(() => computeAdmin(allEvents), [allEvents])

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="animate-spin text-accent" size={32} />
      </div>
    )
  }

  if (admin.totalReviews === 0) {
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
      <h1 className="text-xl font-semibold text-text-primary mb-6">System Admin</h1>

      {/* === Usage Analytics Section === */}
      <SectionHeader title="Usage Analytics" />
      <div className="grid grid-cols-2 md:grid-cols-6 gap-3 mb-4">
        {[
          { label: 'TOTAL REVIEWS', value: String(admin.totalReviews) },
          { label: 'COMPLETED', value: String(admin.completedCount), valueColor: 'text-sev-suggestion' },
          { label: 'FAILED', value: String(admin.failedCount), valueColor: admin.failedCount > 0 ? 'text-sev-error' : 'text-text-primary' },
          { label: 'TOTAL TOKENS', value: fmtTokens(admin.totalTokens) },
          { label: 'AVG DURATION', value: formatDuration(admin.avgDuration) },
          { label: 'FILES REVIEWED', value: String(admin.totalFiles) },
        ].map(c => (
          <div key={c.label} className="bg-surface-1 border border-border rounded-lg p-3">
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">{c.label}</div>
            <div className={`text-lg font-bold font-code mt-1 ${c.valueColor || 'text-text-primary'}`}>{c.value}</div>
          </div>
        ))}
      </div>

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

      {/* === Model Breakdown Section === */}
      <SectionHeader title="Model Breakdown" />
      {admin.modelData.length > 0 && (
        <div className="bg-surface-1 border border-border rounded-lg overflow-hidden mb-8">
          <table className="w-full text-[12px]">
            <thead>
              <tr className="border-b border-border">
                {['MODEL', 'REVIEWS', 'TOKENS', 'AVG SCORE', 'AVG DURATION'].map(h => (
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
                </tr>
              ))}
            </tbody>
          </table>
        </div>
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
