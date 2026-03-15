import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi } from 'vitest'
import { CommentCard } from '../CommentCard'
import type { Comment } from '../../api/types'

function makeComment(overrides: Partial<Comment> = {}): Comment {
  return {
    id: 'comment-1',
    file_path: 'src/main.ts',
    line_number: 42,
    content: 'This variable is unused.',
    severity: 'Warning',
    category: 'Style',
    confidence: 0.85,
    suggestion: 'Remove the unused variable.',
    tags: ['cleanup'],
    fix_effort: 'Low',
    ...overrides,
  }
}

describe('CommentCard', () => {
  it('renders the comment content', () => {
    render(<CommentCard comment={makeComment()} />)
    expect(screen.getByText('This variable is unused.')).toBeInTheDocument()
  })

  it('renders the severity badge', () => {
    render(<CommentCard comment={makeComment({ severity: 'Error' })} />)
    expect(screen.getByText('Error')).toBeInTheDocument()
  })

  it('renders the category', () => {
    render(<CommentCard comment={makeComment({ category: 'Security' })} />)
    expect(screen.getByText('Security')).toBeInTheDocument()
  })

  it('renders the confidence percentage', () => {
    render(<CommentCard comment={makeComment({ confidence: 0.92 })} />)
    expect(screen.getByText('92%')).toBeInTheDocument()
  })

  it('renders the suggestion text', () => {
    render(<CommentCard comment={makeComment({ suggestion: 'Use const instead.' })} />)
    expect(screen.getByText('Use const instead.')).toBeInTheDocument()
  })

  it('renders tags', () => {
    render(<CommentCard comment={makeComment({ tags: ['perf', 'critical'] })} />)
    expect(screen.getByText('perf')).toBeInTheDocument()
    expect(screen.getByText('critical')).toBeInTheDocument()
  })

  it('renders fix effort badge', () => {
    render(<CommentCard comment={makeComment({ fix_effort: 'High' })} />)
    expect(screen.getByText('High')).toBeInTheDocument()
  })

  it('renders a visible accepted badge when feedback is positive', () => {
    render(<CommentCard comment={makeComment({ feedback: 'accept' })} />)
    expect(screen.getByText('Accepted')).toBeInTheDocument()
  })

  it('renders a visible rejected badge when feedback is negative', () => {
    render(<CommentCard comment={makeComment({ feedback: 'reject' })} />)
    expect(screen.getByText('Rejected')).toBeInTheDocument()
  })

  it('renders a visible dismissed badge from lifecycle state', () => {
    render(<CommentCard comment={makeComment({ status: 'Dismissed' })} />)
    expect(screen.getByText('Dismissed')).toBeInTheDocument()
  })

  it('renders a visible new outcome badge for open findings without other outcomes', () => {
    render(<CommentCard comment={makeComment()} />)
    expect(screen.getByText('New')).toBeInTheDocument()
  })

  it('renders addressed and stale outcome badges when present', () => {
    render(
      <>
        <CommentCard comment={makeComment({ status: 'Resolved', outcomes: ['addressed'] })} />
        <CommentCard comment={makeComment({ id: 'comment-2', outcomes: ['stale'] })} />
      </>,
    )

    expect(screen.getByText('Addressed')).toBeInTheDocument()
    expect(screen.getByText('Stale')).toBeInTheDocument()
  })

  it('shows "Suggested fix" toggle when code_suggestion is present', () => {
    const comment = makeComment({
      code_suggestion: {
        original_code: 'let x = 1;',
        suggested_code: 'const x = 1;',
        explanation: 'Use const for immutable bindings.',
        diff: '- let x = 1;\n+ const x = 1;',
      },
    })
    render(<CommentCard comment={comment} />)
    expect(screen.getByText('Suggested fix')).toBeInTheDocument()
  })

  it('expands code suggestion on click', async () => {
    const user = userEvent.setup()
    const comment = makeComment({
      code_suggestion: {
        original_code: 'let x = 1;',
        suggested_code: 'const x = 1;',
        explanation: 'Use const for immutable bindings.',
        diff: '- let x = 1;\n+ const x = 1;',
      },
    })
    render(<CommentCard comment={comment} />)

    // Code suggestion content should not be visible initially
    expect(screen.queryByText('Use const for immutable bindings.')).not.toBeInTheDocument()

    // Click to expand
    await user.click(screen.getByText('Suggested fix'))

    // Now explanation and diff content should be visible
    expect(screen.getByText('Use const for immutable bindings.')).toBeInTheDocument()
    const pre = document.querySelector('pre')!
    expect(pre).toBeInTheDocument()
    expect(pre.textContent).toContain('- let x = 1;')
    expect(pre.textContent).toContain('+ const x = 1;')
  })

  it('does not show code suggestion section when none is provided', () => {
    render(<CommentCard comment={makeComment({ code_suggestion: undefined })} />)
    expect(screen.queryByText('Suggested fix')).not.toBeInTheDocument()
  })

  it('calls onFeedback with "accept" when accept button is clicked', async () => {
    const user = userEvent.setup()
    const onFeedback = vi.fn()
    render(<CommentCard comment={makeComment()} onFeedback={onFeedback} />)

    await user.click(screen.getByTitle('Accept finding'))
    expect(onFeedback).toHaveBeenCalledWith('accept')
  })

  it('calls onFeedback with "reject" when dismiss button is clicked', async () => {
    const user = userEvent.setup()
    const onFeedback = vi.fn()
    render(<CommentCard comment={makeComment()} onFeedback={onFeedback} />)

    await user.click(screen.getByTitle('Dismiss finding'))
    expect(onFeedback).toHaveBeenCalledWith('reject')
  })

  it('saves feedback notes for already labeled findings', async () => {
    const user = userEvent.setup()
    const onFeedback = vi.fn()
    render(
      <CommentCard
        comment={makeComment({ feedback: 'accept' })}
        onFeedback={onFeedback}
      />,
    )

    await user.click(screen.getByRole('button', { name: 'Add note' }))
    await user.type(screen.getByPlaceholderText('Why was this finding useful?'), 'This blocks a real auth regression.')
    await user.click(screen.getByRole('button', { name: 'Save note' }))

    expect(onFeedback).toHaveBeenCalledWith('accept', 'This blocks a real auth regression.')
  })

  it('renders persisted feedback notes', () => {
    render(
      <CommentCard
        comment={makeComment({
          feedback: 'reject',
          feedback_explanation: 'This is already enforced by the design system.',
        })}
      />,
    )

    expect(screen.getByText('This is already enforced by the design system.')).toBeInTheDocument()
  })

  it('does not render feedback buttons when onFeedback is not provided', () => {
    render(<CommentCard comment={makeComment()} />)
    expect(screen.queryByTitle('Accept finding')).not.toBeInTheDocument()
    expect(screen.queryByTitle('Dismiss finding')).not.toBeInTheDocument()
  })

  it('renders lifecycle status and actions when lifecycle controls are provided', () => {
    render(<CommentCard comment={makeComment({ status: 'Resolved' })} onLifecycleChange={() => {}} />)
    expect(screen.getByText('Resolved')).toBeInTheDocument()
    expect(screen.getByTitle('Reopen finding')).toBeInTheDocument()
  })

  it('calls onLifecycleChange when resolving an open finding', async () => {
    const user = userEvent.setup()
    const onLifecycleChange = vi.fn()
    render(<CommentCard comment={makeComment({ status: 'Open' })} onLifecycleChange={onLifecycleChange} />)

    await user.click(screen.getByTitle('Mark finding as resolved'))
    expect(onLifecycleChange).toHaveBeenCalledWith('resolved')
  })

  it('does not show suggestion text when not provided', () => {
    render(<CommentCard comment={makeComment({ suggestion: undefined })} />)
    expect(screen.queryByText('Remove the unused variable.')).not.toBeInTheDocument()
  })

  it('renders empty tags array without issue', () => {
    const { container } = render(<CommentCard comment={makeComment({ tags: [] })} />)
    // The tags container should not appear
    expect(container.querySelector('.bg-surface-3')).not.toBeInTheDocument()
  })
})
