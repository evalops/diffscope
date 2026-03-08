import { Check, X, ChevronDown, ChevronRight, Copy, CheckCheck } from 'lucide-react'
import { useState } from 'react'
import type { Comment } from '../api/types'
import { SeverityBadge } from './SeverityBadge'

const severityBorder: Record<string, string> = {
  Error: 'border-l-sev-error',
  Warning: 'border-l-sev-warning',
  Info: 'border-l-sev-info',
  Suggestion: 'border-l-sev-suggestion',
}

interface Props {
  comment: Comment
  variant?: 'card' | 'inline'
  onFeedback?: (action: 'accept' | 'reject') => void
}

export function CommentCard({ comment, variant = 'card', onFeedback }: Props) {
  const [expanded, setExpanded] = useState(false)
  const [copied, setCopied] = useState(false)

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

  return (
    <div className={rootClass}>
      {/* Header */}
      <div className={`flex items-center gap-2 px-3 ${isInline ? 'py-1.5 border-b border-border-subtle' : 'py-2'}`}>
        <SeverityBadge severity={comment.severity} />
        <span className="text-[11px] text-text-muted">{comment.category}</span>
        <span className="text-[10px] text-text-muted/60">
          {Math.round(comment.confidence * 100)}%
        </span>

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
                className="p-1 rounded hover:bg-sev-suggestion/10 text-text-muted hover:text-sev-suggestion transition-colors"
                title="Accept finding"
              >
                <Check size={13} />
              </button>
              <button
                onClick={() => onFeedback('reject')}
                className="p-1 rounded hover:bg-sev-error/10 text-text-muted hover:text-sev-error transition-colors"
                title="Dismiss finding"
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
