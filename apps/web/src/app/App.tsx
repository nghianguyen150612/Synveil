import { BrowserRouter, MemoryRouter } from 'react-router-dom'

import type { AuthApi } from '../api/auth'
import type { BackupApi } from '../api/backups'
import type { BootstrapApi } from '../api/bootstrap'
import type { FileMetadataApi } from '../api/files'
import { AuthProvider } from '../auth/AuthProvider'
import { useAuthAwareBackupApi, useAuthAwareFileApi } from '../auth/authAwareApi'
import { RuntimeDiagnostics } from '../diagnostics/RuntimeDiagnostics'
import { AppRoutes } from './routes'

export interface AppProps {
  readonly router?: 'browser' | 'memory'
  readonly initialEntries?: readonly string[]
  readonly authApi?: AuthApi
  readonly bootstrapApi?: BootstrapApi
  readonly backupApi?: BackupApi
  readonly fileApi?: FileMetadataApi
}

interface ApplicationContentProps {
  readonly authApi?: AuthApi
  readonly bootstrapApi?: BootstrapApi
  readonly backupApi?: BackupApi
  readonly fileApi?: FileMetadataApi
}

function ApplicationContent({ authApi, bootstrapApi, backupApi, fileApi }: ApplicationContentProps) {
  return (
    <AuthProvider authApi={authApi} bootstrapApi={bootstrapApi}>
      <RuntimeDiagnostics />
      <AuthenticatedApplication backupApi={backupApi} fileApi={fileApi} />
    </AuthProvider>
  )
}

function AuthenticatedApplication({ backupApi, fileApi }: Pick<ApplicationContentProps, 'backupApi' | 'fileApi'>) {
  const protectedBackupApi = useAuthAwareBackupApi(backupApi)
  const protectedFileApi = useAuthAwareFileApi(fileApi)
  return <AppRoutes backupApi={protectedBackupApi} fileApi={protectedFileApi} />
}

export function App({
  router = 'browser',
  initialEntries = ['/'],
  authApi,
  bootstrapApi,
  backupApi,
  fileApi,
}: AppProps) {
  if (router === 'memory') {
    return (
      <MemoryRouter initialEntries={[...initialEntries]}>
        <ApplicationContent
          authApi={authApi}
          bootstrapApi={bootstrapApi}
          backupApi={backupApi}
          fileApi={fileApi}
        />
      </MemoryRouter>
    )
  }

  return (
    <BrowserRouter>
      <ApplicationContent
        authApi={authApi}
        bootstrapApi={bootstrapApi}
        backupApi={backupApi}
        fileApi={fileApi}
      />
    </BrowserRouter>
  )
}
