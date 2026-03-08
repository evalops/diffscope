const BASE = '/api'

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...options,
  })
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText)
    throw new Error(`API error ${res.status}: ${text}`)
  }
  return res.json()
}

export const api = {
  getStatus: () => request<import('./types').StatusResponse>('/status'),

  startReview: (body: import('./types').StartReviewRequest) =>
    request<{ id: string; status: string }>('/review', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  getReview: (id: string) => request<import('./types').ReviewSession>(`/review/${id}`),

  listReviews: () => request<import('./types').ReviewSession[]>('/reviews'),

  submitFeedback: (reviewId: string, commentId: string, action: 'accept' | 'reject') =>
    request<{ ok: boolean }>(`/review/${reviewId}/feedback`, {
      method: 'POST',
      body: JSON.stringify({ comment_id: commentId, action }),
    }),

  getDoctor: () => request<import('./types').DoctorResponse>('/doctor'),

  getConfig: () => request<Record<string, unknown>>('/config'),

  updateConfig: (updates: Record<string, unknown>) =>
    request<Record<string, unknown>>('/config', {
      method: 'PUT',
      body: JSON.stringify(updates),
    }),

  testProvider: (req: import('./types').TestProviderRequest) =>
    request<import('./types').TestProviderResponse>('/providers/test', {
      method: 'POST',
      body: JSON.stringify(req),
    }),

  getGhStatus: () => request<import('./types').GhStatusResponse>('/gh/status'),

  getGhRepos: (params?: { page?: number; per_page?: number; search?: string }) => {
    const qs = new URLSearchParams()
    if (params?.page) qs.set('page', String(params.page))
    if (params?.per_page) qs.set('per_page', String(params.per_page))
    if (params?.search) qs.set('search', params.search)
    const suffix = qs.toString() ? `?${qs}` : ''
    return request<import('./types').GhRepo[]>(`/gh/repos${suffix}`)
  },

  getGhPrs: (repo: string, state?: string) => {
    const qs = new URLSearchParams({ repo })
    if (state) qs.set('state', state)
    return request<import('./types').GhPullRequest[]>(`/gh/prs?${qs}`)
  },

  startPrReview: (body: import('./types').StartPrReviewRequest) =>
    request<{ id: string; status: string }>('/gh/review', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  reviewDiff: (diffContent: string, title?: string) =>
    request<{ id: string; status: string }>('/review', {
      method: 'POST',
      body: JSON.stringify({ diff_source: 'raw', diff_content: diffContent, title }),
    }),
}
