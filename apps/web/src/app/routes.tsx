import { Route, Routes } from 'react-router-dom'

import { AppShell } from '../components/layout/AppShell'
import { DevHealthPage } from '../features/health/DevHealthPage'
import { HomePage } from '../features/home/HomePage'
import { NotFoundPage } from '../pages/NotFoundPage'

export function AppRoutes() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route path="/" element={<HomePage />} />
        <Route path="/health/dev" element={<DevHealthPage />} />
        <Route path="*" element={<NotFoundPage />} />
      </Route>
    </Routes>
  )
}
