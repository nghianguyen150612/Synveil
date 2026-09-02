import { BrowserRouter, MemoryRouter } from 'react-router-dom'

import type { AuthApi } from '../api/auth'
import type { BootstrapApi } from '../api/bootstrap'
import { AuthProvider } from '../auth/AuthProvider'
import { AppErrorBoundary } from './AppErrorBoundary'
import { AppRoutes } from './routes'

export interface AppProps {
  readonly router?: 'browser' | 'memory'
  readonly initialEntries?: readonly string[]
  readonly authApi?: AuthApi
  readonly bootstrapApi?: BootstrapApi
}

interface ApplicationContentProps {
  readonly authApi?: AuthApi
  readonly bootstrapApi?: BootstrapApi
}

function ApplicationContent({ authApi, bootstrapApi }: ApplicationContentProps) {
  return (
    <AuthProvider authApi={authApi} bootstrapApi={bootstrapApi}>
      <AppErrorBoundary>
        <AppRoutes />
      </AppErrorBoundary>
    </AuthProvider>
  )
}

export function App({
  router = 'browser',
  initialEntries = ['/'],
  authApi,
  bootstrapApi,
}: AppProps) {
  if (router === 'memory') {
    return (
      <MemoryRouter initialEntries={[...initialEntries]}>
        <ApplicationContent authApi={authApi} bootstrapApi={bootstrapApi} />
      </MemoryRouter>
    )
  }

  return (
    <BrowserRouter>
      <ApplicationContent authApi={authApi} bootstrapApi={bootstrapApi} />
    </BrowserRouter>
  )
}
