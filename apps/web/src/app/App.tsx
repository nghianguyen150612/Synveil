import { BrowserRouter, MemoryRouter } from 'react-router-dom'

import { AuthProvider } from '../auth/AuthProvider'
import { AppErrorBoundary } from './AppErrorBoundary'
import { AppRoutes } from './routes'

export interface AppProps {
  readonly router?: 'browser' | 'memory'
  readonly initialEntries?: readonly string[]
}

function ApplicationContent() {
  return (
    <AuthProvider>
      <AppErrorBoundary>
        <AppRoutes />
      </AppErrorBoundary>
    </AuthProvider>
  )
}

export function App({ router = 'browser', initialEntries = ['/'] }: AppProps) {
  if (router === 'memory') {
    return (
      <MemoryRouter initialEntries={[...initialEntries]}>
        <ApplicationContent />
      </MemoryRouter>
    )
  }

  return (
    <BrowserRouter>
      <ApplicationContent />
    </BrowserRouter>
  )
}
