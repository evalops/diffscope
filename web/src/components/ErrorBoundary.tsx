import { Component, type ReactNode } from 'react'
import { AlertTriangle } from 'lucide-react'

interface Props {
  children: ReactNode
}

interface State {
  error: Error | null
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex flex-col items-center justify-center h-full gap-4 p-8">
          <AlertTriangle className="text-sev-error" size={36} />
          <div className="text-center">
            <p className="text-text-primary font-medium mb-1">Something went wrong</p>
            <p className="text-[12px] text-text-muted max-w-md font-code">{this.state.error.message}</p>
          </div>
          <button
            onClick={() => this.setState({ error: null })}
            className="px-3 py-1.5 text-[12px] bg-surface-2 border border-border rounded hover:bg-surface-3 text-text-secondary transition-colors"
          >
            Try again
          </button>
        </div>
      )
    }
    return this.props.children
  }
}
