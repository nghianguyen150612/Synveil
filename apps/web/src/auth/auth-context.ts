import { createContext, type ReactNode } from 'react'

import type { AuthApi, AuthSessionData, LoginRequest } from '../api/auth'
import type { BootstrapAdminRequest, BootstrapApi } from '../api/bootstrap'

export type AuthStatus =
  | 'bootstrapping'
  | 'bootstrap_required'
  | 'unauthenticated'
  | 'authenticated'
  | 'recovering'
  | 'recovery_error'
  | 'error'

export type UnauthenticatedReason =
  | 'session_missing'
  | 'session_expired'
  | 'intentional_logout'
  | 'login_failed'

export type AuthRecoveryResult =
  | {
      readonly status: 'recovered'
      readonly principalId: string
      readonly generation: number
    }
  | { readonly status: 'unauthenticated' }
  | { readonly status: 'failed' }

export interface AuthState {
  readonly status: AuthStatus
  readonly isAuthenticated: boolean
  readonly session: AuthSessionData | null
  readonly generation: number
  readonly unauthenticatedReason?: UnauthenticatedReason
  readonly errorMessage?: string
  readonly notice?: string
  readonly refresh: () => Promise<void>
  readonly recoverSession: () => Promise<AuthRecoveryResult>
  readonly markSessionExpired: () => void
  readonly currentGeneration: () => number
  readonly currentPrincipalId: () => string | null
  readonly isRecoveryRetryGeneration: (generation: number) => boolean
  readonly completeRecoveryGeneration: (generation: number) => void
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
