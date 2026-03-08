import { useState, useEffect } from 'react'
import { Save, RefreshCw, Check } from 'lucide-react'
import { useConfig, useUpdateConfig } from '../api/hooks'
import { MODEL_PRESETS } from '../lib/models'

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


export function Settings() {
  const { data: config, isLoading } = useConfig()
  const updateConfig = useUpdateConfig()
  const [form, setForm] = useState<Record<string, unknown>>({})
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    if (config) setForm(config)
  }, [config])

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

  return (
    <div className="p-6 max-w-2xl mx-auto">
      <div className="flex items-center justify-between mb-6">
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

      <div className="space-y-4">
        {/* Model selection */}
        <section className="bg-surface-1 border border-border rounded-lg p-4">
          <h3 className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">MODEL</h3>

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
            <div>
              <label className="block text-[12px] font-medium text-text-secondary mb-1">Adapter</label>
              <select
                value={String(form.adapter ?? '')}
                onChange={(e) => setForm({ ...form, adapter: e.target.value || null })}
                className="w-full bg-surface border border-border rounded px-3 py-1.5 text-[13px] text-text-primary focus:outline-none focus:ring-1 focus:ring-accent font-code"
              >
                <option value="">Auto-detect</option>
                <option value="openai">OpenAI (direct)</option>
                <option value="anthropic">Anthropic (direct)</option>
                <option value="ollama">Ollama (local)</option>
                <option value="openrouter">OpenRouter</option>
              </select>
            </div>
            {field('Base URL', 'base_url', 'text', 'https://openrouter.ai/api/v1')}
            {field('API Key', 'api_key', 'password', '***')}
          </div>
        </section>

        {/* Review settings with toggles */}
        <section className="bg-surface-1 border border-border rounded-lg p-4">
          <h3 className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-2">REVIEW SETTINGS</h3>

          <div className="space-y-3 border-b border-border-subtle pb-3 mb-3">
            {field('Strictness', 'strictness', 'number', '2', '1 = lenient, 2 = balanced, 3 = strict')}
            {field('Temperature', 'temperature', 'number', '0.3')}
            {field('Max tokens', 'max_tokens', 'number', '4096')}
          </div>

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
        </section>

        {/* Context limits */}
        <section className="bg-surface-1 border border-border rounded-lg p-4">
          <h3 className="text-[10px] font-semibold text-text-muted tracking-[0.08em] font-code mb-3">CONTEXT LIMITS</h3>
          <div className="space-y-3">
            {field('Context window', 'context_window', 'number', '8192', 'Model context in tokens')}
            {field('Max diff chars', 'max_diff_chars', 'number', '40000', 'Diffs larger than this get chunked')}
            {field('Max context chars', 'max_context_chars', 'number', '20000')}
          </div>
        </section>
      </div>
    </div>
  )
}
