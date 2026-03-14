import { NavLink, Outlet } from 'react-router-dom'
import { Home, Settings, Stethoscope, ScrollText, GitCompareArrows, BarChart3, BookOpen, GitPullRequestDraft, Activity, Shield } from 'lucide-react'
import { useStatus } from '../api/hooks'

const sections = [
  {
    items: [
      { to: '/', icon: Home, label: 'Home' },
    ],
  },
  {
    label: 'REVIEW',
    items: [
      { to: '/history', icon: ScrollText, label: 'Logs' },
      { to: '/events', icon: Activity, label: 'Events' },
      { to: '/repos', icon: GitPullRequestDraft, label: 'Repos' },
      { to: '/analytics', icon: BarChart3, label: 'Analytics' },
      { to: '/settings', icon: Settings, label: 'Settings' },
    ],
  },
  {
    label: 'SYSTEM',
    items: [
      { to: '/admin', icon: Shield, label: 'Admin' },
      { to: '/doctor', icon: Stethoscope, label: 'Doctor' },
      { to: '/docs', icon: BookOpen, label: 'Documentation' },
    ],
  },
]

export function Layout() {
  const { data: status } = useStatus()

  return (
    <div className="flex h-screen bg-surface">
      <a
        href="#main-content"
        className="sr-only focus:fixed focus:top-3 focus:left-3 focus:z-[100] focus:px-3 focus:py-2 focus:bg-accent focus:text-surface focus:rounded focus:w-auto focus:h-auto focus:m-0 focus:overflow-visible focus:clip-auto focus:whitespace-normal font-medium focus:outline-none focus:ring-2 focus:ring-accent focus:ring-offset-2 focus:ring-offset-surface"
      >
        Skip to main content
      </a>
      <aside className="w-52 min-w-[11rem] bg-surface-1 border-r border-border flex flex-col shrink-0" aria-label="Primary navigation">
        {/* Logo / org */}
        <div className="px-4 py-3.5 border-b border-border">
          <div className="flex items-center gap-2">
            <GitCompareArrows size={18} className="text-accent" />
            <h1 className="text-sm font-semibold text-text-primary tracking-tight">DiffScope</h1>
          </div>
        </div>

        {/* Navigation sections */}
        <nav className="flex-1 overflow-y-auto py-1">
          {sections.map((section, si) => (
            <div key={si}>
              {section.label && (
                <div className="px-4 pt-4 pb-1.5 text-[10px] font-semibold text-text-muted tracking-[0.1em]">
                  {section.label}
                </div>
              )}
              {section.items.map(({ to, icon: Icon, label }) => (
                <NavLink
                  key={to}
                  to={to}
                  end={to === '/'}
                  className={({ isActive }) =>
                    `flex items-center gap-2.5 px-4 py-[7px] text-[13px] transition-colors ${
                      isActive
                        ? 'bg-sidebar-active text-accent font-medium'
                        : 'text-text-secondary hover:bg-surface-2 hover:text-text-primary'
                    }`
                  }
                >
                  <Icon size={15} />
                  {label}
                </NavLink>
              ))}
            </div>
          ))}
        </nav>

        {/* Status footer */}
        <div className="px-4 py-3 border-t border-border">
          <div className="flex items-center gap-2 text-[11px] text-text-muted">
            <span className={`w-1.5 h-1.5 rounded-full ${status ? 'bg-accent' : 'bg-text-muted'}`} />
            <span className="truncate font-code">{status?.model || 'Connecting...'}</span>
          </div>
          {status?.branch && (
            <div className="text-[10px] text-text-muted/60 font-code mt-1 truncate pl-3.5">
              {status.repo_path.split('/').pop()}/{status.branch}
            </div>
          )}
        </div>
      </aside>

      <main id="main-content" className="flex-1 overflow-auto min-w-0" role="main">
        <Outlet />
      </main>
    </div>
  )
}
