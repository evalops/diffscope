import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from './client'
import type { StartReviewRequest, StartPrReviewRequest } from './types'
import { REFETCH } from '../lib/constants'

export function useStatus() {
  return useQuery({
    queryKey: ['status'],
    queryFn: api.getStatus,
    refetchInterval: REFETCH.status,
  })
}

export function useReviews() {
  return useQuery({
    queryKey: ['reviews'],
    queryFn: api.listReviews,
    refetchInterval: REFETCH.reviews,
  })
}

export function useReview(id: string | undefined) {
  return useQuery({
    queryKey: ['review', id],
    queryFn: () => api.getReview(id!),
    enabled: !!id,
    refetchInterval: (query) => {
      const status = query.state.data?.status
      return status === 'Running' || status === 'Pending' ? REFETCH.activeReview : false
    },
  })
}

export function useStartReview() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (request: StartReviewRequest) => api.startReview(request),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['reviews'] })
    },
  })
}

export function useSubmitFeedback(reviewId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ commentId, action }: { commentId: string; action: 'accept' | 'reject' }) =>
      api.submitFeedback(reviewId, commentId, action),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['review', reviewId] })
    },
  })
}

export function useDoctor() {
  return useQuery({
    queryKey: ['doctor'],
    queryFn: api.getDoctor,
    enabled: false,
  })
}

export function useConfig() {
  return useQuery({
    queryKey: ['config'],
    queryFn: api.getConfig,
  })
}

export function useUpdateConfig() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (updates: Record<string, unknown>) => api.updateConfig(updates),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['config'] })
    },
  })
}

export function useGhStatus() {
  return useQuery({
    queryKey: ['gh-status'],
    queryFn: api.getGhStatus,
    refetchInterval: false,
  })
}

export function useGhRepos(params?: { page?: number; search?: string }, enabled = true) {
  return useQuery({
    queryKey: ['gh-repos', params],
    queryFn: () => api.getGhRepos(params),
    enabled,
  })
}

export function useGhPrs(repo: string | undefined, state?: string) {
  return useQuery({
    queryKey: ['gh-prs', repo, state],
    queryFn: () => api.getGhPrs(repo!, state),
    enabled: !!repo,
  })
}

export function useEvents(params?: {
  source?: string; model?: string; status?: string;
  time_from?: string; time_to?: string;
}) {
  return useQuery({
    queryKey: ['events', params],
    queryFn: () => api.listEvents(params),
    refetchInterval: REFETCH.reviews,
  })
}

export function useEventStats(params?: { time_from?: string; time_to?: string }) {
  return useQuery({
    queryKey: ['event-stats', params],
    queryFn: () => api.getEventStats(params),
    refetchInterval: REFETCH.reviews,
  })
}

export function useAnalyticsTrends() {
  return useQuery({
    queryKey: ['analytics-trends'],
    queryFn: api.getAnalyticsTrends,
    refetchInterval: REFETCH.reviews,
  })
}

export function useAgentTools() {
  return useQuery({
    queryKey: ['agent-tools'],
    queryFn: api.getAgentTools,
    staleTime: Infinity,
  })
}

export function useStartPrReview() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (request: StartPrReviewRequest) => api.startPrReview(request),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['reviews'] })
    },
  })
}
