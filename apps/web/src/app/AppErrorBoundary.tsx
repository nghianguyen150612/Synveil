import { Component, type ReactNode } from 'react'

import { recordClientRenderFailure } from '../diagnostics/record'
import type { DiagnosticRouteCategory } from '../diagnostics/types'

interface AppErrorBoundaryProps {
  readonly children: ReactNode
  readonly routeCategory?: DiagnosticRouteCategory
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
    recordClientRenderFailure(this.props.routeCategory)
  }

  render() {
    if (this.state.hasError) {
      return (
        <main
          className="error-screen"
          role="alert"
          aria-labelledby="application-error-title"
        >
          <div className="content-column">
            <p className="eyebrow">Synveil</p>
            <h1 id="application-error-title">Something went wrong in this page.</h1>
            <p>
              The page could not be displayed safely. Reload to try again or open diagnostics for troubleshooting.
            </p>
            <div className="error-screen-actions">
              <button type="button" onClick={() => window.location.reload()}>
                Reload page
              </button>
              <a className="button-link button-link--secondary" href="/health/dev">
                Open diagnostics
              </a>
            </div>
          </div>
        </main>
      )
    }

    return this.props.children
  }
}
