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
import { Events } from './pages/Events'
import { Admin } from './pages/Admin'
import { Docs } from './pages/Docs'

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
              <Route path="/events" element={<Events />} />
              <Route path="/admin" element={<Admin />} />
              <Route path="/doctor" element={<Doctor />} />
              <Route path="/docs" element={<Docs />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </ErrorBoundary>
    </QueryClientProvider>
  )
}
