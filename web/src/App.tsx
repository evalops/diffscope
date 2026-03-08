import { BrowserRouter, Routes, Route } from 'react-router-dom'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ErrorBoundary } from './components/ErrorBoundary'
import { Layout } from './components/Layout'
import { Dashboard } from './pages/Dashboard'
import { ReviewView } from './pages/ReviewView'
import { History } from './pages/History'
import { Analytics } from './pages/Analytics'
import { Settings } from './pages/Settings'
import { Doctor } from './pages/Doctor'
import { Repos } from './pages/Repos'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
})

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ErrorBoundary>
        <BrowserRouter>
          <Routes>
            <Route element={<Layout />}>
              <Route path="/" element={<Dashboard />} />
              <Route path="/review/:id" element={<ReviewView />} />
              <Route path="/history" element={<History />} />
              <Route path="/analytics" element={<Analytics />} />
              <Route path="/settings" element={<Settings />} />
              <Route path="/repos" element={<Repos />} />
              <Route path="/doctor" element={<Doctor />} />
              <Route path="/docs" element={
                <div className="p-6 max-w-3xl mx-auto">
                  <h1 className="text-xl font-semibold text-text-primary mb-4">Documentation</h1>
                  <div className="bg-surface-1 border border-border rounded-lg p-4 space-y-3 text-[13px] text-text-secondary">
                    <p><span className="font-code text-accent">diffscope review</span> — Review code changes with AI</p>
                    <p><span className="font-code text-accent">diffscope serve</span> — Start the web UI server</p>
                    <p><span className="font-code text-accent">diffscope doctor</span> — Check your setup</p>
                    <p><span className="font-code text-accent">diffscope smart-review</span> — Generate PR summaries</p>
                    <p className="text-[11px] text-text-muted pt-2 border-t border-border-subtle">
                      Set your API key via <span className="font-code">DIFFSCOPE_API_KEY</span> or in Settings.
                      OpenRouter models use <span className="font-code">vendor/model-name</span> format.
                    </p>
                  </div>
                </div>
              } />
            </Route>
          </Routes>
        </BrowserRouter>
      </ErrorBoundary>
    </QueryClientProvider>
  )
}
