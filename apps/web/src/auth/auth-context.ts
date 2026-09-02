import { createContext, type ReactNode } from 'react'

export type AuthStatus = 'not_configured'

export interface AuthState {
  readonly status: AuthStatus
  readonly isAuthenticated: false
  readonly user: null
}

export const placeholderAuthState: AuthState = {
  status: 'not_configured',
  isAuthenticated: false,
  user: null,
}

export const AuthContext = createContext<AuthState | undefined>(undefined)

export interface AuthProviderProps {
  readonly children: ReactNode
}
