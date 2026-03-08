import type { DiffFile, DiffHunk } from '../api/types'

export function parseDiff(raw: string): DiffFile[] {
  const files: DiffFile[] = []
  const fileSections = raw.split(/^diff --git /m).filter(Boolean)

  for (const section of fileSections) {
    const lines = section.split('\n')

    // Extract paths from "a/path b/path"
    const headerMatch = lines[0]?.match(/a\/(.+?) b\/(.+)/)
    if (!headerMatch) continue

    const oldPath = headerMatch[1]
    const newPath = headerMatch[2]

    // Determine file status
    let status: DiffFile['status'] = 'modified'
    if (section.includes('new file mode')) status = 'added'
    else if (section.includes('deleted file mode')) status = 'deleted'
    else if (section.includes('rename from')) status = 'renamed'

    const hunks: DiffHunk[] = []
    const hunkRegex = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/

    let currentHunk: DiffHunk | null = null
    let oldLine = 0
    let newLine = 0

    for (const line of lines) {
      const hunkMatch = line.match(hunkRegex)
      if (hunkMatch) {
        currentHunk = {
          header: line,
          oldStart: parseInt(hunkMatch[1]),
          oldCount: parseInt(hunkMatch[2] ?? '1'),
          newStart: parseInt(hunkMatch[3]),
          newCount: parseInt(hunkMatch[4] ?? '1'),
          lines: [],
        }
        oldLine = currentHunk.oldStart
        newLine = currentHunk.newStart
        hunks.push(currentHunk)
        continue
      }

      if (!currentHunk) continue

      if (line.startsWith('+')) {
        currentHunk.lines.push({
          type: 'add',
          content: line.slice(1),
          newNumber: newLine++,
        })
      } else if (line.startsWith('-')) {
        currentHunk.lines.push({
          type: 'del',
          content: line.slice(1),
          oldNumber: oldLine++,
        })
      } else if (line.startsWith(' ') || line === '') {
        if (currentHunk.lines.length > 0 || line.startsWith(' ')) {
          currentHunk.lines.push({
            type: 'context',
            content: line.startsWith(' ') ? line.slice(1) : line,
            oldNumber: oldLine++,
            newNumber: newLine++,
          })
        }
      }
    }

    files.push({
      path: newPath,
      oldPath: oldPath !== newPath ? oldPath : undefined,
      hunks,
      status,
    })
  }

  return files
}

export function getFileExtension(path: string): string {
  const parts = path.split('.')
  return parts.length > 1 ? parts.pop()! : ''
}

export function getFileName(path: string): string {
  return path.split('/').pop() || path
}

export function getFileDir(path: string): string {
  const parts = path.split('/')
  parts.pop()
  return parts.join('/')
}
