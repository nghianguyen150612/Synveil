import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AuthApi, AuthSessionResponse } from '../api/auth'
import type { BootstrapApi, BootstrapStatusResponse } from '../api/bootstrap'
import { ApiRequestError, type ApiErrorPayload } from '../api/errors'
import { mutationRecoveryStore } from '../api/mutationRecovery'
import { App } from './App'

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

function apiError(status: number, code: string): ApiRequestError {
  const payload: ApiErrorPayload = {
    code,
    message: 'safe server message',
    request_id: 'error-1',
    retryable: status >= 500,
  }
  return new ApiRequestError(status, payload)
}

function makeApis(options: {
  readonly setupRequired?: boolean
  readonly login?: AuthApi['login']
  readonly getCurrentSession?: AuthApi['getCurrentSession']
  readonly createFirstAdmin?: BootstrapApi['createFirstAdmin']
} = {}): { authApi: AuthApi; bootstrapApi: BootstrapApi } {
  const setupRequired = options.setupRequired ?? false
  return {
    authApi: {
      login: options.login ?? vi.fn().mockResolvedValue(session),
      getCurrentSession:
        options.getCurrentSession ??
        vi.fn().mockRejectedValue(apiError(401, 'authentication_failed')),
      getCsrfToken: vi.fn().mockResolvedValue({
        data: { csrf_token: '0'.repeat(128) },
        meta: { request_id: 'csrf-1' },
      }),
      logout: vi.fn().mockResolvedValue(undefined),
    },
    bootstrapApi: {
      getStatus: vi.fn().mockResolvedValue(setupRequired ? openStatus : closedStatus),
      createFirstAdmin: options.createFirstAdmin ?? vi.fn().mockResolvedValue(closedStatus),
    },
  }
}

function fillSetupForm() {
  fireEvent.change(screen.getByLabelText('Login identifier'), {
    target: { value: 'alice' },
  })
  fireEvent.change(screen.getByLabelText('Login key'), {
    target: { value: 'alice-key' },
  })
  fireEvent.change(screen.getByLabelText('Password'), {
    target: { value: 'correct horse battery staple' },
  })
  fireEvent.change(screen.getByLabelText('Confirm password'), {
    target: { value: 'correct horse battery staple' },
  })
}

function fillLoginForm() {
  fireEvent.change(screen.getByLabelText('Login identifier'), {
    target: { value: 'alice' },
  })
  fireEvent.change(screen.getByLabelText('Login key'), {
    target: { value: 'alice-key' },
  })
  fireEvent.change(screen.getByLabelText('Password'), {
    target: { value: 'correct horse battery staple' },
  })
}

describe('web authentication and bootstrap routes', () => {
  beforeEach(() => mutationRecoveryStore.clear())

  it('routes an open server to the setup page', async () => {
    render(<App router="memory" initialEntries={['/']} {...makeApis({ setupRequired: true })} />)

    expect(await screen.findByRole('heading', { name: /set up synveil/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /create administrator/i })).toBeInTheDocument()
  })

  it('validates password confirmation in the setup UI only', async () => {
    const apis = makeApis({ setupRequired: true })
    render(<App router="memory" initialEntries={['/']} {...apis} />)
    await screen.findByRole('heading', { name: /set up synveil/i })
    fillSetupForm()
    fireEvent.change(screen.getByLabelText('Confirm password'), {
      target: { value: 'different password' },
    })
    fireEvent.click(screen.getByRole('button', { name: /create administrator/i }))

    expect(await screen.findByRole('alert')).toHaveTextContent('The passwords do not match.')
    expect(apis.bootstrapApi.createFirstAdmin).not.toHaveBeenCalled()
  })

  it('submits setup and transitions to explicit login', async () => {
    const createFirstAdmin = vi.fn().mockResolvedValue(closedStatus)
    const apis = makeApis({ setupRequired: true, createFirstAdmin })
    render(<App router="memory" initialEntries={['/']} {...apis} />)
    await screen.findByRole('heading', { name: /set up synveil/i })
    fillSetupForm()
    fireEvent.click(screen.getByRole('button', { name: /create administrator/i }))

    expect(await screen.findByRole('heading', { name: /sign in/i })).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Administrator created')
    expect(createFirstAdmin).toHaveBeenCalledWith({
      login: 'alice',
      login_key: 'alice-key',
      password: 'correct horse battery staple',
    })
  })

  it('moves a setup-closed race to login without exposing server details', async () => {
    const createFirstAdmin = vi
      .fn()
      .mockRejectedValue(apiError(409, 'bootstrap_closed'))
    const apis = makeApis({ setupRequired: true, createFirstAdmin })
    render(<App router="memory" initialEntries={['/setup']} {...apis} />)
    await screen.findByRole('heading', { name: /set up synveil/i })
    fillSetupForm()
    fireEvent.click(screen.getByRole('button', { name: /create administrator/i }))

    expect(await screen.findByRole('heading', { name: /sign in/i })).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Initial setup is already complete')
    expect(screen.queryByText('safe server message')).not.toBeInTheDocument()
  })

  it('keeps invalid login generic', async () => {
    const login = vi.fn().mockRejectedValue(apiError(401, 'authentication_failed'))
    const apis = makeApis({ login })
    render(<App router="memory" initialEntries={['/']} {...apis} />)
    await screen.findByRole('heading', { name: /sign in/i })
    fillLoginForm()
    fireEvent.click(screen.getByRole('button', { name: /^sign in$/i }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The login details were not accepted.',
    )
    expect(screen.queryByText('safe server message')).not.toBeInTheDocument()
  })

  it('logs in and renders only the authenticated placeholder shell', async () => {
    const login = vi.fn().mockResolvedValue(session)
    const apis = makeApis({ login })
    render(<App router="memory" initialEntries={['/']} {...apis} />)
    await screen.findByRole('heading', { name: /sign in/i })
    fillLoginForm()
    fireEvent.click(screen.getByRole('button', { name: /^sign in$/i }))

    expect(await screen.findByRole('heading', { name: /welcome to synveil/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /logout/i })).toBeInTheDocument()
  })

  it('restores an authenticated session and logout uses the CSRF boundary', async () => {
    const getCurrentSession = vi.fn().mockResolvedValue(session)
    const apis = makeApis({ getCurrentSession })
    render(<App router="memory" initialEntries={['/']} {...apis} />)
    await screen.findByRole('heading', { name: /welcome to synveil/i })
    fireEvent.click(screen.getByRole('button', { name: /logout/i }))

    expect(await screen.findByRole('heading', { name: /sign in/i })).toBeInTheDocument()
    expect(apis.authApi.getCsrfToken).toHaveBeenCalledOnce()
    expect(apis.authApi.logout).toHaveBeenCalledOnce()
  })

  it('intentional logout clears protected UI without deleting safe same-tab retry identity', async () => {
    const getCurrentSession = vi.fn().mockResolvedValue(session)
    const apis = makeApis({ getCurrentSession })
    render(<App router="memory" initialEntries={['/']} {...apis} />)
    await screen.findByRole('heading', { name: /welcome to synveil/i })
    const record = mutationRecoveryStore.add({
      action_kind: 'create_backup_set',
      idempotency_key: '01925000-0000-7000-8000-000000000099',
      request_scope: {},
      request: { library_id: 'library-1', name: 'Safe retry' },
    })

    fireEvent.click(screen.getByRole('button', { name: /logout/i }))

    expect(await screen.findByRole('heading', { name: /sign in/i })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: /welcome to synveil/i })).not.toBeInTheDocument()
    expect(mutationRecoveryStore.all().map((item) => item.action_id)).toEqual([record.action_id])
    expect(apis.authApi.logout).toHaveBeenCalledOnce()
  })

  it('treats an expired or missing session as unauthenticated', async () => {
    const apis = makeApis({ getCurrentSession: vi.fn().mockRejectedValue(apiError(401, 'authentication_failed')) })
    render(<App router="memory" initialEntries={['/health/dev']} {...apis} />)

    expect(await screen.findByRole('heading', { name: /sign in/i })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Diagnostics' })).not.toBeInTheDocument()
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: /welcome to synveil/i })).not.toBeInTheDocument()
    })
  })
})
