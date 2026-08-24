import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import type { AuthApi, AuthSessionResponse } from '../api/auth'
import type { BootstrapApi, BootstrapStatusResponse } from '../api/bootstrap'
import { App } from './App'

const session: AuthSessionResponse = {
  data: {
    authenticated: true,
    user_id: '00000000-0000-7000-8000-000000000001',
    is_instance_admin: true,
    session_id: '00000000-0000-7000-8000-000000000002',
  },
  meta: { request_id: 'test-session' },
}

function authenticatedApis(): { authApi: AuthApi; bootstrapApi: BootstrapApi } {
  const status: BootstrapStatusResponse = {
    data: { setup_required: false },
    meta: { request_id: 'test-bootstrap' },
  }
  return {
    authApi: {
      login: async () => session,
      getCurrentSession: async () => session,
      getCsrfToken: async () => ({
        data: { csrf_token: '0'.repeat(128) },
        meta: { request_id: 'test-csrf' },
      }),
      logout: async () => undefined,
    },
    bootstrapApi: {
      getStatus: async () => status,
      createFirstAdmin: async () => status,
    },
  }
}

describe('application bootstrap', () => {
  it('restores a session and renders the minimal authenticated shell', async () => {
    render(<App router="memory" initialEntries={['/']} {...authenticatedApis()} />)

    expect(await screen.findByRole('banner')).toBeInTheDocument()
    expect(screen.getByRole('main')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: /welcome to synveil/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /logout/i })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /^Development health$/ })).toHaveAttribute(
      'href',
      '/health/dev',
    )
  })

  it('renders the explicit not-found route without product data', async () => {
    render(<App router="memory" initialEntries={['/does-not-exist']} {...authenticatedApis()} />)

    expect(await screen.findByRole('heading', { name: /page does not exist/i })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /return home/i })).toHaveAttribute('href', '/')
  })
})
