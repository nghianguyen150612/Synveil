import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { authApi as defaultAuthApi, type AuthApi, type AuthSessionData } from '../api/auth'
import { isApiRequestError } from '../api/errors'
import { bindMutationRecoveryPrincipal } from '../api/mutationRecovery'
import { recordAuthRecoveryFailure } from '../diagnostics/record'
import { bindDiagnosticsPrincipal } from '../diagnostics/store'
import {
  bootstrapApi as defaultBootstrapApi,
  type BootstrapApi,
} from '../api/bootstrap'
import {
  AuthContext,
  type AuthRecoveryResult,
  type AuthProviderProps,
  type AuthState,
  type AuthStatus,
  type UnauthenticatedReason,
} from './auth-context'

interface AuthViewState {
  readonly status: AuthStatus
  readonly session: AuthSessionData | null
  readonly generation: number
  readonly unauthenticatedReason?: UnauthenticatedReason
  readonly errorMessage?: string
  readonly notice?: string
}

const INITIAL_STATE: AuthViewState = {
  status: 'bootstrapping',
  session: null,
  generation: 0,
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
  unauthenticatedReason?: UnauthenticatedReason,
): AuthViewState {
  return {
    status,
    session,
    generation: 0,
    ...(errorMessage ? { errorMessage } : {}),
    ...(notice ? { notice } : {}),
    ...(unauthenticatedReason ? { unauthenticatedReason } : {}),
  }
}

function sessionBoundaryKey(state: AuthViewState): string | null {
  return state.session
    ? `${state.session.user_id}\u0000${state.session.session_id}`
    : null
}

function withGeneration(previous: AuthViewState, next: AuthViewState): AuthViewState {
  const previousBoundary = sessionBoundaryKey(previous)
  const nextBoundary = sessionBoundaryKey(next)
  const generation =
    previousBoundary !== nextBoundary && (previousBoundary !== null || nextBoundary !== null)
      ? previous.generation + 1
      : previous.generation
  return { ...next, generation }
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
  const currentRef = useRef<AuthViewState>(INITIAL_STATE)
  const requestSequence = useRef(0)
  const mounted = useRef(false)
  const bootstrapStarted = useRef(false)
  const recoveryFlight = useRef<Promise<AuthRecoveryResult> | null>(null)
  const recoveryFlightToken = useRef<symbol | null>(null)
  const recoveryRetryGeneration = useRef<number | null>(null)

  const abandonRecoveryFlight = useCallback(() => {
    recoveryFlight.current = null
    recoveryFlightToken.current = null
  }, [])

  const commit = useCallback((next: AuthViewState) => {
    const generated = withGeneration(currentRef.current, next)
    currentRef.current = generated
    if (mounted.current) {
      setCurrent(generated)
    }
    return generated
  }, [])

  const beginRequest = useCallback((next?: AuthViewState) => {
    const sequence = requestSequence.current + 1
    requestSequence.current = sequence
    if (next) {
      commit(next)
    }
    return sequence
  }, [commit])

  const applyIfCurrent = useCallback(
    (sequence: number, next: AuthViewState) => {
      if (mounted.current && requestSequence.current === sequence) {
        commit(next)
      }
    },
    [commit],
  )

  const refresh = useCallback(async () => {
    abandonRecoveryFlight()
    recoveryRetryGeneration.current = null
    const sequence = beginRequest(viewState('bootstrapping'))
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
          applyIfCurrent(
            sequence,
            viewState('unauthenticated', null, undefined, undefined, 'session_missing'),
          )
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
  }, [abandonRecoveryFlight, applyIfCurrent, authApi, beginRequest, bootstrapApi])

  const markSessionExpired = useCallback(() => {
    requestSequence.current += 1
    abandonRecoveryFlight()
    recoveryRetryGeneration.current = null
    commit(
      viewState('unauthenticated', null, undefined, undefined, 'session_expired'),
    )
  }, [abandonRecoveryFlight, commit])

  const recoverSession = useCallback((): Promise<AuthRecoveryResult> => {
    const existing = recoveryFlight.current
    if (existing) {
      return existing
    }

    const sequence = beginRequest(
      viewState('recovering', currentRef.current.session),
    )
    const flightToken = Symbol('auth-recovery-flight')
    recoveryFlightToken.current = flightToken
    const flight = (async (): Promise<AuthRecoveryResult> => {
      try {
        const response = await authApi.getCurrentSession()
        if (requestSequence.current !== sequence) {
          return { status: 'failed' }
        }

        // A recovered browser session receives a newly issued, session-bound
        // CSRF cookie. Mutation identity is retained separately and never
        // contains this proof.
        await authApi.getCsrfToken()
        if (requestSequence.current !== sequence) {
          return { status: 'failed' }
        }
        const previous = currentRef.current
        const generated = withGeneration(
          currentRef.current,
          viewState('authenticated', response.data),
        )
        // Every successful recovery establishes a fresh protected-response
        // generation, even when the public principal and session identifiers
        // are unchanged. The protected route remounts from canonical GETs.
        const next = generated.generation === previous.generation
          ? { ...generated, generation: previous.generation + 1 }
          : generated
        currentRef.current = next
        recoveryRetryGeneration.current = next.generation
        if (mounted.current) {
          setCurrent(next)
        }
        return {
          status: 'recovered',
          principalId: response.data.user_id,
          generation: next.generation,
        }
      } catch (error) {
        recordAuthRecoveryFailure(error)
        if (isUnauthorized(error)) {
          applyIfCurrent(
            sequence,
            viewState('unauthenticated', null, undefined, undefined, 'session_expired'),
          )
          return { status: 'unauthenticated' }
        }
        applyIfCurrent(
          sequence,
          viewState(
            'recovery_error',
            currentRef.current.session,
            isTemporaryFailure(error)
              ? 'Unable to verify your session right now. Synveil is temporarily unavailable.'
              : 'Unable to verify your session right now. Check your connection and try again.',
          ),
        )
        return { status: 'failed' }
      } finally {
        // A newer auth transition may supersede this request sequence. Clear
        // this flight by identity so its settled result cannot be reused by a
        // later session, without erasing a newer recovery flight.
        if (recoveryFlightToken.current === flightToken) {
          recoveryFlight.current = null
          recoveryFlightToken.current = null
        }
      }
    })()
    recoveryFlight.current = flight
    return flight
  }, [applyIfCurrent, authApi, beginRequest])

  const login = useCallback(
    async (credentials: Parameters<AuthApi['login']>[0]) => {
      abandonRecoveryFlight()
      recoveryRetryGeneration.current = null
      const sequence = beginRequest()
      try {
        const response = await authApi.login(credentials)
        applyIfCurrent(sequence, viewState('authenticated', response.data))
      } catch (error) {
        if (isUnauthorized(error)) {
          applyIfCurrent(
            sequence,
            viewState(
              'unauthenticated',
              null,
              INVALID_LOGIN_ERROR,
              undefined,
              'login_failed',
            ),
          )
        } else if (isTemporaryFailure(error)) {
          applyIfCurrent(sequence, viewState('error', null, TEMPORARY_ERROR))
        } else {
          applyIfCurrent(
            sequence,
            viewState('unauthenticated', null, GENERIC_ERROR, undefined, 'login_failed'),
          )
        }
        throw error
      }
    },
    [abandonRecoveryFlight, applyIfCurrent, authApi, beginRequest],
  )

  const bootstrap = useCallback(
    async (request: Parameters<BootstrapApi['createFirstAdmin']>[0]) => {
      abandonRecoveryFlight()
      const sequence = beginRequest()
      try {
        await bootstrapApi.createFirstAdmin(request)
        // The reviewed contract has no automatic post-bootstrap session
        // issuance. The server has closed setup; the user signs in normally.
        applyIfCurrent(
          sequence,
          viewState(
            'unauthenticated',
            null,
            undefined,
            SETUP_COMPLETE_NOTICE,
            'session_missing',
          ),
        )
      } catch (error) {
        if (isBootstrapClosed(error)) {
          applyIfCurrent(
            sequence,
            viewState(
              'unauthenticated',
              null,
              undefined,
              BOOTSTRAP_CLOSED_NOTICE,
              'session_missing',
            ),
          )
        } else if (isTemporaryFailure(error)) {
          applyIfCurrent(sequence, viewState('error', null, TEMPORARY_ERROR))
        } else {
          applyIfCurrent(sequence, viewState('bootstrap_required', null, GENERIC_ERROR))
        }
        throw error
      }
    },
    [abandonRecoveryFlight, applyIfCurrent, bootstrapApi, beginRequest],
  )

  const logout = useCallback(async () => {
    abandonRecoveryFlight()
    recoveryRetryGeneration.current = null
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
        applyIfCurrent(
          sequence,
          viewState(
            'unauthenticated',
            null,
            TEMPORARY_ERROR,
            undefined,
            'intentional_logout',
          ),
        )
        return
      }
    }
    applyIfCurrent(
      sequence,
      viewState('unauthenticated', null, undefined, undefined, 'intentional_logout'),
    )
  }, [abandonRecoveryFlight, applyIfCurrent, authApi, beginRequest])

  useEffect(() => {
    mounted.current = true
    if (!bootstrapStarted.current) {
      bootstrapStarted.current = true
      void refresh()
    }
    return () => {
      mounted.current = false
    }
  }, [refresh])

  const currentGeneration = useCallback(
    () => currentRef.current.generation,
    [],
  )
  const currentPrincipalId = useCallback(
    () => currentRef.current.session?.user_id ?? null,
    [],
  )
  const isRecoveryRetryGeneration = useCallback(
    (generation: number) => recoveryRetryGeneration.current === generation,
    [],
  )
  const completeRecoveryGeneration = useCallback((generation: number) => {
    if (recoveryRetryGeneration.current === generation) {
      recoveryRetryGeneration.current = null
    }
  }, [])

  const state = useMemo<AuthState>(
    () => ({
      status: current.status,
      isAuthenticated: current.status === 'authenticated',
      session: current.session,
      generation: current.generation,
      ...(current.unauthenticatedReason
        ? { unauthenticatedReason: current.unauthenticatedReason }
        : {}),
      ...(current.errorMessage ? { errorMessage: current.errorMessage } : {}),
      ...(current.notice ? { notice: current.notice } : {}),
      refresh,
      recoverSession,
      markSessionExpired,
      currentGeneration,
      currentPrincipalId,
      isRecoveryRetryGeneration,
      completeRecoveryGeneration,
      login,
      bootstrap,
      logout,
    }),
    [
      bootstrap,
      current,
      currentGeneration,
      currentPrincipalId,
      completeRecoveryGeneration,
      isRecoveryRetryGeneration,
      login,
      logout,
      markSessionExpired,
      recoverSession,
      refresh,
    ],
  )

  // The binding is non-secret, in-memory routing context. Principal changes
  // clear the prior browser diagnostic view before protected content renders.
  bindDiagnosticsPrincipal(current.session?.user_id ?? null)
  bindMutationRecoveryPrincipal(current.session?.user_id ?? null)

  return <AuthContext.Provider value={state}>{children}</AuthContext.Provider>
}
