import { useState, useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { useReviews } from '../api/hooks'
import { Loader2, Search, ChevronLeft, ChevronRight } from 'lucide-react'
import { scoreColorClass } from '../lib/scores'
import { STATUS_STYLES, PAGE_SIZE } from '../lib/constants'
import type { MergeReadiness, ReviewStatus } from '../api/types'

export function History() {
  const navigate = useNavigate()
  const { data: reviews, isLoading } = useReviews()
  const [search, setSearch] = useState('')
  const [statusFilter, setStatusFilter] = useState<ReviewStatus | 'All'>('All')
  const [page, setPage] = useState(1)

  const filtered = useMemo(() => {
    let list = reviews || []
    if (statusFilter !== 'All') {
      list = list.filter(r => r.status === statusFilter)
    }
    if (search.trim()) {
      const q = search.toLowerCase()
      list = list.filter(r =>
        r.id.toLowerCase().includes(q) ||
        r.diff_source.toLowerCase().includes(q)
      )
    }
    return list
  }, [reviews, statusFilter, search])

  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE))
  const paginated = filtered.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE)

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="animate-spin text-accent" size={32} />
      </div>
    )
  }

  const readinessStyles: Record<MergeReadiness, string> = {
    Ready: 'text-sev-suggestion bg-sev-suggestion/10',
    NeedsAttention: 'text-sev-warning bg-sev-warning/10',
    NeedsReReview: 'text-accent bg-accent/10',
  }

  return (
    <div className="p-6 max-w-5xl mx-auto">
      <h1 className="text-xl font-semibold text-text-primary mb-4">Review Logs</h1>

      {/* Search + filter bar */}
      <div className="flex items-center gap-3 mb-4">
        <div className="relative flex-1 max-w-sm">
          <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-text-muted" />
          <input
            type="text"
            value={search}
            onChange={e => { setSearch(e.target.value); setPage(1) }}
            placeholder="Search by ID or source..."
            className="w-full bg-surface-1 border border-border rounded pl-9 pr-3 py-1.5 text-[12px] text-text-primary placeholder:text-text-muted/40 focus:outline-none focus:ring-1 focus:ring-accent font-code"
          />
        </div>
        <div className="ml-auto flex items-center gap-2">
          <span className="text-[11px] text-text-muted">Status:</span>
          <select
            value={statusFilter}
            onChange={e => { setStatusFilter(e.target.value as ReviewStatus | 'All'); setPage(1) }}
            className="bg-surface-1 border border-border rounded px-2 py-1.5 text-[12px] text-text-primary focus:outline-none focus:ring-1 focus:ring-accent font-code"
          >
            <option value="All">All</option>
            <option value="Complete">Completed</option>
            <option value="Failed">Failed</option>
            <option value="Running">Running</option>
            <option value="Pending">Pending</option>
          </select>
        </div>
      </div>

      {/* Table */}
      <div className="bg-surface-1 border border-border rounded-lg overflow-hidden">
        <table className="w-full text-[12px]">
          <thead>
            <tr className="border-b border-border">
              {['#', 'SOURCE', 'SCORE', 'FILES', 'FINDINGS', 'READINESS', 'STATUS', 'ID'].map(h => (
                <th key={h} className="text-left px-4 py-2.5 font-semibold text-text-muted tracking-[0.05em] font-code text-[10px]">{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {paginated.length === 0 ? (
              <tr>
                <td colSpan={8} className="px-4 py-10 text-center text-text-muted">
                  {search || statusFilter !== 'All' ? 'No matching reviews.' : 'No reviews yet.'}
                </td>
              </tr>
            ) : (
              paginated.map((review, i) => (
                <tr
                  key={review.id}
                  onClick={() => navigate(`/review/${review.id}`)}
                  className="border-b border-border-subtle hover:bg-surface-2 cursor-pointer transition-colors"
                >
                  <td className="px-4 py-2.5 text-text-muted font-code">
                    {(page - 1) * PAGE_SIZE + i + 1}
                  </td>
                  <td className="px-4 py-2.5 text-text-primary font-medium">{review.diff_source}</td>
                  <td className="px-4 py-2.5">
                    {review.summary ? (
                      <span className={`font-code font-bold ${scoreColorClass(review.summary.overall_score)}`}>
                        {review.summary.overall_score.toFixed(1)}
                      </span>
                    ) : (
                      <span className="text-text-muted">{'\u2014'}</span>
                    )}
                  </td>
                  <td className="px-4 py-2.5 text-text-secondary font-code">{review.files_reviewed}</td>
                  <td className="px-4 py-2.5 text-text-secondary font-code">{review.summary?.total_comments ?? '\u2014'}</td>
                  <td className="px-4 py-2.5">
                    {review.summary ? (
                      <span
                        className={`text-[10px] px-2 py-0.5 rounded font-code ${readinessStyles[review.summary.merge_readiness]}`}
                        title={review.summary.readiness_reasons.join(' | ')}
                      >
                        {review.summary.merge_readiness === 'Ready'
                          ? 'Ready'
                          : review.summary.merge_readiness === 'NeedsAttention'
                            ? 'Attention'
                            : 'Re-review'}
                      </span>
                    ) : (
                      <span className="text-text-muted">{'\u2014'}</span>
                    )}
                  </td>
                  <td className="px-4 py-2.5">
                    <span className={`text-[10px] px-2 py-0.5 rounded font-code ${STATUS_STYLES[review.status] || 'text-text-muted bg-surface-3'}`}>
                      {review.status}
                    </span>
                  </td>
                  <td className="px-4 py-2.5 font-code text-text-muted">{review.id.slice(0, 8)}</td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      {/* Pagination */}
      <div className="flex items-center justify-between mt-3 text-[11px] text-text-muted">
        <span>{filtered.length} review{filtered.length !== 1 ? 's' : ''}</span>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setPage(p => Math.max(1, p - 1))}
            disabled={page <= 1}
            className="p-1 rounded hover:bg-surface-2 disabled:opacity-30 disabled:cursor-default"
          >
            <ChevronLeft size={14} />
          </button>
          <span className="font-code">Page {page} of {totalPages}</span>
          <button
            onClick={() => setPage(p => Math.min(totalPages, p + 1))}
            disabled={page >= totalPages}
            className="p-1 rounded hover:bg-surface-2 disabled:opacity-30 disabled:cursor-default"
          >
            <ChevronRight size={14} />
          </button>
        </div>
      </div>
    </div>
  )
}
