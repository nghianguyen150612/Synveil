import { createContext, type ReactNode } from 'react'

import type { AuthApi, AuthSessionData, LoginRequest } from '../api/auth'
import type { BootstrapAdminRequest, BootstrapApi } from '../api/bootstrap'

export type AuthStatus =
  | 'loading'
  | 'bootstrap_required'
  | 'unauthenticated'
  | 'authenticated'
  | 'error'

export interface AuthState {
  readonly status: AuthStatus
  readonly isAuthenticated: boolean
  readonly session: AuthSessionData | null
  readonly errorMessage?: string
  readonly notice?: string
  readonly refresh: () => Promise<void>
  readonly login: (credentials: LoginRequest) => Promise<void>
  readonly bootstrap: (request: BootstrapAdminRequest) => Promise<void>
  readonly logout: () => Promise<void>
}

export const AuthContext = createContext<AuthState | undefined>(undefined)

export interface AuthProviderProps {
  readonly children: ReactNode
  readonly authApi?: AuthApi
  readonly bootstrapApi?: BootstrapApi
}
