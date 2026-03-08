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

export function FileSidebar({ files, comments, selectedFile, onSelectFile }: Props) {
  const fileStats = useMemo(() => {
    const stats = new Map<string, { count: number; worst: number }>()
    for (const c of comments) {
      const path = c.file_path.replace(/^\.\//, '')
      const existing = stats.get(path) || { count: 0, worst: 4 }
      existing.count++
      existing.worst = Math.min(existing.worst, sevRank[c.severity])
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

              return (
                <button
                  key={file.path}
                  onClick={() => onSelectFile(file.path)}
                  className={`w-full text-left px-3 py-1 flex items-center gap-1.5 text-[12px] transition-colors ${
                    isSelected
                      ? 'bg-accent/10 text-accent'
                      : 'text-text-secondary hover:bg-surface-2 hover:text-text-primary'
                  }`}
                >
                  <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${statusDot[file.status]}`} />
                  <Icon size={12} className="shrink-0 text-text-muted" />
                  <span className="truncate font-code">{getFileName(file.path)}</span>
                  {stats && (
                    <span className={`ml-auto shrink-0 text-[10px] text-white px-1 py-0 rounded ${countColor(stats.worst)}`}>
                      {stats.count}
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
