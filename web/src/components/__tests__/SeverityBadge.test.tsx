import { render, screen } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import { SeverityBadge, SeverityDot } from '../SeverityBadge'
import type { Severity } from '../../api/types'

describe('SeverityBadge', () => {
  const severities: Severity[] = ['Error', 'Warning', 'Info', 'Suggestion']

  it.each(severities)('renders the "%s" label', (severity) => {
    render(<SeverityBadge severity={severity} />)
    expect(screen.getByText(severity)).toBeInTheDocument()
  })

  it('applies error styling for Error severity', () => {
    const { container } = render(<SeverityBadge severity="Error" />)
    const badge = container.firstElementChild!
    expect(badge.className).toContain('text-sev-error')
    const dot = badge.querySelector('span')!
    expect(dot.className).toContain('bg-sev-error')
  })

  it('applies warning styling for Warning severity', () => {
    const { container } = render(<SeverityBadge severity="Warning" />)
    const badge = container.firstElementChild!
    expect(badge.className).toContain('text-sev-warning')
    const dot = badge.querySelector('span')!
    expect(dot.className).toContain('bg-sev-warning')
  })

  it('applies info styling for Info severity', () => {
    const { container } = render(<SeverityBadge severity="Info" />)
    const badge = container.firstElementChild!
    expect(badge.className).toContain('text-sev-info')
    const dot = badge.querySelector('span')!
    expect(dot.className).toContain('bg-sev-info')
  })

  it('applies suggestion styling for Suggestion severity', () => {
    const { container } = render(<SeverityBadge severity="Suggestion" />)
    const badge = container.firstElementChild!
    expect(badge.className).toContain('text-sev-suggestion')
    const dot = badge.querySelector('span')!
    expect(dot.className).toContain('bg-sev-suggestion')
  })
})

describe('SeverityDot', () => {
  it('renders a dot with the correct color class', () => {
    const { container } = render(<SeverityDot severity="Error" />)
    const dot = container.firstElementChild!
    expect(dot.className).toContain('bg-sev-error')
    expect(dot.className).toContain('rounded-full')
  })

  it('renders different colors for each severity', () => {
    const { container: c1 } = render(<SeverityDot severity="Warning" />)
    expect(c1.firstElementChild!.className).toContain('bg-sev-warning')

    const { container: c2 } = render(<SeverityDot severity="Suggestion" />)
    expect(c2.firstElementChild!.className).toContain('bg-sev-suggestion')
  })
})
