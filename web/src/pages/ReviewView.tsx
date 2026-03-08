import { useParams } from 'react-router-dom'
import { useState, useMemo } from 'react'
import { Loader2, AlertTriangle, MessageSquare, FileCode, ChevronDown, Activity, Clock, Cpu, GitBranch } from 'lucide-react'
import { useReview, useSubmitFeedback } from '../api/hooks'
import { DiffViewer } from '../components/DiffViewer'
import { FileSidebar } from '../components/FileSidebar'
import { ScoreGauge } from '../components/ScoreGauge'
import { SeverityBadge } from '../components/SeverityBadge'
import { CommentCard } from '../components/CommentCard'
import { parseDiff } from '../lib/parseDiff'
import type { Severity, ReviewEvent } from '../api/types'

type ViewMode = 'diff' | 'list'

export function ReviewView() {
  const { id } = useParams<{ id: string }>()
  const { data: review, isLoading } = useReview(id)
  const feedback = useSubmitFeedback(id || '')
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [viewMode, setViewMode] = useState<ViewMode>('diff')
  const [severityFilter, setSeverityFilter] = useState<Set<Severity>>(new Set(['Error', 'Warning', 'Info', 'Suggestion']))
  const [categoryFilter, setCategoryFilter] = useState<string | null>(null)
  const [showEvent, setShowEvent] = useState(false)

  const diffFiles = useMemo(() => {
    if (!review?.diff_content) return []
    return parseDiff(review.diff_content)
  }, [review?.diff_content])

  // All hooks MUST be above this line — no hooks after early returns

  if (isLoading || !review) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="animate-spin text-accent" size={32} />
      </div>
    )
  }

  if (review.status === 'Pending' || review.status === 'Running') {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4">
        <div className="relative">
          <Loader2 className="animate-spin text-accent" size={40} />
        </div>
        <div className="text-center">
          <p className="text-text-primary text-sm">
            {review.status === 'Pending' ? 'Starting review...' : 'Analyzing code...'}
          </p>
          <p className="text-[11px] text-text-muted mt-1">This may take a while for local models</p>
        </div>
        <div className="flex gap-1 mt-2">
          {[0,1,2,3].map(i => (
            <div key={i} className="w-1.5 h-1.5 rounded-full bg-accent animate-pulse" style={{ animationDelay: `${i * 150}ms` }} />
          ))}
        </div>
      </div>
    )
  }

  if (review.status === 'Failed') {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4">
        <AlertTriangle className="text-sev-error" size={40} />
        <p className="text-text-primary">Review failed</p>
        <p className="text-sm text-sev-error max-w-md text-center font-code">{review.error}</p>
      </div>
    )
  }

  // Empty review — no diff and no comments
  if (review.comments.length === 0 && diffFiles.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4">
        <div className="w-16 h-16 rounded-full bg-accent/10 flex items-center justify-center">
          <FileCode className="text-accent" size={28} />
        </div>
        <div className="text-center">
          <p className="text-text-primary font-medium">No changes found</p>
          <p className="text-[12px] text-text-muted mt-1 max-w-sm">
            {review.diff_source === 'branch'
              ? 'This branch has no diff against the base branch.'
              : review.diff_source === 'staged'
              ? 'No staged changes were found.'
              : 'The diff was empty — nothing to review.'}
          </p>
        </div>
        {review.summary && (
          <div className="flex items-center gap-2 mt-2">
            <ScoreGauge score={review.summary.overall_score} />
            <span className="text-[11px] text-text-muted">
              {review.summary.total_comments} findings &middot; {review.diff_source} &middot; {review.id.slice(0, 8)}
            </span>
          </div>
        )}
      </div>
    )
  }

  const toggleSeverity = (sev: Severity) => {
    const next = new Set(severityFilter)
    if (next.has(sev)) next.delete(sev)
    else next.add(sev)
    setSeverityFilter(next)
  }

  const filteredComments = review.comments.filter((c) => {
    if (!severityFilter.has(c.severity)) return false
    if (selectedFile && c.file_path.replace(/^\.\//, '') !== selectedFile) return false
    if (categoryFilter && c.category !== categoryFilter) return false
    return true
  })

  const categories = [...new Set(review.comments.map(c => c.category))]

  const handleFeedback = (commentId: string, action: 'accept' | 'reject') => {
    feedback.mutate({ commentId, action })
  }

  // Group comments by file for list view (no useMemo — filteredComments changes every render)
  const groupedByFile = new Map<string, typeof filteredComments>()
  for (const c of filteredComments) {
    const key = c.file_path
    if (!groupedByFile.has(key)) groupedByFile.set(key, [])
    groupedByFile.get(key)!.push(c)
  }

  const visibleDiffFiles = selectedFile
    ? diffFiles.filter(f => f.path === selectedFile)
    : diffFiles

  return (
    <div className="flex h-full">
      {/* File sidebar */}
      {diffFiles.length > 0 && (
        <FileSidebar
          files={diffFiles}
          comments={review.comments}
          selectedFile={selectedFile}
          onSelectFile={setSelectedFile}
        />
      )}

      <div className="flex-1 overflow-auto flex flex-col min-w-0">
        {/* Summary bar */}
        {review.summary && (
          <div className="p-3 border-b border-border bg-surface-1 flex items-center gap-4">
            <ScoreGauge score={review.summary.overall_score} />
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-3 mb-1.5">
                {Object.entries(review.summary.by_severity).map(([sev, count]) => (
                  <div key={sev} className="flex items-center gap-1">
                    <SeverityBadge severity={sev as Severity} />
                    <span className="text-[12px] font-medium text-text-primary">{count}</span>
                  </div>
                ))}
              </div>
              <div className="text-[11px] text-text-muted flex items-center gap-3">
                <span className="flex items-center gap-1">
                  <MessageSquare size={11} />
                  {review.summary.total_comments} findings
                </span>
                <span className="flex items-center gap-1">
                  <FileCode size={11} />
                  {review.summary.files_reviewed} files
                </span>
                <span className="font-code">{review.diff_source}</span>
                <span className="font-code text-text-muted/50">{review.id.slice(0, 8)}</span>
              </div>
            </div>
            {review.event && (
              <button
                onClick={() => setShowEvent(v => !v)}
                className="flex items-center gap-1 px-2 py-1 rounded text-[11px] text-text-muted hover:text-text-primary hover:bg-surface-2 transition-colors"
                title="Toggle event details"
              >
                <Activity size={12} />
                <span>{formatDuration(review.event.duration_ms)}</span>
                <ChevronDown size={10} className={`transition-transform ${showEvent ? 'rotate-180' : ''}`} />
              </button>
            )}
          </div>
        )}

        {/* Wide event panel */}
        {showEvent && review.event && <EventPanel event={review.event} />}

        {/* Toolbar */}
        <div className="px-3 py-2 border-b border-border bg-surface flex items-center gap-2">
          {/* View mode toggle */}
          <div className="flex items-center bg-surface-2 rounded p-0.5">
            <button
              onClick={() => setViewMode('diff')}
              className={`px-2 py-0.5 rounded text-[11px] font-medium transition-colors ${
                viewMode === 'diff' ? 'bg-accent text-white' : 'text-text-muted hover:text-text-primary'
              }`}
            >
              Diff
            </button>
            <button
              onClick={() => setViewMode('list')}
              className={`px-2 py-0.5 rounded text-[11px] font-medium transition-colors ${
                viewMode === 'list' ? 'bg-accent text-white' : 'text-text-muted hover:text-text-primary'
              }`}
            >
              List
            </button>
          </div>

          <div className="w-px h-4 bg-border mx-1" />

          {/* Severity filters */}
          {(['Error', 'Warning', 'Info', 'Suggestion'] as Severity[]).map(sev => (
            <button
              key={sev}
              onClick={() => toggleSeverity(sev)}
              className={`text-[11px] px-2 py-0.5 rounded transition-colors ${
                severityFilter.has(sev)
                  ? 'bg-surface-3 text-text-primary'
                  : 'text-text-muted/40 hover:text-text-muted'
              }`}
            >
              <SeverityBadge severity={sev} />
            </button>
          ))}

          <div className="w-px h-4 bg-border mx-1" />

          {/* Category filter */}
          <div className="relative">
            <select
              value={categoryFilter || ''}
              onChange={e => setCategoryFilter(e.target.value || null)}
              className="text-[11px] bg-surface-2 border border-border rounded px-2 py-1 text-text-secondary appearance-none pr-6 cursor-pointer"
            >
              <option value="">All categories</option>
              {categories.map(cat => (
                <option key={cat} value={cat}>{cat}</option>
              ))}
            </select>
            <ChevronDown size={10} className="absolute right-1.5 top-1/2 -translate-y-1/2 text-text-muted pointer-events-none" />
          </div>

          <span className="ml-auto text-[11px] text-text-muted">
            {filteredComments.length}/{review.comments.length}
          </span>
        </div>

        {/* Main content */}
        <div className="flex-1 overflow-auto p-4">
          {viewMode === 'diff' && diffFiles.length > 0 ? (
            <DiffViewer
              files={visibleDiffFiles}
              comments={filteredComments}
              onFeedback={handleFeedback}
            />
          ) : (
            /* List view / fallback when no diff content */
            <div className="space-y-4 max-w-3xl">
              {[...groupedByFile.entries()].map(([file, fileComments]) => (
                <div key={file}>
                  <div className="flex items-center gap-2 mb-2">
                    <FileCode size={13} className="text-text-muted" />
                    <span className="font-code text-[12px] text-text-muted">{file.split('/').slice(0, -1).join('/')}/</span>
                    <span className="font-code text-[12px] text-text-primary font-medium">{file.split('/').pop()}</span>
                  </div>
                  <div className="space-y-2 ml-5">
                    {fileComments
                      .sort((a, b) => a.line_number - b.line_number)
                      .map(comment => (
                        <div key={comment.id}>
                          <span className="text-[10px] text-text-muted font-code">L{comment.line_number}</span>
                          <CommentCard
                            comment={comment}
                            onFeedback={action => handleFeedback(comment.id, action)}
                          />
                        </div>
                      ))}
                  </div>
                </div>
              ))}

              {filteredComments.length === 0 && (
                <div className="text-center py-16 text-text-muted">
                  {review.comments.length === 0
                    ? 'No findings. Code looks good!'
                    : 'No findings match the current filters.'}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  return `${(ms / 60_000).toFixed(1)}m`
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function EventPanel({ event }: { event: ReviewEvent }) {
  const stats = [
    { icon: Clock, label: 'Total', value: formatDuration(event.duration_ms) },
    ...(event.llm_total_ms != null
      ? [{ icon: Cpu, label: 'LLM', value: formatDuration(event.llm_total_ms) }]
      : []),
    ...(event.diff_fetch_ms != null && event.diff_fetch_ms > 0
      ? [{ icon: GitBranch, label: 'Diff fetch', value: formatDuration(event.diff_fetch_ms) }]
      : []),
  ]

  return (
    <div className="border-b border-border bg-surface-1/50 px-4 py-3">
      <div className="grid grid-cols-2 md:grid-cols-4 gap-x-6 gap-y-3 text-[11px]">
        {/* Timing */}
        <div>
          <div className="text-text-muted mb-1.5 uppercase tracking-wider font-medium">Timing</div>
          {stats.map(s => (
            <div key={s.label} className="flex items-center gap-1.5 text-text-secondary mb-0.5">
              <s.icon size={10} className="text-text-muted" />
              <span className="text-text-muted">{s.label}</span>
              <span className="font-code font-medium text-text-primary">{s.value}</span>
            </div>
          ))}
        </div>

        {/* Diff Stats */}
        <div>
          <div className="text-text-muted mb-1.5 uppercase tracking-wider font-medium">Diff</div>
          <div className="text-text-secondary space-y-0.5">
            <div><span className="text-text-muted">Size:</span> <span className="font-code text-text-primary">{formatBytes(event.diff_bytes)}</span></div>
            <div><span className="text-text-muted">Files:</span> <span className="font-code text-text-primary">{event.diff_files_reviewed}/{event.diff_files_total}</span> reviewed</div>
            {event.diff_files_skipped > 0 && (
              <div><span className="text-text-muted">Skipped:</span> <span className="font-code text-text-primary">{event.diff_files_skipped}</span></div>
            )}
          </div>
        </div>

        {/* Model */}
        <div>
          <div className="text-text-muted mb-1.5 uppercase tracking-wider font-medium">Model</div>
          <div className="text-text-secondary space-y-0.5">
            <div className="font-code text-text-primary truncate" title={event.model}>{event.model}</div>
            {event.provider && <div><span className="text-text-muted">via</span> {event.provider}</div>}
            {event.base_url && <div className="font-code text-text-muted truncate text-[10px]" title={event.base_url}>{event.base_url}</div>}
          </div>
        </div>

        {/* Results / GitHub */}
        <div>
          <div className="text-text-muted mb-1.5 uppercase tracking-wider font-medium">Results</div>
          <div className="text-text-secondary space-y-0.5">
            {Object.entries(event.comments_by_severity).map(([sev, n]) => (
              <div key={sev} className="flex items-center gap-1">
                <SeverityBadge severity={sev as Severity} />
                <span className="font-code text-text-primary">{n}</span>
              </div>
            ))}
            {event.github_posted && (
              <div className="flex items-center gap-1 mt-1 text-accent">
                <GitBranch size={10} />
                <span>Posted to PR #{event.github_pr}</span>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
