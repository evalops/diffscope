import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from './client'
import type { StartReviewRequest } from './types'
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
