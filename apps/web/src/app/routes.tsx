import { Navigate, Route, Routes } from 'react-router-dom'

import { AppShell } from '../components/layout/AppShell'
import { DevHealthPage } from '../features/health/DevHealthPage'
import { HomePage } from '../features/home/HomePage'
import { AuthErrorScreen, AuthLoadingScreen } from '../auth/AuthStatusScreens'
import { useAuth } from '../auth/useAuth'
import { LoginPage } from '../pages/LoginPage'
import { NotFoundPage } from '../pages/NotFoundPage'
import { SetupPage } from '../pages/SetupPage'

export function AppRoutes() {
  const auth = useAuth()

  if (auth.status === 'loading') {
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

  if (auth.status === 'unauthenticated') {
    return (
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="*" element={<Navigate to="/login" replace />} />
      </Routes>
    )
  }

  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route path="/" element={<HomePage />} />
        <Route path="/health/dev" element={<DevHealthPage />} />
        <Route path="/setup" element={<Navigate to="/" replace />} />
        <Route path="/login" element={<Navigate to="/" replace />} />
        <Route path="*" element={<NotFoundPage />} />
      </Route>
    </Routes>
  )
}
