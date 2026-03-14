import { describe, expect, it } from 'vitest'

import type { AnalyticsTrendsResponse, ReviewSession } from '../../api/types'
import { buildAnalyticsCsv, buildAnalyticsExportReport } from '../../lib/analytics'

function makeReview(): ReviewSession {
  return {
    id: 'review-1',
    status: 'Complete',
    diff_source: 'git:main',
    started_at: '2026-03-13T12:00:00Z',
    completed_at: '2026-03-13T12:05:00Z',
    files_reviewed: 3,
    comments: [
      {
        id: 'c-1',
        file_path: 'src/auth.ts',
        line_number: 10,
        content: 'Guard the tenant boundary before access checks',
        rule_id: 'sec.auth.boundary',
        severity: 'Error',
        category: 'Security',
        confidence: 0.95,
        tags: [],
        fix_effort: 'Medium',
        feedback: 'accept',
        status: 'Open',
      },
      {
        id: 'c-2',
        file_path: 'src/ui.ts',
        line_number: 42,
        content: 'This naming suggestion is optional',
        rule_id: 'style.naming',
        severity: 'Suggestion',
        category: 'Style',
        confidence: 0.6,
        tags: [],
        fix_effort: 'Low',
        feedback: 'reject',
        status: 'Resolved',
      },
    ],
    summary: {
      total_comments: 2,
      by_severity: { Error: 1, Warning: 0, Info: 0, Suggestion: 1 },
      by_category: { Security: 1, Style: 1 },
      critical_issues: 1,
      files_reviewed: 3,
      overall_score: 8.7,
      recommendations: [],
      open_comments: 1,
      open_by_severity: { Error: 1 },
      open_blocking_comments: 1,
      open_informational_comments: 0,
      resolved_comments: 1,
      dismissed_comments: 0,
      open_blockers: 1,
      completeness: {
        total_findings: 2,
        acknowledged_findings: 1,
        fixed_findings: 1,
        stale_findings: 0,
      },
      merge_readiness: 'NeedsAttention',
      verification: {
        state: 'Verified',
        judge_count: 2,
        required_votes: 2,
        warning_count: 0,
        filtered_comments: 0,
        abstained_comments: 0,
      },
      readiness_reasons: ['Open blocker remains'],
    },
  }
}

function makeTrends(): AnalyticsTrendsResponse {
  return {
    eval_trend_path: '.diffscope.eval-trend.json',
    feedback_eval_trend_path: '.diffscope.feedback-eval-trend.json',
    warnings: ['feedback trend lagging behind latest review'],
    eval_trend: {
      entries: [
        {
          timestamp: '2026-03-12T00:00:00Z',
          micro_f1: 0.82,
          micro_precision: 0.84,
          micro_recall: 0.8,
          fixture_count: 12,
          weighted_score: 0.79,
          suite_micro_f1: {},
          category_micro_f1: {},
          language_micro_f1: {},
          verification_warning_count: 1,
          verification_parse_failure_count: 0,
          verification_request_failure_count: 0,
        },
      ],
    },
    feedback_eval_trend: {
      entries: [
        {
          timestamp: '2026-03-12T00:00:00Z',
          labeled_comments: 6,
          accepted: 4,
          rejected: 2,
          acceptance_rate: 4 / 6,
          confidence_threshold: 0.8,
          confidence_agreement_rate: 0.75,
          confidence_f1: 0.71,
          attention_by_category: [
            {
              name: 'Security',
              feedback_total: 5,
              high_confidence_total: 2,
              high_confidence_acceptance_rate: 0.25,
              eval_score: 0.63,
              gap: -0.38,
            },
          ],
          attention_by_rule: [
            {
              name: 'sec.auth.boundary',
              feedback_total: 3,
              high_confidence_total: 1,
              high_confidence_acceptance_rate: 0.2,
              eval_score: 0.57,
              gap: -0.37,
            },
          ],
        },
      ],
    },
  }
}

describe('Analytics exports', () => {
  it('builds a report with review quality, lifecycle, and reinforcement sections', () => {
    const report = buildAnalyticsExportReport(
      [makeReview()],
      makeTrends(),
      '2026-03-14T00:00:00Z',
    )

    expect(report.generatedAt).toBe('2026-03-14T00:00:00Z')
    expect(report.reviewQuality.summary.totalReviews).toBe(1)
    expect(report.reviewQuality.summary.avgScore).toBe(8.7)
    expect(report.lifecycle.summary.totalOpenComments).toBe(1)
    expect(report.lifecycle.summary.totalResolvedComments).toBe(1)
    expect(report.lifecycle.summary.totalOpenBlockers).toBe(1)
    expect(report.reinforcement.summary.labeledFeedbackTotal).toBe(2)
    expect(report.reinforcement.summary.feedbackCoverageRate).toBe(1)
    expect(report.reinforcement.summary.feedbackAcceptanceRate).toBe(0.5)
    expect(report.reinforcement.latestAttentionGaps.byCategory[0].name).toBe('Security')
    expect(report.reinforcement.latestAttentionGaps.byRule[0].name).toBe('sec.auth.boundary')
    expect(report.sources.warnings).toContain('feedback trend lagging behind latest review')
  })

  it('flattens the analytics report into csv rows', () => {
    const csv = buildAnalyticsCsv(
      buildAnalyticsExportReport([makeReview()], makeTrends(), '2026-03-14T00:00:00Z'),
    )

    expect(csv).toContain('report,group,label,metric,value')
    expect(csv).toContain('"review_quality","summary","","totalReviews","1"')
    expect(csv).toContain('"lifecycle","summary","","totalOpenBlockers","1"')
    expect(csv).toContain('"reinforcement","summary","","feedbackCoverageRate","1"')
    expect(csv).toContain('"reinforcement","attention_gaps_by_category","Security","gap","-0.38"')
    expect(csv).toContain('"reinforcement","top_accepted_rules","sec.auth.boundary","accepted","1"')
  })
})
