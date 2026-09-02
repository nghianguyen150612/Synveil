import { Navigate, Route, Routes, useLocation } from 'react-router-dom'

import type { BackupApi } from '../api/backups'
import type { FileMetadataApi } from '../api/files'
import { AppShell } from '../components/layout/AppShell'
import { routeCategoryFromPath } from '../diagnostics/routes'
import { BackupOverviewPage } from '../features/backups/BackupOverviewPage'
import { BackupsPage } from '../features/backups/BackupsPage'
import { PruneWorkflowPage } from '../features/backups/PruneWorkflowPage'
import { RestoreWorkflowPage } from '../features/backups/RestoreWorkflowPage'
import { DevHealthPage } from '../features/health/DevHealthPage'
import { HomePage } from '../features/home/HomePage'
import { AuthErrorScreen, AuthLoadingScreen } from '../auth/AuthStatusScreens'
import { ProtectedRoute } from '../auth/ProtectedRoute'
import { returnRouteFromState } from '../auth/returnRoute'
import { useAuth } from '../auth/useAuth'
import { LoginPage } from '../pages/LoginPage'
import { NotFoundPage } from '../pages/NotFoundPage'
import { SetupPage } from '../pages/SetupPage'
import { AppErrorBoundary } from './AppErrorBoundary'

export interface AppRoutesProps {
  readonly backupApi?: BackupApi
  readonly fileApi?: FileMetadataApi
}

export function AppRoutes({ backupApi, fileApi }: AppRoutesProps) {
  const auth = useAuth()

  if (auth.status === 'bootstrapping') {
    return <AuthLoadingScreen />
  }

  if (auth.status === 'error') {
    return <AuthErrorScreen message={auth.errorMessage} onRetry={auth.refresh} />
  }

  if (auth.status === 'bootstrap_required') {
    return (
      <Routes>
        <Route path="/setup" element={<SetupPage />} />
        <Route path="*" element={<Navigate to="/setup" replace />} />
      </Routes>
    )
  }

  return (
    <Routes>
      <Route
        path="/login"
        element={auth.status === 'authenticated' ? <AuthenticatedLoginRedirect /> : <LoginPage />}
      />
      <Route path="/setup" element={<Navigate to={auth.status === 'authenticated' ? '/' : '/login'} replace />} />
      <Route element={<ProtectedRoute />}>
        <Route element={<ProtectedAppShell />}>
          <Route path="/" element={<HomePage />} />
          <Route
            path="/backups/:backupSetId/snapshots/:snapshotId/restore"
            element={<RestoreWorkflowPage backupApi={backupApi} fileApi={fileApi} />}
          />
          <Route
            path="/backups/:backupSetId/restore/:restorePlanId"
            element={<RestoreWorkflowPage backupApi={backupApi} fileApi={fileApi} />}
          />
          <Route
            path="/backups/:backupSetId/snapshots/:snapshotId/prune"
            element={<PruneWorkflowPage backupApi={backupApi} />}
          />
          <Route
            path="/backups/:backupSetId/prune/:prunePlanId"
            element={<PruneWorkflowPage backupApi={backupApi} />}
          />
          <Route
            path="/backups"
            element={<BackupOverviewPage backupApi={backupApi} fileApi={fileApi} />}
          />
          <Route path="/backups/:backupSetId" element={<BackupsPage backupApi={backupApi} />} />
          <Route
            path="/backups/:backupSetId/operations/:operationKind/:operationId"
            element={<BackupsPage backupApi={backupApi} />}
          />
          <Route path="/health/dev" element={<DevHealthPage />} />
          <Route path="*" element={<NotFoundPage />} />
        </Route>
      </Route>
    </Routes>
  )
}

function ProtectedAppShell() {
  const location = useLocation()
  return (
    <AppErrorBoundary routeCategory={routeCategoryFromPath(location.pathname)}>
      <AppShell />
    </AppErrorBoundary>
  )
}

function AuthenticatedLoginRedirect() {
  const location = useLocation()
  return <Navigate to={returnRouteFromState(location.state)} replace />
}
