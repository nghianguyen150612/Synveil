import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AuthApi, AuthSessionResponse } from '../api/auth'
import type { BootstrapApi } from '../api/bootstrap'
import { ApiRequestError } from '../api/errors'
import { bindMutationRecoveryPrincipal, mutationRecoveryStore } from '../api/mutationRecovery'
import { AuthProvider } from '../auth/AuthProvider'
import { useAuth } from '../auth/useAuth'
import { diagnosticStore } from './store'

const session: AuthSessionResponse = {
  data: {
    authenticated: true,
    user_id: '00000000-0000-7000-8000-000000000301',
    is_instance_admin: false,
    session_id: '00000000-0000-7000-8000-000000000302',
  },
  meta: { request_id: 'session' },
}

function Probe() {
  const auth = useAuth()
  return (
    <>
      <output aria-label="auth status">{auth.status}</output>
      <button type="button" onClick={() => void auth.recoverSession()}>Recover</button>
      <button
        type="button"
        onClick={() => void Promise.all(Array.from({ length: 5 }, () => auth.recoverSession()))}
      >
        Recover concurrently
      </button>
      <button type="button" onClick={() => void auth.refresh()}>Refresh</button>
      <button type="button" onClick={() => void auth.logout()}>Logout</button>
    </>
  )
}

describe('AuthProvider diagnostics integration', () => {
  beforeEach(() => {
    diagnosticStore.clear()
  })

  it('records one safe AUTH_RECOVERY_FAILED event for five concurrent single-flight recoveries', async () => {
    const authApi: AuthApi = {
      login: vi.fn().mockResolvedValue(session),
      getCurrentSession: vi.fn()
        .mockResolvedValueOnce(session)
        .mockRejectedValueOnce(new ApiRequestError(503, {
          code: 'internal_dependency_unavailable',
          message: 'private recovery implementation detail',
          request_id: 'recovery-R1',
          retryable: true,
          details: { user_id: 'private-user', session_id: 'private-session' },
        })),
      getCsrfToken: vi.fn().mockResolvedValue({
        data: { csrf_token: '0'.repeat(128) },
        meta: { request_id: 'csrf' },
      }),
      logout: vi.fn().mockResolvedValue(undefined),
    }
    const bootstrapApi: BootstrapApi = {
      getStatus: vi.fn().mockResolvedValue({
        data: { setup_required: false },
        meta: { request_id: 'bootstrap' },
      }),
      createFirstAdmin: vi.fn(),
    }

    render(
      <AuthProvider authApi={authApi} bootstrapApi={bootstrapApi}>
        <Probe />
      </AuthProvider>,
    )
    await waitFor(() => expect(screen.getByLabelText('auth status')).toHaveTextContent('authenticated'))

    fireEvent.click(screen.getByRole('button', { name: 'Recover concurrently' }))
    await waitFor(() => expect(screen.getByLabelText('auth status')).toHaveTextContent('recovery_error'))
    await waitFor(() => expect(authApi.getCurrentSession).toHaveBeenCalledTimes(2))

    expect(diagnosticStore.all()[0]).toMatchObject({
      category: 'AUTH',
      operation: 'AUTH_RECOVERY',
      code: 'AUTH_RECOVERY_FAILED',
      http_status: 503,
      server_request_id: 'recovery-R1',
    })
    expect(diagnosticStore.all()).toHaveLength(1)
    const serialized = JSON.stringify(diagnosticStore.all())
    expect(serialized).not.toContain('private recovery implementation detail')
    expect(serialized).not.toContain('private-user')
    expect(serialized).not.toContain('private-session')
  })

  it('clears diagnostics across account changes and logout without clearing mutation recovery', async () => {
    const accountB: AuthSessionResponse = {
      data: {
        ...session.data,
        user_id: '00000000-0000-7000-8000-000000000303',
        session_id: '00000000-0000-7000-8000-000000000304',
      },
      meta: { request_id: 'session-b' },
    }
    const authApi: AuthApi = {
      login: vi.fn().mockResolvedValue(session),
      getCurrentSession: vi.fn().mockResolvedValueOnce(session).mockResolvedValueOnce(accountB),
      getCsrfToken: vi.fn().mockResolvedValue({
        data: { csrf_token: '0'.repeat(128) },
        meta: { request_id: 'csrf' },
      }),
      logout: vi.fn().mockResolvedValue(undefined),
    }
    const bootstrapApi: BootstrapApi = {
      getStatus: vi.fn().mockResolvedValue({
        data: { setup_required: false },
        meta: { request_id: 'bootstrap' },
      }),
      createFirstAdmin: vi.fn(),
    }

    render(
      <AuthProvider authApi={authApi} bootstrapApi={bootstrapApi}>
        <Probe />
      </AuthProvider>,
    )
    await waitFor(() => expect(screen.getByLabelText('auth status')).toHaveTextContent('authenticated'))
    diagnosticStore.record({
      severity: 'ERROR',
      category: 'API',
      operation: 'SNAPSHOT_LIST',
      outcome: 'FAILED',
      code: 'API_NOT_FOUND',
      http_method: 'GET',
      http_status: 404,
      server_request_id: 'account-a-R1',
      retryable: false,
    })
    bindMutationRecoveryPrincipal(session.data.user_id)
    const pending = mutationRecoveryStore.add({
      action_kind: 'create_restore_plan',
      idempotency_key: '00000000-0000-7000-8000-000000000305',
      request_scope: { snapshotId: 'private-snapshot' },
      request: { destination_name: 'Private destination' },
    })

    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByLabelText('auth status')).toHaveTextContent('authenticated'))
    expect(diagnosticStore.all()).toHaveLength(0)

    diagnosticStore.record({
      severity: 'ERROR',
      category: 'API',
      operation: 'SNAPSHOT_LIST',
      outcome: 'FAILED',
      code: 'API_NOT_FOUND',
      http_method: 'GET',
      http_status: 404,
      server_request_id: 'account-b-R1',
      retryable: false,
    })
    fireEvent.click(screen.getByRole('button', { name: 'Logout' }))
    await waitFor(() => expect(screen.getByLabelText('auth status')).toHaveTextContent('unauthenticated'))
    expect(diagnosticStore.all()).toHaveLength(0)
    expect(mutationRecoveryStore.all().map((record) => record.action_id)).toEqual([pending.action_id])
  })
})
