import { useState, useMemo, useRef, useEffect, useCallback } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  AreaChart, Area, BarChart, Bar,
  ResponsiveContainer, XAxis, YAxis, Tooltip, CartesianGrid,
} from 'recharts'
import { useEvents } from '../api/hooks'
import {
  Loader2, Search, ChevronDown, ChevronLeft, ChevronRight,
  Download, Radio, GitCompareArrows, ExternalLink,
} from 'lucide-react'
import { CHART_THEME, PAGE_SIZE } from '../lib/constants'
import { estimateCost, formatCost, totalCost } from '../lib/cost'
import type { ReviewEvent, FileMetricEvent, HotspotDetail } from '../api/types'

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

type SortField = 'duration' | 'tokens' | 'score' | 'files' | 'comments' | 'cost'
type SortDir = 'asc' | 'desc'

function computeStats(events: ReviewEvent[]) {
  const completed = events.filter(e => e.event_type === 'review.completed')
  const totalReviews = completed.length
  const avgDuration = totalReviews > 0
    ? completed.reduce((s, e) => s + e.duration_ms, 0) / totalReviews : 0
  const totalTokens = completed.reduce((s, e) => s + (e.tokens_total ?? 0), 0)
  const avgScore = totalReviews > 0
    ? completed.filter(e => e.overall_score != null).reduce((s, e) => s + e.overall_score!, 0)
      / (completed.filter(e => e.overall_score != null).length || 1) : 0
  const failedCount = events.filter(e => e.event_type === 'review.failed').length

  // Timeline data (most recent 30)
  const timeline = [...completed].reverse().slice(-30).map((e, i) => ({
    idx: i + 1,
    label: `#${i + 1}`,
    duration: e.duration_ms / 1000,
    tokens: e.tokens_total ?? 0,
    score: e.overall_score ?? 0,
  }))

  return { totalReviews, avgDuration, totalTokens, avgScore, failedCount, timeline }
}

/* ---- Export helpers ---- */
function downloadBlob(blob: Blob, filename: string) {
  const url = window.URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  window.URL.revokeObjectURL(url)
}

function exportCSV(events: ReviewEvent[]) {
  const cols = [
    'review_id', 'source', 'model', 'duration_ms', 'tokens_total', 'tokens_prompt',
    'tokens_completion', 'files_reviewed', 'comments_total', 'overall_score',
    'event_type', 'cost_usd', 'github_repo', 'github_pr',
  ] as const
  const header = cols.join(',')
  const rows = events.map(e => [
    e.review_id,
    e.diff_source,
    e.model,
    e.duration_ms,
    e.tokens_total ?? '',
    e.tokens_prompt ?? '',
    e.tokens_completion ?? '',
    e.diff_files_reviewed,
    e.comments_total,
    e.overall_score ?? '',
    e.event_type,
    estimateCost(e).toFixed(6),
    e.github_repo ?? '',
    e.github_pr ?? '',
  ].map(v => `"${String(v).replace(/"/g, '""')}"`).join(','))
  const csv = [header, ...rows].join('\n')
  downloadBlob(new Blob([csv], { type: 'text/csv' }), 'diffscope-events.csv')
}

function exportJSON(events: ReviewEvent[]) {
  const json = JSON.stringify(events, null, 2)
  downloadBlob(new Blob([json], { type: 'application/json' }), 'diffscope-events.json')
}

/* ---- Comparison Panel ---- */
function ComparisonPanel({ a, b }: { a: ReviewEvent; b: ReviewEvent }) {
  const metrics: { label: string; valA: string; valB: string; rawA: number; rawB: number; lowerBetter?: boolean }[] = [
    { label: 'Duration', valA: formatDuration(a.duration_ms), valB: formatDuration(b.duration_ms), rawA: a.duration_ms, rawB: b.duration_ms, lowerBetter: true },
    { label: 'Tokens', valA: fmtTokens(a.tokens_total ?? 0), valB: fmtTokens(b.tokens_total ?? 0), rawA: a.tokens_total ?? 0, rawB: b.tokens_total ?? 0, lowerBetter: true },
    { label: 'Score', valA: (a.overall_score ?? 0).toFixed(1), valB: (b.overall_score ?? 0).toFixed(1), rawA: a.overall_score ?? 0, rawB: b.overall_score ?? 0 },
    { label: 'Files', valA: String(a.diff_files_reviewed), valB: String(b.diff_files_reviewed), rawA: a.diff_files_reviewed, rawB: b.diff_files_reviewed },
    { label: 'Comments', valA: String(a.comments_total), valB: String(b.comments_total), rawA: a.comments_total, rawB: b.comments_total, lowerBetter: true },
    { label: 'Cost', valA: formatCost(estimateCost(a)), valB: formatCost(estimateCost(b)), rawA: estimateCost(a), rawB: estimateCost(b), lowerBetter: true },
  ]

  const deltaColor = (rawA: number, rawB: number, lowerBetter?: boolean) => {
    if (rawA === rawB) return 'text-text-muted'
    const aIsBetter = lowerBetter ? rawA < rawB : rawA > rawB
    return aIsBetter ? 'text-sev-suggestion' : 'text-sev-error'
  }

  // Collect per-file paths from both events
  const filesA = a.file_metrics ?? []
  const filesB = b.file_metrics ?? []
  const allPaths = [...new Set([...filesA.map(f => f.file_path), ...filesB.map(f => f.file_path)])]

  return (
    <div className="bg-surface-1 border border-border rounded-lg p-4 mt-4">
      <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3 uppercase">Comparison</div>

      {/* Header row */}
      <div className="grid grid-cols-4 gap-2 text-[10px] font-code text-text-muted uppercase tracking-wider mb-1 px-1">
        <div>Metric</div>
        <div className="text-right">{a.review_id.slice(0, 8)}</div>
        <div className="text-right">{b.review_id.slice(0, 8)}</div>
        <div className="text-right">Delta</div>
      </div>

      <div className="space-y-0.5">
        {metrics.map(m => {
          const diff = m.rawA - m.rawB
          const diffStr = diff === 0 ? '--' : (diff > 0 ? '+' : '') + (m.label === 'Cost' ? formatCost(Math.abs(diff)) : m.label === 'Duration' ? formatDuration(Math.abs(diff)) : m.label === 'Tokens' ? fmtTokens(Math.abs(diff)) : String(Math.abs(diff)))
          return (
            <div key={m.label} className="grid grid-cols-4 gap-2 text-[11px] px-1 py-0.5 rounded hover:bg-surface-2">
              <div className="text-text-secondary">{m.label}</div>
              <div className="text-right font-code text-text-primary">{m.valA}</div>
              <div className="text-right font-code text-text-primary">{m.valB}</div>
              <div className={`text-right font-code font-medium ${deltaColor(m.rawA, m.rawB, m.lowerBetter)}`}>{diffStr}</div>
            </div>
          )
        })}
      </div>

      {/* Per-file comparison if both have file metrics */}
      {allPaths.length > 0 && (
        <div className="mt-3 pt-3 border-t border-border/50">
          <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-2 uppercase">Per-File Metrics</div>
          <table className="w-full text-[11px]">
            <thead>
              <tr className="text-text-muted text-left text-[10px] uppercase tracking-wider">
                <th className="font-medium pr-3 py-0.5">File</th>
                <th className="font-medium pr-3 py-0.5 text-right">{a.review_id.slice(0, 8)} Latency</th>
                <th className="font-medium pr-3 py-0.5 text-right">{b.review_id.slice(0, 8)} Latency</th>
                <th className="font-medium pr-3 py-0.5 text-right">{a.review_id.slice(0, 8)} Tokens</th>
                <th className="font-medium py-0.5 text-right">{b.review_id.slice(0, 8)} Tokens</th>
              </tr>
            </thead>
            <tbody>
              {allPaths.map(path => {
                const fa = filesA.find(f => f.file_path === path)
                const fb = filesB.find(f => f.file_path === path)
                return (
                  <tr key={path} className="text-text-secondary">
                    <td className="font-code text-text-primary truncate max-w-48 pr-3 py-0.5" title={path}>
                      {path.split('/').pop()}
                    </td>
                    <td className="font-code text-right pr-3 py-0.5">{fa ? formatDuration(fa.latency_ms) : '\u2014'}</td>
                    <td className="font-code text-right pr-3 py-0.5">{fb ? formatDuration(fb.latency_ms) : '\u2014'}</td>
                    <td className="font-code text-right pr-3 py-0.5">{fa ? fmtTokens(fa.total_tokens) : '\u2014'}</td>
                    <td className="font-code text-right py-0.5">{fb ? fmtTokens(fb.total_tokens) : '\u2014'}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}

/* ---- Event Row ---- */
function EventRow({
  event, expanded, onToggle, compareMode, compareSelected, onCompareToggle, isNew,
}: {
  event: ReviewEvent
  expanded: boolean
  onToggle: () => void
  compareMode: boolean
  compareSelected: boolean
  onCompareToggle: (id: string) => void
  isNew: boolean
}) {
  const navigate = useNavigate()
  const isCompleted = event.event_type === 'review.completed'
  const statusColor = isCompleted ? 'text-badge-completed' : 'text-badge-failed'
  const statusLabel = isCompleted ? 'OK' : 'FAIL'

  return (
    <>
      <tr
        onClick={onToggle}
        className={`border-b border-border-subtle hover:bg-surface-2 cursor-pointer transition-colors ${isNew ? 'animate-new-row' : ''}`}
      >
        {compareMode && (
          <td className="px-2 py-2">
            <input
              type="checkbox"
              checked={compareSelected}
              onChange={() => onCompareToggle(event.review_id)}
              onClick={e => e.stopPropagation()}
              className="accent-accent w-3 h-3"
            />
          </td>
        )}
        <td className="px-3 py-2 font-code text-accent text-[11px]">
          <button
            onClick={e => { e.stopPropagation(); navigate(`/review/${event.review_id}`) }}
            className="hover:underline underline-offset-2"
          >
            {event.review_id.slice(0, 8)}
          </button>
        </td>
        <td className="px-3 py-2 text-text-primary text-[12px]">
          {event.title || event.diff_source}
          {event.github_repo && (
            <span className="text-text-muted text-[10px] ml-1.5">
              {event.github_pr ? `${event.github_repo}#${event.github_pr}` : event.github_repo}
            </span>
          )}
        </td>
        <td className="px-3 py-2 font-code text-text-secondary text-[11px] truncate max-w-24" title={event.model}>{event.model}</td>
        <td className="px-3 py-2 font-code text-text-primary text-[12px]">{formatDuration(event.duration_ms)}</td>
        <td className="px-3 py-2 font-code text-text-secondary text-[12px]">{event.tokens_total != null ? fmtTokens(event.tokens_total) : '\u2014'}</td>
        <td className="px-3 py-2 font-code text-text-secondary text-[12px]">{formatCost(estimateCost(event))}</td>
        <td className="px-3 py-2 font-code text-text-secondary text-[12px]">{event.diff_files_reviewed}</td>
        <td className="px-3 py-2 font-code text-text-primary text-[12px]">{event.comments_total}</td>
        <td className="px-3 py-2">
          {event.overall_score != null ? (
            <span className={`font-code font-bold text-[12px] ${event.overall_score >= 7 ? 'text-sev-suggestion' : event.overall_score >= 4 ? 'text-sev-warning' : 'text-sev-error'}`}>
              {event.overall_score.toFixed(1)}
            </span>
          ) : <span className="text-text-muted">{'\u2014'}</span>}
        </td>
        <td className="px-3 py-2">
          <span className={`text-[10px] font-code font-medium ${statusColor}`}>{statusLabel}</span>
        </td>
        <td className="px-3 py-2 text-text-muted">
          <ChevronDown size={12} className={`transition-transform ${expanded ? 'rotate-180' : ''}`} />
        </td>
      </tr>
      {expanded && <EventDetail event={event} colSpan={compareMode ? 12 : 11} />}
    </>
  )
}

/* ---- Event Detail ---- */
function EventDetail({ event, colSpan }: { event: ReviewEvent; colSpan: number }) {
  const navigate = useNavigate()
  const fileMetrics: FileMetricEvent[] = event.file_metrics ?? []
  const hotspots: HotspotDetail[] = event.hotspot_details ?? []

  return (
    <tr>
      <td colSpan={colSpan} className="bg-surface/60 px-4 py-3 border-b border-border">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4 text-[11px]">
          {/* Timing breakdown */}
          <div>
            <div className="text-text-muted uppercase tracking-wider font-medium mb-1.5">Timing</div>
            <div className="space-y-0.5 text-text-secondary">
              <div>Total: <span className="font-code text-text-primary">{formatDuration(event.duration_ms)}</span></div>
              {event.llm_total_ms != null && (
                <div>LLM: <span className="font-code text-text-primary">{formatDuration(event.llm_total_ms)}</span></div>
              )}
              {event.diff_fetch_ms != null && event.diff_fetch_ms > 0 && (
                <div>Diff fetch: <span className="font-code text-text-primary">{formatDuration(event.diff_fetch_ms)}</span></div>
              )}
            </div>
          </div>

          {/* Token breakdown */}
          <div>
            <div className="text-text-muted uppercase tracking-wider font-medium mb-1.5">Tokens</div>
            <div className="space-y-0.5 text-text-secondary">
              {event.tokens_total != null && (
                <>
                  <div>Total: <span className="font-code text-text-primary">{fmtTokens(event.tokens_total)}</span></div>
                  {event.tokens_prompt != null && (
                    <div>Prompt: <span className="font-code text-text-primary">{fmtTokens(event.tokens_prompt)}</span></div>
                  )}
                  {event.tokens_completion != null && (
                    <div>Completion: <span className="font-code text-text-primary">{fmtTokens(event.tokens_completion)}</span></div>
                  )}
                  <div>Cost: <span className="font-code text-text-primary">{formatCost(estimateCost(event))}</span></div>
                </>
              )}
            </div>
          </div>

          {/* Results summary */}
          <div>
            <div className="text-text-muted uppercase tracking-wider font-medium mb-1.5">Results</div>
            <div className="space-y-0.5 text-text-secondary">
              {Object.entries(event.comments_by_severity).map(([sev, n]) => (
                <div key={sev}>
                  <span className={`inline-block w-1.5 h-1.5 rounded-full mr-1.5 ${sev === 'Error' ? 'bg-sev-error' : sev === 'Warning' ? 'bg-sev-warning' : sev === 'Info' ? 'bg-sev-info' : 'bg-sev-suggestion'}`} />
                  {sev}: <span className="font-code text-text-primary">{n}</span>
                </div>
              ))}
              {event.convention_suppressed != null && event.convention_suppressed > 0 && (
                <div className="text-text-muted">{event.convention_suppressed} suppressed</div>
              )}
              {event.comments_by_pass && Object.keys(event.comments_by_pass).length > 0 && (
                <div className="mt-1 pt-1 border-t border-border/50">
                  {Object.entries(event.comments_by_pass).map(([pass, n]) => (
                    <div key={pass}><span className="font-code text-text-primary">{n}</span> {pass}</div>
                  ))}
                </div>
              )}
              {event.github_posted && (
                <div className="text-accent mt-1">Posted to PR #{event.github_pr}</div>
              )}
            </div>
          </div>
        </div>

        {/* View Review button */}
        <div className="mt-3 pt-3 border-t border-border/50">
          <button
            onClick={() => navigate(`/review/${event.review_id}`)}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 text-[11px] font-code font-medium text-accent bg-accent/10 hover:bg-accent/20 rounded transition-colors"
          >
            <ExternalLink size={12} />
            View Review
          </button>
        </div>

        {/* Hotspots */}
        {hotspots.length > 0 && (
          <div className="mt-3 pt-3 border-t border-border/50">
            <div className="text-text-muted uppercase tracking-wider font-medium text-[11px] mb-1.5">High Risk Files</div>
            <div className="space-y-1">
              {hotspots.map(h => (
                <div key={h.file_path} className="flex items-center gap-2 text-[11px]">
                  <span className="font-code text-text-primary truncate flex-1">{h.file_path}</span>
                  <span className={`font-code font-medium ${h.risk_score > 0.6 ? 'text-sev-error' : h.risk_score > 0.3 ? 'text-sev-warning' : 'text-sev-info'}`}>
                    {(h.risk_score * 100).toFixed(0)}%
                  </span>
                  <span className="text-text-muted truncate max-w-48">{h.reasons.join(', ')}</span>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Per-file metrics waterfall */}
        {fileMetrics.length > 0 && (
          <div className="mt-3 pt-3 border-t border-border/50">
            <div className="text-text-muted uppercase tracking-wider font-medium text-[11px] mb-1.5">
              Per-File Breakdown
            </div>
            <table className="w-full text-[11px]">
              <thead>
                <tr className="text-text-muted text-left">
                  <th className="font-medium pr-3 py-0.5">File</th>
                  <th className="font-medium pr-3 py-0.5 text-right">Latency</th>
                  <th className="font-medium pr-3 py-0.5 text-right">Tokens</th>
                  <th className="font-medium py-0.5 text-right">Comments</th>
                  <th className="font-medium py-0.5 pl-3" style={{ width: '30%' }}></th>
                </tr>
              </thead>
              <tbody>
                {fileMetrics.map(m => {
                  const maxLatency = Math.max(...fileMetrics.map(f => f.latency_ms))
                  const pct = maxLatency > 0 ? (m.latency_ms / maxLatency) * 100 : 0
                  return (
                    <tr key={m.file_path} className="text-text-secondary">
                      <td className="font-code text-text-primary truncate max-w-48 pr-3 py-0.5" title={m.file_path}>
                        {m.file_path.split('/').pop()}
                      </td>
                      <td className="font-code text-right pr-3 py-0.5">{formatDuration(m.latency_ms)}</td>
                      <td className="font-code text-right pr-3 py-0.5">{fmtTokens(m.total_tokens)}</td>
                      <td className="font-code text-right py-0.5">{m.comment_count}</td>
                      <td className="pl-3 py-0.5">
                        <div className="h-2 bg-surface-3 rounded-full overflow-hidden">
                          <div className="h-full bg-accent/60 rounded-full" style={{ width: `${pct}%` }} />
                        </div>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        )}

        {/* Error */}
        {event.error && (
          <div className="mt-3 pt-3 border-t border-border/50">
            <div className="text-sev-error text-[11px] font-code bg-sev-error/10 rounded px-2 py-1">{event.error}</div>
          </div>
        )}
      </td>
    </tr>
  )
}

/* ---- Inject animation keyframes ---- */
const STYLE_ID = 'events-page-styles'
function ensureStyles() {
  if (document.getElementById(STYLE_ID)) return
  const style = document.createElement('style')
  style.id = STYLE_ID
  style.textContent = `
    @keyframes newRowFlash {
      0% { background-color: rgba(74,222,128,0.18); }
      100% { background-color: transparent; }
    }
    .animate-new-row {
      animation: newRowFlash 1.5s ease-out;
    }
    @keyframes livePulse {
      0%, 100% { opacity: 1; }
      50% { opacity: 0.4; }
    }
    .animate-live-pulse {
      animation: livePulse 1.5s ease-in-out infinite;
    }
  `
  document.head.appendChild(style)
}

/* ---- Main Component ---- */
export function Events() {
  const { data: events, isLoading } = useEvents()
  const [search, setSearch] = useState('')
  const [sourceFilter, setSourceFilter] = useState<string>('all')
  const [modelFilter, setModelFilter] = useState<string>('all')
  const [sortField, setSortField] = useState<SortField>('duration')
  const [sortDir, setSortDir] = useState<SortDir>('desc')
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [page, setPage] = useState(1)

  // Live tail state
  const [liveMode, setLiveMode] = useState(false)
  const prevEventCountRef = useRef<number>(0)
  const [newEventIds, setNewEventIds] = useState<Set<string>>(new Set())

  // Compare mode state
  const [compareMode, setCompareMode] = useState(false)
  const [compareIds, setCompareIds] = useState<string[]>([])

  // Export dropdown
  const [exportOpen, setExportOpen] = useState(false)
  const exportRef = useRef<HTMLDivElement>(null)

  const allEvents = events ?? []

  // Inject styles on mount
  useEffect(() => { ensureStyles() }, [])

  // Close export dropdown on outside click
  useEffect(() => {
    if (!exportOpen) return
    const handler = (e: MouseEvent) => {
      if (exportRef.current && !exportRef.current.contains(e.target as Node)) setExportOpen(false)
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [exportOpen])

  // Live tail: detect new events
  useEffect(() => {
    if (!liveMode) {
      prevEventCountRef.current = allEvents.length
      return
    }
    if (allEvents.length > prevEventCountRef.current && prevEventCountRef.current > 0) {
      // New events arrived. Events are assumed sorted newest-first or we detect by count diff.
      const newCount = allEvents.length - prevEventCountRef.current
      const newIds = new Set(allEvents.slice(0, newCount).map(e => e.review_id))
      setNewEventIds(newIds)
      // Clear highlight after animation
      const timer = setTimeout(() => setNewEventIds(new Set()), 1800)
      return () => clearTimeout(timer)
    }
    prevEventCountRef.current = allEvents.length
  }, [allEvents, liveMode])

  // Derive unique sources and models for filter dropdowns
  const sources = useMemo(() => [...new Set(allEvents.map(e => e.diff_source))].sort(), [allEvents])
  const models = useMemo(() => [...new Set(allEvents.map(e => e.model))].sort(), [allEvents])

  const filtered = useMemo(() => {
    let list = allEvents
    if (sourceFilter !== 'all') list = list.filter(e => e.diff_source === sourceFilter)
    if (modelFilter !== 'all') list = list.filter(e => e.model === modelFilter)
    if (search.trim()) {
      const q = search.toLowerCase()
      list = list.filter(e =>
        e.review_id.toLowerCase().includes(q) ||
        (e.title || '').toLowerCase().includes(q) ||
        e.diff_source.toLowerCase().includes(q) ||
        (e.github_repo || '').toLowerCase().includes(q)
      )
    }
    // Sort
    const dir = sortDir === 'asc' ? 1 : -1
    list = [...list].sort((a, b) => {
      switch (sortField) {
        case 'duration': return (a.duration_ms - b.duration_ms) * dir
        case 'tokens': return ((a.tokens_total ?? 0) - (b.tokens_total ?? 0)) * dir
        case 'score': return ((a.overall_score ?? 0) - (b.overall_score ?? 0)) * dir
        case 'files': return (a.diff_files_reviewed - b.diff_files_reviewed) * dir
        case 'comments': return (a.comments_total - b.comments_total) * dir
        case 'cost': return (estimateCost(a) - estimateCost(b)) * dir
        default: return 0
      }
    })
    return list
  }, [allEvents, sourceFilter, modelFilter, search, sortField, sortDir])

  const stats = useMemo(() => computeStats(allEvents), [allEvents])
  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE))
  const paginated = filtered.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE)

  const toggleSort = (field: SortField) => {
    if (sortField === field) setSortDir(d => d === 'asc' ? 'desc' : 'asc')
    else { setSortField(field); setSortDir('desc') }
  }
  const sortIndicator = (field: SortField) => sortField === field ? (sortDir === 'asc' ? ' \u25b4' : ' \u25be') : ''

  const handleCompareToggle = useCallback((id: string) => {
    setCompareIds(prev => {
      if (prev.includes(id)) return prev.filter(x => x !== id)
      if (prev.length >= 2) return [prev[1], id]
      return [...prev, id]
    })
  }, [])

  // Resolve compare events
  const compareEventA = compareIds.length >= 1 ? allEvents.find(e => e.review_id === compareIds[0]) : undefined
  const compareEventB = compareIds.length >= 2 ? allEvents.find(e => e.review_id === compareIds[1]) : undefined

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="animate-spin text-accent" size={32} />
      </div>
    )
  }

  return (
    <div className="p-6 max-w-6xl mx-auto">
      {/* Title row with live toggle */}
      <div className="flex items-center gap-3 mb-6">
        <h1 className="text-xl font-semibold text-text-primary">Review Events</h1>
        <button
          onClick={() => setLiveMode(v => !v)}
          className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded text-[11px] font-code font-medium transition-colors ${
            liveMode ? 'bg-sev-suggestion/15 text-sev-suggestion' : 'bg-surface-1 border border-border text-text-muted hover:text-text-secondary'
          }`}
        >
          {liveMode && <span className="relative flex h-2 w-2"><span className="animate-live-pulse absolute inline-flex h-full w-full rounded-full bg-sev-suggestion" /><span className="relative inline-flex rounded-full h-2 w-2 bg-sev-suggestion" /></span>}
          <Radio size={12} />
          Live
        </button>
      </div>

      {/* Stat cards */}
      <div className="grid grid-cols-2 md:grid-cols-6 gap-3 mb-6">
        {[
          { label: 'REVIEWS', value: String(stats.totalReviews), sub: stats.failedCount > 0 ? `${stats.failedCount} failed` : undefined, subColor: 'text-sev-error' },
          { label: 'AVG DURATION', value: formatDuration(stats.avgDuration) },
          { label: 'TOTAL TOKENS', value: fmtTokens(stats.totalTokens) },
          { label: 'AVG SCORE', value: stats.avgScore.toFixed(1), valueColor: stats.avgScore >= 7 ? 'text-sev-suggestion' : stats.avgScore >= 4 ? 'text-sev-warning' : 'text-sev-error' },
          { label: 'TOTAL FILES', value: String(allEvents.reduce((s, e) => s + e.diff_files_reviewed, 0)) },
          { label: 'TOTAL COST', value: formatCost(totalCost(allEvents)) },
        ].map(card => (
          <div key={card.label} className="bg-surface-1 border border-border rounded-lg p-3">
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">{card.label}</div>
            <div className={`text-lg font-bold font-code mt-1 ${card.valueColor || 'text-text-primary'}`}>{card.value}</div>
            {card.sub && <div className={`text-[10px] font-code ${card.subColor || 'text-text-muted'}`}>{card.sub}</div>}
          </div>
        ))}
      </div>

      {/* Charts */}
      {stats.timeline.length > 1 && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3 mb-6">
          <div className="bg-surface-1 border border-border rounded-lg p-4">
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">DURATION OVER TIME (seconds)</div>
            <div className="h-28">
              <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                <AreaChart data={stats.timeline}>
                  <defs>
                    <linearGradient id="durGrad" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor={CHART_THEME.accent} stopOpacity={0.3} />
                      <stop offset="95%" stopColor={CHART_THEME.accent} stopOpacity={0.02} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid {...gridProps} />
                  <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                  <YAxis tick={axisTick} axisLine={false} tickLine={false} />
                  <Tooltip {...tooltipStyle} />
                  <Area type="monotone" dataKey="duration" stroke={CHART_THEME.accent} fill="url(#durGrad)" strokeWidth={1.5} dot={false} />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          </div>
          <div className="bg-surface-1 border border-border rounded-lg p-4">
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">TOKEN USAGE OVER TIME</div>
            <div className="h-28">
              <ResponsiveContainer width="100%" height="99%" minWidth={50} minHeight={50}>
                <BarChart data={stats.timeline}>
                  <CartesianGrid {...gridProps} />
                  <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} />
                  <YAxis tick={axisTick} axisLine={false} tickLine={false} />
                  <Tooltip {...tooltipStyle} />
                  <Bar dataKey="tokens" fill={CHART_THEME.accent} radius={[2, 2, 0, 0]} barSize={12} />
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>
        </div>
      )}

      {/* Filters + Export + Compare */}
      <div className="flex items-center gap-3 mb-3">
        <div className="relative flex-1 max-w-xs">
          <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-text-muted" />
          <input
            type="text"
            value={search}
            onChange={e => { setSearch(e.target.value); setPage(1) }}
            placeholder="Search by ID, title, repo..."
            className="w-full bg-surface-1 border border-border rounded pl-9 pr-3 py-1.5 text-[12px] text-text-primary placeholder:text-text-muted/40 focus:outline-none focus:ring-1 focus:ring-accent font-code"
          />
        </div>
        <select
          value={sourceFilter}
          onChange={e => { setSourceFilter(e.target.value); setPage(1) }}
          className="bg-surface-1 border border-border rounded px-2 py-1.5 text-[12px] text-text-primary focus:outline-none focus:ring-1 focus:ring-accent font-code"
        >
          <option value="all">All sources</option>
          {sources.map(s => <option key={s} value={s}>{s}</option>)}
        </select>
        <select
          value={modelFilter}
          onChange={e => { setModelFilter(e.target.value); setPage(1) }}
          className="bg-surface-1 border border-border rounded px-2 py-1.5 text-[12px] text-text-primary focus:outline-none focus:ring-1 focus:ring-accent font-code"
        >
          <option value="all">All models</option>
          {models.map(m => <option key={m} value={m}>{m}</option>)}
        </select>

        {/* Export dropdown */}
        <div className="relative" ref={exportRef}>
          <button
            onClick={() => setExportOpen(v => !v)}
            className="inline-flex items-center gap-1.5 bg-surface-1 border border-border rounded px-2.5 py-1.5 text-[12px] text-text-secondary hover:text-text-primary font-code transition-colors"
          >
            <Download size={13} />
            Export
            <ChevronDown size={10} className={`transition-transform ${exportOpen ? 'rotate-180' : ''}`} />
          </button>
          {exportOpen && (
            <div className="absolute right-0 top-full mt-1 z-20 bg-surface-1 border border-border rounded-lg shadow-lg overflow-hidden min-w-[140px]">
              <button
                onClick={() => { exportCSV(filtered); setExportOpen(false) }}
                className="w-full text-left px-3 py-2 text-[12px] text-text-secondary hover:bg-surface-2 hover:text-text-primary font-code transition-colors"
              >
                Export CSV
              </button>
              <button
                onClick={() => { exportJSON(filtered); setExportOpen(false) }}
                className="w-full text-left px-3 py-2 text-[12px] text-text-secondary hover:bg-surface-2 hover:text-text-primary font-code transition-colors border-t border-border/50"
              >
                Export JSON
              </button>
            </div>
          )}
        </div>

        {/* Compare toggle */}
        <button
          onClick={() => { setCompareMode(v => !v); setCompareIds([]) }}
          className={`inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded text-[12px] font-code font-medium transition-colors ${
            compareMode ? 'bg-accent/15 text-accent border border-accent/30' : 'bg-surface-1 border border-border text-text-muted hover:text-text-secondary'
          }`}
        >
          <GitCompareArrows size={13} />
          Compare
        </button>
      </div>

      {/* Compare mode hint */}
      {compareMode && (
        <div className="text-[11px] text-text-muted font-code mb-2">
          Select 2 events to compare. {compareIds.length}/2 selected.
          {compareIds.length > 0 && (
            <button onClick={() => setCompareIds([])} className="ml-2 text-accent hover:underline">Clear</button>
          )}
        </div>
      )}

      {/* Event table */}
      <div className="bg-surface-1 border border-border rounded-lg overflow-hidden">
        <table className="w-full text-[12px]">
          <thead>
            <tr className="border-b border-border">
              {compareMode && <th className="px-2 py-2.5 w-8"></th>}
              <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px]">ID</th>
              <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px]">SOURCE</th>
              <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px]">MODEL</th>
              <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px] cursor-pointer hover:text-text-secondary" onClick={() => toggleSort('duration')}>DURATION{sortIndicator('duration')}</th>
              <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px] cursor-pointer hover:text-text-secondary" onClick={() => toggleSort('tokens')}>TOKENS{sortIndicator('tokens')}</th>
              <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px] cursor-pointer hover:text-text-secondary" onClick={() => toggleSort('cost')}>COST{sortIndicator('cost')}</th>
              <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px] cursor-pointer hover:text-text-secondary" onClick={() => toggleSort('files')}>FILES{sortIndicator('files')}</th>
              <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px] cursor-pointer hover:text-text-secondary" onClick={() => toggleSort('comments')}>CMTS{sortIndicator('comments')}</th>
              <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px] cursor-pointer hover:text-text-secondary" onClick={() => toggleSort('score')}>SCORE{sortIndicator('score')}</th>
              <th className="text-left px-3 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px]">STATUS</th>
              <th className="px-3 py-2.5 w-8"></th>
            </tr>
          </thead>
          <tbody>
            {paginated.length === 0 ? (
              <tr>
                <td colSpan={compareMode ? 12 : 11} className="px-4 py-10 text-center text-text-muted">
                  {allEvents.length === 0 ? 'No review events yet. Complete a review to see events.' : 'No matching events.'}
                </td>
              </tr>
            ) : (
              paginated.map(event => (
                <EventRow
                  key={event.review_id}
                  event={event}
                  expanded={expandedId === event.review_id}
                  onToggle={() => setExpandedId(expandedId === event.review_id ? null : event.review_id)}
                  compareMode={compareMode}
                  compareSelected={compareIds.includes(event.review_id)}
                  onCompareToggle={handleCompareToggle}
                  isNew={newEventIds.has(event.review_id)}
                />
              ))
            )}
          </tbody>
        </table>
      </div>

      {/* Comparison panel */}
      {compareMode && compareEventA && compareEventB && (
        <ComparisonPanel a={compareEventA} b={compareEventB} />
      )}

      {/* Pagination */}
      <div className="flex items-center justify-between mt-3 text-[11px] text-text-muted">
        <span>{filtered.length} event{filtered.length !== 1 ? 's' : ''}</span>
        <div className="flex items-center gap-2">
          <button onClick={() => setPage(p => Math.max(1, p - 1))} disabled={page <= 1} className="p-1 rounded hover:bg-surface-2 disabled:opacity-30 disabled:cursor-default">
            <ChevronLeft size={14} />
          </button>
          <span className="font-code">Page {page} of {totalPages}</span>
          <button onClick={() => setPage(p => Math.min(totalPages, p + 1))} disabled={page >= totalPages} className="p-1 rounded hover:bg-surface-2 disabled:opacity-30 disabled:cursor-default">
            <ChevronRight size={14} />
          </button>
        </div>
      </div>
    </div>
  )
}
