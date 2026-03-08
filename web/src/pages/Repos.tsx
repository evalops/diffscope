import { useState, useEffect, useCallback } from 'react'
import { useNavigate } from 'react-router-dom'
import { ArrowLeft, Search, Lock, Star, GitPullRequest, Loader2, ChevronRight, RefreshCw, X, Eye, EyeOff } from 'lucide-react'
import { initGitHub, clearGitHub, getOctokit, fetchUser, fetchRepos, searchRepos, fetchPullRequests, fetchPrDiff, postReviewToGitHub } from '../lib/github'
import type { GhRepo, GhPullRequest, ReviewComment } from '../lib/github'
import { api } from '../api/client'

const STORAGE_KEY = 'diffscope_github_token'

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

type View = 'repos' | 'prs' | 'pr-detail'

interface GhUser {
  login: string
  avatar_url: string
  name: string | null
}

export function Repos() {
  const navigate = useNavigate()

  // Auth state
  const [token, setToken] = useState(() => localStorage.getItem(STORAGE_KEY) ?? '')
  const [tokenInput, setTokenInput] = useState('')
  const [showToken, setShowToken] = useState(false)
  const [connected, setConnected] = useState(false)
  const [user, setUser] = useState<GhUser | null>(null)
  const [authError, setAuthError] = useState<string | null>(null)

  // View state
  const [view, setView] = useState<View>('repos')

  // Repo list state
  const [repos, setRepos] = useState<GhRepo[]>([])
  const [reposLoading, setReposLoading] = useState(false)
  const [reposError, setReposError] = useState<string | null>(null)
  const [reposPage, setReposPage] = useState(1)
  const [hasMore, setHasMore] = useState(true)
  const [searchQuery, setSearchQuery] = useState('')
  const [searchTimeout, setSearchTimeout] = useState<ReturnType<typeof setTimeout> | null>(null)

  // PR list state
  const [selectedRepo, setSelectedRepo] = useState<GhRepo | null>(null)
  const [prs, setPrs] = useState<GhPullRequest[]>([])
  const [prsLoading, setPrsLoading] = useState(false)
  const [prsError, setPrsError] = useState<string | null>(null)
  const [prFilter, setPrFilter] = useState<'open' | 'closed' | 'all'>('open')

  // PR detail state
  const [selectedPr, setSelectedPr] = useState<GhPullRequest | null>(null)
  const [reviewing, setReviewing] = useState(false)
  const [reviewError, setReviewError] = useState<string | null>(null)
  const [postResults, setPostResults] = useState(false)

  // Initialize from stored token
  useEffect(() => {
    if (token) {
      connectGitHub(token)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const connectGitHub = async (tok: string) => {
    setAuthError(null)
    try {
      initGitHub(tok)
      const u = await fetchUser()
      setUser(u)
      setConnected(true)
      localStorage.setItem(STORAGE_KEY, tok)
      setToken(tok)
    } catch (err) {
      clearGitHub()
      setConnected(false)
      setUser(null)
      setAuthError(err instanceof Error ? err.message : 'Failed to authenticate')
    }
  }

  const handleDisconnect = () => {
    clearGitHub()
    setConnected(false)
    setUser(null)
    setToken('')
    setTokenInput('')
    localStorage.removeItem(STORAGE_KEY)
    setRepos([])
    setPrs([])
    setSelectedRepo(null)
    setSelectedPr(null)
    setView('repos')
  }

  const handleConnect = () => {
    if (tokenInput.trim()) {
      connectGitHub(tokenInput.trim())
    }
  }

  // Fetch repos
  const loadRepos = useCallback(async (page: number, query: string, append = false) => {
    if (!getOctokit()) return
    setReposLoading(true)
    setReposError(null)
    try {
      let data: GhRepo[]
      if (query.trim()) {
        data = await searchRepos(query.trim())
        setHasMore(false)
      } else {
        data = await fetchRepos(page)
        setHasMore(data.length === 20)
      }
      setRepos(prev => append ? [...prev, ...data] : data)
    } catch (err) {
      setReposError(err instanceof Error ? err.message : 'Failed to load repos')
    } finally {
      setReposLoading(false)
    }
  }, [])

  // Load repos on connect
  useEffect(() => {
    if (connected) {
      setReposPage(1)
      loadRepos(1, searchQuery)
    }
  }, [connected, loadRepos, searchQuery])

  // Debounced search
  const handleSearchChange = (value: string) => {
    setSearchQuery(value)
    if (searchTimeout) clearTimeout(searchTimeout)
    const timeout = setTimeout(() => {
      setReposPage(1)
      loadRepos(1, value)
    }, 300)
    setSearchTimeout(timeout)
  }

  const handleLoadMore = () => {
    const nextPage = reposPage + 1
    setReposPage(nextPage)
    loadRepos(nextPage, searchQuery, true)
  }

  // Select repo -> load PRs
  const handleSelectRepo = async (repo: GhRepo) => {
    setSelectedRepo(repo)
    setView('prs')
    setPrsLoading(true)
    setPrsError(null)
    try {
      const [owner, name] = repo.full_name.split('/')
      const data = await fetchPullRequests(owner, name, prFilter)
      setPrs(data)
    } catch (err) {
      setPrsError(err instanceof Error ? err.message : 'Failed to load PRs')
    } finally {
      setPrsLoading(false)
    }
  }

  // Reload PRs when filter changes
  useEffect(() => {
    if (selectedRepo && view === 'prs') {
      const loadPrs = async () => {
        setPrsLoading(true)
        setPrsError(null)
        try {
          const [owner, name] = selectedRepo.full_name.split('/')
          const data = await fetchPullRequests(owner, name, prFilter)
          setPrs(data)
        } catch (err) {
          setPrsError(err instanceof Error ? err.message : 'Failed to load PRs')
        } finally {
          setPrsLoading(false)
        }
      }
      loadPrs()
    }
  }, [prFilter, selectedRepo, view])

  // Select PR
  const handleSelectPr = (pr: GhPullRequest) => {
    setSelectedPr(pr)
    setView('pr-detail')
    setReviewError(null)
  }

  // Review PR
  const handleReview = async () => {
    if (!selectedRepo || !selectedPr) return
    setReviewing(true)
    setReviewError(null)
    try {
      const [owner, name] = selectedRepo.full_name.split('/')
      const diff = await fetchPrDiff(owner, name, selectedPr.number)
      const result = await api.reviewDiff(diff, `${selectedRepo.full_name}#${selectedPr.number}: ${selectedPr.title}`)

      // If posting results, poll for completion then post inline comments
      if (postResults) {
        let review = await api.getReview(result.id)
        // Poll until complete (max 5 min)
        const deadline = Date.now() + 300_000
        while ((review.status === 'Pending' || review.status === 'Running') && Date.now() < deadline) {
          await new Promise(r => setTimeout(r, 2000))
          review = await api.getReview(result.id)
        }
        if (review.status === 'Complete' && review.comments.length > 0) {
          const ghComments: ReviewComment[] = review.comments.map(c => ({
            file_path: c.file_path,
            line_number: c.line_number,
            content: c.content,
            severity: c.severity,
            category: c.category,
            suggestion: c.suggestion,
            confidence: c.confidence,
            code_suggestion: c.code_suggestion ? {
              original_code: c.code_suggestion.original_code,
              suggested_code: c.code_suggestion.suggested_code,
              explanation: c.code_suggestion.explanation,
            } : undefined,
          }))
          await postReviewToGitHub(
            owner, name, selectedPr.number, ghComments,
            review.summary ? {
              overall_score: review.summary.overall_score,
              total_comments: review.summary.total_comments,
              recommendations: review.summary.recommendations,
            } : undefined,
          )
        }
      }

      navigate(`/review/${result.id}`)
    } catch (err) {
      setReviewError(err instanceof Error ? err.message : 'Failed to start review')
    } finally {
      setReviewing(false)
    }
  }

  // Back navigation
  const handleBackToRepos = () => {
    setView('repos')
    setSelectedRepo(null)
    setPrs([])
  }

  const handleBackToPrs = () => {
    setView('prs')
    setSelectedPr(null)
  }

  // Not connected - show token input
  if (!connected) {
    return (
      <div className="p-6 max-w-2xl mx-auto">
        <h1 className="text-xl font-semibold text-text-primary mb-4">GitHub Repos</h1>
        <div className="bg-surface-1 border border-border rounded-lg p-4">
          <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">CONNECT GITHUB</div>
          <p className="text-[12px] text-text-secondary mb-3">
            Enter a Personal Access Token to browse your repositories and review pull requests.
          </p>
          <p className="text-[11px] text-text-muted mb-4">
            Generate at{' '}
            <a href="https://github.com/settings/tokens" target="_blank" rel="noopener noreferrer" className="text-accent hover:underline">
              github.com/settings/tokens
            </a>
            {' '}&mdash; needs <code className="font-code text-accent bg-surface px-1 py-0.5 rounded text-[10px]">repo</code> scope.
            The token is stored in your browser only and never sent to the DiffScope backend.
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
              disabled={!tokenInput.trim()}
              className="px-4 py-1.5 rounded text-[12px] font-medium bg-accent text-surface hover:bg-accent-dim disabled:opacity-50 transition-colors"
            >
              Connect
            </button>
          </div>
          {authError && (
            <div className="mt-3 flex items-center gap-2 text-[12px] text-sev-error">
              <span className="inline-block w-2 h-2 rounded-full bg-sev-error" />
              {authError}
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
          onChange={(e) => handleSearchChange(e.target.value)}
          placeholder="Search repositories..."
          className="w-full bg-surface-1 border border-border rounded-lg pl-9 pr-9 py-2 text-[13px] text-text-primary placeholder:text-text-muted/30 focus:outline-none focus:ring-1 focus:ring-accent"
        />
        {searchQuery && (
          <button
            onClick={() => handleSearchChange('')}
            className="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-secondary"
          >
            <X size={14} />
          </button>
        )}
      </div>

      {/* Repo grid */}
      {reposError && (
        <div className="bg-surface-1 border border-sev-error/30 rounded-lg p-4 mb-4">
          <p className="text-[12px] text-sev-error">{reposError}</p>
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        {repos.map(repo => (
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

      {/* Loading / Load more */}
      {reposLoading && (
        <div className="flex justify-center py-6">
          <Loader2 size={20} className="animate-spin text-text-muted" />
        </div>
      )}

      {!reposLoading && hasMore && repos.length > 0 && !searchQuery && (
        <div className="flex justify-center pt-4">
          <button
            onClick={handleLoadMore}
            className="px-4 py-1.5 rounded text-[12px] font-medium bg-surface-2 border border-border text-text-secondary hover:text-text-primary hover:border-text-muted transition-colors"
          >
            Load more
          </button>
        </div>
      )}

      {!reposLoading && repos.length === 0 && (
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
          {(['open', 'closed', 'all'] as const).map(f => (
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
          <p className="text-[12px] text-sev-error">{prsError}</p>
        </div>
      )}

      {prsLoading ? (
        <div className="flex justify-center py-8">
          <Loader2 size={20} className="animate-spin text-text-muted" />
        </div>
      ) : prs.length === 0 ? (
        <div className="text-center py-8 text-[13px] text-text-muted">
          No {prFilter === 'all' ? '' : prFilter} pull requests.
        </div>
      ) : (
        <div className="space-y-2">
          {prs.map(pr => (
            <button
              key={pr.number}
              onClick={() => handleSelectPr(pr)}
              className="w-full bg-surface-1 border border-border rounded-lg p-3 text-left hover:border-text-muted transition-colors group"
            >
              <div className="flex items-start gap-2">
                <GitPullRequest size={14} className={`mt-0.5 shrink-0 ${pr.state === 'open' ? 'text-accent' : 'text-sev-error'}`} />
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
            <GitPullRequest size={16} className={`mt-0.5 shrink-0 ${selectedPr.state === 'open' ? 'text-accent' : 'text-sev-error'}`} />
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
              disabled={reviewing}
              className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-[13px] font-medium bg-accent text-surface hover:bg-accent-dim disabled:opacity-50 transition-colors"
            >
              {reviewing ? (
                <>
                  <Loader2 size={16} className="animate-spin" />
                  Fetching diff & reviewing...
                </>
              ) : (
                <>
                  <RefreshCw size={14} />
                  Review this PR
                </>
              )}
            </button>

            {reviewError && (
              <div className="mt-3 flex items-center gap-2 text-[12px] text-sev-error">
                <span className="inline-block w-2 h-2 rounded-full bg-sev-error" />
                {reviewError}
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
        {user && (
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2">
              <img
                src={user.avatar_url}
                alt={user.login}
                className="w-6 h-6 rounded-full"
              />
              <span className="text-[12px] text-text-secondary font-code">{user.login}</span>
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
