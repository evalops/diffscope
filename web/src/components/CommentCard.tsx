import { Check, X, ChevronDown, ChevronRight, Copy, CheckCheck } from 'lucide-react'
import { useState } from 'react'
import type { Comment, CommentOutcome } from '../api/types'
import { getCommentOutcomes } from '../lib/commentOutcomes'
import { SeverityBadge } from './SeverityBadge'

const severityBorder: Record<string, string> = {
  Error: 'border-l-sev-error',
  Warning: 'border-l-sev-warning',
  Info: 'border-l-sev-info',
  Suggestion: 'border-l-sev-suggestion',
}

const lifecycleBadge: Record<string, string> = {
  Open: 'bg-accent/10 text-accent border border-accent/20',
  Resolved: 'bg-sev-suggestion/10 text-sev-suggestion border border-sev-suggestion/20',
  Dismissed: 'bg-text-muted/10 text-text-muted border border-border',
}

const outcomeBadge: Record<CommentOutcome, { label: string; className: string }> = {
  new: {
    label: 'New',
    className: 'bg-accent/10 text-accent border border-accent/20',
  },
  accepted: {
    label: 'Accepted',
    className: 'bg-sev-suggestion/10 text-sev-suggestion border border-sev-suggestion/20',
  },
  rejected: {
    label: 'Rejected',
    className: 'bg-sev-error/10 text-sev-error border border-sev-error/20',
  },
  addressed: {
    label: 'Addressed',
    className: 'bg-sev-suggestion/10 text-sev-suggestion border border-sev-suggestion/20',
  },
  stale: {
    label: 'Stale',
    className: 'bg-accent/10 text-accent border border-accent/20',
  },
  auto_fixed: {
    label: 'Auto fixed',
    className: 'bg-sev-suggestion/10 text-sev-suggestion border border-sev-suggestion/20',
  },
}

interface Props {
  comment: Comment
  variant?: 'card' | 'inline'
  onFeedback?: (action: 'accept' | 'reject', explanation?: string) => void
  onLifecycleChange?: (status: 'open' | 'resolved' | 'dismissed') => void
  isActive?: boolean
  onActivate?: () => void
}

export function CommentCard({ comment, variant = 'card', onFeedback, onLifecycleChange, isActive = false, onActivate }: Props) {
  const [expanded, setExpanded] = useState(false)
  const [copied, setCopied] = useState(false)
  const [editingFeedbackNote, setEditingFeedbackNote] = useState(false)
  const [feedbackNoteDraft, setFeedbackNoteDraft] = useState('')
  const accepted = comment.feedback === 'accept'
  const rejected = comment.feedback === 'reject'
  const lifecycle = comment.status ?? 'Open'
  const outcomes = getCommentOutcomes(comment)

  const copyCode = () => {
    if (comment.code_suggestion?.suggested_code) {
      navigator.clipboard.writeText(comment.code_suggestion.suggested_code)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    }
  }

  const isInline = variant === 'inline'
  const rootClass = isInline
    ? `mx-3 my-1.5 border border-border rounded-md bg-surface-2 border-l-2 ${severityBorder[comment.severity]}`
    : 'border border-border rounded-md bg-surface-2'
  const feedbackNoteLabel = accepted
    ? 'Why was this finding useful?'
    : 'Why should similar findings be suppressed?'
  const hasFeedbackNote = Boolean(comment.feedback_explanation?.trim())
  const canSaveFeedbackNote = feedbackNoteDraft.trim().length > 0 || hasFeedbackNote

  const startEditingFeedbackNote = () => {
    setFeedbackNoteDraft(comment.feedback_explanation ?? '')
    setEditingFeedbackNote(true)
  }

  const saveFeedbackNote = () => {
    if (!comment.feedback || !onFeedback) return
    onFeedback(comment.feedback, feedbackNoteDraft.trim())
    setEditingFeedbackNote(false)
  }

  const cancelFeedbackNote = () => {
    setFeedbackNoteDraft(comment.feedback_explanation ?? '')
    setEditingFeedbackNote(false)
  }

  return (
    <div
      className={`${rootClass} ${isActive ? 'ring-1 ring-accent/50' : ''} focus:outline-none focus-visible:ring-1 focus-visible:ring-accent/50`}
      data-review-comment-card="true"
      data-comment-id={comment.id}
      tabIndex={0}
      onFocus={() => onActivate?.()}
      onMouseDown={() => onActivate?.()}
    >
      {/* Header */}
      <div className={`flex items-center gap-2 px-3 ${isInline ? 'py-1.5 border-b border-border-subtle' : 'py-2'}`}>
        <SeverityBadge severity={comment.severity} />
        <span className="text-[11px] text-text-muted">{comment.category}</span>
        <span className="text-[10px] text-text-muted/60">
          {Math.round(comment.confidence * 100)}%
        </span>
        <span className={`text-[10px] px-1.5 py-0.5 rounded font-medium ${lifecycleBadge[lifecycle] ?? lifecycleBadge.Open}`}>
          {lifecycle}
        </span>
        {outcomes.map((outcome) => (
          <span key={outcome} className={`text-[10px] px-1.5 py-0.5 rounded font-medium ${outcomeBadge[outcome].className}`}>
            {outcomeBadge[outcome].label}
          </span>
        ))}

        <div className="ml-auto flex items-center gap-1">
          {comment.fix_effort && (
            <span className={`text-[10px] px-1.5 py-0.5 rounded font-medium ${
              comment.fix_effort === 'Low' ? 'text-sev-suggestion bg-sev-suggestion/10' :
              comment.fix_effort === 'Medium' ? 'text-sev-warning bg-sev-warning/10' :
              'text-sev-error bg-sev-error/10'
            }`}>
              {comment.fix_effort}
            </span>
          )}
          {onFeedback && (
            <>
              <button
                onClick={() => onFeedback('accept')}
                className={`p-1 rounded transition-colors ${
                  accepted
                    ? 'bg-sev-suggestion/15 text-sev-suggestion'
                    : 'text-text-muted hover:bg-sev-suggestion/10 hover:text-sev-suggestion'
                }`}
                title={accepted ? 'Accepted finding' : 'Accept finding'}
                aria-pressed={accepted}
              >
                <Check size={13} />
              </button>
              <button
                onClick={() => onFeedback('reject')}
                className={`p-1 rounded transition-colors ${
                  rejected
                    ? 'bg-sev-error/15 text-sev-error'
                    : 'text-text-muted hover:bg-sev-error/10 hover:text-sev-error'
                }`}
                title={rejected ? 'Dismissed finding' : 'Dismiss finding'}
                aria-pressed={rejected}
              >
                <X size={13} />
              </button>
            </>
          )}
        </div>
      </div>

      {/* Content */}
      <div className="px-3 py-2">
        <p className="text-[12.5px] text-text-primary leading-relaxed">{comment.content}</p>

        {comment.suggestion && (
          <p className="text-[12px] text-text-secondary mt-1.5 leading-relaxed italic">{comment.suggestion}</p>
        )}

        {comment.tags.length > 0 && (
          <div className="flex items-center gap-1 mt-2">
            {comment.tags.map(tag => (
              <span key={tag} className="px-1.5 py-0.5 bg-surface-3 text-text-muted rounded text-[10px] font-code">
                {tag}
              </span>
            ))}
          </div>
        )}

        {comment.feedback && (onFeedback || hasFeedbackNote) && (
          <div className="mt-3 rounded border border-border-subtle bg-surface/40 px-3 py-2">
            <div className="flex items-center justify-between gap-2">
              <span className="text-[10px] text-text-muted font-code">Feedback note</span>
              {!editingFeedbackNote && onFeedback && (
                <button
                  type="button"
                  onClick={startEditingFeedbackNote}
                  className="text-[10px] text-accent hover:text-accent-dim"
                >
                  {hasFeedbackNote ? 'Edit note' : 'Add note'}
                </button>
              )}
            </div>

            {editingFeedbackNote ? (
              <div className="mt-2 space-y-2">
                <textarea
                  value={feedbackNoteDraft}
                  onChange={(event) => setFeedbackNoteDraft(event.target.value)}
                  placeholder={feedbackNoteLabel}
                  rows={3}
                  className="w-full rounded border border-border bg-surface-2 px-2 py-1.5 text-[11px] text-text-primary outline-none focus:border-accent"
                />
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    onClick={saveFeedbackNote}
                    disabled={!canSaveFeedbackNote}
                    className="rounded border border-accent/30 bg-accent/10 px-2 py-1 text-[10px] font-medium text-accent disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    Save note
                  </button>
                  <button
                    type="button"
                    onClick={cancelFeedbackNote}
                    className="rounded border border-border px-2 py-1 text-[10px] text-text-muted hover:text-text-primary"
                  >
                    Cancel
                  </button>
                </div>
              </div>
            ) : hasFeedbackNote ? (
              <p className="mt-2 text-[11px] leading-relaxed text-text-secondary">
                {comment.feedback_explanation}
              </p>
            ) : null}
          </div>
        )}

        {onLifecycleChange && (
          <div className="mt-3 pt-2 border-t border-border-subtle flex items-center gap-2">
            <span className="text-[10px] text-text-muted font-code">Workflow</span>
            {lifecycle === 'Open' ? (
              <>
                <button
                  onClick={() => onLifecycleChange('resolved')}
                  className="px-2 py-0.5 rounded text-[10px] font-medium bg-sev-suggestion/10 text-sev-suggestion border border-sev-suggestion/20 hover:bg-sev-suggestion/15 transition-colors"
                  title="Mark finding as resolved"
                >
                  Resolve
                </button>
                <button
                  onClick={() => onLifecycleChange('dismissed')}
                  className="px-2 py-0.5 rounded text-[10px] font-medium bg-surface-3 text-text-muted border border-border hover:text-text-primary transition-colors"
                  title="Dismiss finding from merge readiness"
                >
                  Dismiss
                </button>
              </>
            ) : (
              <button
                onClick={() => onLifecycleChange('open')}
                className="px-2 py-0.5 rounded text-[10px] font-medium bg-accent/10 text-accent border border-accent/20 hover:bg-accent/15 transition-colors"
                title="Reopen finding"
              >
                Reopen
              </button>
            )}
          </div>
        )}
      </div>

      {/* Code suggestion */}
      {comment.code_suggestion && (
        <div className="border-t border-border-subtle">
          <button
            onClick={() => setExpanded(!expanded)}
            className="flex items-center gap-1.5 px-3 py-1.5 text-[11px] text-accent hover:text-accent-dim w-full text-left"
          >
            {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            Suggested fix
          </button>
          {expanded && (
            <div className="relative">
              <button
                onClick={copyCode}
                className="absolute top-2 right-2 p-1 rounded bg-surface-3 hover:bg-border text-text-muted transition-colors"
                title="Copy suggested code"
              >
                {copied ? <CheckCheck size={12} className="text-sev-suggestion" /> : <Copy size={12} />}
              </button>
              <pre className="px-3 py-2 text-[11.5px] font-code overflow-x-auto bg-surface/50 leading-relaxed text-text-primary">
                {comment.code_suggestion.diff || comment.code_suggestion.suggested_code}
              </pre>
              {comment.code_suggestion.explanation && (
                <p className="px-3 py-1.5 text-[11px] text-text-muted border-t border-border-subtle">
                  {comment.code_suggestion.explanation}
                </p>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
