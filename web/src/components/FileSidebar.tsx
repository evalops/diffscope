import { useMemo } from 'react'
import { FileCode, FilePlus, FileX, FileSymlink } from 'lucide-react'
import type { DiffFile, Comment, Severity } from '../api/types'
import { getFileName, getFileDir } from '../lib/parseDiff'

interface Props {
  files: DiffFile[]
  comments: Comment[]
  selectedFile: string | null
  onSelectFile: (file: string | null) => void
}

type FileStats = {
  total: number
  worst: number
  openBlockers: number
  openInformational: number
  resolved: number
  dismissed: number
}

const statusIcon = {
  added: FilePlus,
  deleted: FileX,
  renamed: FileSymlink,
  modified: FileCode,
}

const statusDot = {
  added: 'bg-sev-suggestion',
  deleted: 'bg-sev-error',
  renamed: 'bg-sev-info',
  modified: 'bg-text-muted',
}

const sevRank: Record<Severity, number> = { Error: 0, Warning: 1, Info: 2, Suggestion: 3 }

function buildReadinessLabel(stats: FileStats): { label: string; tone: string } {
  if (stats.openBlockers > 0) {
    return {
      label: `${stats.openBlockers} blocker${stats.openBlockers === 1 ? '' : 's'}`,
      tone: 'text-sev-warning',
    }
  }

  if (stats.openInformational > 0) {
    return {
      label: 'Info only',
      tone: 'text-accent',
    }
  }

  return {
    label: 'Clear',
    tone: 'text-sev-suggestion',
  }
}

function buildReadinessDetails(stats: FileStats): string | null {
  const details: string[] = []

  if (stats.openBlockers > 0 && stats.openInformational > 0) {
    details.push(`${stats.openInformational} info`)
  }
  if (stats.resolved > 0) {
    details.push(`${stats.resolved} resolved`)
  }
  if (stats.dismissed > 0) {
    details.push(`${stats.dismissed} dismissed`)
  }

  return details.length > 0 ? details.join(' • ') : null
}

export function FileSidebar({ files, comments, selectedFile, onSelectFile }: Props) {
  const fileStats = useMemo(() => {
    const stats = new Map<string, FileStats>()
    for (const c of comments) {
      const path = c.file_path.replace(/^\.\//, '')
      const existing = stats.get(path) || {
        total: 0,
        worst: 4,
        openBlockers: 0,
        openInformational: 0,
        resolved: 0,
        dismissed: 0,
      }
      const lifecycle = c.status ?? 'Open'

      existing.total++
      existing.worst = Math.min(existing.worst, sevRank[c.severity])

      if (lifecycle === 'Resolved') {
        existing.resolved++
      } else if (lifecycle === 'Dismissed') {
        existing.dismissed++
      } else if (c.severity === 'Error' || c.severity === 'Warning') {
        existing.openBlockers++
      } else {
        existing.openInformational++
      }

      stats.set(path, existing)
    }
    return stats
  }, [comments])

  const countColor = (worst: number) =>
    worst === 0 ? 'bg-sev-error' :
    worst === 1 ? 'bg-sev-warning' :
    worst === 2 ? 'bg-sev-info' :
    'bg-sev-suggestion'

  // Group by directory
  const grouped = useMemo(() => {
    const dirs = new Map<string, DiffFile[]>()
    for (const f of files) {
      const dir = getFileDir(f.path) || '.'
      if (!dirs.has(dir)) dirs.set(dir, [])
      dirs.get(dir)!.push(f)
    }
    return dirs
  }, [files])

  const totalComments = comments.length

  return (
    <div className="w-56 border-r border-border bg-surface-1 overflow-y-auto flex flex-col">
      <div className="p-3 border-b border-border">
        <div className="flex items-center justify-between">
          <span className="text-[11px] font-semibold text-text-muted uppercase tracking-wider">Files</span>
          <span className="text-[11px] text-text-muted">{files.length}</span>
        </div>
      </div>

      {/* All files button */}
      <button
        onClick={() => onSelectFile(null)}
        className={`w-full text-left px-3 py-1.5 text-[12px] transition-colors border-b border-border-subtle ${
          selectedFile === null
            ? 'bg-accent/10 text-accent'
            : 'text-text-secondary hover:bg-surface-2'
        }`}
      >
        All files
        {totalComments > 0 && (
          <span className="ml-1.5 text-[10px] text-text-muted">({totalComments} findings)</span>
        )}
      </button>

      {/* File tree */}
      <div className="flex-1 py-1">
        {[...grouped.entries()].map(([dir, dirFiles]) => (
          <div key={dir}>
            {grouped.size > 1 && (
              <div className="px-3 py-1 text-[10px] text-text-muted font-code truncate">
                {dir}/
              </div>
            )}
            {dirFiles.map((file) => {
              const Icon = statusIcon[file.status]
              const stats = fileStats.get(file.path)
              const isSelected = selectedFile === file.path
              const readiness = stats ? buildReadinessLabel(stats) : null
              const readinessDetails = stats ? buildReadinessDetails(stats) : null

              return (
                <button
                  key={file.path}
                  onClick={() => onSelectFile(file.path)}
                  className={`w-full text-left px-3 py-1.5 flex items-start gap-1.5 text-[12px] transition-colors ${
                    isSelected
                      ? 'bg-accent/10 text-accent'
                      : 'text-text-secondary hover:bg-surface-2 hover:text-text-primary'
                  }`}
                >
                  <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${statusDot[file.status]}`} />
                  <Icon size={12} className="shrink-0 text-text-muted" />
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-code">{getFileName(file.path)}</div>
                    {readiness && (
                      <div className="mt-0.5 flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[10px]">
                        <span className={`font-medium ${readiness.tone}`}>{readiness.label}</span>
                        {readinessDetails && (
                          <span className="text-text-muted">{readinessDetails}</span>
                        )}
                      </div>
                    )}
                  </div>
                  {stats && (
                    <span className={`ml-auto shrink-0 text-[10px] text-white px-1 py-0 rounded ${countColor(stats.worst)}`}>
                      {stats.total}
                    </span>
                  )}
                </button>
              )
            })}
          </div>
        ))}
      </div>
    </div>
  )
}
