import { describe, expect, it, vi } from 'vitest'

import { ApiClient } from './client'
import { ApiRequestError } from './errors'

describe('ApiClient', () => {
  it('maps the reviewed API error envelope without exposing transport details', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(
        JSON.stringify({
          error: {
            code: 'internal_dependency_unavailable',
            message: 'Required service dependencies are not ready.',
            request_id: 'request-1234',
            retryable: true,
          },
        }),
        { status: 503, headers: { 'content-type': 'application/json' } },
      ),
    )
    const client = new ApiClient({ baseUrl: '/api/v1', fetchImpl })

    await expect(client.get('/system/health')).rejects.toMatchObject({
      status: 503,
      code: 'internal_dependency_unavailable',
      requestId: 'request-1234',
      retryable: true,
    })
    expect(fetchImpl).toHaveBeenCalledWith(
      '/api/v1/system/health',
      expect.objectContaining({ credentials: 'same-origin', method: 'GET' }),
    )
  })

  it('returns typed JSON through the single public API boundary', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(JSON.stringify({ status: 'live' }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    )
    const client = new ApiClient({ fetchImpl })

    await expect(client.get<{ status: string }>('/health/live')).resolves.toEqual({
      status: 'live',
    })
    expect(fetchImpl.mock.calls[0]?.[0]).toBe('/health/live')
  })

  it('uses a safe fallback for non-JSON failures', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      new Response('upstream diagnostic should not become UI copy', { status: 500 }),
    )
    const client = new ApiClient({ fetchImpl })

    const failure = await client.get('/health/live').catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(ApiRequestError)
    expect((failure as ApiRequestError).message).toBe('The request could not be completed.')
  })
})
