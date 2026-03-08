import { scoreColorClass, scoreRingClass } from '../lib/scores'

interface Props {
  score: number
  size?: 'sm' | 'md'
}

export function ScoreGauge({ score, size = 'md' }: Props) {
  const color = scoreColorClass(score)
  const ring = scoreRingClass(score)

  if (size === 'sm') {
    return (
      <span className={`font-code text-xs font-bold ${color}`}>
        {score.toFixed(1)}
      </span>
    )
  }

  return (
    <div className={`${ring} ring-2 rounded-lg bg-surface-2 px-4 py-2.5 text-center`}>
      <div className={`text-2xl font-bold font-code ${color}`}>{score.toFixed(1)}</div>
      <div className="text-[10px] text-text-muted mt-0.5">Score</div>
    </div>
  )
}
