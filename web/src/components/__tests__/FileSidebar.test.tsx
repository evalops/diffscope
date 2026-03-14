import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

import { FileSidebar } from '../FileSidebar'
import type { Comment, DiffFile } from '../../api/types'

function makeFile(path: string): DiffFile {
  return {
    path,
    status: 'modified',
    hunks: [],
  }
}

function makeComment(overrides: Partial<Comment> = {}): Comment {
  return {
    id: 'comment-1',
    file_path: 'src/a.ts',
    line_number: 1,
    content: 'A finding',
    severity: 'Error',
    category: 'Bug',
    confidence: 0.9,
    tags: [],
    fix_effort: 'Medium',
    status: 'Open',
    ...overrides,
  }
}

describe('FileSidebar', () => {
  it('renders file-level readiness summaries from comment lifecycle state', () => {
    render(
      <FileSidebar
        files={[makeFile('src/a.ts'), makeFile('src/b.ts'), makeFile('src/c.ts')]}
        comments={[
          makeComment(),
          makeComment({ id: 'comment-2', severity: 'Info' }),
          makeComment({ id: 'comment-3', severity: 'Warning', status: 'Resolved' }),
          makeComment({ id: 'comment-4', file_path: 'src/b.ts', severity: 'Info' }),
          makeComment({ id: 'comment-5', file_path: 'src/b.ts', severity: 'Suggestion', status: 'Dismissed' }),
          makeComment({ id: 'comment-6', file_path: 'src/c.ts', severity: 'Warning', status: 'Resolved' }),
        ]}
        selectedFile={null}
        onSelectFile={() => {}}
      />,
    )

    expect(screen.getByText('1 blocker')).toBeInTheDocument()
    expect(screen.getByText('1 info • 1 resolved')).toBeInTheDocument()
    expect(screen.getByText('Info only')).toBeInTheDocument()
    expect(screen.getByText('1 dismissed')).toBeInTheDocument()
    expect(screen.getByText('Clear')).toBeInTheDocument()
  })

  it('keeps file selection working', async () => {
    const user = userEvent.setup()
    const onSelectFile = vi.fn()

    render(
      <FileSidebar
        files={[makeFile('src/a.ts')]}
        comments={[makeComment()]}
        selectedFile={null}
        onSelectFile={onSelectFile}
      />,
    )

    await user.click(screen.getByRole('button', { name: /a.ts/i }))
    expect(onSelectFile).toHaveBeenCalledWith('src/a.ts')
  })
})
