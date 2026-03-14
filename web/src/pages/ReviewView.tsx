import { useParams } from 'react-router-dom'
import { useState, useMemo, useEffect, useRef, useCallback } from 'react'
import { Loader2, AlertTriangle, MessageSquare, FileCode, ChevronDown, Activity, Clock, Cpu, GitBranch } from 'lucide-react'
import { useReview, useSubmitFeedback, useUpdateCommentLifecycle } from '../api/hooks'
import { DiffViewer } from '../components/DiffViewer'
import { FileSidebar } from '../components/FileSidebar'
import { ScoreGauge } from '../components/ScoreGauge'
import { SeverityBadge } from '../components/SeverityBadge'
import { CommentCard } from '../components/CommentCard'
import { parseDiff } from '../lib/parseDiff'
import type { Comment, CommentLifecycleStatus, DiffFile, MergeReadiness, Severity, ReviewEvent } from '../api/types'

type ViewMode = 'diff' | 'list'

const BLOCKING_SEVERITIES = new Set<Severity>(['Error', 'Warning'])
const LOW_FEEDBACK_COVERAGE_THRESHOLD = 0.5
const MIN_FEEDBACK_COVERAGE_FINDINGS = 3

type ReviewCommentFilters = {
  severityFilter: Set<Severity>
  selectedFile?: string | null
  categoryFilter: string | null
  lifecycleFilter: CommentLifecycleStatus | 'All'
  blockerOnly?: boolean
}

type ReviewCommentSectionKey = 'stale' | 'unresolved' | 'informational' | 'fixed'

type ReviewCommentSection = {
  key: ReviewCommentSectionKey
  title: string
  description: string
  badgeClassName: string
  files: Array<{
    path: string
    comments: Comment[]
  }>
}

const REVIEW_COMMENT_SECTION_ORDER: ReviewCommentSectionKey[] = ['stale', 'unresolved', 'informational', 'fixed']

const REVIEW_COMMENT_SECTION_META: Record<ReviewCommentSectionKey, Omit<ReviewCommentSection, 'files'>> = {
  stale: {
    key: 'stale',
    title: 'Stale',
    description: 'Open findings from a review that predates newer commits.',
    badgeClassName: 'bg-accent/10 text-accent border border-accent/20',
  },
  unresolved: {
    key: 'unresolved',
    title: 'Unresolved',
    description: 'Open blocking findings that still need action.',
    badgeClassName: 'bg-sev-warning/10 text-sev-warning border border-sev-warning/20',
  },
  informational: {
    key: 'informational',
    title: 'Informational',
    description: 'Open non-blocking findings worth keeping in view.',
    badgeClassName: 'bg-surface-2 text-text-muted border border-border',
  },
  fixed: {
    key: 'fixed',
    title: 'Fixed',
    description: 'Resolved and dismissed findings retained for audit history.',
    badgeClassName: 'bg-sev-suggestion/10 text-sev-suggestion border border-sev-suggestion/20',
  },
}

const REVIEW_COMMENT_CARD_SELECTOR = '[data-review-comment-card="true"]'

function normalizeCommentFilePath(filePath: string): string {
  return filePath.replace(/^\.\//, '')
}

function isEditableShortcutTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false

  const tagName = target.tagName.toLowerCase()
  if (tagName === 'input' || tagName === 'textarea' || tagName === 'select') {
    return true
  }

  return target.isContentEditable || target.closest('[contenteditable="true"]') !== null
}

function isBlockingComment(comment: Pick<Comment, 'severity' | 'status'>): boolean {
  return (comment.status ?? 'Open') === 'Open' && BLOCKING_SEVERITIES.has(comment.severity)
}

function classifyReviewCommentSection(
  comment: Comment,
  mergeReadiness?: MergeReadiness,
): ReviewCommentSectionKey {
  const status = comment.status ?? 'Open'
  if (status === 'Resolved' || status === 'Dismissed') {
    return 'fixed'
  }

  if (mergeReadiness === 'NeedsReReview') {
    return 'stale'
  }

  return BLOCKING_SEVERITIES.has(comment.severity) ? 'unresolved' : 'informational'
}

function buildReviewCommentSections(
  comments: Comment[],
  mergeReadiness?: MergeReadiness,
): ReviewCommentSection[] {
  const groupedSections = new Map<ReviewCommentSectionKey, Map<string, Comment[]>>()

  for (const comment of comments) {
    const sectionKey = classifyReviewCommentSection(comment, mergeReadiness)
    if (!groupedSections.has(sectionKey)) {
      groupedSections.set(sectionKey, new Map())
    }

    const sectionFiles = groupedSections.get(sectionKey)!
    if (!sectionFiles.has(comment.file_path)) {
      sectionFiles.set(comment.file_path, [])
    }
    sectionFiles.get(comment.file_path)!.push(comment)
  }

  return REVIEW_COMMENT_SECTION_ORDER.flatMap((sectionKey) => {
    const sectionFiles = groupedSections.get(sectionKey)
    if (!sectionFiles || sectionFiles.size === 0) {
      return []
    }

    return [{
      ...REVIEW_COMMENT_SECTION_META[sectionKey],
      files: [...sectionFiles.entries()].map(([path, sectionComments]) => ({
        path,
        comments: sectionComments,
      })),
    }]
  })
}

function buildVisibleCommentIds(
  comments: Comment[],
  viewMode: ViewMode,
  visibleDiffFiles: DiffFile[],
  commentSections: ReviewCommentSection[],
): string[] {
  if (viewMode === 'list' || visibleDiffFiles.length === 0) {
    return commentSections.flatMap((section) =>
      section.files.flatMap((file) => file.comments.map((comment) => comment.id)),
    )
  }

  const commentsByFile = new Map<string, Map<number, Comment[]>>()
  for (const comment of comments) {
    const filePath = normalizeCommentFilePath(comment.file_path)
    if (!commentsByFile.has(filePath)) {
      commentsByFile.set(filePath, new Map())
    }

    const fileComments = commentsByFile.get(filePath)!
    if (!fileComments.has(comment.line_number)) {
      fileComments.set(comment.line_number, [])
    }

    fileComments.get(comment.line_number)!.push(comment)
  }

  const orderedCommentIds: string[] = []
  const seenCommentIds = new Set<string>()

  for (const file of visibleDiffFiles) {
    const fileComments = commentsByFile.get(file.path)
    if (!fileComments) continue

    for (const hunk of file.hunks) {
      for (const line of hunk.lines) {
        const lineNumber = line.type === 'del' ? line.oldNumber : line.newNumber
        if (!lineNumber) continue

        for (const comment of fileComments.get(lineNumber) ?? []) {
          if (seenCommentIds.has(comment.id)) continue
          seenCommentIds.add(comment.id)
          orderedCommentIds.push(comment.id)
        }
      }
    }
  }

  for (const comment of comments) {
    if (seenCommentIds.has(comment.id)) continue
    orderedCommentIds.push(comment.id)
  }

  return orderedCommentIds
}

function filterReviewComments(
  comments: Comment[],
  {
    severityFilter,
    selectedFile = null,
    categoryFilter,
    lifecycleFilter,
    blockerOnly = false,
  }: ReviewCommentFilters,
): Comment[] {
  return comments.filter((comment) => {
    if (selectedFile && normalizeCommentFilePath(comment.file_path) !== selectedFile) return false
    if (categoryFilter && comment.category !== categoryFilter) return false

    if (blockerOnly) {
      return isBlockingComment(comment)
    }

    if (!severityFilter.has(comment.severity)) return false
    if (lifecycleFilter !== 'All' && (comment.status ?? 'Open') !== lifecycleFilter) return false
    return true
  })
}

export function ReviewView() {
  const { id } = useParams<{ id: string }>()
  const { data: review, isLoading } = useReview(id)
  const feedback = useSubmitFeedback(id || '')
  const lifecycle = useUpdateCommentLifecycle(id || '')
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [viewMode, setViewMode] = useState<ViewMode>('diff')
  const [severityFilter, setSeverityFilter] = useState<Set<Severity>>(new Set(['Error', 'Warning', 'Info', 'Suggestion']))
  const [categoryFilter, setCategoryFilter] = useState<string | null>(null)
  const [lifecycleFilter, setLifecycleFilter] = useState<CommentLifecycleStatus | 'All'>('All')
  const [showOnlyBlockers, setShowOnlyBlockers] = useState(false)
  const [showEvent, setShowEvent] = useState(false)
  const [activeCommentId, setActiveCommentId] = useState<string | null>(null)
  const reviewRootRef = useRef<HTMLDivElement | null>(null)
  const diffContent = review?.diff_content
  const comments = useMemo(() => review?.comments ?? [], [review?.comments])

  const handleFeedback = useCallback((commentId: string, action: 'accept' | 'reject') => {
    feedback.mutate({ commentId, action })
  }, [feedback])

  const handleLifecycleChange = useCallback((commentId: string, status: 'open' | 'resolved' | 'dismissed') => {
    lifecycle.mutate({ commentId, status })
  }, [lifecycle])

  const diffFiles = useMemo(() => {
    if (!diffContent) return []
    return parseDiff(diffContent)
  }, [diffContent])

  const blockerCount = useMemo(() => (
    review?.summary?.open_blocking_comments ?? comments.filter(isBlockingComment).length
  ), [comments, review?.summary?.open_blocking_comments])

  const blockerFilteredComments = useMemo(() => filterReviewComments(comments, {
    severityFilter,
    categoryFilter,
    lifecycleFilter,
    blockerOnly: showOnlyBlockers,
  }), [comments, severityFilter, categoryFilter, lifecycleFilter, showOnlyBlockers])

  const filteredDiffFiles = useMemo(() => {
    if (!showOnlyBlockers) return diffFiles
    const blockerFiles = new Set(blockerFilteredComments.map((comment) => normalizeCommentFilePath(comment.file_path)))
    return diffFiles.filter((file) => blockerFiles.has(file.path))
  }, [diffFiles, blockerFilteredComments, showOnlyBlockers])

  const activeSelectedFile = selectedFile && filteredDiffFiles.some((file) => file.path === selectedFile)
    ? selectedFile
    : null

  const visibleDiffFiles = activeSelectedFile
    ? filteredDiffFiles.filter((file) => file.path === activeSelectedFile)
    : filteredDiffFiles

  const filteredComments = useMemo(() => filterReviewComments(comments, {
    severityFilter,
    selectedFile: activeSelectedFile,
    categoryFilter,
    lifecycleFilter,
    blockerOnly: showOnlyBlockers,
  }), [comments, severityFilter, activeSelectedFile, categoryFilter, lifecycleFilter, showOnlyBlockers])

  const commentSections = useMemo(
    () => buildReviewCommentSections(filteredComments, review?.summary?.merge_readiness),
    [filteredComments, review?.summary?.merge_readiness],
  )

  const visibleCommentIds = useMemo(
    () => buildVisibleCommentIds(filteredComments, viewMode, visibleDiffFiles, commentSections),
    [commentSections, filteredComments, viewMode, visibleDiffFiles],
  )

  const activeVisibleCommentId = activeCommentId && visibleCommentIds.includes(activeCommentId)
    ? activeCommentId
    : null

  const focusComment = useCallback((commentId: string) => {
    if (!reviewRootRef.current) return

    const element = Array.from(
      reviewRootRef.current.querySelectorAll<HTMLElement>(REVIEW_COMMENT_CARD_SELECTOR),
    ).find((candidate) => candidate.dataset.commentId === commentId)

    if (!element) return

    element.focus({ preventScroll: true })
    element.scrollIntoView?.({ block: 'center', behavior: 'smooth' })
  }, [])

  const focusCommentSoon = useCallback((commentId: string) => {
    queueMicrotask(() => focusComment(commentId))
  }, [focusComment])

  const focusNextVisibleComment = useCallback(() => {
    if (visibleCommentIds.length === 0) return

    const currentIndex = activeVisibleCommentId ? visibleCommentIds.indexOf(activeVisibleCommentId) : -1
    const nextIndex = currentIndex >= 0 ? (currentIndex + 1) % visibleCommentIds.length : 0
    const nextCommentId = visibleCommentIds[nextIndex]

    setActiveCommentId(nextCommentId)
    focusCommentSoon(nextCommentId)
  }, [activeVisibleCommentId, focusCommentSoon, visibleCommentIds])

  const runKeyboardAction = useCallback((action: 'accept' | 'reject' | 'resolve') => {
    if (visibleCommentIds.length === 0) return

    const currentCommentId = activeVisibleCommentId
      ? activeVisibleCommentId
      : visibleCommentIds[0]
    const currentComment = filteredComments.find((comment) => comment.id === currentCommentId)
    if (!currentComment) return

    if (action === 'accept') {
      handleFeedback(currentCommentId, 'accept')
    } else if (action === 'reject') {
      handleFeedback(currentCommentId, 'reject')
    } else if ((currentComment.status ?? 'Open') === 'Open') {
      handleLifecycleChange(currentCommentId, 'resolved')
    } else {
      return
    }

    if (visibleCommentIds.length > 1) {
      const currentIndex = visibleCommentIds.indexOf(currentCommentId)
      const nextCommentId = visibleCommentIds[(currentIndex + 1) % visibleCommentIds.length]
      setActiveCommentId(nextCommentId)
      focusCommentSoon(nextCommentId)
      return
    }

    setActiveCommentId(currentCommentId)
    focusCommentSoon(currentCommentId)
  }, [activeVisibleCommentId, filteredComments, focusCommentSoon, handleFeedback, handleLifecycleChange, visibleCommentIds])

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return
      if (isEditableShortcutTarget(event.target)) return

      const key = event.key.toLowerCase()
      if (key === 'j' || key === 'n') {
        event.preventDefault()
        focusNextVisibleComment()
        return
      }

      if (key === 'a') {
        event.preventDefault()
        runKeyboardAction('accept')
        return
      }

      if (key === 'r') {
        event.preventDefault()
        runKeyboardAction('reject')
        return
      }

      if (key === 'e') {
        event.preventDefault()
        runKeyboardAction('resolve')
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [focusNextVisibleComment, runKeyboardAction])

  // All hooks MUST be above this line — no hooks after early returns

  if (isLoading || !review) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="animate-spin text-accent" size={32} />
      </div>
    )
  }

  if (review.status === 'Pending') {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4">
        <Loader2 className="animate-spin text-accent" size={40} />
        <p className="text-text-primary text-sm">Starting review...</p>
        <div className="flex gap-1 mt-2">
          {[0,1,2,3].map(i => (
            <div key={i} className="w-1.5 h-1.5 rounded-full bg-accent animate-pulse" style={{ animationDelay: `${i * 150}ms` }} />
          ))}
        </div>
      </div>
    )
  }

  // Running state: show progress bar + partial results (fall through to main UI below)
  const isRunning = review.status === 'Running'

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

  const categories = [...new Set(comments.map(c => c.category))]

  const readinessStyles: Record<MergeReadiness, string> = {
    Ready: 'bg-sev-suggestion/10 text-sev-suggestion border border-sev-suggestion/20',
    NeedsAttention: 'bg-sev-warning/10 text-sev-warning border border-sev-warning/20',
    NeedsReReview: 'bg-accent/10 text-accent border border-accent/20',
  }

  const verificationStyles: Record<NonNullable<typeof review.summary>['verification']['state'], string> = {
    NotApplicable: 'text-text-muted',
    Verified: 'text-sev-suggestion',
    Inconclusive: 'text-accent',
  }

  const openErrorCount = review.summary?.open_by_severity.Error ?? 0
  const openWarningCount = review.summary?.open_by_severity.Warning ?? 0
  const labeledFeedbackCount = comments.filter((comment) => comment.feedback === 'accept' || comment.feedback === 'reject').length
  const feedbackCoverage = comments.length > 0 ? labeledFeedbackCount / comments.length : 1
  const feedbackCoveragePercent = Math.round(feedbackCoverage * 100)
  const shouldShowFeedbackCallout = comments.length >= MIN_FEEDBACK_COVERAGE_FINDINGS
    && feedbackCoverage < LOW_FEEDBACK_COVERAGE_THRESHOLD

  return (
    <div className="flex h-full" ref={reviewRootRef}>
      {/* File sidebar */}
      {filteredDiffFiles.length > 0 && (
        <FileSidebar
          files={filteredDiffFiles}
          comments={showOnlyBlockers ? blockerFilteredComments : review.comments}
          selectedFile={activeSelectedFile}
          onSelectFile={setSelectedFile}
        />
      )}

      <div className="flex-1 overflow-auto flex flex-col min-w-0">
        {/* Progress banner (visible during Running state) */}
        {isRunning && <ProgressBanner progress={review.progress} comments={review.comments.length} />}

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
                  <span className="text-text-primary">
                    {review.summary.completeness.acknowledged_findings}/{review.summary.completeness.total_findings}
                  </span>
                  acknowledged
                </span>
                <span className="flex items-center gap-1">
                  <span className="text-accent">{review.summary.open_comments}</span>
                  open
                </span>
                <span className="flex items-center gap-1">
                  <span className={review.summary.open_blocking_comments > 0 ? 'text-sev-warning' : 'text-sev-suggestion'}>
                    {review.summary.open_blocking_comments}
                  </span>
                  blocking
                </span>
                <span className="flex items-center gap-1">
                  <span className={review.summary.open_informational_comments > 0 ? 'text-accent' : 'text-text-muted'}>
                    {review.summary.open_informational_comments}
                  </span>
                  informational
                </span>
                <span className="flex items-center gap-1">
                  <FileCode size={11} />
                  {review.summary.files_reviewed} files
                </span>
                <span className="font-code">{review.diff_source}</span>
                <span className="font-code text-text-muted/50">{review.id.slice(0, 8)}</span>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <span className={`text-[10px] px-2 py-0.5 rounded font-code ${readinessStyles[review.summary.merge_readiness]}`}>
                {review.summary.merge_readiness === 'Ready'
                  ? 'Merge Ready'
                  : review.summary.merge_readiness === 'NeedsAttention'
                    ? 'Needs Attention'
                    : 'Needs Re-review'}
              </span>
              <span className="text-[10px] text-text-muted font-code">
                {review.summary.completeness.fixed_findings} fixed · {review.summary.completeness.stale_findings} stale
              </span>
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

        {review.summary && (
          <div className="px-3 py-2 border-b border-border bg-surface flex items-center gap-4 text-[11px]">
            <span className={`font-code ${verificationStyles[review.summary.verification.state]}`}>
              Verification: {review.summary.verification.state}
            </span>
            {review.summary.verification.judge_count > 0 && (
              <span className="text-text-muted font-code">
                judges {review.summary.verification.required_votes}/{review.summary.verification.judge_count}
              </span>
            )}
            {review.summary.verification.warning_count > 0 && (
              <span className="text-accent font-code">
                {review.summary.verification.warning_count} warning{review.summary.verification.warning_count === 1 ? '' : 's'}
              </span>
            )}
            {review.summary.readiness_reasons.length > 0 && (
              <span className="text-text-muted truncate" title={review.summary.readiness_reasons.join(' | ')}>
                {review.summary.readiness_reasons.join(' | ')}
              </span>
            )}
          </div>
        )}

        {review.summary && (
          <div className="px-3 py-3 border-b border-border bg-surface grid grid-cols-2 lg:grid-cols-4 gap-2">
            <SummaryCard
              label="Error Blockers"
              value={openErrorCount}
              hint="Open Error findings"
              tone={openErrorCount > 0 ? 'error' : 'muted'}
            />
            <SummaryCard
              label="Warning Blockers"
              value={openWarningCount}
              hint="Open Warning findings"
              tone={openWarningCount > 0 ? 'warning' : 'muted'}
            />
            <SummaryCard
              label="Blocking Open"
              value={review.summary.open_blocking_comments}
              hint="Error + Warning"
              tone={review.summary.open_blocking_comments > 0 ? 'warning' : 'muted'}
            />
            <SummaryCard
              label="Informational Open"
              value={review.summary.open_informational_comments}
              hint="Info + Suggestion"
              tone={review.summary.open_informational_comments > 0 ? 'accent' : 'muted'}
            />
          </div>
        )}

        {shouldShowFeedbackCallout && (
          <div className="px-3 py-3 border-b border-accent/20 bg-accent/5">
            <div className="flex items-start gap-3">
              <div className="mt-0.5 w-7 h-7 rounded-full bg-accent/10 flex items-center justify-center shrink-0">
                <MessageSquare size={14} className="text-accent" />
              </div>
              <div className="min-w-0 flex-1">
                <div className="text-[10px] font-semibold text-accent tracking-[0.08em] font-code uppercase">
                  Train the reviewer
                </div>
                <p className="mt-1 text-[12px] text-text-primary leading-relaxed">
                  {labeledFeedbackCount === 0
                    ? 'No thumbs recorded yet. Label findings below to train the reviewer.'
                    : `You've labeled ${labeledFeedbackCount} of ${comments.length} findings. Add a few more thumbs so future reviews learn what to keep or suppress.`}
                </p>
              </div>
              <div className="shrink-0 text-right">
                <div className="text-[13px] font-semibold font-code text-accent">{feedbackCoveragePercent}%</div>
                <div className="text-[10px] text-text-muted">coverage</div>
              </div>
            </div>
          </div>
        )}

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
              disabled={showOnlyBlockers}
              className={`text-[11px] px-2 py-0.5 rounded transition-colors ${
                showOnlyBlockers
                  ? 'text-text-muted/40 cursor-not-allowed'
                  : severityFilter.has(sev)
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

          <div className="relative">
            <select
              value={lifecycleFilter}
              onChange={e => setLifecycleFilter(e.target.value as CommentLifecycleStatus | 'All')}
              disabled={showOnlyBlockers}
              className={`text-[11px] bg-surface-2 border border-border rounded px-2 py-1 text-text-secondary appearance-none pr-6 ${showOnlyBlockers ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'}`}
            >
              <option value="All">All statuses</option>
              <option value="Open">Open</option>
              <option value="Resolved">Resolved</option>
              <option value="Dismissed">Dismissed</option>
            </select>
            <ChevronDown size={10} className="absolute right-1.5 top-1/2 -translate-y-1/2 text-text-muted pointer-events-none" />
          </div>

          <button
            onClick={() => setShowOnlyBlockers(value => !value)}
            className={`text-[11px] px-2 py-1 rounded border transition-colors ${
              showOnlyBlockers
                ? 'bg-sev-warning/10 text-sev-warning border-sev-warning/20'
                : 'bg-surface-2 text-text-muted border-border hover:text-text-primary'
            }`}
            title="Show only open Error and Warning findings"
          >
            Blockers only
            <span className="ml-1 font-code">{blockerCount}</span>
          </button>

          {showOnlyBlockers && (
            <span className="text-[10px] text-sev-warning font-code">Open Error + Warning</span>
          )}

          <span className="ml-auto text-[11px] text-text-muted">
            {filteredComments.length}/{review.comments.length}
          </span>

          {filteredComments.length > 0 && (
            <span className="hidden xl:flex items-center gap-2 text-[10px] text-text-muted font-code">
              <span>N/J next</span>
              <span>A accept</span>
              <span>R reject</span>
              <span>E resolve</span>
            </span>
          )}
        </div>

        {/* Main content */}
        <div className="flex-1 overflow-auto p-4">
          {viewMode === 'diff' && visibleDiffFiles.length > 0 ? (
            <DiffViewer
              files={visibleDiffFiles}
              comments={filteredComments}
              onFeedback={handleFeedback}
              onLifecycleChange={handleLifecycleChange}
              activeCommentId={activeVisibleCommentId}
              onActivateComment={setActiveCommentId}
            />
          ) : (
            /* List view / fallback when no diff content */
            <div className="space-y-4 max-w-3xl">
              {commentSections.map((section) => (
                <section key={section.key} className="border border-border rounded-lg overflow-hidden bg-surface-1/60">
                  <div className="px-4 py-3 border-b border-border bg-surface-2/70 flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <h2 className="text-[12px] font-semibold text-text-primary">{section.title}</h2>
                        <span className={`text-[10px] px-2 py-0.5 rounded font-code ${section.badgeClassName}`}>
                          {section.files.reduce((total, file) => total + file.comments.length, 0)}
                        </span>
                      </div>
                      <p className="mt-1 text-[11px] text-text-muted">{section.description}</p>
                    </div>
                  </div>

                  <div className="p-4 space-y-4">
                    {section.files.map(({ path, comments: fileComments }) => (
                      <div key={`${section.key}-${path}`}>
                        <div className="flex items-center gap-2 mb-2">
                          <FileCode size={13} className="text-text-muted" />
                          <span className="font-code text-[12px] text-text-muted">{path.split('/').slice(0, -1).join('/')}/</span>
                          <span className="font-code text-[12px] text-text-primary font-medium">{path.split('/').pop()}</span>
                        </div>
                        <div className="space-y-2 ml-5">
                          {fileComments.map(comment => (
                            <div key={comment.id}>
                              <span className="text-[10px] text-text-muted font-code">L{comment.line_number}</span>
                              <CommentCard
                                comment={comment}
                                onFeedback={action => handleFeedback(comment.id, action)}
                                onLifecycleChange={status => handleLifecycleChange(comment.id, status)}
                                isActive={activeVisibleCommentId === comment.id}
                                onActivate={() => setActiveCommentId(comment.id)}
                              />
                            </div>
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                </section>
              ))}

              {filteredComments.length === 0 && (
                <div className="text-center py-16 text-text-muted">
                  {showOnlyBlockers
                    ? blockerCount === 0
                      ? 'No open blockers remain in this review.'
                      : 'No blockers match the current filters.'
                    : review.comments.length === 0
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

function SummaryCard(
  {
    label,
    value,
    hint,
    tone,
  }: {
    label: string
    value: number
    hint: string
    tone: 'error' | 'warning' | 'accent' | 'muted'
  },
) {
  const toneStyles = {
    error: 'border-sev-error/20 bg-sev-error/5 text-sev-error',
    warning: 'border-sev-warning/20 bg-sev-warning/5 text-sev-warning',
    accent: 'border-accent/20 bg-accent/5 text-accent',
    muted: 'border-border bg-surface-1 text-text-muted',
  }

  return (
    <div className={`rounded-lg border px-3 py-2 ${toneStyles[tone]}`}>
      <div className="text-[10px] font-code uppercase tracking-[0.08em]">{label}</div>
      <div className="mt-1 text-lg font-semibold text-text-primary">{value}</div>
      <div className="text-[11px]">{hint}</div>
    </div>
  )
}

function ProgressBanner({ progress, comments }: { progress?: import('../api/types').ReviewProgress, comments: number }) {
  if (!progress) {
    return (
      <div className="px-4 py-3 border-b border-accent/30 bg-accent/5 flex items-center gap-3">
        <Loader2 className="animate-spin text-accent" size={14} />
        <span className="text-[12px] text-text-primary">Analyzing code...</span>
      </div>
    )
  }

  const pct = progress.files_total > 0
    ? ((progress.files_completed + progress.files_skipped) / progress.files_total) * 100
    : 0

  return (
    <div className="border-b border-accent/30 bg-accent/5">
      {/* Thin progress bar */}
      <div className="h-0.5 bg-surface-2">
        <div
          className="h-full bg-accent transition-all duration-500 ease-out"
          style={{ width: `${pct}%` }}
        />
      </div>
      <div className="px-4 py-2.5 flex items-center gap-3">
        <Loader2 className="animate-spin text-accent flex-shrink-0" size={14} />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 text-[12px]">
            <span className="text-text-primary font-medium">
              {progress.files_completed + progress.files_skipped}/{progress.files_total} files
            </span>
            {progress.current_file && (
              <span className="text-text-muted font-code truncate" title={progress.current_file}>
                {progress.current_file}
              </span>
            )}
          </div>
        </div>
        <div className="flex items-center gap-3 text-[11px] text-text-muted flex-shrink-0">
          {comments > 0 && (
            <span className="text-accent font-medium">{comments} findings</span>
          )}
          <span>{formatDuration(progress.elapsed_ms)} elapsed</span>
          {progress.estimated_remaining_ms != null && progress.estimated_remaining_ms > 0 && (
            <span>~{formatDuration(progress.estimated_remaining_ms)} left</span>
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
  const [showFileMetrics, setShowFileMetrics] = useState(false)
  const [showHotspots, setShowHotspots] = useState(false)
  const [showAgentActivity, setShowAgentActivity] = useState(false)

  const fmtTokens = (n: number) => n.toLocaleString()

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

        {/* Model + Tokens */}
        <div>
          <div className="text-text-muted mb-1.5 uppercase tracking-wider font-medium">Model</div>
          <div className="text-text-secondary space-y-0.5">
            <div className="font-code text-text-primary truncate" title={event.model}>{event.model}</div>
            {event.provider && <div><span className="text-text-muted">via</span> {event.provider}</div>}
            {event.base_url && <div className="font-code text-text-muted truncate text-[10px]" title={event.base_url}>{event.base_url}</div>}
            {event.agent_iterations != null && event.agent_iterations > 0 && (
              <div className="mt-1 pt-1 border-t border-border/50">
                <div className="text-accent font-medium">Agent ({event.agent_iterations} iterations)</div>
              </div>
            )}
            {event.tokens_total != null && event.tokens_total > 0 && (
              <div className="mt-1 pt-1 border-t border-border/50">
                <div><span className="text-text-muted">Tokens:</span> <span className="font-code text-text-primary">{fmtTokens(event.tokens_total)}</span></div>
                {event.tokens_prompt != null && (
                  <div className="text-[10px] text-text-muted font-code">
                    {fmtTokens(event.tokens_prompt)} prompt / {fmtTokens(event.tokens_completion ?? 0)} completion
                  </div>
                )}
              </div>
            )}
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
            {event.convention_suppressed != null && event.convention_suppressed > 0 && (
              <div className="text-text-muted">{event.convention_suppressed} suppressed by conventions</div>
            )}
            {event.comments_by_pass && Object.keys(event.comments_by_pass).length > 0 && (
              <div className="mt-1 pt-1 border-t border-border/50">
                {Object.entries(event.comments_by_pass).map(([pass, n]) => (
                  <div key={pass} className="text-text-muted">
                    <span className="font-code text-text-primary">{n}</span> {pass}
                  </div>
                ))}
              </div>
            )}
            {event.github_posted && (
              <div className="flex items-center gap-1 mt-1 text-accent">
                <GitBranch size={10} />
                <span>Posted to PR #{event.github_pr}</span>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Hotspots */}
      {event.hotspot_details && event.hotspot_details.length > 0 && (
        <div className="mt-2 pt-2 border-t border-border/50">
          <button onClick={() => setShowHotspots(!showHotspots)} className="text-[11px] text-text-muted hover:text-text-secondary flex items-center gap-1">
            <span className={`transition-transform ${showHotspots ? 'rotate-90' : ''}`}>&#9654;</span>
            High Risk Files ({event.hotspot_details.length})
          </button>
          {showHotspots && (
            <div className="mt-1 space-y-1">
              {event.hotspot_details.map(h => (
                <div key={h.file_path} className="flex items-center gap-2 text-[11px]">
                  <span className="font-code text-text-primary truncate flex-1">{h.file_path}</span>
                  <span className={`font-code font-medium ${h.risk_score > 0.6 ? 'text-sev-error' : h.risk_score > 0.3 ? 'text-sev-warning' : 'text-sev-info'}`}>
                    {(h.risk_score * 100).toFixed(0)}%
                  </span>
                  <span className="text-text-muted truncate max-w-48">{h.reasons.join(', ')}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Agent activity */}
      {event.agent_iterations != null && event.agent_iterations > 0 && event.agent_tool_calls && (
        <div className="mt-2 pt-2 border-t border-border/50">
          <button onClick={() => setShowAgentActivity(!showAgentActivity)} className="text-[11px] text-text-muted hover:text-text-secondary flex items-center gap-1">
            <span className={`transition-transform ${showAgentActivity ? 'rotate-90' : ''}`}>&#9654;</span>
            Agent Activity ({event.agent_iterations} iterations, {event.agent_tool_calls.length} tool calls)
          </button>
          {showAgentActivity && (
            <table className="mt-1 w-full text-[11px]">
              <thead>
                <tr className="text-text-muted text-left">
                  <th className="font-medium pr-3 py-0.5">Iteration</th>
                  <th className="font-medium pr-3 py-0.5">Tool</th>
                  <th className="font-medium py-0.5 text-right">Duration</th>
                </tr>
              </thead>
              <tbody>
                {event.agent_tool_calls.map((tc, i) => (
                  <tr key={i} className="text-text-secondary">
                    <td className="font-code text-text-muted pr-3 py-0.5">#{tc.iteration + 1}</td>
                    <td className="font-code text-text-primary pr-3 py-0.5">{tc.tool_name}</td>
                    <td className="font-code text-right py-0.5">{formatDuration(tc.duration_ms)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}

      {/* Per-file breakdown */}
      {event.file_metrics && event.file_metrics.length > 0 && (
        <div className="mt-2 pt-2 border-t border-border/50">
          <button onClick={() => setShowFileMetrics(!showFileMetrics)} className="text-[11px] text-text-muted hover:text-text-secondary flex items-center gap-1">
            <span className={`transition-transform ${showFileMetrics ? 'rotate-90' : ''}`}>&#9654;</span>
            Per-File Breakdown ({event.file_metrics.length} files)
          </button>
          {showFileMetrics && (
            <table className="mt-1 w-full text-[11px]">
              <thead>
                <tr className="text-text-muted text-left">
                  <th className="font-medium pr-3 py-0.5">File</th>
                  <th className="font-medium pr-3 py-0.5 text-right">Latency</th>
                  <th className="font-medium pr-3 py-0.5 text-right">Tokens</th>
                  <th className="font-medium py-0.5 text-right">Comments</th>
                </tr>
              </thead>
              <tbody>
                {event.file_metrics.map(m => (
                  <tr key={m.file_path} className="text-text-secondary">
                    <td className="font-code text-text-primary truncate max-w-64 pr-3 py-0.5">{m.file_path}</td>
                    <td className="font-code text-right pr-3 py-0.5">{formatDuration(m.latency_ms)}</td>
                    <td className="font-code text-right pr-3 py-0.5">{fmtTokens(m.total_tokens)}</td>
                    <td className="font-code text-right py-0.5">{m.comment_count}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </div>
  )
}
