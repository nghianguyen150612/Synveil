import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { authApi as defaultAuthApi, type AuthApi, type AuthSessionData } from '../api/auth'
import { isApiRequestError } from '../api/errors'
import {
  bootstrapApi as defaultBootstrapApi,
  type BootstrapApi,
} from '../api/bootstrap'
import {
  AuthContext,
  type AuthProviderProps,
  type AuthState,
  type AuthStatus,
} from './auth-context'

interface AuthViewState {
  readonly status: AuthStatus
  readonly session: AuthSessionData | null
  readonly errorMessage?: string
  readonly notice?: string
}

const INITIAL_STATE: AuthViewState = {
  status: 'loading',
  session: null,
}

const GENERIC_ERROR = 'Something went wrong. Please try again.'
const TEMPORARY_ERROR = 'Synveil is temporarily unavailable. Please try again.'
const INVALID_LOGIN_ERROR = 'The login details were not accepted.'
const SETUP_COMPLETE_NOTICE = 'Administrator created. Sign in to continue.'
const BOOTSTRAP_CLOSED_NOTICE = 'Initial setup is already complete. Sign in to continue.'

function isUnauthorized(error: unknown): boolean {
  return isApiRequestError(error) && error.status === 401
}

function isBootstrapClosed(error: unknown): boolean {
  return (
    isApiRequestError(error) &&
    (error.status === 409 || error.code === 'bootstrap_closed')
  )
}

function isTemporaryFailure(error: unknown): boolean {
  return isApiRequestError(error) && error.status >= 500
}

function safeErrorMessage(error: unknown, fallback: string): string {
  if (isTemporaryFailure(error)) {
    return TEMPORARY_ERROR
  }
  return fallback
}

function viewState(
  status: AuthStatus,
  session: AuthSessionData | null = null,
  errorMessage?: string,
  notice?: string,
): AuthViewState {
  return {
    status,
    session,
    ...(errorMessage ? { errorMessage } : {}),
    ...(notice ? { notice } : {}),
  }
}

/**
 * Browser auth state is derived from the server's bootstrap and session
 * endpoints. It never reads or stores the HttpOnly session credential.
 */
export function AuthProvider({
  children,
  authApi = defaultAuthApi,
  bootstrapApi = defaultBootstrapApi,
}: AuthProviderProps) {
  const [current, setCurrent] = useState<AuthViewState>(INITIAL_STATE)
  const requestSequence = useRef(0)
  const mounted = useRef(false)

  const beginRequest = useCallback(() => {
    const sequence = requestSequence.current + 1
    requestSequence.current = sequence
    setCurrent(viewState('loading'))
    return sequence
  }, [])

  const applyIfCurrent = useCallback(
    (sequence: number, next: AuthViewState) => {
      if (mounted.current && requestSequence.current === sequence) {
        setCurrent(next)
      }
    },
    [],
  )

  const refresh = useCallback(async () => {
    const sequence = beginRequest()
    try {
      const bootstrap = await bootstrapApi.getStatus()
      if (requestSequence.current !== sequence) {
        return
      }
      if (bootstrap.data.setup_required) {
        applyIfCurrent(sequence, viewState('bootstrap_required'))
        return
      }

      try {
        const session = await authApi.getCurrentSession()
        applyIfCurrent(sequence, viewState('authenticated', session.data))
      } catch (error) {
        if (isUnauthorized(error)) {
          applyIfCurrent(sequence, viewState('unauthenticated'))
        } else {
          applyIfCurrent(
            sequence,
            viewState('error', null, safeErrorMessage(error, GENERIC_ERROR)),
          )
        }
      }
    } catch (error) {
      applyIfCurrent(
        sequence,
        viewState('error', null, safeErrorMessage(error, GENERIC_ERROR)),
      )
    }
  }, [applyIfCurrent, authApi, beginRequest, bootstrapApi])

  const login = useCallback(
    async (credentials: Parameters<AuthApi['login']>[0]) => {
      const sequence = beginRequest()
      try {
        const response = await authApi.login(credentials)
        applyIfCurrent(sequence, viewState('authenticated', response.data))
      } catch (error) {
        if (isUnauthorized(error)) {
          applyIfCurrent(
            sequence,
            viewState('unauthenticated', null, INVALID_LOGIN_ERROR),
          )
        } else if (isTemporaryFailure(error)) {
          applyIfCurrent(sequence, viewState('error', null, TEMPORARY_ERROR))
        } else {
          applyIfCurrent(sequence, viewState('unauthenticated', null, GENERIC_ERROR))
        }
        throw error
      }
    },
    [applyIfCurrent, authApi, beginRequest],
  )

  const bootstrap = useCallback(
    async (request: Parameters<BootstrapApi['createFirstAdmin']>[0]) => {
      const sequence = beginRequest()
      try {
        await bootstrapApi.createFirstAdmin(request)
        // The reviewed contract has no automatic post-bootstrap session
        // issuance. The server has closed setup; the user signs in normally.
        applyIfCurrent(
          sequence,
          viewState('unauthenticated', null, undefined, SETUP_COMPLETE_NOTICE),
        )
      } catch (error) {
        if (isBootstrapClosed(error)) {
          applyIfCurrent(sequence, viewState('unauthenticated', null, undefined, BOOTSTRAP_CLOSED_NOTICE))
        } else if (isTemporaryFailure(error)) {
          applyIfCurrent(sequence, viewState('error', null, TEMPORARY_ERROR))
        } else {
          applyIfCurrent(sequence, viewState('bootstrap_required', null, GENERIC_ERROR))
        }
        throw error
      }
    },
    [applyIfCurrent, bootstrapApi, beginRequest],
  )

  const logout = useCallback(async () => {
    const sequence = beginRequest()
    try {
      // The API client reads only the non-HttpOnly CSRF cookie when the POST
      // is made. The response token is intentionally discarded here.
      await authApi.getCsrfToken()
      await authApi.logout()
    } catch (error) {
      // A revoked or expired session is already a successful local logout.
      // Clear local state even when the server is unavailable or the proof is
      // stale; the next visit will restore authoritative state.
      if (isTemporaryFailure(error)) {
        applyIfCurrent(sequence, viewState('unauthenticated', null, TEMPORARY_ERROR))
        return
      }
    }
    applyIfCurrent(sequence, viewState('unauthenticated'))
  }, [applyIfCurrent, authApi, beginRequest])

  useEffect(() => {
    mounted.current = true
    void refresh()
    return () => {
      mounted.current = false
    }
  }, [refresh])

  const state = useMemo<AuthState>(
    () => ({
      status: current.status,
      isAuthenticated: current.status === 'authenticated',
      session: current.session,
      ...(current.errorMessage ? { errorMessage: current.errorMessage } : {}),
      ...(current.notice ? { notice: current.notice } : {}),
      refresh,
      login,
      bootstrap,
      logout,
    }),
    [bootstrap, current, login, logout, refresh],
  )

  return <AuthContext.Provider value={state}>{children}</AuthContext.Provider>
}
