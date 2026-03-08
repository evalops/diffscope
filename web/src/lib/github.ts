import { Octokit } from '@octokit/rest'

let octokit: Octokit | null = null

export function initGitHub(token: string) {
  octokit = new Octokit({ auth: token })
}

export function getOctokit(): Octokit | null {
  return octokit
}

export function clearGitHub() {
  octokit = null
}

export interface GhRepo {
  full_name: string
  description: string | null
  language: string | null
  updated_at: string
  default_branch: string
  open_issues_count: number
  stargazers_count: number
  private: boolean
}

export interface GhPullRequest {
  number: number
  title: string
  author: string
  state: string
  created_at: string
  updated_at: string
  additions: number
  deletions: number
  changed_files: number
  head_branch: string
  base_branch: string
  labels: string[]
  draft: boolean
}

export async function fetchUser() {
  if (!octokit) throw new Error('GitHub not connected')
  const { data } = await octokit.users.getAuthenticated()
  return { login: data.login, avatar_url: data.avatar_url, name: data.name }
}

export async function fetchRepos(page = 1, perPage = 20): Promise<GhRepo[]> {
  if (!octokit) throw new Error('GitHub not connected')
  const { data } = await octokit.repos.listForAuthenticatedUser({
    sort: 'updated',
    per_page: perPage,
    page,
  })
  return data.map(r => ({
    full_name: r.full_name,
    description: r.description,
    language: r.language,
    updated_at: r.updated_at ?? '',
    default_branch: r.default_branch,
    open_issues_count: r.open_issues_count,
    stargazers_count: r.stargazers_count,
    private: r.private,
  }))
}

export async function searchRepos(query: string, perPage = 20): Promise<GhRepo[]> {
  if (!octokit) throw new Error('GitHub not connected')
  const { data } = await octokit.search.repos({
    q: query,
    per_page: perPage,
    sort: 'updated',
  })
  return data.items.map(r => ({
    full_name: r.full_name,
    description: r.description ?? null,
    language: r.language ?? null,
    updated_at: r.updated_at ?? '',
    default_branch: r.default_branch ?? 'main',
    open_issues_count: r.open_issues_count ?? 0,
    stargazers_count: r.stargazers_count ?? 0,
    private: r.private ?? false,
  }))
}

export async function fetchPullRequests(owner: string, repo: string, state: 'open' | 'closed' | 'all' = 'open'): Promise<GhPullRequest[]> {
  if (!octokit) throw new Error('GitHub not connected')
  const { data } = await octokit.pulls.list({
    owner,
    repo,
    state,
    sort: 'updated',
    direction: 'desc',
    per_page: 30,
  })
  return data.map(pr => ({
    number: pr.number,
    title: pr.title,
    author: pr.user?.login ?? 'unknown',
    state: pr.state,
    created_at: pr.created_at,
    updated_at: pr.updated_at,
    additions: 0,
    deletions: 0,
    changed_files: 0,
    head_branch: pr.head.ref,
    base_branch: pr.base.ref,
    labels: pr.labels.map(l => typeof l === 'string' ? l : l.name ?? ''),
    draft: pr.draft ?? false,
  }))
}

export async function fetchPrDiff(owner: string, repo: string, prNumber: number): Promise<string> {
  if (!octokit) throw new Error('GitHub not connected')
  const { data } = await octokit.pulls.get({
    owner,
    repo,
    pull_number: prNumber,
    mediaType: { format: 'diff' },
  })
  return data as unknown as string
}

/** Map DiffScope severity to an emoji prefix */
function severityIcon(severity: string): string {
  switch (severity) {
    case 'Error': return ':rotating_light:'
    case 'Warning': return ':warning:'
    case 'Info': return ':information_source:'
    case 'Suggestion': return ':bulb:'
    default: return ':mag:'
  }
}

export interface ReviewComment {
  file_path: string
  line_number: number
  content: string
  severity: string
  category: string
  suggestion?: string
  confidence: number
  code_suggestion?: {
    original_code: string
    suggested_code: string
    explanation: string
  }
}

/**
 * Post review comments to a GitHub PR using the Pull Request Reviews API.
 * Creates a single review with inline comments on specific lines.
 */
export async function postReviewToGitHub(
  owner: string,
  repo: string,
  prNumber: number,
  comments: ReviewComment[],
  summary?: { overall_score: number; total_comments: number; recommendations: string[] },
): Promise<void> {
  if (!octokit) throw new Error('GitHub not connected')

  // Get the PR to find the latest commit SHA (required for inline comments)
  const { data: pr } = await octokit.pulls.get({
    owner,
    repo,
    pull_number: prNumber,
  })
  const commitId = pr.head.sha

  // Build inline review comments
  const reviewComments: Array<{
    path: string
    line: number
    side: 'RIGHT'
    body: string
  }> = []

  for (const c of comments) {
    // Build the comment body
    let body = `${severityIcon(c.severity)} **${c.severity}** | ${c.category}`
    if (c.confidence > 0) {
      body += ` | confidence: ${Math.round(c.confidence * 100)}%`
    }
    body += `\n\n${c.content}`

    if (c.suggestion) {
      body += `\n\n> **Suggestion:** ${c.suggestion}`
    }

    if (c.code_suggestion?.suggested_code) {
      body += `\n\n**Suggested fix:**\n\`\`\`suggestion\n${c.code_suggestion.suggested_code}\n\`\`\``
    }

    // file_path may have a leading slash or prefix — normalize
    const path = c.file_path.replace(/^\/+/, '').replace(/^[ab]\//, '')

    reviewComments.push({
      path,
      line: c.line_number,
      side: 'RIGHT' as const,
      body,
    })
  }

  // Build the review body (summary)
  let reviewBody = '## DiffScope Review\n\n'
  if (summary) {
    reviewBody += `**Score:** ${summary.overall_score}/10 | **Findings:** ${summary.total_comments}\n\n`
    if (summary.recommendations.length > 0) {
      reviewBody += '**Recommendations:**\n'
      for (const rec of summary.recommendations) {
        reviewBody += `- ${rec}\n`
      }
      reviewBody += '\n'
    }
  } else {
    reviewBody += `Found **${comments.length}** issue${comments.length === 1 ? '' : 's'}.\n\n`
  }
  reviewBody += '_Automated review by [DiffScope](https://github.com/haasonsaas/diffscope)_'

  // Determine event type based on severity
  const hasErrors = comments.some(c => c.severity === 'Error')
  const event = hasErrors ? 'REQUEST_CHANGES' : 'COMMENT'

  // Create the review with inline comments
  await octokit.pulls.createReview({
    owner,
    repo,
    pull_number: prNumber,
    commit_id: commitId,
    body: reviewBody,
    event,
    comments: reviewComments,
  })
}
