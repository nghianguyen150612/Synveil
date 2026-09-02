import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { AuthApi, AuthSessionResponse } from '../api/auth'
import type { BootstrapApi, BootstrapStatusResponse } from '../api/bootstrap'
import { ApiRequestError, type ApiErrorPayload } from '../api/errors'
import { AuthProvider } from './AuthProvider'
import { useAuth } from './useAuth'

const openStatus: BootstrapStatusResponse = {
  data: { setup_required: true },
  meta: { request_id: 'bootstrap-open' },
}

const closedStatus: BootstrapStatusResponse = {
  data: { setup_required: false },
  meta: { request_id: 'bootstrap-closed' },
}

const session: AuthSessionResponse = {
  data: {
    authenticated: true,
    user_id: '00000000-0000-7000-8000-000000000001',
    is_instance_admin: true,
    session_id: '00000000-0000-7000-8000-000000000002',
  },
  meta: { request_id: 'session-1' },
}

const csrfResponse = {
  data: { csrf_token: '0'.repeat(128) },
  meta: { request_id: 'csrf-1' },
}

function apiError(status: number, code = 'http_error'): ApiRequestError {
  const payload: ApiErrorPayload = {
    code,
    message: 'safe server message',
    request_id: 'error-1',
    retryable: status >= 500,
  }
  return new ApiRequestError(status, payload)
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve
  })
  return { promise, resolve }
}

function makeApis(
  authOverrides: Partial<AuthApi> = {},
  bootstrapOverrides: Partial<BootstrapApi> = {},
) {
  const authApi: AuthApi = {
    login: vi.fn().mockResolvedValue(session),
    getCurrentSession: vi.fn().mockRejectedValue(apiError(401, 'authentication_failed')),
    getCsrfToken: vi.fn().mockResolvedValue(csrfResponse),
    logout: vi.fn().mockResolvedValue(undefined),
    ...authOverrides,
  }
  const bootstrapApi: BootstrapApi = {
    getStatus: vi.fn().mockResolvedValue(closedStatus),
    createFirstAdmin: vi.fn().mockResolvedValue(closedStatus),
    ...bootstrapOverrides,
  }
  return { authApi, bootstrapApi }
}

function Probe() {
  const auth = useAuth()
  return (
    <div>
      <output aria-label="auth status">{auth.status}</output>
      <output aria-label="auth error">{auth.errorMessage ?? ''}</output>
      <button type="button" onClick={() => void auth.refresh()}>
        refresh
      </button>
      <button
        type="button"
        onClick={() =>
          void auth
            .login({ login: 'alice', login_key: 'alice-key', password: 'password' })
            .catch(() => undefined)
        }
      >
        login
      </button>
      <button
        type="button"
        onClick={() =>
          void auth
            .bootstrap({ login: 'alice', login_key: 'alice-key', password: 'password' })
            .catch(() => undefined)
        }
      >
        bootstrap
      </button>
      <button type="button" onClick={() => void auth.logout()}>
        logout
      </button>
    </div>
  )
}

function renderProvider(apis: ReturnType<typeof makeApis>) {
  return render(
    <AuthProvider authApi={apis.authApi} bootstrapApi={apis.bootstrapApi}>
      <Probe />
    </AuthProvider>,
  )
}

describe('AuthProvider', () => {
  it('represents initial loading and then the open bootstrap state', async () => {
    const status = deferred<BootstrapStatusResponse>()
    const apis = makeApis({}, { getStatus: vi.fn(() => status.promise) })
    renderProvider(apis)

    expect(screen.getByLabelText('auth status')).toHaveTextContent('loading')
    status.resolve(openStatus)

    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('bootstrap_required')
    })
    expect(apis.authApi.getCurrentSession).not.toHaveBeenCalled()
  })

  it('restores a closed bootstrap to unauthenticated when the session is absent', async () => {
    const apis = makeApis()
    renderProvider(apis)

    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('unauthenticated')
    })
  })

  it('lets the newest refresh win when an older status request resolves later', async () => {
    const first = deferred<BootstrapStatusResponse>()
    const apis = makeApis({}, {
      getStatus: vi
        .fn()
        .mockImplementationOnce(() => first.promise)
        .mockResolvedValueOnce(closedStatus),
    })
    renderProvider(apis)
    fireEvent.click(screen.getByRole('button', { name: 'refresh' }))

    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('unauthenticated')
    })
    first.resolve(openStatus)

    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('unauthenticated')
    })
  })

  it('transitions to authenticated after a successful login', async () => {
    const apis = makeApis({ login: vi.fn().mockResolvedValue(session) })
    renderProvider(apis)
    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('unauthenticated')
    })

    fireEvent.click(screen.getByRole('button', { name: 'login' }))
    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('authenticated')
    })
    expect(apis.authApi.login).toHaveBeenCalledWith({
      login: 'alice',
      login_key: 'alice-key',
      password: 'password',
    })
  })

  it('keeps invalid login generic and unauthenticated', async () => {
    const apis = makeApis({ login: vi.fn().mockRejectedValue(apiError(401, 'authentication_failed')) })
    renderProvider(apis)
    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('unauthenticated')
    })

    fireEvent.click(screen.getByRole('button', { name: 'login' }))
    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('unauthenticated')
      expect(screen.getByLabelText('auth error')).toHaveTextContent(
        'The login details were not accepted.',
      )
    })
  })

  it('moves a bootstrap-closed race to explicit login state', async () => {
    const apis = makeApis(
      {},
      {
        getStatus: vi.fn().mockResolvedValue(openStatus),
        createFirstAdmin: vi.fn().mockRejectedValue(apiError(409, 'bootstrap_closed')),
      },
    )
    renderProvider(apis)
    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('bootstrap_required')
    })

    fireEvent.click(screen.getByRole('button', { name: 'bootstrap' }))
    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('unauthenticated')
    })
  })

  it('obtains CSRF before logout and clears local state even when the session is revoked', async () => {
    const apis = makeApis({
      getCurrentSession: vi.fn().mockResolvedValue(session),
    })
    renderProvider(apis)
    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('authenticated')
    })

    fireEvent.click(screen.getByRole('button', { name: 'logout' }))
    await waitFor(() => {
      expect(screen.getByLabelText('auth status')).toHaveTextContent('unauthenticated')
    })
    expect(apis.authApi.getCsrfToken).toHaveBeenCalledOnce()
    expect(apis.authApi.logout).toHaveBeenCalledOnce()
  })
})
