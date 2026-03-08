/** Score color class — green >= 8, yellow >= 5, red < 5 */
export function scoreColorClass(score: number): string {
  if (score >= 8) return 'text-accent'
  if (score >= 5) return 'text-sev-warning'
  return 'text-sev-error'
}

/** Ring color class for score gauges */
export function scoreRingClass(score: number): string {
  if (score >= 8) return 'ring-sev-suggestion/20'
  if (score >= 5) return 'ring-sev-warning/20'
  return 'ring-sev-error/20'
}
