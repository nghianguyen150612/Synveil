import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useEffect, useState } from 'react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiClient } from '../api/client'
import type { ApiErrorPayload } from '../api/errors'
import { toUserFacingMessage } from '../api/errors'
import { SafeDiagnosticsPanel } from './SafeDiagnosticsPanel'
import { diagnosticStore } from './store'

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

function failurePayload(): ApiErrorPayload {
  return {
    code: 'dependency_unavailable',
    message: 'private upstream detail for snapshot_id=private-snapshot',
    request_id: 'R1',
    retryable: true,
    details: {
      snapshot_id: 'private-snapshot',
      storage_key: 'private-storage-key',
    },
  }
}

function SnapshotFailureSurface({ client }: { readonly client: ApiClient }) {
  const [message, setMessage] = useState('Loading snapshot')

  useEffect(() => {
    let active = true
    void client
      .get('/api/v1/backups/snapshots/private-snapshot', {
        diagnosticOperation: 'SNAPSHOT_GET',
      })
      .catch((error: unknown) => {
        if (active) {
          setMessage(toUserFacingMessage(error))
        }
      })
    return () => {
      active = false
    }
  }, [client])

  return (
    <>
      <p role="alert">{message}</p>
      <SafeDiagnosticsPanel />
    </>
  )
}

describe('failure correlation end to end', () => {
  beforeEach(() => {
    diagnosticStore.clear()
    diagnosticStore.bindPrincipal('correlation-test-user')
  })

  it('keeps ordinary UI safe while correlating and explicitly copying R1', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(503, { error: failurePayload() }),
    )
    const client = new ApiClient({ fetchImpl })
    const writeText = vi.fn().mockResolvedValue(undefined)
    const previous = Object.getOwnPropertyDescriptor(navigator, 'clipboard')
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })

    try {
      render(
        <MemoryRouter initialEntries={['/health/dev']}>
          <SnapshotFailureSurface client={client} />
        </MemoryRouter>,
      )

      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The service is temporarily unavailable. Please try again.',
      )
      const event = diagnosticStore.all()[0]
      expect(event).toMatchObject({
        category: 'API',
        operation: 'SNAPSHOT_GET',
        http_status: 503,
        api_error_code: 'dependency_unavailable',
        server_request_id: 'R1',
        retryable: true,
      })
      expect(screen.getByText('R1')).toBeInTheDocument()

      fireEvent.click(screen.getByRole('button', { name: 'Copy diagnostics' }))
      await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Diagnostics copied.'))
      const copied = String(writeText.mock.calls[0]?.[0])
      expect(copied).toContain('R1')
      expect(copied).toContain('SNAPSHOT_GET')
      for (const secret of [
        'private upstream detail',
        'private-snapshot',
        'private-storage-key',
        'snapshot_id',
        'storage_key',
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
})
