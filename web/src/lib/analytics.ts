import type {
  AnalyticsTrendsResponse,
  FeedbackEvalTrendGap,
  ReviewSession,
  Severity,
} from '../api/types'

type ReviewComment = ReviewSession['comments'][number]

const FEEDBACK_LEARNING_ACCEPT_TAGS = new Set([
  'feedback-calibration:accepted-id',
  'feedback-calibration:boosted',
  'semantic-feedback:accepted',
])

const FEEDBACK_LEARNING_REJECT_TAGS = new Set([
  'feedback-calibration:demoted',
  'semantic-feedback:rejected',
])

function isLabeledFeedbackComment(comment: ReviewComment): boolean {
  return comment.feedback === 'accept' || comment.feedback === 'reject'
}

function isFeedbackLearningComment(comment: ReviewComment): boolean {
  return comment.tags.some(tag => (
    tag === 'feedback-calibration'
    || tag.startsWith('feedback-calibration:')
    || tag.startsWith('semantic-feedback:')
  ))
}

function hasFeedbackLearningAcceptTag(comment: ReviewComment): boolean {
  return comment.tags.some(tag => FEEDBACK_LEARNING_ACCEPT_TAGS.has(tag))
}

function hasFeedbackLearningRejectTag(comment: ReviewComment): boolean {
  return comment.tags.some(tag => FEEDBACK_LEARNING_REJECT_TAGS.has(tag))
}

export function computeAnalytics(reviews: ReviewSession[]) {
  const completed = getCompletedReviews(reviews)

  const scoreOverTime = completed.map((r, i) => ({
    reviewId: r.id,
    idx: i + 1,
    label: `#${i + 1}`,
    score: r.summary!.overall_score,
    findings: r.summary!.total_comments,
    files: r.files_reviewed,
  }))

  const severityOverTime = completed.map((r, i) => ({
    reviewId: r.id,
    idx: i + 1,
    label: `#${i + 1}`,
    Error: r.summary!.by_severity.Error || 0,
    Warning: r.summary!.by_severity.Warning || 0,
    Info: r.summary!.by_severity.Info || 0,
    Suggestion: r.summary!.by_severity.Suggestion || 0,
  }))

  const catTotals: Record<string, number> = {}
  for (const r of completed) {
    for (const [cat, count] of Object.entries(r.summary!.by_category)) {
      catTotals[cat] = (catTotals[cat] || 0) + count
    }
  }
  const categoryData = Object.entries(catTotals)
    .sort((a, b) => b[1] - a[1])
    .map(([name, value]) => ({ name, value }))

  const feedbackTotalsByCategory: Record<string, { accepted: number; rejected: number }> = {}
  const feedbackTotalsByRule: Record<string, { accepted: number; rejected: number }> = {}
  const feedbackCoverageSeries = completed.map((r, i) => {
    let accepted = 0
    let rejected = 0

    for (const comment of r.comments) {
      if (comment.feedback === 'accept') {
        accepted += 1
      } else if (comment.feedback === 'reject') {
        rejected += 1
      } else {
        continue
      }

      const current = feedbackTotalsByCategory[comment.category] ?? { accepted: 0, rejected: 0 }
      if (comment.feedback === 'accept') {
        current.accepted += 1
      } else {
        current.rejected += 1
      }
      feedbackTotalsByCategory[comment.category] = current

      const ruleId = comment.rule_id?.trim()
      if (ruleId) {
        const currentRule = feedbackTotalsByRule[ruleId] ?? { accepted: 0, rejected: 0 }
        if (comment.feedback === 'accept') {
          currentRule.accepted += 1
        } else {
          currentRule.rejected += 1
        }
        feedbackTotalsByRule[ruleId] = currentRule
      }
    }

    const totalComments = r.comments.length
    const labeled = accepted + rejected

    return {
      reviewId: r.id,
      idx: i + 1,
      label: `#${i + 1}`,
      coverage: totalComments > 0 ? labeled / totalComments : 0,
      acceptanceRate: labeled > 0 ? accepted / labeled : 0,
      labeled,
      accepted,
      rejected,
      totalComments,
    }
  })

  const feedbackLearningSeries = completed.map((r, i) => {
    const labeledComments = r.comments.filter(isLabeledFeedbackComment)
    const tunedComments = labeledComments.filter(isFeedbackLearningComment)
    const baselineComments = labeledComments.filter(comment => !isFeedbackLearningComment(comment))
    const tunedAccepted = tunedComments.filter(comment => comment.feedback === 'accept').length
    const tunedRejected = tunedComments.filter(comment => comment.feedback === 'reject').length
    const baselineAccepted = baselineComments.filter(comment => comment.feedback === 'accept').length

    return {
      reviewId: r.id,
      idx: i + 1,
      label: `#${i + 1}`,
      tunedLabeled: tunedComments.length,
      tunedAccepted,
      tunedRejected,
      baselineLabeled: baselineComments.length,
      baselineAccepted,
      tunedAcceptanceRate: tunedComments.length > 0 ? tunedAccepted / tunedComments.length : null,
      baselineAcceptanceRate: baselineComments.length > 0 ? baselineAccepted / baselineComments.length : null,
      acceptanceLift: tunedComments.length > 0 && baselineComments.length > 0
        ? (tunedAccepted / tunedComments.length) - (baselineAccepted / baselineComments.length)
        : null,
      boostedAccepted: tunedComments.filter(comment => (
        comment.feedback === 'accept' && hasFeedbackLearningAcceptTag(comment)
      )).length,
      demotedRejected: tunedComments.filter(comment => (
        comment.feedback === 'reject' && hasFeedbackLearningRejectTag(comment)
      )).length,
    }
  })

  const feedbackCategoryData = Object.entries(feedbackTotalsByCategory)
    .map(([name, totals]) => {
      const total = totals.accepted + totals.rejected
      return {
        name,
        accepted: totals.accepted,
        rejected: totals.rejected,
        total,
        acceptanceRate: total > 0 ? totals.accepted / total : 0,
      }
    })
    .sort((left, right) => right.total - left.total || right.accepted - left.accepted)

  const feedbackRuleData = Object.entries(feedbackTotalsByRule)
    .map(([name, totals]) => {
      const total = totals.accepted + totals.rejected
      return {
        name,
        accepted: totals.accepted,
        rejected: totals.rejected,
        total,
        acceptanceRate: total > 0 ? totals.accepted / total : 0,
      }
    })
    .sort((left, right) => right.total - left.total || right.accepted - left.accepted)

  const topAcceptedCategories = feedbackCategoryData
    .filter(item => item.accepted > 0)
    .sort((left, right) => right.accepted - left.accepted || right.total - left.total)
    .slice(0, 5)

  const topRejectedCategories = feedbackCategoryData
    .filter(item => item.rejected > 0)
    .sort((left, right) => right.rejected - left.rejected || right.total - left.total)
    .slice(0, 5)

  const topAcceptedRules = feedbackRuleData
    .filter(item => item.accepted > 0)
    .sort((left, right) => right.accepted - left.accepted || right.total - left.total)
    .slice(0, 5)

  const topRejectedRules = feedbackRuleData
    .filter(item => item.rejected > 0)
    .sort((left, right) => right.rejected - left.rejected || right.total - left.total)
    .slice(0, 5)

  const lifecycleSeries = completed.map((r, i) => ({
    reviewId: r.id,
    idx: i + 1,
    label: `#${i + 1}`,
    open: r.summary!.open_comments,
    resolved: r.summary!.resolved_comments,
    dismissed: r.summary!.dismissed_comments,
    openBlockers: r.summary!.open_blockers,
  }))

  const completenessSeries = completed.map((r, i) => {
    const completeness = getCompletenessSummary(r.summary!)
    const totalFindings = completeness.total_findings

    return {
      reviewId: r.id,
      idx: i + 1,
      label: `#${i + 1}`,
      totalFindings,
      acknowledged: completeness.acknowledged_findings,
      fixed: completeness.fixed_findings,
      stale: completeness.stale_findings,
      acknowledgedRate: totalFindings > 0 ? completeness.acknowledged_findings / totalFindings : 0,
      fixedRate: totalFindings > 0 ? completeness.fixed_findings / totalFindings : 0,
    }
  })

  const meanTimeToResolutionSeries = completed.map((r, i) => {
    const startedAtMs = toTimestampMs(r.started_at)
    const resolutionHours = startedAtMs == null
      ? []
      : r.comments.flatMap(comment => {
        if (comment.status !== 'Resolved') {
          return []
        }

        const resolvedAtMs = toTimestampMs(comment.resolved_at)
        if (resolvedAtMs == null || resolvedAtMs < startedAtMs) {
          return []
        }

        return [(resolvedAtMs - startedAtMs) / (1000 * 60 * 60)]
      })
    const totalHours = resolutionHours.reduce((sum, hours) => sum + hours, 0)

    return {
      reviewId: r.id,
      idx: i + 1,
      label: `#${i + 1}`,
      meanHours: resolutionHours.length > 0 ? totalHours / resolutionHours.length : null,
      resolvedCount: resolutionHours.length,
    }
  })

  const totalFindings = completed.reduce((s, r) => s + r.summary!.total_comments, 0)
  const avgFindings = completed.length > 0 ? totalFindings / completed.length : 0
  const avgScore = completed.length > 0
    ? completed.reduce((s, r) => s + r.summary!.overall_score, 0) / completed.length : 0
  const totalFiles = completed.reduce((s, r) => s + r.files_reviewed, 0)
  const totalOpenComments = completed.reduce((sum, r) => sum + r.summary!.open_comments, 0)
  const totalResolvedComments = completed.reduce((sum, r) => sum + r.summary!.resolved_comments, 0)
  const totalDismissedComments = completed.reduce((sum, r) => sum + r.summary!.dismissed_comments, 0)
  const totalOpenBlockers = completed.reduce((sum, r) => sum + r.summary!.open_blockers, 0)
  const totalAcknowledgedFindings = completed.reduce(
    (sum, r) => sum + getCompletenessSummary(r.summary!).acknowledged_findings,
    0,
  )
  const totalFixedFindings = completed.reduce(
    (sum, r) => sum + getCompletenessSummary(r.summary!).fixed_findings,
    0,
  )
  const totalStaleFindings = completed.reduce(
    (sum, r) => sum + getCompletenessSummary(r.summary!).stale_findings,
    0,
  )
  const totalCompletenessFindings = completed.reduce(
    (sum, r) => sum + getCompletenessSummary(r.summary!).total_findings,
    0,
  )
  const resolvedWithTimestampCount = meanTimeToResolutionSeries.reduce(
    (sum, point) => sum + point.resolvedCount,
    0,
  )
  const totalResolutionHours = meanTimeToResolutionSeries.reduce(
    (sum, point) => sum + (point.meanHours ?? 0) * point.resolvedCount,
    0,
  )
  const reviewsWithTimedResolutions = meanTimeToResolutionSeries.filter(point => point.resolvedCount > 0).length
  const totalLifecycleComments = totalOpenComments + totalResolvedComments + totalDismissedComments
  const labeledFeedbackTotal = feedbackCoverageSeries.reduce((sum, point) => sum + point.labeled, 0)
  const acceptedFeedbackTotal = feedbackCoverageSeries.reduce((sum, point) => sum + point.accepted, 0)
  const rejectedFeedbackTotal = feedbackCoverageSeries.reduce((sum, point) => sum + point.rejected, 0)
  const feedbackLearningLabeledTotal = feedbackLearningSeries.reduce(
    (sum, point) => sum + point.tunedLabeled,
    0,
  )
  const feedbackLearningAcceptedTotal = feedbackLearningSeries.reduce(
    (sum, point) => sum + point.tunedAccepted,
    0,
  )
  const feedbackLearningRejectedTotal = feedbackLearningSeries.reduce(
    (sum, point) => sum + point.tunedRejected,
    0,
  )
  const feedbackLearningBaselineLabeledTotal = feedbackLearningSeries.reduce(
    (sum, point) => sum + point.baselineLabeled,
    0,
  )
  const feedbackLearningBaselineAcceptedTotal = feedbackLearningSeries.reduce(
    (sum, point) => sum + point.baselineAccepted,
    0,
  )
  const feedbackLearningBoostedAcceptedTotal = feedbackLearningSeries.reduce(
    (sum, point) => sum + point.boostedAccepted,
    0,
  )
  const feedbackLearningDemotedRejectedTotal = feedbackLearningSeries.reduce(
    (sum, point) => sum + point.demotedRejected,
    0,
  )
  const totalCommentCount = completed.reduce((sum, r) => sum + r.comments.length, 0)
  const reviewsWithFeedback = feedbackCoverageSeries.filter(point => point.labeled > 0).length
  const feedbackLearningReviewCount = feedbackLearningSeries.filter(
    point => point.tunedLabeled > 0,
  ).length
  const feedbackLearningAcceptanceRate = feedbackLearningLabeledTotal > 0
    ? feedbackLearningAcceptedTotal / feedbackLearningLabeledTotal
    : 0
  const feedbackLearningBaselineAcceptanceRate = feedbackLearningBaselineLabeledTotal > 0
    ? feedbackLearningBaselineAcceptedTotal / feedbackLearningBaselineLabeledTotal
    : null
  const feedbackLearningAcceptanceLift = feedbackLearningBaselineAcceptanceRate != null
    && feedbackLearningLabeledTotal > 0
    ? feedbackLearningAcceptanceRate - feedbackLearningBaselineAcceptanceRate
    : null

  const sevTotals: Record<Severity, number> = { Error: 0, Warning: 0, Info: 0, Suggestion: 0 }
  for (const r of completed) {
    for (const [sev, count] of Object.entries(r.summary!.by_severity)) {
      sevTotals[sev as Severity] = (sevTotals[sev as Severity] || 0) + count
    }
  }

  const criticalReviews = completed.filter(r => r.summary!.critical_issues > 0).length
  const criticalRate = completed.length > 0 ? (criticalReviews / completed.length * 100) : 0

  return {
    scoreOverTime,
    severityOverTime,
    categoryData,
    lifecycleSeries,
    completenessSeries,
    meanTimeToResolutionSeries,
    feedbackCoverageSeries,
    feedbackLearningSeries,
    topAcceptedCategories,
    topRejectedCategories,
    topAcceptedRules,
    topRejectedRules,
    stats: {
      totalReviews: completed.length,
      avgScore,
      totalFindings,
      avgFindings,
      totalFiles,
      sevTotals,
      criticalRate,
      labeledFeedbackTotal,
      acceptedFeedbackTotal,
      rejectedFeedbackTotal,
      totalOpenComments,
      totalResolvedComments,
      totalDismissedComments,
      totalOpenBlockers,
      totalAcknowledgedFindings,
      totalFixedFindings,
      totalStaleFindings,
      completenessRate: totalCompletenessFindings > 0 ? totalAcknowledgedFindings / totalCompletenessFindings : 0,
      meanTimeToResolutionHours: resolvedWithTimestampCount > 0
        ? totalResolutionHours / resolvedWithTimestampCount
        : null,
      resolvedWithTimestampCount,
      reviewsWithTimedResolutions,
      resolutionRate: totalLifecycleComments > 0
        ? (totalResolvedComments + totalDismissedComments) / totalLifecycleComments
        : 0,
      feedbackCoverageRate: totalCommentCount > 0 ? labeledFeedbackTotal / totalCommentCount : 0,
      feedbackAcceptanceRate: labeledFeedbackTotal > 0 ? acceptedFeedbackTotal / labeledFeedbackTotal : 0,
      reviewsWithFeedback,
      feedbackLearningLabeledTotal,
      feedbackLearningAcceptedTotal,
      feedbackLearningRejectedTotal,
      feedbackLearningReviewCount,
      feedbackLearningAcceptanceRate,
      feedbackLearningBaselineLabeledTotal,
      feedbackLearningBaselineAcceptanceRate,
      feedbackLearningAcceptanceLift,
      feedbackLearningBoostedAcceptedTotal,
      feedbackLearningDemotedRejectedTotal,
    },
  }
}

export type AnalyticsDrilldownSelection =
  | { type: 'review'; reviewId: string }
  | { type: 'category'; category: string }
  | { type: 'rule'; ruleId: string }

export interface AnalyticsDrilldown {
  title: string
  description: string
  reviews: Array<{
    id: string
    label: string
    startedAt: string | number
    overallScore?: number
    findingCount: number
  }>
  comments: Array<{
    reviewId: string
    reviewLabel: string
    id: string
    filePath: string
    lineNumber: number
    content: string
    category: string
    ruleId?: string
  }>
  relatedRules: string[]
}

export function buildAnalyticsDrilldown(
  reviews: ReviewSession[],
  selection: AnalyticsDrilldownSelection,
): AnalyticsDrilldown | null {
  const completed = getCompletedReviews(reviews)
  const labeledReviews = completed.map((review, index) => ({
    review,
    label: `#${index + 1}`,
  }))

  if (selection.type === 'review') {
    const match = labeledReviews.find(entry => entry.review.id === selection.reviewId)
    if (!match) {
      return null
    }

    const relatedRules = Array.from(new Set(
      match.review.comments
        .map(comment => comment.rule_id?.trim())
        .filter((ruleId): ruleId is string => Boolean(ruleId)),
    )).sort()

    return {
      title: `Review ${match.label}`,
      description: `${match.review.comments.length} finding${match.review.comments.length === 1 ? '' : 's'} across ${match.review.files_reviewed} reviewed file${match.review.files_reviewed === 1 ? '' : 's'}.`,
      reviews: [{
        id: match.review.id,
        label: match.label,
        startedAt: match.review.started_at,
        overallScore: match.review.summary?.overall_score,
        findingCount: match.review.comments.length,
      }],
      comments: match.review.comments.map(comment => ({
        reviewId: match.review.id,
        reviewLabel: match.label,
        id: comment.id,
        filePath: comment.file_path,
        lineNumber: comment.line_number,
        content: comment.content,
        category: comment.category,
        ruleId: comment.rule_id?.trim(),
      })),
      relatedRules,
    }
  }

  const matches = labeledReviews.flatMap(({ review, label }) => review.comments
    .filter(comment => selection.type === 'category'
      ? comment.category === selection.category
      : comment.rule_id?.trim() === selection.ruleId)
    .map(comment => ({ review, label, comment })))

  if (matches.length === 0) {
    return null
  }

  const reviewMap = new Map<string, AnalyticsDrilldown['reviews'][number]>()
  for (const { review, label } of matches) {
    if (!reviewMap.has(review.id)) {
      reviewMap.set(review.id, {
        id: review.id,
        label,
        startedAt: review.started_at,
        overallScore: review.summary?.overall_score,
        findingCount: review.comments.length,
      })
    }
  }

  const relatedRules = Array.from(new Set(
    matches
      .map(({ comment }) => comment.rule_id?.trim())
      .filter((ruleId): ruleId is string => Boolean(ruleId)),
  )).sort()

  return {
    title: selection.type === 'category'
      ? `Category · ${selection.category}`
      : `Rule · ${selection.ruleId}`,
    description: `${matches.length} finding${matches.length === 1 ? '' : 's'} across ${reviewMap.size} review${reviewMap.size === 1 ? '' : 's'}.`,
    reviews: Array.from(reviewMap.values()),
    comments: matches.map(({ review, label, comment }) => ({
      reviewId: review.id,
      reviewLabel: label,
      id: comment.id,
      filePath: comment.file_path,
      lineNumber: comment.line_number,
      content: comment.content,
      category: comment.category,
      ruleId: comment.rule_id?.trim(),
    })),
    relatedRules,
  }
}

export function formatTrendLabel(timestamp: string, index: number): string {
  const parsed = new Date(timestamp)
  if (Number.isNaN(parsed.getTime())) return `#${index + 1}`
  return `${parsed.getMonth() + 1}/${parsed.getDate()}`
}

export function formatPercent(value: number | undefined): string {
  return value == null ? 'n/a' : `${(value * 100).toFixed(0)}%`
}

export function formatDurationHours(value: number | null | undefined): string {
  if (value == null || Number.isNaN(value)) {
    return 'n/a'
  }
  if (value >= 48) {
    return `${(value / 24).toFixed(1)}d`
  }
  if (value >= 1) {
    return `${value.toFixed(1)}h`
  }
  return `${Math.round(value * 60)}m`
}

export function computeTrendAnalytics(trends: AnalyticsTrendsResponse | undefined) {
  const evalEntries = trends?.eval_trend.entries ?? []
  const feedbackEntries = trends?.feedback_eval_trend.entries ?? []

  const evalSeries = evalEntries.map((entry, index) => ({
    idx: index + 1,
    label: formatTrendLabel(entry.timestamp, index),
    microF1: entry.micro_f1,
    weightedScore: entry.weighted_score ?? entry.micro_f1,
    fixtures: entry.fixture_count,
    warnings: entry.verification_warning_count ?? 0,
    parseFailures: entry.verification_parse_failure_count ?? 0,
    requestFailures: entry.verification_request_failure_count ?? 0,
  }))

  const feedbackSeries = feedbackEntries.map((entry, index) => ({
    idx: index + 1,
    label: formatTrendLabel(entry.timestamp, index),
    acceptanceRate: entry.acceptance_rate,
    confidenceF1: entry.confidence_f1 ?? 0,
    confidenceAgreement: entry.confidence_agreement_rate ?? 0,
    labeledComments: entry.labeled_comments,
  }))

  return {
    warnings: trends?.warnings ?? [],
    evalEntries,
    feedbackEntries,
    evalSeries,
    feedbackSeries,
    latestEval: evalEntries[evalEntries.length - 1],
    latestFeedback: feedbackEntries[feedbackEntries.length - 1],
    evalTrendPath: trends?.eval_trend_path ?? '.diffscope.eval-trend.json',
    feedbackTrendPath: trends?.feedback_eval_trend_path ?? '.diffscope.feedback-eval-trend.json',
  }
}

type AnalyticsSnapshot = ReturnType<typeof computeAnalytics>
type TrendAnalyticsSnapshot = ReturnType<typeof computeTrendAnalytics>

type AnalyticsExportCsvRow = {
  report: string
  group: string
  label: string
  metric: string
  value: string | number
}

export interface AnalyticsExportReport {
  generatedAt: string
  sources: {
    evalTrendPath: string
    feedbackTrendPath: string
    warnings: string[]
  }
  reviewQuality: {
    summary: {
      totalReviews: number
      avgScore: number
      totalFindings: number
      avgFindings: number
      totalFiles: number
      criticalRate: number
    }
    severityBreakdown: Array<{ name: Severity; value: number }>
    categoryTotals: AnalyticsSnapshot['categoryData']
    scoreOverTime: AnalyticsSnapshot['scoreOverTime']
  }
  lifecycle: {
    summary: {
      totalOpenComments: number
      totalResolvedComments: number
      totalDismissedComments: number
      totalOpenBlockers: number
      totalAcknowledgedFindings: number
      totalFixedFindings: number
      totalStaleFindings: number
      completenessRate: number
      meanTimeToResolutionHours?: number
      resolvedWithTimestampCount: number
      resolutionRate: number
    }
    byReview: AnalyticsSnapshot['lifecycleSeries']
    completenessByReview: AnalyticsSnapshot['completenessSeries']
    meanTimeToResolutionByReview: AnalyticsSnapshot['meanTimeToResolutionSeries']
  }
  reinforcement: {
    summary: {
      labeledFeedbackTotal: number
      acceptedFeedbackTotal: number
      rejectedFeedbackTotal: number
      feedbackCoverageRate: number
      feedbackAcceptanceRate: number
      reviewsWithFeedback: number
      feedbackLearningLabeledTotal: number
      feedbackLearningAcceptedTotal: number
      feedbackLearningRejectedTotal: number
      feedbackLearningReviewCount: number
      feedbackLearningAcceptanceRate: number
      feedbackLearningBaselineLabeledTotal: number
      feedbackLearningBaselineAcceptanceRate?: number
      feedbackLearningAcceptanceLift?: number
      feedbackLearningBoostedAcceptedTotal: number
      feedbackLearningDemotedRejectedTotal: number
      latestMicroF1?: number
      latestWeightedScore?: number
      latestAcceptanceRate?: number
      latestConfidenceF1?: number
    }
    coverageByReview: AnalyticsSnapshot['feedbackCoverageSeries']
    feedbackLearningByReview: AnalyticsSnapshot['feedbackLearningSeries']
    topAcceptedCategories: AnalyticsSnapshot['topAcceptedCategories']
    topRejectedCategories: AnalyticsSnapshot['topRejectedCategories']
    topAcceptedRules: AnalyticsSnapshot['topAcceptedRules']
    topRejectedRules: AnalyticsSnapshot['topRejectedRules']
    evalSeries: TrendAnalyticsSnapshot['evalSeries']
    feedbackSeries: TrendAnalyticsSnapshot['feedbackSeries']
    latestAttentionGaps: {
      byCategory: FeedbackEvalTrendGap[]
      byRule: FeedbackEvalTrendGap[]
    }
  }
}

function downloadBlob(blob: Blob, filename: string) {
  const url = window.URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  window.URL.revokeObjectURL(url)
}

function escapeCsvValue(value: string | number): string {
  return `"${String(value).replace(/"/g, '""')}"`
}

function appendSummaryRows(
  rows: AnalyticsExportCsvRow[],
  report: string,
  group: string,
  summary: Record<string, string | number | undefined>,
) {
  Object.entries(summary).forEach(([metric, value]) => {
    if (value == null) {
      return
    }

    rows.push({ report, group, label: '', metric, value })
  })
}

function appendFeedbackBreakdownRows(
  rows: AnalyticsExportCsvRow[],
  group: string,
  items: AnalyticsSnapshot['topAcceptedCategories'],
) {
  items.forEach(item => {
    rows.push({ report: 'reinforcement', group, label: item.name, metric: 'accepted', value: item.accepted })
    rows.push({ report: 'reinforcement', group, label: item.name, metric: 'rejected', value: item.rejected })
    rows.push({ report: 'reinforcement', group, label: item.name, metric: 'total', value: item.total })
    rows.push({ report: 'reinforcement', group, label: item.name, metric: 'acceptance_rate', value: item.acceptanceRate })
  })
}

export function buildAnalyticsExportReport(
  reviews: ReviewSession[],
  trends: AnalyticsTrendsResponse | undefined,
  generatedAt = new Date().toISOString(),
): AnalyticsExportReport {
  const analytics = computeAnalytics(reviews)
  const trendAnalytics = computeTrendAnalytics(trends)

  return {
    generatedAt,
    sources: {
      evalTrendPath: trendAnalytics.evalTrendPath,
      feedbackTrendPath: trendAnalytics.feedbackTrendPath,
      warnings: trendAnalytics.warnings,
    },
    reviewQuality: {
      summary: {
        totalReviews: analytics.stats.totalReviews,
        avgScore: analytics.stats.avgScore,
        totalFindings: analytics.stats.totalFindings,
        avgFindings: analytics.stats.avgFindings,
        totalFiles: analytics.stats.totalFiles,
        criticalRate: analytics.stats.criticalRate,
      },
      severityBreakdown: Object.entries(analytics.stats.sevTotals).map(([name, value]) => ({
        name: name as Severity,
        value,
      })),
      categoryTotals: analytics.categoryData,
      scoreOverTime: analytics.scoreOverTime,
    },
    lifecycle: {
      summary: {
        totalOpenComments: analytics.stats.totalOpenComments,
        totalResolvedComments: analytics.stats.totalResolvedComments,
        totalDismissedComments: analytics.stats.totalDismissedComments,
        totalOpenBlockers: analytics.stats.totalOpenBlockers,
        totalAcknowledgedFindings: analytics.stats.totalAcknowledgedFindings,
        totalFixedFindings: analytics.stats.totalFixedFindings,
        totalStaleFindings: analytics.stats.totalStaleFindings,
        completenessRate: analytics.stats.completenessRate,
        meanTimeToResolutionHours: analytics.stats.meanTimeToResolutionHours ?? undefined,
        resolvedWithTimestampCount: analytics.stats.resolvedWithTimestampCount,
        resolutionRate: analytics.stats.resolutionRate,
      },
      byReview: analytics.lifecycleSeries,
      completenessByReview: analytics.completenessSeries,
      meanTimeToResolutionByReview: analytics.meanTimeToResolutionSeries,
    },
    reinforcement: {
      summary: {
        labeledFeedbackTotal: analytics.stats.labeledFeedbackTotal,
        acceptedFeedbackTotal: analytics.stats.acceptedFeedbackTotal,
        rejectedFeedbackTotal: analytics.stats.rejectedFeedbackTotal,
        feedbackCoverageRate: analytics.stats.feedbackCoverageRate,
        feedbackAcceptanceRate: analytics.stats.feedbackAcceptanceRate,
        reviewsWithFeedback: analytics.stats.reviewsWithFeedback,
        feedbackLearningLabeledTotal: analytics.stats.feedbackLearningLabeledTotal,
        feedbackLearningAcceptedTotal: analytics.stats.feedbackLearningAcceptedTotal,
        feedbackLearningRejectedTotal: analytics.stats.feedbackLearningRejectedTotal,
        feedbackLearningReviewCount: analytics.stats.feedbackLearningReviewCount,
        feedbackLearningAcceptanceRate: analytics.stats.feedbackLearningAcceptanceRate,
        feedbackLearningBaselineLabeledTotal: analytics.stats.feedbackLearningBaselineLabeledTotal,
        feedbackLearningBaselineAcceptanceRate: analytics.stats.feedbackLearningBaselineAcceptanceRate ?? undefined,
        feedbackLearningAcceptanceLift: analytics.stats.feedbackLearningAcceptanceLift ?? undefined,
        feedbackLearningBoostedAcceptedTotal: analytics.stats.feedbackLearningBoostedAcceptedTotal,
        feedbackLearningDemotedRejectedTotal: analytics.stats.feedbackLearningDemotedRejectedTotal,
        latestMicroF1: trendAnalytics.latestEval?.micro_f1,
        latestWeightedScore: trendAnalytics.latestEval?.weighted_score,
        latestAcceptanceRate: trendAnalytics.latestFeedback?.acceptance_rate,
        latestConfidenceF1: trendAnalytics.latestFeedback?.confidence_f1,
      },
      coverageByReview: analytics.feedbackCoverageSeries,
      feedbackLearningByReview: analytics.feedbackLearningSeries,
      topAcceptedCategories: analytics.topAcceptedCategories,
      topRejectedCategories: analytics.topRejectedCategories,
      topAcceptedRules: analytics.topAcceptedRules,
      topRejectedRules: analytics.topRejectedRules,
      evalSeries: trendAnalytics.evalSeries,
      feedbackSeries: trendAnalytics.feedbackSeries,
      latestAttentionGaps: {
        byCategory: trendAnalytics.latestFeedback?.attention_by_category ?? [],
        byRule: trendAnalytics.latestFeedback?.attention_by_rule ?? [],
      },
    },
  }
}

export function buildAnalyticsCsv(report: AnalyticsExportReport): string {
  const rows: AnalyticsExportCsvRow[] = []

  rows.push({ report: 'meta', group: 'generated', label: '', metric: 'generated_at', value: report.generatedAt })
  rows.push({ report: 'meta', group: 'sources', label: 'eval', metric: 'path', value: report.sources.evalTrendPath })
  rows.push({ report: 'meta', group: 'sources', label: 'feedback', metric: 'path', value: report.sources.feedbackTrendPath })
  report.sources.warnings.forEach((warning, index) => {
    rows.push({ report: 'meta', group: 'warnings', label: String(index + 1), metric: 'warning', value: warning })
  })

  appendSummaryRows(rows, 'review_quality', 'summary', report.reviewQuality.summary)
  report.reviewQuality.severityBreakdown.forEach(item => {
    rows.push({ report: 'review_quality', group: 'severity_breakdown', label: item.name, metric: 'count', value: item.value })
  })
  report.reviewQuality.categoryTotals.forEach(item => {
    rows.push({ report: 'review_quality', group: 'category_totals', label: item.name, metric: 'count', value: item.value })
  })
  report.reviewQuality.scoreOverTime.forEach(point => {
    rows.push({ report: 'review_quality', group: 'score_over_time', label: point.label, metric: 'score', value: point.score })
    rows.push({ report: 'review_quality', group: 'score_over_time', label: point.label, metric: 'findings', value: point.findings })
    rows.push({ report: 'review_quality', group: 'score_over_time', label: point.label, metric: 'files', value: point.files })
  })

  appendSummaryRows(rows, 'lifecycle', 'summary', report.lifecycle.summary)
  report.lifecycle.byReview.forEach(point => {
    rows.push({ report: 'lifecycle', group: 'by_review', label: point.label, metric: 'open', value: point.open })
    rows.push({ report: 'lifecycle', group: 'by_review', label: point.label, metric: 'resolved', value: point.resolved })
    rows.push({ report: 'lifecycle', group: 'by_review', label: point.label, metric: 'dismissed', value: point.dismissed })
    rows.push({ report: 'lifecycle', group: 'by_review', label: point.label, metric: 'open_blockers', value: point.openBlockers })
  })
  report.lifecycle.completenessByReview.forEach(point => {
    rows.push({ report: 'lifecycle', group: 'completeness_by_review', label: point.label, metric: 'total_findings', value: point.totalFindings })
    rows.push({ report: 'lifecycle', group: 'completeness_by_review', label: point.label, metric: 'acknowledged', value: point.acknowledged })
    rows.push({ report: 'lifecycle', group: 'completeness_by_review', label: point.label, metric: 'fixed', value: point.fixed })
    rows.push({ report: 'lifecycle', group: 'completeness_by_review', label: point.label, metric: 'stale', value: point.stale })
    rows.push({ report: 'lifecycle', group: 'completeness_by_review', label: point.label, metric: 'acknowledged_rate', value: point.acknowledgedRate })
    rows.push({ report: 'lifecycle', group: 'completeness_by_review', label: point.label, metric: 'fixed_rate', value: point.fixedRate })
  })
  report.lifecycle.meanTimeToResolutionByReview.forEach(point => {
    if (point.meanHours != null) {
      rows.push({ report: 'lifecycle', group: 'mean_time_to_resolution_by_review', label: point.label, metric: 'mean_hours', value: point.meanHours })
    }
    rows.push({ report: 'lifecycle', group: 'mean_time_to_resolution_by_review', label: point.label, metric: 'resolved_count', value: point.resolvedCount })
  })

  appendSummaryRows(rows, 'reinforcement', 'summary', report.reinforcement.summary)
  report.reinforcement.coverageByReview.forEach(point => {
    rows.push({ report: 'reinforcement', group: 'coverage_by_review', label: point.label, metric: 'coverage', value: point.coverage })
    rows.push({ report: 'reinforcement', group: 'coverage_by_review', label: point.label, metric: 'acceptance_rate', value: point.acceptanceRate })
    rows.push({ report: 'reinforcement', group: 'coverage_by_review', label: point.label, metric: 'labeled', value: point.labeled })
    rows.push({ report: 'reinforcement', group: 'coverage_by_review', label: point.label, metric: 'accepted', value: point.accepted })
    rows.push({ report: 'reinforcement', group: 'coverage_by_review', label: point.label, metric: 'rejected', value: point.rejected })
    rows.push({ report: 'reinforcement', group: 'coverage_by_review', label: point.label, metric: 'total_comments', value: point.totalComments })
  })
  report.reinforcement.feedbackLearningByReview.forEach(point => {
    rows.push({ report: 'reinforcement', group: 'feedback_learning_by_review', label: point.label, metric: 'tuned_labeled', value: point.tunedLabeled })
    rows.push({ report: 'reinforcement', group: 'feedback_learning_by_review', label: point.label, metric: 'tuned_accepted', value: point.tunedAccepted })
    rows.push({ report: 'reinforcement', group: 'feedback_learning_by_review', label: point.label, metric: 'tuned_rejected', value: point.tunedRejected })
    rows.push({ report: 'reinforcement', group: 'feedback_learning_by_review', label: point.label, metric: 'baseline_labeled', value: point.baselineLabeled })
    if (point.tunedAcceptanceRate != null) {
      rows.push({ report: 'reinforcement', group: 'feedback_learning_by_review', label: point.label, metric: 'tuned_acceptance_rate', value: point.tunedAcceptanceRate })
    }
    if (point.baselineAcceptanceRate != null) {
      rows.push({ report: 'reinforcement', group: 'feedback_learning_by_review', label: point.label, metric: 'baseline_acceptance_rate', value: point.baselineAcceptanceRate })
    }
    if (point.acceptanceLift != null) {
      rows.push({ report: 'reinforcement', group: 'feedback_learning_by_review', label: point.label, metric: 'acceptance_lift', value: point.acceptanceLift })
    }
    rows.push({ report: 'reinforcement', group: 'feedback_learning_by_review', label: point.label, metric: 'boosted_accepted', value: point.boostedAccepted })
    rows.push({ report: 'reinforcement', group: 'feedback_learning_by_review', label: point.label, metric: 'demoted_rejected', value: point.demotedRejected })
  })
  appendFeedbackBreakdownRows(rows, 'top_accepted_categories', report.reinforcement.topAcceptedCategories)
  appendFeedbackBreakdownRows(rows, 'top_rejected_categories', report.reinforcement.topRejectedCategories)
  appendFeedbackBreakdownRows(rows, 'top_accepted_rules', report.reinforcement.topAcceptedRules)
  appendFeedbackBreakdownRows(rows, 'top_rejected_rules', report.reinforcement.topRejectedRules)
  report.reinforcement.evalSeries.forEach(point => {
    rows.push({ report: 'reinforcement', group: 'eval_series', label: point.label, metric: 'micro_f1', value: point.microF1 })
    rows.push({ report: 'reinforcement', group: 'eval_series', label: point.label, metric: 'weighted_score', value: point.weightedScore })
    rows.push({ report: 'reinforcement', group: 'eval_series', label: point.label, metric: 'fixtures', value: point.fixtures })
  })
  report.reinforcement.feedbackSeries.forEach(point => {
    rows.push({ report: 'reinforcement', group: 'feedback_series', label: point.label, metric: 'acceptance_rate', value: point.acceptanceRate })
    rows.push({ report: 'reinforcement', group: 'feedback_series', label: point.label, metric: 'confidence_f1', value: point.confidenceF1 })
    rows.push({ report: 'reinforcement', group: 'feedback_series', label: point.label, metric: 'confidence_agreement', value: point.confidenceAgreement })
    rows.push({ report: 'reinforcement', group: 'feedback_series', label: point.label, metric: 'labeled_comments', value: point.labeledComments })
  })
  report.reinforcement.latestAttentionGaps.byCategory.forEach(item => {
    rows.push({ report: 'reinforcement', group: 'attention_gaps_by_category', label: item.name, metric: 'gap', value: item.gap ?? '' })
    rows.push({ report: 'reinforcement', group: 'attention_gaps_by_category', label: item.name, metric: 'feedback_total', value: item.feedback_total })
    rows.push({ report: 'reinforcement', group: 'attention_gaps_by_category', label: item.name, metric: 'high_confidence_total', value: item.high_confidence_total })
  })
  report.reinforcement.latestAttentionGaps.byRule.forEach(item => {
    rows.push({ report: 'reinforcement', group: 'attention_gaps_by_rule', label: item.name, metric: 'gap', value: item.gap ?? '' })
    rows.push({ report: 'reinforcement', group: 'attention_gaps_by_rule', label: item.name, metric: 'feedback_total', value: item.feedback_total })
    rows.push({ report: 'reinforcement', group: 'attention_gaps_by_rule', label: item.name, metric: 'high_confidence_total', value: item.high_confidence_total })
  })

  return [
    'report,group,label,metric,value',
    ...rows.map(row => [row.report, row.group, row.label, row.metric, row.value].map(escapeCsvValue).join(',')),
  ].join('\n')
}

export function exportAnalyticsCsv(report: AnalyticsExportReport) {
  downloadBlob(new Blob([buildAnalyticsCsv(report)], { type: 'text/csv' }), 'diffscope-analytics-report.csv')
}

export function exportAnalyticsJson(report: AnalyticsExportReport) {
  downloadBlob(
    new Blob([JSON.stringify(report, null, 2)], { type: 'application/json' }),
    'diffscope-analytics-report.json',
  )
}

function toDate(value: string | number | undefined): Date | null {
  if (value == null) {
    return null
  }

  const date = typeof value === 'number'
    ? new Date(value * 1000)
    : new Date(value)
  return Number.isNaN(date.getTime()) ? null : date
}

function toTimestampMs(value: string | number | undefined): number | null {
  return toDate(value)?.getTime() ?? null
}

function getCompletedReviews(reviews: ReviewSession[]) {
  return reviews
    .filter((review): review is ReviewSession & { summary: NonNullable<ReviewSession['summary']> } => (
      review.status === 'Complete' && Boolean(review.summary)
    ))
    .sort((left, right) => (toTimestampMs(left.started_at) ?? 0) - (toTimestampMs(right.started_at) ?? 0))
}

function getCompletenessSummary(summary: NonNullable<ReviewSession['summary']>) {
  return summary.completeness ?? {
    total_findings: summary.total_comments,
    acknowledged_findings: summary.resolved_comments + summary.dismissed_comments,
    fixed_findings: summary.resolved_comments,
    stale_findings: 0,
  }
}
