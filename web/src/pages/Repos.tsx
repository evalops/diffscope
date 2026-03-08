import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { ArrowLeft, Search, Lock, Star, GitPullRequest, Loader2, ChevronRight, RefreshCw, X, Eye, EyeOff } from 'lucide-react'
import { useGhStatus, useGhRepos, useGhPrs, useStartPrReview, useUpdateConfig, useConfig } from '../api/hooks'
import type { GhRepo, GhPullRequest } from '../api/types'

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

  // Auth state
  const [tokenInput, setTokenInput] = useState('')
  const [showToken, setShowToken] = useState(false)
  const [tokenError, setTokenError] = useState<string | null>(null)
  const [savingToken, setSavingToken] = useState(false)

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

  const reposParams = debouncedSearch
    ? { search: debouncedSearch }
    : { page: reposPage }
  const { data: repos, isLoading: reposLoading, error: reposError } = useGhRepos(reposParams, connected)
  const { data: prs, isLoading: prsLoading, error: prsError } = useGhPrs(
    selectedRepo?.full_name,
    prFilter,
  )
  const startPrReview = useStartPrReview()

  // Debounce search
  useEffect(() => {
    const timeout = setTimeout(() => {
      setDebouncedSearch(searchQuery)
      setReposPage(1)
    }, 300)
    return () => clearTimeout(timeout)
  }, [searchQuery])

  const handleConnect = async () => {
    if (!tokenInput.trim()) return
    setSavingToken(true)
    setTokenError(null)
    try {
      await updateConfig.mutateAsync({ github_token: tokenInput.trim() })
      await refetchGhStatus()
      // Check if it actually authenticated
      setTokenInput('')
    } catch (err) {
      setTokenError(err instanceof Error ? err.message : 'Failed to save token')
    } finally {
      setSavingToken(false)
    }
  }

  const handleDisconnect = async () => {
    try {
      await updateConfig.mutateAsync({ github_token: '' })
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

  // Not connected - show token input
  if (!connected) {
    // Check if token is configured but invalid vs not set at all
    const hasToken = config && typeof config === 'object' && (config as Record<string, unknown>).github_token === '***'
    return (
      <div className="p-6 max-w-2xl mx-auto">
        <h1 className="text-xl font-semibold text-text-primary mb-4">GitHub Repos</h1>
        <div className="bg-surface-1 border border-border rounded-lg p-4">
          <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">CONNECT GITHUB</div>
          {hasToken ? (
            <div className="flex items-center gap-2 mb-3 text-[12px] text-sev-warning bg-sev-warning/5 border border-sev-warning/20 rounded px-3 py-2">
              A GitHub token is configured but authentication failed. Update it below.
            </div>
          ) : null}
          <p className="text-[12px] text-text-secondary mb-3">
            Enter a Personal Access Token to browse your repositories and review pull requests.
          </p>
          <p className="text-[11px] text-text-muted mb-4">
            Generate at{' '}
            <a href="https://github.com/settings/tokens" target="_blank" rel="noopener noreferrer" className="text-accent hover:underline">
              github.com/settings/tokens
            </a>
            {' '}&mdash; needs <code className="font-code text-accent bg-surface px-1 py-0.5 rounded text-[10px]">repo</code> scope.
            The token is stored securely on the DiffScope backend.
          </p>
          <div className="flex gap-2">
            <div className="relative flex-1">
              <input
                type={showToken ? 'text' : 'password'}
                value={tokenInput}
                onChange={(e) => setTokenInput(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleConnect()}
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
              onClick={handleConnect}
              disabled={!tokenInput.trim() || savingToken}
              className="px-4 py-1.5 rounded text-[12px] font-medium bg-accent text-surface hover:bg-accent-dim disabled:opacity-50 transition-colors"
            >
              {savingToken ? <Loader2 size={14} className="animate-spin" /> : 'Connect'}
            </button>
          </div>
          {tokenError && (
            <div className="mt-3 flex items-center gap-2 text-[12px] text-sev-error">
              <span className="inline-block w-2 h-2 rounded-full bg-sev-error" />
              {tokenError}
            </div>
          )}
        </div>
      </div>
    )
  }

  // Render repo list
  const renderRepoList = () => (
    <>
      {/* Search bar */}
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
          <button
            onClick={() => setSearchQuery('')}
            className="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-secondary"
          >
            <X size={14} />
          </button>
        )}
      </div>

      {/* Error */}
      {reposError && (
        <div className="bg-surface-1 border border-sev-error/30 rounded-lg p-4 mb-4">
          <p className="text-[12px] text-sev-error">{reposError instanceof Error ? reposError.message : 'Failed to load repos'}</p>
        </div>
      )}

      {/* Repo grid */}
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
                  <span
                    className="w-2 h-2 rounded-full shrink-0"
                    style={{ backgroundColor: LANG_COLORS[repo.language] ?? '#8b949e' }}
                  />
                  {repo.language}
                </span>
              )}
              {repo.stargazers_count > 0 && (
                <span className="flex items-center gap-0.5">
                  <Star size={10} />
                  {repo.stargazers_count}
                </span>
              )}
              <span>{timeAgo(repo.updated_at)}</span>
            </div>
          </button>
        ))}
      </div>

      {/* Loading */}
      {reposLoading && (
        <div className="flex justify-center py-6">
          <Loader2 size={20} className="animate-spin text-text-muted" />
        </div>
      )}

      {/* Load more */}
      {!reposLoading && !debouncedSearch && (repos ?? []).length >= 20 && (
        <div className="flex justify-center pt-4">
          <button
            onClick={() => setReposPage(p => p + 1)}
            className="px-4 py-1.5 rounded text-[12px] font-medium bg-surface-2 border border-border text-text-secondary hover:text-text-primary hover:border-text-muted transition-colors"
          >
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
      <button
        onClick={handleBackToRepos}
        className="flex items-center gap-1.5 text-[12px] text-text-muted hover:text-text-secondary transition-colors mb-3"
      >
        <ArrowLeft size={14} />
        repos
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
        <div className="flex justify-center py-8">
          <Loader2 size={20} className="animate-spin text-text-muted" />
        </div>
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
                    <span className="text-[13px] font-medium text-text-primary group-hover:text-accent transition-colors truncate">
                      {pr.title}
                    </span>
                    <span className="text-[11px] text-text-muted shrink-0">#{pr.number}</span>
                  </div>
                  <div className="flex items-center gap-3 text-[10px] text-text-muted">
                    <span>{pr.author}</span>
                    <span>{timeAgo(pr.created_at)}</span>
                    <span className="font-code">
                      {pr.head_branch} <span className="text-text-muted/50">&rarr;</span> {pr.base_branch}
                    </span>
                  </div>
                  {(pr.draft || pr.labels.length > 0) && (
                    <div className="flex items-center gap-1.5 mt-1.5">
                      {pr.draft && (
                        <span className="px-1.5 py-0.5 rounded text-[9px] font-medium bg-surface-2 text-text-muted border border-border">
                          Draft
                        </span>
                      )}
                      {pr.labels.map(label => (
                        <span key={label} className="px-1.5 py-0.5 rounded text-[9px] font-medium bg-accent/10 text-accent border border-accent/20">
                          {label}
                        </span>
                      ))}
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
        <button
          onClick={handleBackToPrs}
          className="flex items-center gap-1.5 text-[12px] text-text-muted hover:text-text-secondary transition-colors mb-3"
        >
          <ArrowLeft size={14} />
          PRs
        </button>

        <div className="bg-surface-1 border border-border rounded-lg p-4 mb-4">
          <div className="flex items-start gap-2 mb-3">
            <GitPullRequest size={16} className={`mt-0.5 shrink-0 ${
              selectedPr.state === 'open' ? 'text-accent' : selectedPr.state === 'merged' ? 'text-purple-400' : 'text-sev-error'
            }`} />
            <div>
              <h2 className="text-[15px] font-semibold text-text-primary">
                {selectedPr.title}
                <span className="text-text-muted font-normal ml-2">#{selectedPr.number}</span>
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
                  selectedPr.state === 'open'
                    ? 'bg-accent/15 text-accent border border-accent/30'
                    : selectedPr.state === 'merged'
                    ? 'bg-purple-400/15 text-purple-400 border border-purple-400/30'
                    : 'bg-sev-error/15 text-sev-error border border-sev-error/30'
                }`}>
                  {selectedPr.draft ? 'Draft' : selectedPr.state}
                </span>
              </div>
            </div>
            <div>
              <span className="text-text-muted">Branches</span>
              <div className="text-text-primary font-code mt-0.5 text-[11px]">
                {selectedPr.head_branch} &rarr; {selectedPr.base_branch}
              </div>
            </div>
            <div>
              <span className="text-text-muted">Updated</span>
              <div className="text-text-secondary mt-0.5">{timeAgo(selectedPr.updated_at)}</div>
            </div>
          </div>

          {selectedPr.labels.length > 0 && (
            <div className="flex items-center gap-1.5 mb-4">
              {selectedPr.labels.map(label => (
                <span key={label} className="px-1.5 py-0.5 rounded text-[9px] font-medium bg-accent/10 text-accent border border-accent/20">
                  {label}
                </span>
              ))}
            </div>
          )}

          <div className="border-t border-border-subtle pt-4">
            {/* Post results toggle */}
            <div className="flex items-center justify-between mb-4">
              <div>
                <div className="text-[13px] text-text-primary">Post results to GitHub</div>
                <div className="text-[11px] text-text-muted mt-0.5">Post inline review comments on the PR</div>
              </div>
              <button
                onClick={() => setPostResults(!postResults)}
                className={`relative w-10 h-[22px] rounded-full transition-colors ${
                  postResults ? 'bg-toggle-on' : 'bg-toggle-off'
                }`}
              >
                <span className={`absolute top-[3px] w-4 h-4 rounded-full bg-white shadow transition-transform ${
                  postResults ? 'left-[22px]' : 'left-[3px]'
                }`} />
              </button>
            </div>

            {/* Review button */}
            <button
              onClick={handleReview}
              disabled={startPrReview.isPending}
              className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-[13px] font-medium bg-accent text-surface hover:bg-accent-dim disabled:opacity-50 transition-colors"
            >
              {startPrReview.isPending ? (
                <>
                  <Loader2 size={16} className="animate-spin" />
                  Starting review...
                </>
              ) : (
                <>
                  <RefreshCw size={14} />
                  Review this PR
                </>
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
      {/* Header */}
      <div className="flex items-center justify-between mb-4">
        <h1 className="text-xl font-semibold text-text-primary">GitHub Repos</h1>
        {username && (
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2">
              {avatarUrl && (
                <img
                  src={avatarUrl}
                  alt={username}
                  className="w-6 h-6 rounded-full"
                />
              )}
              <span className="text-[12px] text-text-secondary font-code">{username}</span>
              <span className="inline-block w-2 h-2 rounded-full bg-accent" />
            </div>
            <button
              onClick={handleDisconnect}
              className="text-[11px] text-text-muted hover:text-sev-error transition-colors"
            >
              Disconnect
            </button>
          </div>
        )}
      </div>

      {/* View content */}
      {view === 'repos' && renderRepoList()}
      {view === 'prs' && renderPrList()}
      {view === 'pr-detail' && renderPrDetail()}
    </div>
  )
}
