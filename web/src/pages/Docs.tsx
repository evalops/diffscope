import { BookOpen, Terminal, Key, GitBranch } from 'lucide-react'

const commands = [
  { cmd: 'diffscope review', desc: 'Review code changes with AI (default: HEAD)' },
  { cmd: 'diffscope serve', desc: 'Start the web UI server' },
  { cmd: 'diffscope doctor', desc: 'Check endpoint, model, and config' },
  { cmd: 'diffscope smart-review', desc: 'Generate PR summaries' },
  { cmd: 'diffscope eval', desc: 'Run fixture-based quality evals' },
  { cmd: 'diffscope feedback-eval', desc: 'Calibrate from accepted/rejected feedback' },
]

export function Docs() {
  return (
    <div className="p-6 max-w-3xl mx-auto">
      <div className="flex items-center gap-2 mb-6">
        <BookOpen size={20} className="text-accent" />
        <h1 className="text-xl font-semibold text-text-primary">Documentation</h1>
      </div>

      <section className="space-y-4">
        <h2 className="text-sm font-semibold text-text-primary border-b border-border pb-1.5 flex items-center gap-2">
          <Terminal size={14} className="text-text-muted" />
          CLI commands
        </h2>
        <div className="bg-surface-1 border border-border rounded-lg overflow-hidden">
          <ul className="divide-y divide-border-subtle">
            {commands.map(({ cmd, desc }) => (
              <li key={cmd} className="px-4 py-3 flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-4">
                <code className="text-[13px] font-code text-accent shrink-0">{cmd}</code>
                <span className="text-[13px] text-text-secondary">{desc}</span>
              </li>
            ))}
          </ul>
        </div>
      </section>

      <section className="mt-8 space-y-3">
        <h2 className="text-sm font-semibold text-text-primary border-b border-border pb-1.5 flex items-center gap-2">
          <Key size={14} className="text-text-muted" />
          Configuration
        </h2>
        <div className="bg-surface-1 border border-border rounded-lg p-4 space-y-3 text-[13px] text-text-secondary">
          <p>
            Set your API key via <code className="font-code text-text-primary bg-surface-2 px-1 rounded">DIFFSCOPE_API_KEY</code> or in
            Settings. For provider-specific keys use <code className="font-code text-text-primary bg-surface-2 px-1 rounded">OPENROUTER_API_KEY</code>,{' '}
            <code className="font-code text-text-primary bg-surface-2 px-1 rounded">ANTHROPIC_API_KEY</code>, or{' '}
            <code className="font-code text-text-primary bg-surface-2 px-1 rounded">OPENAI_API_KEY</code>.
          </p>
          <p>
            OpenRouter and other gateway providers use <code className="font-code text-text-primary bg-surface-2 px-1 rounded">vendor/model-name</code> (e.g.{' '}
            <code className="font-code text-text-primary bg-surface-2 px-1 rounded">anthropic/claude-opus-4</code>).
          </p>
        </div>
      </section>

      <section className="mt-8 space-y-3">
        <h2 className="text-sm font-semibold text-text-primary border-b border-border pb-1.5 flex items-center gap-2">
          <GitBranch size={14} className="text-text-muted" />
          Workflow
        </h2>
        <div className="bg-surface-1 border border-border rounded-lg p-4 text-[13px] text-text-secondary space-y-2">
          <p>From the Home page, start a review from <strong className="text-text-primary">HEAD</strong>, <strong className="text-text-primary">staged</strong> changes, or your current <strong className="text-text-primary">branch</strong> vs main. Use Analytics to track score trends and feedback coverage, and Settings to configure providers and models.</p>
        </div>
      </section>
    </div>
  )
}
