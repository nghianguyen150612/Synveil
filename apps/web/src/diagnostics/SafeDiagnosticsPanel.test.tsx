import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { AppErrorBoundary } from '../app/AppErrorBoundary'
import { SafeDiagnosticsPanel } from './SafeDiagnosticsPanel'
import { diagnosticStore } from './store'

function addEvent(serverRequestId = 'R1') {
  diagnosticStore.record({
    severity: 'ERROR',
    category: 'API',
    operation: 'SNAPSHOT_LIST',
    outcome: 'FAILED',
    code: 'API_DEPENDENCY_UNAVAILABLE',
    http_method: 'GET',
    http_status: 503,
    api_error_code: 'dependency_unavailable',
    ...(serverRequestId ? { server_request_id: serverRequestId } : {}),
    retryable: true,
  })
}

async function checkA11y(container: HTMLElement): Promise<void> {
  const axe = await import('axe-core')
  const results = await axe.default.run(container, {
    rules: { 'color-contrast': { enabled: false } },
  })
  const serious = results.violations.filter((violation) =>
    violation.impact === 'serious' || violation.impact === 'critical',
  )
  expect(serious).toEqual([])
}

function renderPanel() {
  return render(
    <MemoryRouter initialEntries={['/health/dev']}>
      <SafeDiagnosticsPanel />
    </MemoryRouter>,
  )
}

describe('SafeDiagnosticsPanel', () => {
  beforeEach(() => {
    diagnosticStore.clear()
    diagnosticStore.bindPrincipal('panel-test-user')
  })

  it('presents only sanitized failure fields and the exact server request ID', () => {
    addEvent()
    renderPanel()

    expect(screen.getByRole('heading', { name: 'Diagnostics' })).toBeInTheDocument()
    expect(screen.getByText('SNAPSHOT_LIST')).toBeInTheDocument()
    expect(screen.getByText(/Request ID:/)).toBeInTheDocument()
    expect(screen.getByText('R1')).toBeInTheDocument()
    const tableText = screen.getByRole('table').textContent ?? ''
    expect(tableText).not.toMatch(/Cookie|CSRF|Idempotency-Key|Authorization|password|email|user_id/i)
    expect(tableText).not.toMatch(/backup_set_id|snapshot_id|storage_key|ObjectId/i)
  })

  it('copies an explicit safe bundle and announces success', async () => {
    addEvent()
    diagnosticStore.record({
      severity: 'ERROR',
      category: 'AUTH',
      operation: 'AUTH_RECOVERY',
      outcome: 'FAILED',
      code: 'AUTH_RECOVERY_FAILED',
      http_status: 503,
      api_error_code: 'internal_dependency_unavailable',
      server_request_id: 'auth-R1',
      retryable: true,
    })
    diagnosticStore.record({
      severity: 'WARNING',
      category: 'MUTATION_RECOVERY',
      operation: 'RESTORE_PLAN_CREATE',
      outcome: 'FAILED',
      code: 'MUTATION_OUTCOME_UNCERTAIN',
      retryable: true,
    })
    diagnosticStore.record({
      severity: 'ERROR',
      category: 'CLIENT_RENDER',
      operation: 'CLIENT_RENDER',
      outcome: 'FAILED',
      code: 'CLIENT_RENDER_FAILURE',
      route_category: 'BACKUP_DETAIL',
      retryable: false,
    })
    const writeText = vi.fn().mockResolvedValue(undefined)
    const previous = Object.getOwnPropertyDescriptor(navigator, 'clipboard')
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    try {
      renderPanel()
      fireEvent.click(screen.getByRole('button', { name: 'Copy diagnostics' }))

      await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Diagnostics copied.'))
      expect(writeText).toHaveBeenCalledOnce()
      const copied = String(writeText.mock.calls[0]?.[0])
      expect(copied).toContain('R1')
      expect(copied).toContain('SNAPSHOT_LIST')
      expect(copied).toContain('AUTH_RECOVERY_FAILED')
      expect(copied).toContain('MUTATION_OUTCOME_UNCERTAIN')
      expect(copied).toContain('CLIENT_RENDER_FAILURE')
      for (const secret of [
        'Cookie',
        'CSRF',
        'Idempotency-Key',
        'Authorization',
        'password',
        'email',
        'user_id',
        'backup_set_id',
        'snapshot_id',
        'storage_key',
        'ObjectId',
      ]) {
        expect(copied).not.toContain(secret)
      }
    } finally {
      if (previous) {
        Object.defineProperty(navigator, 'clipboard', previous)
      } else {
        Reflect.deleteProperty(navigator, 'clipboard')
      }
    }
  })

  it('announces clipboard failure safely and remains rendered', async () => {
    addEvent()
    const previous = Object.getOwnPropertyDescriptor(navigator, 'clipboard')
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error('private clipboard detail')) },
    })
    try {
      renderPanel()
      fireEvent.click(screen.getByRole('button', { name: 'Copy diagnostics' }))
      await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent(/could not be copied/i))
      expect(screen.getByRole('heading', { name: 'Diagnostics' })).toBeInTheDocument()
      expect(screen.queryByText(/private clipboard detail/i)).not.toBeInTheDocument()
    } finally {
      if (previous) {
        Object.defineProperty(navigator, 'clipboard', previous)
      } else {
        Reflect.deleteProperty(navigator, 'clipboard')
      }
    }
  })

  it('clears only diagnostics and keeps a friendly empty state', () => {
    addEvent()
    renderPanel()
    fireEvent.click(screen.getByRole('button', { name: 'Clear diagnostics' }))

    expect(diagnosticStore.all()).toHaveLength(0)
    expect(screen.getByText('No recent diagnostic events.')).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Diagnostics cleared.')
  })

  it('does not render a broken request-ID label when correlation is absent', () => {
    addEvent('')
    renderPanel()

    expect(screen.getByText('Not available')).toBeInTheDocument()
    expect(screen.queryByText(/Request ID:/)).not.toBeInTheDocument()
  })

  it('has no serious or critical accessibility violations when populated', async () => {
    addEvent()
    const { container } = renderPanel()
    await checkA11y(container)
  })

  it('has no serious or critical accessibility violations when empty', async () => {
    const { container } = renderPanel()
    await checkA11y(container)
  })

  it('keeps the error-boundary fallback accessible', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const { container } = render(
      <AppErrorBoundary routeCategory="DEVELOPMENT_HEALTH">
        <ThrowingPanelChild />
      </AppErrorBoundary>,
    )
    await checkA11y(container)
    consoleError.mockRestore()
  })
})

function ThrowingPanelChild(): never {
  throw new Error('private panel failure')
}
