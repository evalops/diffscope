import type {
  AnalyticsTrendsResponse,
  EvalTrendEntry,
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

const CONTEXT_SOURCE_TAG_PREFIX = 'context-source:'
const PATTERN_REPOSITORY_TAG_PREFIX = 'pattern-repository:'

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

function extractPatternRepositorySources(comment: ReviewComment): string[] {
  const sources = comment.tags
    .filter(tag => tag.startsWith(PATTERN_REPOSITORY_TAG_PREFIX))
    .map(tag => tag.slice(PATTERN_REPOSITORY_TAG_PREFIX.length))
    .filter(Boolean)

  if (sources.length > 0) {
    return sources
  }

  return comment.tags.includes('pattern-repository') ? ['external'] : []
}

function isPatternRepositoryComment(comment: ReviewComment): boolean {
  return extractPatternRepositorySources(comment).length > 0
}

function extractContextSources(comment: ReviewComment): string[] {
  const explicit = comment.tags
    .filter(tag => tag.startsWith(CONTEXT_SOURCE_TAG_PREFIX))
    .map(tag => tag.slice(CONTEXT_SOURCE_TAG_PREFIX.length))
    .filter(Boolean)

  if (explicit.length > 0) {
    return Array.from(new Set(explicit))
  }

  return Array.from(new Set(
    extractPatternRepositorySources(comment).map(source => `pattern-repository:${source}`),
  ))
}

function isContextSourceComment(comment: ReviewComment): boolean {
  return extractContextSources(comment).length > 0
}

function formatContextSourceName(name: string): string {
  if (name.startsWith('pattern-repository:')) {
    const source = name.slice('pattern-repository:'.length) || 'external'
    return `Pattern repository · ${source}`
  }

  const knownLabels: Record<string, string> = {
    'custom-context': 'Custom context',
    'design-doc': 'Design doc',
    'dependency-graph': 'Dependency graph',
    document: 'Document',
    'jira-issue': 'Jira issue',
    'linear-issue': 'Linear issue',
    'path-focus': 'Path focus',
    rfc: 'RFC',
    'related-test-file': 'Related test file',
    'repository-graph': 'Repository graph',
    'reverse-dependency-summary': 'Reverse dependency summary',
    runbook: 'Runbook',
    'semantic-retrieval': 'Semantic retrieval',
    'similar-implementation': 'Similar implementation',
    'symbol-graph': 'Symbol graph',
  }

  if (knownLabels[name]) {
    return knownLabels[name]
  }

  return name
    .split(':')
    .map(part => part
      .split('-')
      .map(token => token ? token[0].toUpperCase() + token.slice(1) : token)
      .join(' '))
    .join(' · ')
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

  const patternRepositorySourceTotals: Record<string, {
    total: number
    labeled: number
    accepted: number
    rejected: number
    reviewIds: Set<string>
  }> = {}

  const patternRepositorySeries = completed.map((r, i) => {
    const patternRepositoryComments = r.comments.filter(isPatternRepositoryComment)
    const labeledPatternRepositoryComments = patternRepositoryComments.filter(isLabeledFeedbackComment)
    const accepted = labeledPatternRepositoryComments.filter(comment => comment.feedback === 'accept').length
    const rejected = labeledPatternRepositoryComments.filter(comment => comment.feedback === 'reject').length
    const reviewSources = new Set<string>()

    for (const comment of patternRepositoryComments) {
      const sources = Array.from(new Set(extractPatternRepositorySources(comment)))
      const isLabeled = isLabeledFeedbackComment(comment)

      for (const source of sources) {
        reviewSources.add(source)
        const current = patternRepositorySourceTotals[source] ?? {
          total: 0,
          labeled: 0,
          accepted: 0,
          rejected: 0,
          reviewIds: new Set<string>(),
        }

        current.total += 1
        if (isLabeled) {
          current.labeled += 1
          if (comment.feedback === 'accept') {
            current.accepted += 1
          } else if (comment.feedback === 'reject') {
            current.rejected += 1
          }
        }
        current.reviewIds.add(r.id)
        patternRepositorySourceTotals[source] = current
      }
    }

    return {
      reviewId: r.id,
      idx: i + 1,
      label: `#${i + 1}`,
      findings: patternRepositoryComments.length,
      labeled: labeledPatternRepositoryComments.length,
      accepted,
      rejected,
      sourceCount: reviewSources.size,
      acceptanceRate: labeledPatternRepositoryComments.length > 0
        ? accepted / labeledPatternRepositoryComments.length
        : null,
    }
  })

  const patternRepositorySourceData = Object.entries(patternRepositorySourceTotals)
    .map(([name, totals]) => ({
      name,
      total: totals.total,
      labeled: totals.labeled,
      accepted: totals.accepted,
      rejected: totals.rejected,
      reviewCount: totals.reviewIds.size,
      acceptanceRate: totals.labeled > 0 ? totals.accepted / totals.labeled : 0,
    }))
    .sort((left, right) => right.total - left.total || right.accepted - left.accepted)

  const contextSourceTotals: Record<string, {
    total: number
    labeled: number
    accepted: number
    rejected: number
    resolved: number
    reviewIds: Set<string>
  }> = {}

  const contextSourceSeries = completed.map((r, i) => {
    const contextSourceComments = r.comments.filter(isContextSourceComment)
    const labeledContextSourceComments = contextSourceComments.filter(isLabeledFeedbackComment)
    const accepted = labeledContextSourceComments.filter(comment => comment.feedback === 'accept').length
    const rejected = labeledContextSourceComments.filter(comment => comment.feedback === 'reject').length
    const resolved = contextSourceComments.filter(comment => comment.status === 'Resolved').length
    const reviewSources = new Set<string>()

    for (const comment of contextSourceComments) {
      const sources = Array.from(new Set(extractContextSources(comment)))
      const isLabeled = isLabeledFeedbackComment(comment)
      const isResolved = comment.status === 'Resolved'

      for (const source of sources) {
        reviewSources.add(source)
        const current = contextSourceTotals[source] ?? {
          total: 0,
          labeled: 0,
          accepted: 0,
          rejected: 0,
          resolved: 0,
          reviewIds: new Set<string>(),
        }

        current.total += 1
        if (isLabeled) {
          current.labeled += 1
          if (comment.feedback === 'accept') {
            current.accepted += 1
          } else if (comment.feedback === 'reject') {
            current.rejected += 1
          }
        }
        if (isResolved) {
          current.resolved += 1
        }
        current.reviewIds.add(r.id)
        contextSourceTotals[source] = current
      }
    }

    return {
      reviewId: r.id,
      idx: i + 1,
      label: `#${i + 1}`,
      findings: contextSourceComments.length,
      labeled: labeledContextSourceComments.length,
      accepted,
      rejected,
      resolved,
      sourceCount: reviewSources.size,
      acceptanceRate: labeledContextSourceComments.length > 0
        ? accepted / labeledContextSourceComments.length
        : null,
      fixRate: contextSourceComments.length > 0
        ? resolved / contextSourceComments.length
        : null,
    }
  })

  const contextSourceData = Object.entries(contextSourceTotals)
    .map(([name, totals]) => ({
      name,
      label: formatContextSourceName(name),
      total: totals.total,
      labeled: totals.labeled,
      accepted: totals.accepted,
      rejected: totals.rejected,
      resolved: totals.resolved,
      reviewCount: totals.reviewIds.size,
      acceptanceRate: totals.labeled > 0 ? totals.accepted / totals.labeled : 0,
      fixRate: totals.total > 0 ? totals.resolved / totals.total : 0,
    }))
    .sort((left, right) => right.total - left.total || right.accepted - left.accepted)

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
  const patternRepositoryFindingTotal = patternRepositorySeries.reduce(
    (sum, point) => sum + point.findings,
    0,
  )
  const patternRepositoryLabeledTotal = patternRepositorySeries.reduce(
    (sum, point) => sum + point.labeled,
    0,
  )
  const patternRepositoryAcceptedTotal = patternRepositorySeries.reduce(
    (sum, point) => sum + point.accepted,
    0,
  )
  const patternRepositoryRejectedTotal = patternRepositorySeries.reduce(
    (sum, point) => sum + point.rejected,
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
  const patternRepositoryReviewCount = patternRepositorySeries.filter(
    point => point.findings > 0,
  ).length
  const patternRepositorySourceCount = patternRepositorySourceData.length
  const patternRepositoryUtilizationRate = completed.length > 0
    ? patternRepositoryReviewCount / completed.length
    : 0
  const patternRepositoryAcceptanceRate = patternRepositoryLabeledTotal > 0
    ? patternRepositoryAcceptedTotal / patternRepositoryLabeledTotal
    : 0
  const patternRepositoryBaselineLabeledTotal = labeledFeedbackTotal - patternRepositoryLabeledTotal
  const patternRepositoryBaselineAcceptedTotal = acceptedFeedbackTotal - patternRepositoryAcceptedTotal
  const patternRepositoryBaselineAcceptanceRate = patternRepositoryBaselineLabeledTotal > 0
    ? patternRepositoryBaselineAcceptedTotal / patternRepositoryBaselineLabeledTotal
    : null
  const patternRepositoryAcceptanceLift = patternRepositoryBaselineAcceptanceRate != null
    && patternRepositoryLabeledTotal > 0
    ? patternRepositoryAcceptanceRate - patternRepositoryBaselineAcceptanceRate
    : null
  const contextSourceFindingTotal = contextSourceSeries.reduce(
    (sum, point) => sum + point.findings,
    0,
  )
  const contextSourceLabeledTotal = contextSourceSeries.reduce(
    (sum, point) => sum + point.labeled,
    0,
  )
  const contextSourceAcceptedTotal = contextSourceSeries.reduce(
    (sum, point) => sum + point.accepted,
    0,
  )
  const contextSourceRejectedTotal = contextSourceSeries.reduce(
    (sum, point) => sum + point.rejected,
    0,
  )
  const contextSourceResolvedTotal = contextSourceSeries.reduce(
    (sum, point) => sum + point.resolved,
    0,
  )
  const contextSourceReviewCount = contextSourceSeries.filter(
    point => point.findings > 0,
  ).length
  const contextSourceSourceCount = contextSourceData.length
  const contextSourceUtilizationRate = completed.length > 0
    ? contextSourceReviewCount / completed.length
    : 0
  const contextSourceAcceptanceRate = contextSourceLabeledTotal > 0
    ? contextSourceAcceptedTotal / contextSourceLabeledTotal
    : 0
  const contextSourceBaselineLabeledTotal = labeledFeedbackTotal - contextSourceLabeledTotal
  const contextSourceBaselineAcceptedTotal = acceptedFeedbackTotal - contextSourceAcceptedTotal
  const contextSourceBaselineAcceptanceRate = contextSourceBaselineLabeledTotal > 0
    ? contextSourceBaselineAcceptedTotal / contextSourceBaselineLabeledTotal
    : null
  const contextSourceAcceptanceLift = contextSourceBaselineAcceptanceRate != null
    && contextSourceLabeledTotal > 0
    ? contextSourceAcceptanceRate - contextSourceBaselineAcceptanceRate
    : null
  const contextSourceFixRate = contextSourceFindingTotal > 0
    ? contextSourceResolvedTotal / contextSourceFindingTotal
    : 0
  const contextSourceBaselineFindingTotal = totalCommentCount - contextSourceFindingTotal
  const contextSourceBaselineResolvedTotal = totalResolvedComments - contextSourceResolvedTotal
  const contextSourceBaselineFixRate = contextSourceBaselineFindingTotal > 0
    ? contextSourceBaselineResolvedTotal / contextSourceBaselineFindingTotal
    : null
  const contextSourceFixLift = contextSourceBaselineFixRate != null
    && contextSourceFindingTotal > 0
    ? contextSourceFixRate - contextSourceBaselineFixRate
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
    patternRepositorySeries,
    patternRepositorySourceData,
    contextSourceSeries,
    contextSourceData,
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
      patternRepositoryFindingTotal,
      patternRepositoryLabeledTotal,
      patternRepositoryAcceptedTotal,
      patternRepositoryRejectedTotal,
      patternRepositoryReviewCount,
      patternRepositorySourceCount,
      patternRepositoryUtilizationRate,
      patternRepositoryAcceptanceRate,
      patternRepositoryBaselineLabeledTotal,
      patternRepositoryBaselineAcceptanceRate,
      patternRepositoryAcceptanceLift,
      contextSourceFindingTotal,
      contextSourceLabeledTotal,
      contextSourceAcceptedTotal,
      contextSourceRejectedTotal,
      contextSourceResolvedTotal,
      contextSourceReviewCount,
      contextSourceSourceCount,
      contextSourceUtilizationRate,
      contextSourceAcceptanceRate,
      contextSourceBaselineLabeledTotal,
      contextSourceBaselineAcceptanceRate,
      contextSourceAcceptanceLift,
      contextSourceFixRate,
      contextSourceBaselineFixRate,
      contextSourceFixLift,
    },
  }
}

export type AnalyticsDrilldownSelection =
  | { type: 'review'; reviewId: string }
  | { type: 'category'; category: string }
  | { type: 'rule'; ruleId: string }
  | { type: 'contextSource'; source: string }
  | { type: 'patternRepositorySource'; source: string }

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
    .filter(comment => {
      if (selection.type === 'category') {
        return comment.category === selection.category
      }
      if (selection.type === 'rule') {
        return comment.rule_id?.trim() === selection.ruleId
      }
      if (selection.type === 'contextSource') {
        return extractContextSources(comment).includes(selection.source)
      }
      return extractPatternRepositorySources(comment).includes(selection.source)
    })
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
      : selection.type === 'rule'
        ? `Rule · ${selection.ruleId}`
        : selection.type === 'contextSource'
          ? `Context source · ${formatContextSourceName(selection.source)}`
        : `Pattern repository · ${selection.source}`,
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

function formatReviewerIdentity(entry: EvalTrendEntry): string {
  const model = entry.model?.trim() || 'unknown reviewer'
  const provider = entry.provider?.trim()
  const reviewMode = entry.review_mode?.trim()

  if (provider && reviewMode) {
    return `${model} via ${provider} [${reviewMode}]`
  }
  if (provider) {
    return `${model} via ${provider}`
  }
  if (reviewMode) {
    return `${model} [${reviewMode}]`
  }
  return model
}

function delta(current: number | undefined, baseline: number | undefined): number | undefined {
  if (current == null || baseline == null) {
    return undefined
  }
  return current - baseline
}

type IndependentAuditorStoryComparison = {
  baselineReviewMode: string
  compareReviewMode: string
  microF1Delta?: number
  weightedScoreDelta?: number
  passRateDelta?: number
  usefulnessScoreDelta?: number
}

type IndependentAuditorStory = {
  timestamp: string
  benchmarkLabel: string
  winnerReviewer: string
  winnerReviewMode?: string
  winnerModel?: string
  winnerProvider?: string
  winnerUsefulnessScore?: number
  winnerWeightedScore?: number
  winnerMicroF1?: number
  winnerPassRate?: number
  winnerVerificationHealth?: number
  winnerLifecycleAccuracy?: number
  comparison?: IndependentAuditorStoryComparison
}

function buildIndependentAuditorStory(evalEntries: EvalTrendEntry[]): IndependentAuditorStory | undefined {
  const grouped = new Map<string, EvalTrendEntry[]>()
  for (const entry of evalEntries) {
    if (!entry.review_mode || !entry.comparison_group) {
      continue
    }

    const key = [
      entry.comparison_group,
      entry.model ?? '',
      entry.provider ?? '',
    ].join('::')
    const current = grouped.get(key) ?? []
    current.push(entry)
    grouped.set(key, current)
  }

  let latestStory: IndependentAuditorStory | undefined

  for (const entries of grouped.values()) {
    const ordered = [...entries].sort((left, right) => right.timestamp.localeCompare(left.timestamp))
    const singlePass = ordered.find(entry => entry.review_mode === 'single-pass')
    const agentLoop = ordered.find(entry => entry.review_mode === 'agent-loop')
    if (!singlePass || !agentLoop) {
      continue
    }

    const winner = (agentLoop.usefulness_score ?? -1) >= (singlePass.usefulness_score ?? -1)
      ? agentLoop
      : singlePass
    const story = {
      timestamp: agentLoop.timestamp > singlePass.timestamp ? agentLoop.timestamp : singlePass.timestamp,
      benchmarkLabel: winner.comparison_group ?? winner.label ?? 'eval',
      winnerReviewer: formatReviewerIdentity(winner),
      winnerReviewMode: winner.review_mode,
      winnerModel: winner.model,
      winnerProvider: winner.provider,
      winnerUsefulnessScore: winner.usefulness_score,
      winnerWeightedScore: winner.weighted_score,
      winnerMicroF1: winner.micro_f1,
      winnerPassRate: winner.pass_rate,
      winnerVerificationHealth: winner.verification_verified_pct,
      winnerLifecycleAccuracy: winner.lifecycle_accuracy,
      comparison: {
        baselineReviewMode: 'single-pass',
        compareReviewMode: 'agent-loop',
        microF1Delta: delta(agentLoop.micro_f1, singlePass.micro_f1),
        weightedScoreDelta: delta(agentLoop.weighted_score, singlePass.weighted_score),
        passRateDelta: delta(agentLoop.pass_rate, singlePass.pass_rate),
        usefulnessScoreDelta: delta(agentLoop.usefulness_score, singlePass.usefulness_score),
      },
    }

    if (!latestStory || story.timestamp > latestStory.timestamp) {
      latestStory = story
    }
  }

  if (latestStory) {
    return latestStory
  }

  const latestEntry = [...evalEntries]
    .reverse()
    .find(entry => entry.usefulness_score != null || entry.review_mode || entry.model)
  if (!latestEntry) {
    return undefined
  }

  return {
    timestamp: latestEntry.timestamp,
    benchmarkLabel: latestEntry.comparison_group ?? latestEntry.label ?? 'eval',
    winnerReviewer: formatReviewerIdentity(latestEntry),
    winnerReviewMode: latestEntry.review_mode,
    winnerModel: latestEntry.model,
    winnerProvider: latestEntry.provider,
    winnerUsefulnessScore: latestEntry.usefulness_score,
    winnerWeightedScore: latestEntry.weighted_score,
    winnerMicroF1: latestEntry.micro_f1,
    winnerPassRate: latestEntry.pass_rate,
    winnerVerificationHealth: latestEntry.verification_verified_pct,
    winnerLifecycleAccuracy: latestEntry.lifecycle_accuracy,
    comparison: undefined,
  }
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
    independentAuditorStory: buildIndependentAuditorStory(evalEntries),
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
      patternRepositoryFindingTotal: number
      patternRepositoryLabeledTotal: number
      patternRepositoryAcceptedTotal: number
      patternRepositoryRejectedTotal: number
      patternRepositoryReviewCount: number
      patternRepositorySourceCount: number
      patternRepositoryUtilizationRate: number
      patternRepositoryAcceptanceRate: number
      patternRepositoryBaselineLabeledTotal: number
      patternRepositoryBaselineAcceptanceRate?: number
      patternRepositoryAcceptanceLift?: number
      contextSourceFindingTotal: number
      contextSourceLabeledTotal: number
      contextSourceAcceptedTotal: number
      contextSourceRejectedTotal: number
      contextSourceResolvedTotal: number
      contextSourceReviewCount: number
      contextSourceSourceCount: number
      contextSourceUtilizationRate: number
      contextSourceAcceptanceRate: number
      contextSourceBaselineLabeledTotal: number
      contextSourceBaselineAcceptanceRate?: number
      contextSourceAcceptanceLift?: number
      contextSourceFixRate: number
      contextSourceBaselineFixRate?: number
      contextSourceFixLift?: number
      latestMicroF1?: number
      latestWeightedScore?: number
      latestAcceptanceRate?: number
      latestConfidenceF1?: number
    }
    coverageByReview: AnalyticsSnapshot['feedbackCoverageSeries']
    feedbackLearningByReview: AnalyticsSnapshot['feedbackLearningSeries']
    patternRepositoryByReview: AnalyticsSnapshot['patternRepositorySeries']
    patternRepositorySources: AnalyticsSnapshot['patternRepositorySourceData']
    contextSourceByReview: AnalyticsSnapshot['contextSourceSeries']
    contextSources: AnalyticsSnapshot['contextSourceData']
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

function appendPatternRepositoryRows(
  rows: AnalyticsExportCsvRow[],
  items: AnalyticsSnapshot['patternRepositorySourceData'],
) {
  items.forEach(item => {
    rows.push({ report: 'reinforcement', group: 'pattern_repository_sources', label: item.name, metric: 'total', value: item.total })
    rows.push({ report: 'reinforcement', group: 'pattern_repository_sources', label: item.name, metric: 'labeled', value: item.labeled })
    rows.push({ report: 'reinforcement', group: 'pattern_repository_sources', label: item.name, metric: 'accepted', value: item.accepted })
    rows.push({ report: 'reinforcement', group: 'pattern_repository_sources', label: item.name, metric: 'rejected', value: item.rejected })
    rows.push({ report: 'reinforcement', group: 'pattern_repository_sources', label: item.name, metric: 'review_count', value: item.reviewCount })
    rows.push({ report: 'reinforcement', group: 'pattern_repository_sources', label: item.name, metric: 'acceptance_rate', value: item.acceptanceRate })
  })
}

function appendContextSourceRows(
  rows: AnalyticsExportCsvRow[],
  items: AnalyticsSnapshot['contextSourceData'],
) {
  items.forEach(item => {
    rows.push({ report: 'reinforcement', group: 'context_sources', label: item.name, metric: 'total', value: item.total })
    rows.push({ report: 'reinforcement', group: 'context_sources', label: item.name, metric: 'labeled', value: item.labeled })
    rows.push({ report: 'reinforcement', group: 'context_sources', label: item.name, metric: 'accepted', value: item.accepted })
    rows.push({ report: 'reinforcement', group: 'context_sources', label: item.name, metric: 'rejected', value: item.rejected })
    rows.push({ report: 'reinforcement', group: 'context_sources', label: item.name, metric: 'resolved', value: item.resolved })
    rows.push({ report: 'reinforcement', group: 'context_sources', label: item.name, metric: 'review_count', value: item.reviewCount })
    rows.push({ report: 'reinforcement', group: 'context_sources', label: item.name, metric: 'acceptance_rate', value: item.acceptanceRate })
    rows.push({ report: 'reinforcement', group: 'context_sources', label: item.name, metric: 'fix_rate', value: item.fixRate })
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
        patternRepositoryFindingTotal: analytics.stats.patternRepositoryFindingTotal,
        patternRepositoryLabeledTotal: analytics.stats.patternRepositoryLabeledTotal,
        patternRepositoryAcceptedTotal: analytics.stats.patternRepositoryAcceptedTotal,
        patternRepositoryRejectedTotal: analytics.stats.patternRepositoryRejectedTotal,
        patternRepositoryReviewCount: analytics.stats.patternRepositoryReviewCount,
        patternRepositorySourceCount: analytics.stats.patternRepositorySourceCount,
        patternRepositoryUtilizationRate: analytics.stats.patternRepositoryUtilizationRate,
        patternRepositoryAcceptanceRate: analytics.stats.patternRepositoryAcceptanceRate,
        patternRepositoryBaselineLabeledTotal: analytics.stats.patternRepositoryBaselineLabeledTotal,
        patternRepositoryBaselineAcceptanceRate: analytics.stats.patternRepositoryBaselineAcceptanceRate ?? undefined,
        patternRepositoryAcceptanceLift: analytics.stats.patternRepositoryAcceptanceLift ?? undefined,
        contextSourceFindingTotal: analytics.stats.contextSourceFindingTotal,
        contextSourceLabeledTotal: analytics.stats.contextSourceLabeledTotal,
        contextSourceAcceptedTotal: analytics.stats.contextSourceAcceptedTotal,
        contextSourceRejectedTotal: analytics.stats.contextSourceRejectedTotal,
        contextSourceResolvedTotal: analytics.stats.contextSourceResolvedTotal,
        contextSourceReviewCount: analytics.stats.contextSourceReviewCount,
        contextSourceSourceCount: analytics.stats.contextSourceSourceCount,
        contextSourceUtilizationRate: analytics.stats.contextSourceUtilizationRate,
        contextSourceAcceptanceRate: analytics.stats.contextSourceAcceptanceRate,
        contextSourceBaselineLabeledTotal: analytics.stats.contextSourceBaselineLabeledTotal,
        contextSourceBaselineAcceptanceRate: analytics.stats.contextSourceBaselineAcceptanceRate ?? undefined,
        contextSourceAcceptanceLift: analytics.stats.contextSourceAcceptanceLift ?? undefined,
        contextSourceFixRate: analytics.stats.contextSourceFixRate,
        contextSourceBaselineFixRate: analytics.stats.contextSourceBaselineFixRate ?? undefined,
        contextSourceFixLift: analytics.stats.contextSourceFixLift ?? undefined,
        latestMicroF1: trendAnalytics.latestEval?.micro_f1,
        latestWeightedScore: trendAnalytics.latestEval?.weighted_score,
        latestAcceptanceRate: trendAnalytics.latestFeedback?.acceptance_rate,
        latestConfidenceF1: trendAnalytics.latestFeedback?.confidence_f1,
      },
      coverageByReview: analytics.feedbackCoverageSeries,
      feedbackLearningByReview: analytics.feedbackLearningSeries,
      patternRepositoryByReview: analytics.patternRepositorySeries,
      patternRepositorySources: analytics.patternRepositorySourceData,
      contextSourceByReview: analytics.contextSourceSeries,
      contextSources: analytics.contextSourceData,
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
  report.reinforcement.patternRepositoryByReview.forEach(point => {
    rows.push({ report: 'reinforcement', group: 'pattern_repository_by_review', label: point.label, metric: 'findings', value: point.findings })
    rows.push({ report: 'reinforcement', group: 'pattern_repository_by_review', label: point.label, metric: 'labeled', value: point.labeled })
    rows.push({ report: 'reinforcement', group: 'pattern_repository_by_review', label: point.label, metric: 'accepted', value: point.accepted })
    rows.push({ report: 'reinforcement', group: 'pattern_repository_by_review', label: point.label, metric: 'rejected', value: point.rejected })
    rows.push({ report: 'reinforcement', group: 'pattern_repository_by_review', label: point.label, metric: 'source_count', value: point.sourceCount })
    if (point.acceptanceRate != null) {
      rows.push({ report: 'reinforcement', group: 'pattern_repository_by_review', label: point.label, metric: 'acceptance_rate', value: point.acceptanceRate })
    }
  })
  appendPatternRepositoryRows(rows, report.reinforcement.patternRepositorySources)
  report.reinforcement.contextSourceByReview.forEach(point => {
    rows.push({ report: 'reinforcement', group: 'context_source_by_review', label: point.label, metric: 'findings', value: point.findings })
    rows.push({ report: 'reinforcement', group: 'context_source_by_review', label: point.label, metric: 'labeled', value: point.labeled })
    rows.push({ report: 'reinforcement', group: 'context_source_by_review', label: point.label, metric: 'accepted', value: point.accepted })
    rows.push({ report: 'reinforcement', group: 'context_source_by_review', label: point.label, metric: 'rejected', value: point.rejected })
    rows.push({ report: 'reinforcement', group: 'context_source_by_review', label: point.label, metric: 'resolved', value: point.resolved })
    rows.push({ report: 'reinforcement', group: 'context_source_by_review', label: point.label, metric: 'source_count', value: point.sourceCount })
    if (point.acceptanceRate != null) {
      rows.push({ report: 'reinforcement', group: 'context_source_by_review', label: point.label, metric: 'acceptance_rate', value: point.acceptanceRate })
    }
    if (point.fixRate != null) {
      rows.push({ report: 'reinforcement', group: 'context_source_by_review', label: point.label, metric: 'fix_rate', value: point.fixRate })
    }
  })
  appendContextSourceRows(rows, report.reinforcement.contextSources)
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
