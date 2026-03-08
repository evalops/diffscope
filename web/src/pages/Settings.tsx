import { useState, useEffect } from 'react'
import { Save, RefreshCw, Check, ChevronDown, ChevronRight, Eye, EyeOff, GitPullRequestDraft } from 'lucide-react'
import { useConfig, useUpdateConfig } from '../api/hooks'
import { api } from '../api/client'
import { MODEL_PRESETS } from '../lib/models'
import type { TestProviderResponse, GhStatusResponse } from '../api/types'

// --------------- shared helpers ---------------

function Toggle({ checked, onChange, label, description }: {
  checked: boolean
  onChange: (v: boolean) => void
  label: string
  description?: string
}) {
  return (
    <div className="flex items-center justify-between py-2">
      <div>
        <div className="text-[13px] text-text-primary">{label}</div>
        {description && <div className="text-[11px] text-text-muted mt-0.5">{description}</div>}
      </div>
      <button
        onClick={() => onChange(!checked)}
        className={`relative w-10 h-[22px] rounded-full transition-colors ${
          checked ? 'bg-toggle-on' : 'bg-toggle-off'
        }`}
      >
        <span className={`absolute top-[3px] w-4 h-4 rounded-full bg-white shadow transition-transform ${
          checked ? 'left-[22px]' : 'left-[3px]'
        }`} />
      </button>
    </div>
  )
}

function Section({ title, children, defaultOpen = true }: {
  title: string
  children: React.ReactNode
  defaultOpen?: boolean
}) {
  const [open, setOpen] = useState(defaultOpen)
  return (
    <section className="bg-surface-1 border border-border rounded-lg">
      <button
        onClick={() => setOpen(!open)}
        className="w-full flex items-center gap-2 px-4 py-3 text-left"
      >
        {open ? <ChevronDown size={14} className="text-text-muted" /> : <ChevronRight size={14} className="text-text-muted" />}
        <h3 className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">{title}</h3>
      </button>
      {open && <div className="px-4 pb-4 pt-0">{children}</div>}
    </section>
  )
}

// --------------- tab definitions ---------------

type TabId = 'providers' | 'review' | 'model' | 'repos' | 'advanced'

const TABS: { id: TabId; label: string }[] = [
  { id: 'providers', label: 'Providers' },
  { id: 'review', label: 'Review' },
  { id: 'model', label: 'Model' },
  { id: 'repos', label: 'Repos' },
  { id: 'advanced', label: 'Advanced' },
]

// --------------- provider config ---------------

interface ProviderDef {
  key: string
  name: string
  icon: string
  defaultUrl: string
  needsKey: boolean
}

const PROVIDERS: ProviderDef[] = [
  { key: 'openrouter', name: 'OpenRouter', icon: '\u{1F500}', defaultUrl: 'https://openrouter.ai/api/v1', needsKey: true },
  { key: 'openai',     name: 'OpenAI',     icon: '\u{1F916}', defaultUrl: 'https://api.openai.com/v1',    needsKey: true },
  { key: 'anthropic',  name: 'Anthropic',   icon: '\u{1F3AD}', defaultUrl: 'https://api.anthropic.com',    needsKey: true },
  { key: 'ollama',     name: 'Ollama',      icon: '\u{1F999}', defaultUrl: 'http://localhost:11434',       needsKey: false },
  { key: 'github',     name: 'GitHub',      icon: '\u{1F419}', defaultUrl: 'https://api.github.com',       needsKey: true },
]

interface ProviderFormState {
  api_key: string
  base_url: string
  enabled: boolean
}

type ProvidersMap = Record<string, ProviderFormState>

function getProviders(form: Record<string, unknown>): ProvidersMap {
  const stored = form.providers as Record<string, Record<string, unknown>> | undefined
  const result: ProvidersMap = {}
  for (const p of PROVIDERS) {
    const s = stored?.[p.key]
    result[p.key] = {
      api_key: (s?.api_key as string) ?? '',
      base_url: (s?.base_url as string) ?? '',
      enabled: s?.enabled !== undefined ? Boolean(s.enabled) : false,
    }
  }
  return result
}

function setProviders(form: Record<string, unknown>, providers: ProvidersMap): Record<string, unknown> {
  const out: Record<string, Record<string, unknown>> = {}
  for (const [key, val] of Object.entries(providers)) {
    out[key] = {
      api_key: val.api_key || undefined,
      base_url: val.base_url || undefined,
      enabled: val.enabled,
    }
  }
  return { ...form, providers: out }
}

// --------------- connection status type ---------------

type ConnStatus = 'untested' | 'ok' | 'failed'

// --------------- main component ---------------

export function Settings() {
  const { data: config, isLoading } = useConfig()
  const updateConfig = useUpdateConfig()
  const [form, setForm] = useState<Record<string, unknown>>({})
  const [saved, setSaved] = useState(false)
  const [activeTab, setActiveTab] = useState<TabId>(() => {
    const hash = window.location.hash.replace('#', '') as TabId
    return TABS.some(t => t.id === hash) ? hash : 'providers'
  })

  // Provider-specific state
  const [showKeys, setShowKeys] = useState<Record<string, boolean>>({})
  const [connStatus, setConnStatus] = useState<Record<string, ConnStatus>>({})
  const [connTesting, setConnTesting] = useState<Record<string, boolean>>({})
  const [discoveredModels, setDiscoveredModels] = useState<Record<string, string[]>>({})

  // Repos tab state
  const [ghStatus, setGhStatus] = useState<GhStatusResponse | null>(null)
  const [ghLoading, setGhLoading] = useState(false)

  useEffect(() => {
    if (config) setForm(config)
  }, [config])

  useEffect(() => {
    window.location.hash = activeTab
  }, [activeTab])

  const handleSave = () => {
    updateConfig.mutate(form, {
      onSuccess: () => {
        setSaved(true)
        setTimeout(() => setSaved(false), 2000)
      },
    })
  }

  if (isLoading) {
    return <div className="p-6 text-text-muted text-sm">Loading...</div>
  }

  // --------------- field helpers ---------------

  const field = (label: string, key: string, type: string = 'text', placeholder?: string, help?: string) => (
    <div>
      <label className="block text-[12px] font-medium text-text-secondary mb-1">{label}</label>
      <input
        type={type}
        value={String(form[key] ?? '')}
        onChange={(e) => setForm({ ...form, [key]: type === 'number' ? Number(e.target.value) : e.target.value })}
        placeholder={placeholder}
        className="w-full bg-surface border border-border rounded px-3 py-1.5 text-[13px] text-text-primary placeholder:text-text-muted/30 focus:outline-none focus:ring-1 focus:ring-accent font-code"
      />
      {help && <p className="text-[10px] text-text-muted mt-1">{help}</p>}
    </div>
  )

  const selectField = (label: string, key: string, options: { value: string; label: string }[], help?: string) => (
    <div>
      <label className="block text-[12px] font-medium text-text-secondary mb-1">{label}</label>
      <select
        value={String(form[key] ?? '')}
        onChange={(e) => setForm({ ...form, [key]: e.target.value || null })}
        className="w-full bg-surface border border-border rounded px-3 py-1.5 text-[13px] text-text-primary focus:outline-none focus:ring-1 focus:ring-accent font-code"
      >
        {options.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}
      </select>
      {help && <p className="text-[10px] text-text-muted mt-1">{help}</p>}
    </div>
  )

  const textareaField = (label: string, key: string, placeholder?: string, help?: string) => (
    <div>
      <label className="block text-[12px] font-medium text-text-secondary mb-1">{label}</label>
      <textarea
        value={String(form[key] ?? '')}
        onChange={(e) => setForm({ ...form, [key]: e.target.value || null })}
        placeholder={placeholder}
        rows={3}
        className="w-full bg-surface border border-border rounded px-3 py-2 text-[13px] text-text-primary placeholder:text-text-muted/30 focus:outline-none focus:ring-1 focus:ring-accent font-code resize-y"
      />
      {help && <p className="text-[10px] text-text-muted mt-1">{help}</p>}
    </div>
  )

  // --------------- provider helpers ---------------

  const providers = getProviders(form)

  const updateProvider = (key: string, patch: Partial<ProviderFormState>) => {
    const updated = { ...providers, [key]: { ...providers[key], ...patch } }
    setForm(setProviders(form, updated))
  }

  const handleTestConnection = async (def: ProviderDef) => {
    setConnTesting(s => ({ ...s, [def.key]: true }))
    setDiscoveredModels(s => ({ ...s, [def.key]: [] }))
    try {
      const prov = providers[def.key]
      const res: TestProviderResponse = await api.testProvider({
        provider: def.key,
        api_key: prov.api_key || undefined,
        base_url: prov.base_url || undefined,
      })
      setConnStatus(s => ({ ...s, [def.key]: res.ok ? 'ok' : 'failed' }))
      if (res.ok && res.models?.length) {
        setDiscoveredModels(s => ({ ...s, [def.key]: res.models }))
      }
    } catch {
      setConnStatus(s => ({ ...s, [def.key]: 'failed' }))
    } finally {
      setConnTesting(s => ({ ...s, [def.key]: false }))
    }
  }

  const fetchGhStatus = async () => {
    setGhLoading(true)
    try {
      const status = await api.getGhStatus()
      setGhStatus(status)
    } catch {
      setGhStatus({ authenticated: false, scopes: [] })
    } finally {
      setGhLoading(false)
    }
  }

  // --------------- status dot ---------------

  const statusDot = (status: ConnStatus) => {
    const colors = { untested: 'bg-text-muted', ok: 'bg-accent', failed: 'bg-sev-error' }
    return <span className={`inline-block w-2 h-2 rounded-full ${colors[status]}`} />
  }

  // --------------- tab content ---------------

  const renderProvidersTab = () => (
    <div className="space-y-3">
      {PROVIDERS.map(def => {
        const prov = providers[def.key]
        const status = connStatus[def.key] ?? 'untested'
        const testing = connTesting[def.key] ?? false
        const models = discoveredModels[def.key] ?? []
        const keyVisible = showKeys[def.key] ?? false

        return (
          <div key={def.key} className="bg-surface-1 border border-border rounded-lg p-4">
            {/* Header row */}
            <div className="flex items-center justify-between mb-3">
              <div className="flex items-center gap-2">
                <span className="text-lg">{def.icon}</span>
                <span className="text-[13px] font-medium text-text-primary">{def.name}</span>
                {statusDot(status)}
              </div>
              <Toggle
                checked={prov.enabled}
                onChange={(v) => updateProvider(def.key, { enabled: v })}
                label=""
              />
            </div>

            {/* API Key */}
            {def.needsKey && (
              <div className="mb-3">
                <label className="block text-[12px] font-medium text-text-secondary mb-1">
                  {def.key === 'github' ? 'Personal Access Token' : 'API Key'}
                </label>
                <div className="relative">
                  <input
                    type={keyVisible ? 'text' : 'password'}
                    value={prov.api_key}
                    onChange={(e) => updateProvider(def.key, { api_key: e.target.value })}
                    placeholder={def.key === 'github' ? 'ghp_...' : '***'}
                    className="w-full bg-surface border border-border rounded px-3 py-1.5 pr-9 text-[13px] text-text-primary placeholder:text-text-muted/30 focus:outline-none focus:ring-1 focus:ring-accent font-code"
                  />
                  <button
                    type="button"
                    onClick={() => setShowKeys(s => ({ ...s, [def.key]: !keyVisible }))}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-secondary"
                  >
                    {keyVisible ? <EyeOff size={14} /> : <Eye size={14} />}
                  </button>
                </div>
                {def.key === 'github' && (
                  <p className="text-[10px] text-text-muted mt-1">
                    Generate at{' '}
                    <a href="https://github.com/settings/tokens" target="_blank" rel="noopener noreferrer" className="text-accent hover:underline">
                      github.com/settings/tokens
                    </a>
                    {' '}&mdash; needs <code className="font-code text-accent bg-surface px-1 py-0.5 rounded text-[9px]">repo</code> scope
                  </p>
                )}
              </div>
            )}

            {/* Base URL */}
            <div className="mb-3">
              <label className="block text-[12px] font-medium text-text-secondary mb-1">Base URL</label>
              <input
                type="text"
                value={prov.base_url}
                onChange={(e) => updateProvider(def.key, { base_url: e.target.value })}
                placeholder={def.defaultUrl}
                className="w-full bg-surface border border-border rounded px-3 py-1.5 text-[13px] text-text-primary placeholder:text-text-muted/30 focus:outline-none focus:ring-1 focus:ring-accent font-code"
              />
            </div>

            {/* Test button */}
            <button
              onClick={() => handleTestConnection(def)}
              disabled={testing}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded text-[11px] font-medium bg-surface-2 border border-border text-text-secondary hover:text-text-primary hover:border-text-muted transition-colors disabled:opacity-50"
            >
              {testing ? <><RefreshCw size={12} className="animate-spin" /> Testing...</> : 'Test Connection'}
            </button>

            {/* Discovered models */}
            {models.length > 0 && (
              <div className="mt-3 border-t border-border-subtle pt-3">
                <div className="text-[10px] text-text-muted mb-1.5 font-code tracking-[0.08em]">DISCOVERED MODELS</div>
                <div className="max-h-28 overflow-y-auto space-y-0.5">
                  {models.map(m => (
                    <div key={m} className="text-[11px] text-text-secondary font-code truncate">{m}</div>
                  ))}
                </div>
              </div>
            )}
          </div>
        )
      })}

      {/* Legacy / Override section */}
      <Section title="LEGACY / OVERRIDE" defaultOpen={false}>
        <p className="text-[10px] text-text-muted mb-3">
          These top-level fields override provider-specific settings. They exist for backwards compatibility.
          If set, they take priority over any provider-specific API key and base URL.
        </p>
        <div className="space-y-3">
          {field('API Key', 'api_key', 'password', '***')}
          {field('Base URL', 'base_url', 'text', 'https://openrouter.ai/api/v1')}
        </div>
      </Section>
    </div>
  )

  const renderReviewTab = () => (
    <div className="space-y-3">
      <Section title="REVIEW BEHAVIOR">
        <div className="space-y-3 border-b border-border-subtle pb-3 mb-3">
          {field('Strictness', 'strictness', 'number', '2', '1 = lenient, 2 = balanced, 3 = strict')}
          {selectField('Review Profile', 'review_profile', [
            { value: '', label: 'Default' },
            { value: 'chill', label: 'Chill' },
            { value: 'balanced', label: 'Balanced' },
            { value: 'assertive', label: 'Assertive' },
          ], 'Personality for review tone')}
          {field('Min Confidence', 'min_confidence', 'number', '0.0', 'Filter out findings below this threshold (0.0 - 1.0)')}
          {field('Output Language', 'output_language', 'text', 'en', 'Language for review output (e.g., en, ja, de)')}
        </div>

        <div className="space-y-3 border-b border-border-subtle pb-3 mb-3">
          <div>
            <label className="block text-[12px] font-medium text-text-secondary mb-1">Comment Types</label>
            <div className="flex gap-2 flex-wrap">
              {['logic', 'syntax', 'style', 'informational'].map(type => {
                const types = Array.isArray(form.comment_types) ? form.comment_types as string[] : []
                const active = types.includes(type)
                return (
                  <button
                    key={type}
                    onClick={() => {
                      const next = active
                        ? types.filter(t => t !== type)
                        : [...types, type]
                      setForm({ ...form, comment_types: next.length > 0 ? next : ['logic', 'syntax', 'style', 'informational'] })
                    }}
                    className={`px-2.5 py-1 rounded text-[11px] font-medium transition-colors ${
                      active
                        ? 'bg-accent/15 text-accent border border-accent/30'
                        : 'bg-surface text-text-muted border border-border hover:text-text-secondary'
                    }`}
                  >
                    {type}
                  </button>
                )
              })}
            </div>
            <p className="text-[10px] text-text-muted mt-1">Types of findings to include</p>
          </div>
          {field('File Change Limit', 'file_change_limit', 'number', '', 'Skip review if diff has more than N files (0 = no limit)')}
        </div>

        <Toggle
          label="Include Fix Suggestions"
          description="Generate AI code fix suggestions with each finding"
          checked={form.include_fix_suggestions !== false}
          onChange={v => setForm({ ...form, include_fix_suggestions: v })}
        />
        <Toggle
          label="Auto-detect Instructions"
          description="Absorb .cursorrules, CLAUDE.md, agents.md from project"
          checked={form.auto_detect_instructions !== false}
          onChange={v => setForm({ ...form, auto_detect_instructions: v })}
        />
        <Toggle
          label="Smart Review Summary"
          description="Generate a high-level summary of code changes"
          checked={!!form.smart_review_summary}
          onChange={v => setForm({ ...form, smart_review_summary: v })}
        />
        <Toggle
          label="Smart Review Diagram"
          description="Generate diagrams for complex changes"
          checked={!!form.smart_review_diagram}
          onChange={v => setForm({ ...form, smart_review_diagram: v })}
        />

        {textareaField('Review Instructions', 'review_instructions', 'Custom instructions for the reviewer (e.g., "Focus on security issues in auth code")', 'Additional context passed to the LLM for every review')}
      </Section>
    </div>
  )

  const renderModelTab = () => (
    <div className="space-y-3">
      <Section title="MODEL">
        {/* Quick select grid */}
        <div className="mb-4">
          <div className="text-[11px] text-text-secondary mb-2">Quick Select (via OpenRouter)</div>
          <div className="grid grid-cols-2 gap-1 max-h-44 overflow-y-auto pr-1">
            {MODEL_PRESETS.map(preset => (
              <button
                key={preset.id}
                onClick={() => setForm({ ...form, model: preset.id, adapter: 'openrouter' })}
                className={`text-left px-2.5 py-1.5 rounded text-[11px] transition-colors ${
                  form.model === preset.id
                    ? 'bg-accent/10 border border-accent/30 text-accent'
                    : 'bg-surface hover:bg-surface-2 border border-transparent text-text-secondary'
                }`}
              >
                <div className="font-medium truncate">{preset.label}</div>
                <div className="text-[10px] text-text-muted flex gap-2">
                  <span>{preset.ctx}</span>
                  <span className={preset.price === 'free' ? 'text-accent' : ''}>{preset.price}</span>
                </div>
              </button>
            ))}
          </div>
        </div>

        <div className="space-y-3 border-t border-border-subtle pt-3">
          {field('Model name', 'model', 'text', 'anthropic/claude-sonnet-4.6', 'OpenRouter: vendor/model-name')}
          {selectField('Adapter', 'adapter', [
            { value: '', label: 'Auto-detect' },
            { value: 'openai', label: 'OpenAI (direct)' },
            { value: 'anthropic', label: 'Anthropic (direct)' },
            { value: 'ollama', label: 'Ollama (local)' },
            { value: 'openrouter', label: 'OpenRouter' },
          ])}
        </div>
      </Section>

      <Section title="LLM TUNING">
        <div className="space-y-3">
          {field('Temperature', 'temperature', 'number', '0.2', 'Creativity (0.0 = deterministic, 2.0 = max)')}
          {field('Max Tokens', 'max_tokens', 'number', '4000', 'Maximum response tokens (up to 128000)')}
          {field('Context Window', 'context_window', 'number', '8192', 'Model context in tokens (for local models)')}
        </div>
      </Section>
    </div>
  )

  const renderReposTab = () => {
    const ghToken = localStorage.getItem('diffscope_github_token')
    const isConnected = !!ghToken

    return (
      <div className="space-y-3">
        <p className="text-[13px] text-text-secondary">
          Connect GitHub to browse repositories and review pull requests directly from the web UI.
        </p>

        <div className="bg-surface-1 border border-border rounded-lg p-4">
          <div className="flex items-center gap-2 mb-3">
            <GitPullRequestDraft size={16} className="text-text-muted" />
            <div className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code">GITHUB CONNECTION</div>
          </div>

          {isConnected ? (
            <div>
              <div className="flex items-center gap-2 mb-3">
                <span className="inline-block w-2 h-2 rounded-full bg-accent" />
                <span className="text-[13px] text-text-primary">Connected</span>
                <span className="px-2 py-0.5 rounded text-[10px] font-medium bg-accent/15 text-accent border border-accent/30">
                  Token stored in browser
                </span>
              </div>
              <div className="flex items-center gap-2">
                <a
                  href="/repos"
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded text-[11px] font-medium bg-accent text-surface hover:bg-accent-dim transition-colors"
                >
                  Open Repos
                </a>
                <button
                  onClick={() => {
                    localStorage.removeItem('diffscope_github_token')
                    // Force re-render
                    setForm({ ...form })
                  }}
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded text-[11px] font-medium bg-surface-2 border border-border text-text-secondary hover:text-sev-error hover:border-sev-error/30 transition-colors"
                >
                  Disconnect
                </button>
              </div>
            </div>
          ) : (
            <div>
              <div className="flex items-center gap-2 mb-2">
                <span className="inline-block w-2 h-2 rounded-full bg-text-muted" />
                <span className="text-[13px] text-text-secondary">Not connected</span>
              </div>
              <p className="text-[11px] text-text-muted mb-3">
                Go to the{' '}
                <a href="/repos" className="text-accent hover:underline">Repos page</a>
                {' '}to connect with your GitHub Personal Access Token.
                Your token is stored in the browser only and never sent to the DiffScope backend.
              </p>
            </div>
          )}
        </div>

        {/* Backend gh CLI status (legacy) */}
        <Section title="BACKEND GH CLI STATUS" defaultOpen={false}>
          <p className="text-[10px] text-text-muted mb-3">
            Legacy: Check if the backend server has access to GitHub via the <code className="font-code text-accent">gh</code> CLI.
          </p>
          <button
            onClick={fetchGhStatus}
            disabled={ghLoading}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded text-[11px] font-medium bg-surface-2 border border-border text-text-secondary hover:text-text-primary hover:border-text-muted transition-colors disabled:opacity-50"
          >
            {ghLoading ? <><RefreshCw size={12} className="animate-spin" /> Checking...</> : <><RefreshCw size={12} /> Check gh CLI</>}
          </button>
          {ghStatus !== null && (
            <div className="mt-3">
              {ghStatus.authenticated ? (
                <div className="flex items-center gap-2">
                  <span className="inline-block w-2 h-2 rounded-full bg-accent" />
                  <span className="text-[13px] text-text-primary font-code">{ghStatus.username}</span>
                  <span className="px-2 py-0.5 rounded text-[10px] font-medium bg-accent/15 text-accent border border-accent/30">
                    Connected
                  </span>
                </div>
              ) : (
                <div className="flex items-center gap-2">
                  <span className="inline-block w-2 h-2 rounded-full bg-sev-error" />
                  <span className="text-[13px] text-text-secondary">Not connected via gh CLI</span>
                </div>
              )}
            </div>
          )}
        </Section>
      </div>
    )
  }

  const renderAdvancedTab = () => (
    <div className="space-y-3">
      <Section title="CONTEXT LIMITS" defaultOpen={false}>
        <div className="space-y-3">
          {field('Max Diff Chars', 'max_diff_chars', 'number', '40000', 'Diffs larger than this get chunked')}
          {field('Max Context Chars', 'max_context_chars', 'number', '20000', 'Max chars of surrounding file context')}
          {field('Context Budget Chars', 'context_budget_chars', 'number', '24000', 'Total budget for all context sources')}
          {field('Context Max Chunks', 'context_max_chunks', 'number', '24', 'Max number of context chunks to include')}
        </div>
      </Section>

      <Section title="NETWORK & RETRIES" defaultOpen={false}>
        <div className="space-y-3">
          {field('Timeout (seconds)', 'adapter_timeout_secs', 'number', '', 'HTTP timeout for LLM requests (5-600s, default: 60 cloud / 300 local)')}
          {field('Max Retries', 'adapter_max_retries', 'number', '', 'Retries on 429/5xx errors (0-10)')}
          {field('Retry Delay (ms)', 'adapter_retry_delay_ms', 'number', '', 'Base delay between retries (50-30000ms)')}
        </div>
      </Section>

      <Section title="FEEDBACK SUPPRESSION" defaultOpen={false}>
        <div className="space-y-3">
          {field('Suppression Threshold', 'feedback_suppression_threshold', 'number', '3', 'Minimum rejections before suppression kicks in')}
          {field('Suppression Margin', 'feedback_suppression_margin', 'number', '2', 'Rejections must exceed accepts by this much')}
        </div>
        <p className="text-[10px] text-text-muted mt-2">
          When you reject findings, DiffScope learns to suppress similar patterns. These settings control how aggressively it adapts.
        </p>
      </Section>

      <Section title="VAULT INTEGRATION" defaultOpen={false}>
        <p className="text-[10px] text-text-muted mb-3">
          Pull your LLM API key from HashiCorp Vault KV v2 instead of storing it in config. Vault is only queried when no API key is set.
        </p>
        <div className="space-y-3">
          {field('Vault Address', 'vault_addr', 'text', 'https://vault.example.com:8200', 'VAULT_ADDR env var also works')}
          {field('Vault Token', 'vault_token', 'password', '', 'VAULT_TOKEN env var also works')}
          {field('Secret Path', 'vault_path', 'text', 'diffscope', 'Path to the secret in Vault (e.g., ci/diffscope)')}
          {field('Secret Key', 'vault_key', 'text', 'api_key', 'Key within the secret to extract (default: api_key)')}
          {field('KV Mount', 'vault_mount', 'text', 'secret', 'KV engine mount point (default: secret)')}
          {field('Namespace', 'vault_namespace', 'text', '', 'Vault Enterprise namespace (optional)')}
        </div>
      </Section>

      <Section title="EXCLUDE PATTERNS" defaultOpen={false}>
        <div>
          <label className="block text-[12px] font-medium text-text-secondary mb-1">Glob patterns to exclude from review</label>
          <textarea
            value={Array.isArray(form.exclude_patterns) ? (form.exclude_patterns as string[]).join('\n') : ''}
            onChange={(e) => {
              const patterns = e.target.value.split('\n').filter(p => p.trim())
              setForm({ ...form, exclude_patterns: patterns })
            }}
            placeholder="*.lock&#10;dist/**&#10;node_modules/**"
            rows={4}
            className="w-full bg-surface border border-border rounded px-3 py-2 text-[13px] text-text-primary placeholder:text-text-muted/30 focus:outline-none focus:ring-1 focus:ring-accent font-code resize-y"
          />
          <p className="text-[10px] text-text-muted mt-1">One pattern per line. Matched files are skipped entirely.</p>
        </div>
      </Section>
    </div>
  )

  const tabContent = {
    providers: renderProvidersTab,
    review: renderReviewTab,
    model: renderModelTab,
    repos: renderReposTab,
    advanced: renderAdvancedTab,
  } as const

  return (
    <div className="p-6 max-w-2xl mx-auto">
      {/* Header */}
      <div className="flex items-center justify-between mb-4">
        <h1 className="text-xl font-semibold text-text-primary">Settings</h1>
        <button
          onClick={handleSave}
          disabled={updateConfig.isPending}
          className={`flex items-center gap-2 px-3 py-1.5 rounded text-[12px] font-medium transition-all ${
            saved
              ? 'bg-accent/10 text-accent'
              : 'bg-accent text-surface hover:bg-accent-dim disabled:opacity-50'
          }`}
        >
          {saved ? <><Check size={14} /> Saved</> : updateConfig.isPending ? <><RefreshCw size={14} className="animate-spin" /> Saving...</> : <><Save size={14} /> Save</>}
        </button>
      </div>

      {/* Tab bar */}
      <div className="border-b border-border mb-4">
        <div className="flex">
          {TABS.map(tab => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`px-4 py-2 text-[12px] font-medium transition-colors relative ${
                activeTab === tab.id
                  ? 'text-accent'
                  : 'text-text-muted hover:text-text-secondary'
              }`}
            >
              {tab.label}
              {activeTab === tab.id && (
                <span className="absolute bottom-0 left-0 right-0 h-[2px] bg-accent rounded-t" />
              )}
            </button>
          ))}
        </div>
      </div>

      {/* Tab content */}
      {tabContent[activeTab]()}
    </div>
  )
}
