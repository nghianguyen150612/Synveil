import { Navigate, Outlet, useLocation } from 'react-router-dom'

import {
  AuthRecoveryErrorScreen,
  AuthRecoveringScreen,
} from './AuthStatusScreens'
import { returnRouteFromLocation } from './returnRoute'
import { useAuth } from './useAuth'

export function ProtectedRoute() {
  const auth = useAuth()
  const location = useLocation()

  if (auth.status === 'unauthenticated') {
    const intentionalLogout = auth.unauthenticatedReason === 'intentional_logout'
    return (
      <Navigate
        to="/login"
        replace
        state={{
          ...(intentionalLogout
            ? {}
            : { returnTo: returnRouteFromLocation(location) }),
          notice: intentionalLogout
            ? 'You have signed out.'
            : 'Your session has ended. Sign in again to continue.',
        }}
      />
    )
  }

  const suspended = auth.status === 'recovering' || auth.status === 'recovery_error'
  return (
    <>
      <div
        className="protected-route-boundary"
        key={auth.generation}
        hidden={suspended}
        aria-hidden={suspended || undefined}
      >
        <Outlet />
      </div>
      {auth.status === 'recovering' && <AuthRecoveringScreen />}
      {auth.status === 'recovery_error' && (
        <AuthRecoveryErrorScreen
          message={auth.errorMessage}
          onRetry={() => void auth.recoverSession()}
        />
      )}
    </>
  )
}
