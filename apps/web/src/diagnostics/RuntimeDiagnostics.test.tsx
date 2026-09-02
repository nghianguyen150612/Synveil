import { render } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { RuntimeDiagnostics } from './RuntimeDiagnostics'
import { diagnosticStore } from './store'

describe('RuntimeDiagnostics', () => {
  beforeEach(() => {
    diagnosticStore.clear()
    diagnosticStore.bindPrincipal('runtime-test-user')
  })

  it('records global error signals without retaining thrown values', () => {
    const view = render(<RuntimeDiagnostics />)
    window.dispatchEvent(new Event('error'))
    window.dispatchEvent(new Event('unhandledrejection'))

    expect(diagnosticStore.all()).toHaveLength(1)
    expect(diagnosticStore.all()[0]).toMatchObject({
      category: 'CLIENT_RUNTIME',
      operation: 'CLIENT_RUNTIME',
      code: 'CLIENT_RUNTIME_FAILURE',
      occurrences: 2,
    })
    expect(JSON.stringify(diagnosticStore.all())).not.toContain('message')
    view.unmount()
  })

  it('does not require console logging to provide diagnostics', () => {
    const consoleError = vi.spyOn(console, 'error')
    render(<RuntimeDiagnostics />)
    window.dispatchEvent(new Event('error'))

    expect(diagnosticStore.all()).toHaveLength(1)
    expect(consoleError).not.toHaveBeenCalled()
    consoleError.mockRestore()
  })
})
