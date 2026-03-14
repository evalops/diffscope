import { useState, useEffect, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { ArrowLeft, Search, Lock, Star, GitPullRequest, Loader2, ChevronRight, RefreshCw, X, ExternalLink, Copy, Check, Webhook, Eye, EyeOff } from 'lucide-react'
import { useGhStatus, useGhRepos, useGhPrReadiness, useGhPrs, useStartPrReview, useUpdateConfig, useConfig } from '../api/hooks'
import { api } from '../api/client'
import type { GhRepo, GhPullRequest, DeviceFlowResponse, MergeReadiness } from '../api/types'
import { PrReadinessSummary } from '../components/PrReadinessSummary'

const LANG_COLORS: Record<string, string> = {
  TypeScript: '#3178c6',
  JavaScript: '#f1e05a',
  Rust: '#dea584',
  Python: '#3572a5',
  Go: '#00add8',
  Java: '#b07219',
  'C++': '#f34b7d',
  C: '#555555',
  Ruby: '#701516',
  Swift: '#f05138',
  Kotlin: '#a97bff',
  Shell: '#89e051',
  HTML: '#e34c26',
  CSS: '#563d7c',
  Dart: '#00b4ab',
  PHP: '#4f5d95',
  Scala: '#c22d40',
  Elixir: '#6e4a7e',
  Haskell: '#5e5086',
  Lua: '#000080',
  Zig: '#ec915c',
  Vue: '#41b883',
}

const READINESS_STYLES: Record<MergeReadiness, string> = {
  Ready: 'bg-sev-suggestion/10 text-sev-suggestion border border-sev-suggestion/20',
  NeedsAttention: 'bg-sev-warning/10 text-sev-warning border border-sev-warning/20',
  NeedsReReview: 'bg-accent/10 text-accent border border-accent/20',
}

const READINESS_LABELS: Record<MergeReadiness, string> = {
  Ready: 'Ready',
  NeedsAttention: 'Attention',
  NeedsReReview: 'Re-review',
}

function timeAgo(dateStr: string): string {
  if (!dateStr) return ''
  const now = Date.now()
  const then = new Date(dateStr).getTime()
  const seconds = Math.floor((now - then) / 1000)
  if (seconds < 60) return 'just now'
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days}d ago`
  const months = Math.floor(days / 30)
  if (months < 12) return `${months}mo ago`
  const years = Math.floor(months / 12)
  return `${years}y ago`
}

type View = 'repos' | 'prs' | 'pr-detail'

export function Repos() {
  const navigate = useNavigate()

  // View state
  const [view, setView] = useState<View>('repos')
  const [searchQuery, setSearchQuery] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const [reposPage, setReposPage] = useState(1)
  const [selectedRepo, setSelectedRepo] = useState<GhRepo | null>(null)
  const [selectedPr, setSelectedPr] = useState<GhPullRequest | null>(null)
  const [prFilter, setPrFilter] = useState<'open' | 'closed' | 'all' | 'merged'>('open')
  const [postResults, setPostResults] = useState(false)

  // Backend queries
  const { data: ghStatus, refetch: refetchGhStatus } = useGhStatus()
  const { data: config } = useConfig()
  const updateConfig = useUpdateConfig()
  const connected = ghStatus?.authenticated ?? false
  const username = ghStatus?.username
  const avatarUrl = ghStatus?.avatar_url

  // Check if GitHub App is configured (has client_id)
  const hasAppConfig = config && typeof config === 'object' &&
    !!(config as Record<string, unknown>).github_client_id &&
    (config as Record<string, unknown>).github_client_id !== ''

  const reposParams = debouncedSearch
    ? { search: debouncedSearch }
    : { page: reposPage }
  const { data: repos, isLoading: reposLoading, error: reposError } = useGhRepos(reposParams, connected)
  const { data: prs, isLoading: prsLoading, error: prsError } = useGhPrs(
    selectedRepo?.full_name,
    prFilter,
  )
  const {
    data: prReadiness,
    isLoading: prReadinessLoading,
    error: prReadinessError,
  } = useGhPrReadiness(selectedRepo?.full_name, selectedPr?.number)
  const startPrReview = useStartPrReview()

  // Debounce search
  useEffect(() => {
    const timeout = setTimeout(() => {
      setDebouncedSearch(searchQuery)
      setReposPage(1)
    }, 300)
    return () => clearTimeout(timeout)
  }, [searchQuery])

  const handleDisconnect = async () => {
    try {
      await api.disconnectGitHub()
      await refetchGhStatus()
      setSelectedRepo(null)
      setSelectedPr(null)
      setView('repos')
    } catch {
      // ignore
    }
  }

  const handleSelectRepo = (repo: GhRepo) => {
    setSelectedRepo(repo)
    setView('prs')
  }

  const handleSelectPr = (pr: GhPullRequest) => {
    setSelectedPr(pr)
    setView('pr-detail')
  }

  const handleReview = async () => {
    if (!selectedRepo || !selectedPr) return
    try {
      const result = await startPrReview.mutateAsync({
        repo: selectedRepo.full_name,
        pr_number: selectedPr.number,
        post_results: postResults,
      })
      navigate(`/review/${result.id}`)
    } catch {
      // error handled by mutation state
    }
  }

  const handleBackToRepos = () => {
    setView('repos')
    setSelectedRepo(null)
  }

  const handleBackToPrs = () => {
    setView('prs')
    setSelectedPr(null)
  }

  // Not connected — show auth options
  if (!connected) {
    return (
      <div className="p-6 max-w-2xl mx-auto">
        <h1 className="text-xl font-semibold text-text-primary mb-4">GitHub Repos</h1>

        {hasAppConfig ? (
          <DeviceFlowAuth onSuccess={() => refetchGhStatus()} />
        ) : (
          <GitHubSetup
            config={config as Record<string, unknown> | undefined}
            updateConfig={updateConfig}
            refetchGhStatus={refetchGhStatus}
          />
        )}
      </div>
    )
  }

  // Render repo list
  const renderRepoList = () => (
    <>
      <div className="relative mb-4">
        <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-text-muted" />
        <input
          type="text"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search repositories..."
          className="w-full bg-surface-1 border border-border rounded-lg pl-9 pr-9 py-2 text-[13px] text-text-primary placeholder:text-text-muted/30 focus:outline-none focus:ring-1 focus:ring-accent"
        />
        {searchQuery && (
          <button onClick={() => setSearchQuery('')} className="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-secondary">
            <X size={14} />
          </button>
        )}
      </div>

      {reposError && (
        <div className="bg-surface-1 border border-sev-error/30 rounded-lg p-4 mb-4">
          <p className="text-[12px] text-sev-error">{reposError instanceof Error ? reposError.message : 'Failed to load repos'}</p>
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        {(repos ?? []).map(repo => (
          <button
            key={repo.full_name}
            onClick={() => handleSelectRepo(repo)}
            className="bg-surface-1 border border-border rounded-lg p-4 text-left hover:border-text-muted transition-colors group"
          >
            <div className="flex items-center gap-1.5 mb-1.5">
              {repo.private && <Lock size={12} className="text-text-muted shrink-0" />}
              <span className="text-[13px] font-medium text-text-primary truncate group-hover:text-accent transition-colors">
                {repo.full_name}
              </span>
              <ChevronRight size={14} className="text-text-muted ml-auto shrink-0 opacity-0 group-hover:opacity-100 transition-opacity" />
            </div>
            {repo.description && (
              <p className="text-[11px] text-text-secondary line-clamp-2 mb-2">{repo.description}</p>
            )}
            <div className="flex items-center gap-3 text-[10px] text-text-muted">
              {repo.language && (
                <span className="flex items-center gap-1">
                  <span className="w-2 h-2 rounded-full shrink-0" style={{ backgroundColor: LANG_COLORS[repo.language] ?? '#8b949e' }} />
                  {repo.language}
                </span>
              )}
              {repo.stargazers_count > 0 && (
                <span className="flex items-center gap-0.5"><Star size={10} />{repo.stargazers_count}</span>
              )}
              <span>{timeAgo(repo.updated_at)}</span>
            </div>
            {repo.open_blockers !== undefined && repo.blocking_prs !== undefined && (repo.open_blockers > 0 || repo.blocking_prs > 0) && (
              <div className="mt-2 flex items-center gap-2 text-[10px]">
                <span className="px-1.5 py-0.5 rounded font-medium bg-sev-warning/10 text-sev-warning border border-sev-warning/20">
                  {repo.open_blockers} blocker{repo.open_blockers === 1 ? '' : 's'}
                </span>
                <span className="text-text-muted">
                  across {repo.blocking_prs} reviewed PR{repo.blocking_prs === 1 ? '' : 's'}
                </span>
              </div>
            )}
          </button>
        ))}
      </div>

      {reposLoading && (
        <div className="flex justify-center py-6"><Loader2 size={20} className="animate-spin text-text-muted" /></div>
      )}

      {!reposLoading && !debouncedSearch && (repos ?? []).length >= 20 && (
        <div className="flex justify-center pt-4">
          <button onClick={() => setReposPage(p => p + 1)} className="px-4 py-1.5 rounded text-[12px] font-medium bg-surface-2 border border-border text-text-secondary hover:text-text-primary hover:border-text-muted transition-colors">
            Load more
          </button>
        </div>
      )}

      {!reposLoading && (repos ?? []).length === 0 && (
        <div className="text-center py-8 text-[13px] text-text-muted">
          {searchQuery ? 'No repositories found.' : 'No repositories to display.'}
        </div>
      )}
    </>
  )

  // Render PR list
  const renderPrList = () => (
    <>
      <button onClick={handleBackToRepos} className="flex items-center gap-1.5 text-[12px] text-text-muted hover:text-text-secondary transition-colors mb-3">
        <ArrowLeft size={14} />repos
      </button>

      <div className="flex items-center justify-between mb-4">
        <h2 className="text-[15px] font-semibold text-text-primary">{selectedRepo?.full_name}</h2>
        <div className="flex gap-1">
          {(['open', 'closed', 'merged', 'all'] as const).map(f => (
            <button
              key={f}
              onClick={() => setPrFilter(f)}
              className={`px-2.5 py-1 rounded text-[11px] font-medium transition-colors ${
                prFilter === f
                  ? 'bg-accent/15 text-accent border border-accent/30'
                  : 'bg-surface text-text-muted border border-border hover:text-text-secondary'
              }`}
            >
              {f}
            </button>
          ))}
        </div>
      </div>

      {prsError && (
        <div className="bg-surface-1 border border-sev-error/30 rounded-lg p-4 mb-4">
          <p className="text-[12px] text-sev-error">{prsError instanceof Error ? prsError.message : 'Failed to load PRs'}</p>
        </div>
      )}

      {prsLoading ? (
        <div className="flex justify-center py-8"><Loader2 size={20} className="animate-spin text-text-muted" /></div>
      ) : (prs ?? []).length === 0 ? (
        <div className="text-center py-8 text-[13px] text-text-muted">
          No {prFilter === 'all' ? '' : prFilter} pull requests.
        </div>
      ) : (
        <div className="space-y-2">
          {(prs ?? []).map(pr => (
            <button
              key={pr.number}
              onClick={() => handleSelectPr(pr)}
              className="w-full bg-surface-1 border border-border rounded-lg p-3 text-left hover:border-text-muted transition-colors group"
            >
              <div className="flex items-start gap-2">
                <GitPullRequest size={14} className={`mt-0.5 shrink-0 ${
                  pr.state === 'open' ? 'text-accent' : pr.state === 'merged' ? 'text-purple-400' : 'text-sev-error'
                }`} />
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 mb-1">
                    <span className="text-[13px] font-medium text-text-primary group-hover:text-accent transition-colors truncate">{pr.title}</span>
                    <span className="text-[11px] text-text-muted shrink-0">#{pr.number}</span>
                  </div>
                  <div className="flex items-center gap-3 text-[10px] text-text-muted">
                    <span>{pr.author}</span>
                    <span>{timeAgo(pr.created_at)}</span>
                    <span className="font-code">{pr.head_branch} <span className="text-text-muted/50">&rarr;</span> {pr.base_branch}</span>
                  </div>
                  {(pr.draft || pr.labels.length > 0 || pr.merge_readiness || pr.open_blockers !== undefined) && (
                    <div className="flex items-center gap-1.5 mt-1.5 flex-wrap">
                      {pr.draft && <span className="px-1.5 py-0.5 rounded text-[9px] font-medium bg-surface-2 text-text-muted border border-border">Draft</span>}
                      {pr.labels.map(label => (
                        <span key={label} className="px-1.5 py-0.5 rounded text-[9px] font-medium bg-accent/10 text-accent border border-accent/20">{label}</span>
                      ))}
                      {pr.merge_readiness && (
                        <span className={`px-1.5 py-0.5 rounded text-[9px] font-medium ${READINESS_STYLES[pr.merge_readiness]}`}>
                          {READINESS_LABELS[pr.merge_readiness]}
                        </span>
                      )}
                      {pr.open_blockers !== undefined && (
                        <span className={`px-1.5 py-0.5 rounded text-[9px] font-medium border ${pr.open_blockers > 0
                          ? 'bg-sev-warning/10 text-sev-warning border-sev-warning/20'
                          : 'bg-surface-2 text-text-muted border-border'}`}>
                          {pr.open_blockers} blocker{pr.open_blockers === 1 ? '' : 's'}
                        </span>
                      )}
                    </div>
                  )}
                </div>
                <ChevronRight size={14} className="text-text-muted mt-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity" />
              </div>
            </button>
          ))}
        </div>
      )}
    </>
  )

  // Render PR detail
  const renderPrDetail = () => {
    if (!selectedPr || !selectedRepo) return null
    return (
      <>
        <button onClick={handleBackToPrs} className="flex items-center gap-1.5 text-[12px] text-text-muted hover:text-text-secondary transition-colors mb-3">
          <ArrowLeft size={14} />PRs
        </button>

        <div className="bg-surface-1 border border-border rounded-lg p-4 mb-4">
          <div className="flex items-start gap-2 mb-3">
            <GitPullRequest size={16} className={`mt-0.5 shrink-0 ${
              selectedPr.state === 'open' ? 'text-accent' : selectedPr.state === 'merged' ? 'text-purple-400' : 'text-sev-error'
            }`} />
            <div>
              <h2 className="text-[15px] font-semibold text-text-primary">
                {selectedPr.title}<span className="text-text-muted font-normal ml-2">#{selectedPr.number}</span>
              </h2>
              <p className="text-[11px] text-text-muted mt-1">{selectedRepo.full_name}</p>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3 text-[12px] mb-4">
            <div>
              <span className="text-text-muted">Author</span>
              <div className="text-text-primary font-code mt-0.5">{selectedPr.author}</div>
            </div>
            <div>
              <span className="text-text-muted">State</span>
              <div className="mt-0.5">
                <span className={`px-2 py-0.5 rounded text-[10px] font-medium ${
                  selectedPr.state === 'open' ? 'bg-accent/15 text-accent border border-accent/30'
                    : selectedPr.state === 'merged' ? 'bg-purple-400/15 text-purple-400 border border-purple-400/30'
                    : 'bg-sev-error/15 text-sev-error border border-sev-error/30'
                }`}>
                  {selectedPr.draft ? 'Draft' : selectedPr.state}
                </span>
              </div>
            </div>
            <div>
              <span className="text-text-muted">Branches</span>
              <div className="text-text-primary font-code mt-0.5 text-[11px]">{selectedPr.head_branch} &rarr; {selectedPr.base_branch}</div>
            </div>
            <div>
              <span className="text-text-muted">Updated</span>
              <div className="text-text-secondary mt-0.5">{timeAgo(selectedPr.updated_at)}</div>
            </div>
          </div>

          {selectedPr.labels.length > 0 && (
            <div className="flex items-center gap-1.5 mb-4">
              {selectedPr.labels.map(label => (
                <span key={label} className="px-1.5 py-0.5 rounded text-[9px] font-medium bg-accent/10 text-accent border border-accent/20">{label}</span>
              ))}
            </div>
          )}

          <div className="border-t border-border-subtle pt-4">
            <PrReadinessSummary
              readiness={prReadiness}
              isLoading={prReadinessLoading}
              error={prReadinessError}
              onOpenReview={(reviewId) => navigate(`/review/${reviewId}`)}
            />

            <div className="flex items-center justify-between mb-4">
              <div>
                <div className="text-[13px] text-text-primary">Post results to GitHub</div>
                <div className="text-[11px] text-text-muted mt-0.5">Post inline review comments on the PR</div>
              </div>
              <button
                onClick={() => setPostResults(!postResults)}
                className={`relative w-10 h-[22px] rounded-full transition-colors ${postResults ? 'bg-toggle-on' : 'bg-toggle-off'}`}
              >
                <span className={`absolute top-[3px] w-4 h-4 rounded-full bg-white shadow transition-transform ${postResults ? 'left-[22px]' : 'left-[3px]'}`} />
              </button>
            </div>

            <button
              onClick={handleReview}
              disabled={startPrReview.isPending}
              className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-[13px] font-medium bg-accent text-surface hover:bg-accent-dim disabled:opacity-50 transition-colors"
            >
              {startPrReview.isPending ? (
                <><Loader2 size={16} className="animate-spin" />Starting review...</>
              ) : (
                <><RefreshCw size={14} />Review this PR</>
              )}
            </button>

            {startPrReview.isError && (
              <div className="mt-3 flex items-center gap-2 text-[12px] text-sev-error">
                <span className="inline-block w-2 h-2 rounded-full bg-sev-error" />
                {startPrReview.error instanceof Error ? startPrReview.error.message : 'Failed to start review'}
              </div>
            )}
          </div>
        </div>
      </>
    )
  }

  return (
    <div className="p-6 max-w-3xl mx-auto">
      <div className="flex items-center justify-between mb-4">
        <h1 className="text-xl font-semibold text-text-primary">GitHub Repos</h1>
        {username && (
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2">
              {avatarUrl && <img src={avatarUrl} alt={username} className="w-6 h-6 rounded-full" />}
              <span className="text-[12px] text-text-secondary font-code">{username}</span>
              <span className="inline-block w-2 h-2 rounded-full bg-accent" />
            </div>
            <button onClick={handleDisconnect} className="text-[11px] text-text-muted hover:text-sev-error transition-colors">
              Disconnect
            </button>
          </div>
        )}
      </div>

      {view === 'repos' && renderRepoList()}
      {view === 'prs' && renderPrList()}
      {view === 'pr-detail' && renderPrDetail()}
    </div>
  )
}

// ── Device Flow Auth Component ─────────────────────────────────────────

function DeviceFlowAuth({ onSuccess }: { onSuccess: () => void }) {
  const [flow, setFlow] = useState<DeviceFlowResponse | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const startFlow = async () => {
    setLoading(true)
    setError(null)
    try {
      const resp = await api.startDeviceFlow()
      setFlow(resp)
      // Start polling
      startPolling(resp.device_code, resp.interval)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to start auth flow')
    } finally {
      setLoading(false)
    }
  }

  const startPolling = (deviceCode: string, interval: number) => {
    if (pollRef.current) clearInterval(pollRef.current)
    pollRef.current = setInterval(async () => {
      try {
        const resp = await api.pollDeviceFlow(deviceCode)
        if (resp.authenticated) {
          if (pollRef.current) clearInterval(pollRef.current)
          setFlow(null)
          onSuccess()
        } else if (resp.error && resp.error !== 'authorization_pending' && resp.error !== 'slow_down') {
          if (pollRef.current) clearInterval(pollRef.current)
          setError(resp.error === 'expired_token' ? 'Authorization expired. Please try again.' : resp.error)
          setFlow(null)
        }
      } catch {
        // Network error, keep polling
      }
    }, (interval || 5) * 1000)
  }

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (pollRef.current) clearInterval(pollRef.current)
    }
  }, [])

  const handleCopy = () => {
    if (flow?.user_code) {
      navigator.clipboard.writeText(flow.user_code)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    }
  }

  return (
    <div className="bg-surface-1 border border-border rounded-lg p-4">
      <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">CONNECT GITHUB</div>

      {!flow ? (
        <>
          <p className="text-[12px] text-text-secondary mb-4">
            Connect your GitHub account to browse repositories, review pull requests, and post inline comments.
          </p>
          <button
            onClick={startFlow}
            disabled={loading}
            className="flex items-center justify-center gap-2 w-full px-4 py-2.5 rounded-lg text-[13px] font-medium bg-accent text-surface hover:bg-accent-dim disabled:opacity-50 transition-colors"
          >
            {loading ? (
              <><Loader2 size={16} className="animate-spin" />Connecting...</>
            ) : (
              <>Connect with GitHub</>
            )}
          </button>
        </>
      ) : (
        <div className="text-center">
          <p className="text-[12px] text-text-secondary mb-4">
            Enter this code on GitHub to authorize DiffScope:
          </p>

          <div className="flex items-center justify-center gap-2 mb-4">
            <code className="text-2xl font-bold font-code text-accent tracking-[0.15em] bg-surface px-6 py-3 rounded-lg border border-accent/30">
              {flow.user_code}
            </code>
            <button
              onClick={handleCopy}
              className="p-2 rounded text-text-muted hover:text-accent transition-colors"
              title="Copy code"
            >
              {copied ? <Check size={16} className="text-accent" /> : <Copy size={16} />}
            </button>
          </div>

          <a
            href={flow.verification_uri}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg text-[13px] font-medium bg-surface-2 border border-border text-text-primary hover:border-accent/30 hover:text-accent transition-colors mb-4"
          >
            Open GitHub <ExternalLink size={13} />
          </a>

          <div className="flex items-center justify-center gap-2 text-[11px] text-text-muted">
            <Loader2 size={12} className="animate-spin" />
            Waiting for authorization...
          </div>
        </div>
      )}

      {error && (
        <div className="mt-3 flex items-center gap-2 text-[12px] text-sev-error">
          <span className="inline-block w-2 h-2 rounded-full bg-sev-error" />
          {error}
        </div>
      )}
    </div>
  )
}

// ── GitHub App Setup Component (first-time setup) ──────────────────────

function GitHubSetup({
  config,
  updateConfig,
  refetchGhStatus,
}: {
  config: Record<string, unknown> | undefined
  updateConfig: ReturnType<typeof useUpdateConfig>
  refetchGhStatus: () => void
}) {
  const [mode, setMode] = useState<'app' | 'pat'>('app')
  const [clientId, setClientId] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [tokenInput, setTokenInput] = useState('')
  const [showToken, setShowToken] = useState(false)

  // Check if there's already a token configured
  const hasToken = config?.github_token === '***'

  const handleSaveApp = async () => {
    if (!clientId.trim()) return
    setSaving(true)
    setError(null)
    try {
      await updateConfig.mutateAsync({ github_client_id: clientId.trim() })
      // Trigger re-render to show device flow
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save')
    } finally {
      setSaving(false)
    }
  }

  const handleSavePat = async () => {
    if (!tokenInput.trim()) return
    setSaving(true)
    setError(null)
    try {
      await updateConfig.mutateAsync({ github_token: tokenInput.trim() })
      await refetchGhStatus()
      setTokenInput('')
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save token')
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="space-y-4">
      {hasToken && (
        <div className="flex items-center gap-2 text-[12px] text-sev-warning bg-sev-warning/5 border border-sev-warning/20 rounded px-3 py-2">
          A GitHub token is configured but authentication failed. Update it below.
        </div>
      )}

      {/* Mode tabs */}
      <div className="flex gap-1 bg-surface-1 border border-border rounded-lg p-1">
        <button
          onClick={() => setMode('app')}
          className={`flex-1 px-3 py-1.5 rounded text-[12px] font-medium transition-colors ${
            mode === 'app' ? 'bg-accent/15 text-accent' : 'text-text-muted hover:text-text-secondary'
          }`}
        >
          GitHub App (recommended)
        </button>
        <button
          onClick={() => setMode('pat')}
          className={`flex-1 px-3 py-1.5 rounded text-[12px] font-medium transition-colors ${
            mode === 'pat' ? 'bg-accent/15 text-accent' : 'text-text-muted hover:text-text-secondary'
          }`}
        >
          Personal Access Token
        </button>
      </div>

      {mode === 'app' ? (
        <div className="bg-surface-1 border border-border rounded-lg p-4">
          <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">GITHUB APP SETUP</div>

          <div className="space-y-3 text-[12px] text-text-secondary mb-4">
            <p>Create a GitHub App for DiffScope to enable OAuth login, webhooks, and check runs.</p>
            <ol className="list-decimal list-inside space-y-1.5 text-[11px]">
              <li>Go to <a href="https://github.com/settings/apps/new" target="_blank" rel="noopener noreferrer" className="text-accent hover:underline">github.com/settings/apps/new</a></li>
              <li>Set <strong>Homepage URL</strong> to your DiffScope server URL</li>
              <li>Enable <strong>Device flow</strong> under OAuth settings</li>
              <li>Set <strong>Webhook URL</strong> to <code className="font-code text-accent bg-surface px-1 py-0.5 rounded text-[10px]">&lt;your-url&gt;/api/webhooks/github</code></li>
              <li>Add permissions: <code className="font-code text-accent bg-surface px-1 py-0.5 rounded text-[10px]">Pull requests: Read & Write</code>, <code className="font-code text-accent bg-surface px-1 py-0.5 rounded text-[10px]">Checks: Read & Write</code>, <code className="font-code text-accent bg-surface px-1 py-0.5 rounded text-[10px]">Contents: Read</code></li>
              <li>Subscribe to events: <code className="font-code text-accent bg-surface px-1 py-0.5 rounded text-[10px]">Pull request</code></li>
              <li>Copy the <strong>Client ID</strong> and paste below</li>
            </ol>
          </div>

          <div className="flex gap-2">
            <input
              type="text"
              value={clientId}
              onChange={(e) => setClientId(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleSaveApp()}
              placeholder="Iv1.abc123..."
              className="flex-1 bg-surface border border-border rounded px-3 py-1.5 text-[13px] text-text-primary placeholder:text-text-muted/30 focus:outline-none focus:ring-1 focus:ring-accent font-code"
            />
            <button
              onClick={handleSaveApp}
              disabled={!clientId.trim() || saving}
              className="px-4 py-1.5 rounded text-[12px] font-medium bg-accent text-surface hover:bg-accent-dim disabled:opacity-50 transition-colors"
            >
              {saving ? <Loader2 size={14} className="animate-spin" /> : 'Save'}
            </button>
          </div>

          <div className="mt-3 flex items-center gap-3 text-[10px] text-text-muted">
            <span className="flex items-center gap-1"><Webhook size={10} /> Webhooks</span>
            <span>Auto-review on PR open</span>
            <span className="text-text-muted/50">|</span>
            <span>Check Runs on commits</span>
            <span className="text-text-muted/50">|</span>
            <span>Bot identity</span>
          </div>
        </div>
      ) : (
        <div className="bg-surface-1 border border-border rounded-lg p-4">
          <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">PERSONAL ACCESS TOKEN</div>
          <p className="text-[12px] text-text-secondary mb-3">
            Quick setup with a PAT. For webhooks and check runs, use the GitHub App setup instead.
          </p>
          <p className="text-[11px] text-text-muted mb-4">
            Generate at{' '}
            <a href="https://github.com/settings/tokens" target="_blank" rel="noopener noreferrer" className="text-accent hover:underline">
              github.com/settings/tokens
            </a>
            {' '}&mdash; needs <code className="font-code text-accent bg-surface px-1 py-0.5 rounded text-[10px]">repo</code> scope.
          </p>
          <div className="flex gap-2">
            <div className="relative flex-1">
              <input
                type={showToken ? 'text' : 'password'}
                value={tokenInput}
                onChange={(e) => setTokenInput(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleSavePat()}
                placeholder="ghp_..."
                className="w-full bg-surface border border-border rounded px-3 py-1.5 pr-9 text-[13px] text-text-primary placeholder:text-text-muted/30 focus:outline-none focus:ring-1 focus:ring-accent font-code"
              />
              <button
                type="button"
                onClick={() => setShowToken(s => !s)}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-secondary"
              >
                {showToken ? <EyeOff size={14} /> : <Eye size={14} />}
              </button>
            </div>
            <button
              onClick={handleSavePat}
              disabled={!tokenInput.trim() || saving}
              className="px-4 py-1.5 rounded text-[12px] font-medium bg-accent text-surface hover:bg-accent-dim disabled:opacity-50 transition-colors"
            >
              {saving ? <Loader2 size={14} className="animate-spin" /> : 'Connect'}
            </button>
          </div>
        </div>
      )}

      {error && (
        <div className="flex items-center gap-2 text-[12px] text-sev-error">
          <span className="inline-block w-2 h-2 rounded-full bg-sev-error" />
          {error}
        </div>
      )}
    </div>
  )
}
