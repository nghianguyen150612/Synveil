import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  bindMutationRecoveryPrincipal,
  mutationRecoveryStore,
} from '../api/mutationRecovery'
import { diagnosticStore } from '../diagnostics/store'
import { AppErrorBoundary } from './AppErrorBoundary'

function ThrowingChild(): never {
  throw new Error('private component message with file name.txt')
}

describe('AppErrorBoundary', () => {
  beforeEach(() => {
    diagnosticStore.clear()
    diagnosticStore.bindPrincipal('boundary-test-user')
    bindMutationRecoveryPrincipal('boundary-test-user')
    mutationRecoveryStore.clear()
  })

  it('contains a render failure with safe recovery actions and preserves pending mutations', () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const pending = mutationRecoveryStore.add({
      action_kind: 'create_restore_plan',
      idempotency_key: '00000000-0000-7000-8000-000000000161',
      request_scope: { snapshotId: 'private-snapshot' },
      request: { destination_name: 'Private destination' },
    })

    render(
      <AppErrorBoundary routeCategory="BACKUP_DETAIL">
        <ThrowingChild />
      </AppErrorBoundary>,
    )

    expect(screen.getByRole('alert')).toHaveTextContent('Something went wrong in this page.')
    expect(screen.getByRole('button', { name: 'Reload page' })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Open diagnostics' })).toHaveAttribute('href', '/health/dev')
    expect(screen.queryByText(/private component message/i)).not.toBeInTheDocument()
    expect(screen.queryByText(/file name\.txt/i)).not.toBeInTheDocument()
    expect(mutationRecoveryStore.all()).toEqual([pending])

    const event = diagnosticStore.all()[0]
    expect(event).toMatchObject({
      category: 'CLIENT_RENDER',
      operation: 'CLIENT_RENDER',
      code: 'CLIENT_RENDER_FAILURE',
      route_category: 'BACKUP_DETAIL',
      retryable: false,
    })
    const serialized = JSON.stringify(diagnosticStore.all())
    expect(serialized).not.toContain('private component message')
    expect(serialized).not.toContain('file name.txt')
    consoleError.mockRestore()
  })
})
