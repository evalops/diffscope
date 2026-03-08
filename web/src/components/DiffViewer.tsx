import { useState, useMemo } from 'react'
import { ChevronDown, ChevronRight, FileCode, FilePlus, FileX, FileSymlink } from 'lucide-react'
import type { DiffFile, DiffHunk, DiffLine, Comment, Severity } from '../api/types'
import { SeverityBadge } from './SeverityBadge'
import { CommentCard } from './CommentCard'
import { getFileName, getFileDir } from '../lib/parseDiff'
import { tokenize, TOKEN_CLASSES } from '../lib/highlight'

interface Props {
  files: DiffFile[]
  comments: Comment[]
  onFeedback?: (commentId: string, action: 'accept' | 'reject') => void
}

const statusIcon = {
  added: FilePlus,
  deleted: FileX,
  renamed: FileSymlink,
  modified: FileCode,
}

const statusColor = {
  added: 'text-sev-suggestion',
  deleted: 'text-sev-error',
  renamed: 'text-sev-info',
  modified: 'text-text-secondary',
}

export function DiffViewer({ files, comments, onFeedback }: Props) {
  const [collapsedFiles, setCollapsedFiles] = useState<Set<string>>(new Set())

  const commentsByFile = useMemo(() => {
    const map = new Map<string, Map<number, Comment[]>>()
    for (const c of comments) {
      const filePath = c.file_path.replace(/^\.\//, '')
      if (!map.has(filePath)) map.set(filePath, new Map())
      const fileMap = map.get(filePath)!
      if (!fileMap.has(c.line_number)) fileMap.set(c.line_number, [])
      fileMap.get(c.line_number)!.push(c)
    }
    return map
  }, [comments])

  const toggleFile = (path: string) => {
    const next = new Set(collapsedFiles)
    if (next.has(path)) next.delete(path)
    else next.add(path)
    setCollapsedFiles(next)
  }

  const fileCommentSummary = (path: string) => {
    const fileComments = commentsByFile.get(path)
    if (!fileComments) return null
    const counts: Record<Severity, number> = { Error: 0, Warning: 0, Info: 0, Suggestion: 0 }
    for (const lineComments of fileComments.values()) {
      for (const c of lineComments) counts[c.severity]++
    }
    return counts
  }

  return (
    <div className="space-y-3">
      {files.map((file) => {
        const collapsed = collapsedFiles.has(file.path)
        const Icon = statusIcon[file.status]
        const summary = fileCommentSummary(file.path)

        return (
          <div key={file.path} className="border border-border rounded-lg overflow-hidden bg-surface-1">
            {/* File header */}
            <button
              onClick={() => toggleFile(file.path)}
              className="w-full flex items-center gap-2 px-3 py-2 bg-surface-2 hover:bg-surface-3 transition-colors text-left"
            >
              {collapsed
                ? <ChevronRight size={14} className="text-text-muted shrink-0" />
                : <ChevronDown size={14} className="text-text-muted shrink-0" />
              }
              <Icon size={14} className={`shrink-0 ${statusColor[file.status]}`} />
              <span className="font-code text-[12px] text-text-muted">{getFileDir(file.path)}/</span>
              <span className="font-code text-[12px] text-text-primary font-medium">{getFileName(file.path)}</span>
              {file.oldPath && (
                <span className="font-code text-[11px] text-text-muted ml-1">
                  (from {getFileName(file.oldPath)})
                </span>
              )}

              {/* Comment counts */}
              {summary && (
                <div className="ml-auto flex items-center gap-2">
                  {summary.Error > 0 && <span className="text-[11px] text-sev-error font-medium">{summary.Error}</span>}
                  {summary.Warning > 0 && <span className="text-[11px] text-sev-warning font-medium">{summary.Warning}</span>}
                  {summary.Info > 0 && <span className="text-[11px] text-sev-info font-medium">{summary.Info}</span>}
                  {summary.Suggestion > 0 && <span className="text-[11px] text-sev-suggestion font-medium">{summary.Suggestion}</span>}
                </div>
              )}
            </button>

            {/* Diff content */}
            {!collapsed && (
              <div className="overflow-x-auto">
                {file.hunks.map((hunk, i) => (
                  <HunkView
                    key={i}
                    hunk={hunk}
                    filePath={file.path}
                    commentsByLine={commentsByFile.get(file.path)}
                    onFeedback={onFeedback}
                  />
                ))}
                {file.hunks.length === 0 && (
                  <div className="px-4 py-6 text-center text-text-muted text-sm">
                    Binary file or no visible changes
                  </div>
                )}
              </div>
            )}
          </div>
        )
      })}
    </div>
  )
}

export function HunkView({ hunk, filePath, commentsByLine, onFeedback }: {
  hunk: DiffHunk
  filePath: string
  commentsByLine?: Map<number, Comment[]>
  onFeedback?: (commentId: string, action: 'accept' | 'reject') => void
}) {
  return (
    <div>
      {/* Hunk header */}
      <div className="px-3 py-1 bg-diff-hunk/50 border-y border-border-subtle font-code text-[11px] text-accent/70">
        {hunk.header}
      </div>

      {/* Lines */}
      <table className="w-full border-collapse font-code text-[12px] leading-[20px]">
        <tbody>
          {hunk.lines.map((line, i) => {
            const lineNum = line.type === 'del' ? line.oldNumber : line.newNumber
            const lineComments = lineNum ? commentsByLine?.get(lineNum) : undefined

            return (
              <LineRow
                key={i}
                line={line}
                comments={lineComments}
                filePath={filePath}
                onFeedback={onFeedback}
              />
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

export function LineRow({ line, comments, filePath, onFeedback }: {
  line: DiffLine
  comments?: Comment[]
  filePath: string
  onFeedback?: (commentId: string, action: 'accept' | 'reject') => void
}) {
  const bgClass =
    line.type === 'add' ? 'bg-diff-add-bg' :
    line.type === 'del' ? 'bg-diff-del-bg' :
    ''

  const gutterBg =
    line.type === 'add' ? 'bg-diff-add-line/30' :
    line.type === 'del' ? 'bg-diff-del-line/30' :
    ''

  const prefix = line.type === 'add' ? '+' : line.type === 'del' ? '-' : ' '

  // Filter comments to match this file
  const relevantComments = comments?.filter(c => {
    const normalized = c.file_path.replace(/^\.\//, '')
    return normalized === filePath
  })

  return (
    <>
      <tr className={`${bgClass} hover:brightness-125 transition-colors group`}>
        <td className={`w-[1px] whitespace-nowrap text-right px-2 select-none text-text-muted/50 ${gutterBg} border-r border-border-subtle`}>
          {line.oldNumber ?? ''}
        </td>
        <td className={`w-[1px] whitespace-nowrap text-right px-2 select-none text-text-muted/50 ${gutterBg} border-r border-border-subtle`}>
          {line.newNumber ?? ''}
        </td>
        <td className="w-[1px] whitespace-nowrap px-1 select-none text-text-muted/40">{prefix}</td>
        <td className="whitespace-pre pr-4">
          <HighlightedLine content={line.content} />
          {relevantComments && relevantComments.length > 0 && (
            <span className="ml-2 inline-flex items-center">
              {relevantComments.map(c => (
                <SeverityBadge key={c.id} severity={c.severity} />
              ))}
            </span>
          )}
        </td>
      </tr>
      {relevantComments && relevantComments.length > 0 && (
        <tr>
          <td colSpan={4} className="p-0">
            {relevantComments.map(c => (
              <CommentCard
                key={c.id}
                comment={c}
                variant="inline"
                onFeedback={onFeedback ? (action) => onFeedback(c.id, action) : undefined}
              />
            ))}
          </td>
        </tr>
      )}
    </>
  )
}

function HighlightedLine({ content }: { content: string }) {
  const tokens = tokenize(content)
  return (
    <>
      {tokens.map((t, i) =>
        t.type === 'plain'
          ? <span key={i}>{t.text}</span>
          : <span key={i} className={TOKEN_CLASSES[t.type]}>{t.text}</span>
      )}
    </>
  )
}
