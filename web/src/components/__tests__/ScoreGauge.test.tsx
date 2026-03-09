import { render, screen } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import { ScoreGauge } from '../ScoreGauge'

describe('ScoreGauge', () => {
  it('renders the score formatted to one decimal place', () => {
    render(<ScoreGauge score={8.5} />)
    expect(screen.getByText('8.5')).toBeInTheDocument()
  })

  it('renders the "Score" label in default (md) size', () => {
    render(<ScoreGauge score={7} />)
    expect(screen.getByText('Score')).toBeInTheDocument()
  })

  it('does not render "Score" label in sm size', () => {
    render(<ScoreGauge score={7} size="sm" />)
    expect(screen.queryByText('Score')).not.toBeInTheDocument()
  })

  it('uses green/accent color for high scores (>= 8)', () => {
    const { container } = render(<ScoreGauge score={9.2} />)
    const scoreEl = container.querySelector('.text-accent')
    expect(scoreEl).toBeInTheDocument()
    expect(scoreEl!.textContent).toBe('9.2')
  })

  it('uses warning color for medium scores (>= 5, < 8)', () => {
    const { container } = render(<ScoreGauge score={6.0} />)
    const scoreEl = container.querySelector('.text-sev-warning')
    expect(scoreEl).toBeInTheDocument()
    expect(scoreEl!.textContent).toBe('6.0')
  })

  it('uses error color for low scores (< 5)', () => {
    const { container } = render(<ScoreGauge score={3.4} />)
    const scoreEl = container.querySelector('.text-sev-error')
    expect(scoreEl).toBeInTheDocument()
    expect(scoreEl!.textContent).toBe('3.4')
  })

  it('renders score with correct ring class for high score', () => {
    const { container } = render(<ScoreGauge score={8.0} />)
    const wrapper = container.firstElementChild!
    expect(wrapper.className).toContain('ring-sev-suggestion/20')
  })

  it('renders score with correct ring class for medium score', () => {
    const { container } = render(<ScoreGauge score={5.5} />)
    const wrapper = container.firstElementChild!
    expect(wrapper.className).toContain('ring-sev-warning/20')
  })

  it('renders score with correct ring class for low score', () => {
    const { container } = render(<ScoreGauge score={2.0} />)
    const wrapper = container.firstElementChild!
    expect(wrapper.className).toContain('ring-sev-error/20')
  })

  it('renders sm variant as a span with just the score', () => {
    const { container } = render(<ScoreGauge score={8.0} size="sm" />)
    const el = container.firstElementChild!
    expect(el.tagName).toBe('SPAN')
    expect(el.textContent).toBe('8.0')
  })
})
