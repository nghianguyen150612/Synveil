import { Component, type ReactNode } from 'react'

interface AppErrorBoundaryProps {
  readonly children: ReactNode
}

interface AppErrorBoundaryState {
  readonly hasError: boolean
}

/** Prevent a render failure from producing an unstructured blank page. */
export class AppErrorBoundary extends Component<
  AppErrorBoundaryProps,
  AppErrorBoundaryState
> {
  state: AppErrorBoundaryState = { hasError: false }

  static getDerivedStateFromError(): AppErrorBoundaryState {
    return { hasError: true }
  }

  componentDidCatch(): void {
    // Diagnostics stay out of the public UI until the telemetry policy exists.
  }

  render() {
    if (this.state.hasError) {
      return (
        <main className="error-screen" aria-labelledby="application-error-title">
          <div className="content-column">
            <p className="eyebrow">Synveil</p>
            <h1 id="application-error-title">Something went wrong</h1>
            <p>
              The page could not be displayed safely. Reload to try again.
            </p>
            <button type="button" onClick={() => window.location.reload()}>
              Reload page
            </button>
          </div>
        </main>
      )
    }

    return this.props.children
  }
}
