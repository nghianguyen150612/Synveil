import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiClient } from '../api/client'
import { ApiRequestError, type ApiErrorPayload } from '../api/errors'
import {
  bindMutationRecoveryPrincipal,
  mutationRecoveryStore,
  settlePendingMutationFailure,
} from '../api/mutationRecovery'
import {
  diagnosticStore,
  DIAGNOSTICS_STORAGE_KEY,
} from './store'
import { recordAuthRecoveryFailure } from './record'

function errorPayload(overrides: Partial<ApiErrorPayload> = {}): ApiErrorPayload {
  return {
    code: 'dependency_unavailable',
    message: 'private server detail backup_set_id=sensitive-set snapshot_id=sensitive-snapshot',
    request_id: 'R1',
    retryable: true,
    details: {
      snapshot_id: 'sensitive-snapshot',
      storage_key: 'sensitive-storage-key',
      confirm_snapshot_id: 'sensitive-confirmation',
    },
    ...overrides,
  }
}

function jsonResponse(status: number, body: unknown, headers: HeadersInit = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json', ...headers },
  })
}

describe('diagnostic failure recording', () => {
  beforeEach(() => {
    diagnosticStore.clear()
    diagnosticStore.bindPrincipal('diagnostic-test-user')
    bindMutationRecoveryPrincipal('diagnostic-test-user')
    mutationRecoveryStore.clear()
  })

  it('correlates a canonical API failure to a symbolic operation without retaining the response body', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(503, { error: errorPayload() }),
    )
    const client = new ApiClient({ fetchImpl })

    await expect(client.get('/api/v1/backups/snapshots/private-snapshot', {
      diagnosticOperation: 'SNAPSHOT_LIST',
    })).rejects.toBeInstanceOf(ApiRequestError)

    const event = diagnosticStore.all()[0]
    expect(event).toMatchObject({
      category: 'API',
      operation: 'SNAPSHOT_LIST',
      http_method: 'GET',
      http_status: 503,
      code: 'API_DEPENDENCY_UNAVAILABLE',
      api_error_code: 'dependency_unavailable',
      server_request_id: 'R1',
      retryable: true,
    })
    const serialized = JSON.stringify(event)
    expect(serialized).not.toContain('private-snapshot')
    expect(serialized).not.toContain('sensitive-snapshot')
    expect(serialized).not.toContain('sensitive-storage-key')
    expect(serialized).not.toContain('private server detail')
    expect(serialized).not.toContain('api/v1')
    expect(fetchImpl.mock.calls[0]?.[1]).not.toHaveProperty('diagnosticOperation')
  })

  it('uses the accepted X-Request-ID response header when a non-JSON API error has no envelope', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      new Response('private upstream diagnostic', {
        status: 503,
        headers: { 'X-Request-ID': 'header-R1' },
      }),
    )
    const client = new ApiClient({ fetchImpl })

    await expect(client.get('/health/ready', {
      diagnosticOperation: 'AUTH_BOOTSTRAP',
    })).rejects.toBeInstanceOf(ApiRequestError)

    expect(diagnosticStore.all()[0]).toMatchObject({
      operation: 'AUTH_BOOTSTRAP',
      server_request_id: 'header-R1',
      code: 'API_DEPENDENCY_UNAVAILABLE',
    })
    expect(JSON.stringify(diagnosticStore.all())).not.toContain('private upstream diagnostic')
  })

  it('records a concealed 404 without inferring ownership', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(404, {
        error: errorPayload({
          code: 'not_found',
          message: 'ownership detail must not be echoed',
          request_id: 'not-found-R1',
          retryable: false,
        }),
      }),
    )
    const client = new ApiClient({ fetchImpl })

    await expect(client.get('/api/v1/backups/snapshots/foreign', {
      diagnosticOperation: 'SNAPSHOT_GET',
    })).rejects.toBeInstanceOf(ApiRequestError)

    expect(diagnosticStore.all()[0]).toMatchObject({
      category: 'API',
      operation: 'SNAPSHOT_GET',
      http_status: 404,
      code: 'API_NOT_FOUND',
      server_request_id: 'not-found-R1',
    })
    expect(JSON.stringify(diagnosticStore.all())).not.toContain('owner')
  })

  it('records a safe failure when a successful response has an invalid JSON body', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      new Response('private malformed response body', { status: 200 }),
    )
    const client = new ApiClient({ fetchImpl })

    await expect(client.get('/api/v1/backups/sets', {
      diagnosticOperation: 'BACKUP_SET_LIST',
    })).rejects.toThrow()

    expect(diagnosticStore.all()[0]).toMatchObject({
      category: 'API',
      operation: 'BACKUP_SET_LIST',
      code: 'API_INTERNAL_ERROR',
      http_method: 'GET',
      retryable: true,
    })
    expect(JSON.stringify(diagnosticStore.all())).not.toContain('private malformed response body')
  })

  it('classifies transport and abort failures without retaining arbitrary thrown messages', async () => {
    const fetchImpl = vi
      .fn<typeof fetch>()
      .mockRejectedValueOnce(new TypeError('https://private.example/file-name.txt'))
      .mockRejectedValueOnce(new DOMException('secret abort detail', 'AbortError'))
    const client = new ApiClient({ fetchImpl })

    await expect(client.get('/api/v1/libraries', {
      diagnosticOperation: 'LIBRARY_LIST',
    })).rejects.toThrow()
    await expect(client.get('/api/v1/libraries', {
      diagnosticOperation: 'LIBRARY_CHILDREN',
    })).rejects.toThrow()

    expect(diagnosticStore.all()).toEqual(expect.arrayContaining([
      expect.objectContaining({ category: 'NETWORK', operation: 'LIBRARY_LIST', code: 'NETWORK_UNAVAILABLE', retryable: true }),
      expect.objectContaining({ category: 'NETWORK', operation: 'LIBRARY_CHILDREN', code: 'REQUEST_ABORTED', retryable: false }),
    ]))
    const serialized = JSON.stringify(diagnosticStore.all())
    expect(serialized).not.toContain('private.example')
    expect(serialized).not.toContain('secret abort detail')
  })

  it('observes uncertain restore and prune delivery using only semantic operation names', () => {
    const restore = mutationRecoveryStore.add({
      action_kind: 'create_restore_plan',
      idempotency_key: '00000000-0000-7000-8000-000000000159',
      request_scope: { snapshotId: 'private-snapshot', backupSetId: 'private-set' },
      request: {
        destination_name: 'Private destination',
        snapshot_id: 'private-snapshot',
      },
    })
    expect(settlePendingMutationFailure(restore, new TypeError('network loss'))).toBe('retained')

    const prune = mutationRecoveryStore.add({
      action_kind: 'execute_prune_plan',
      idempotency_key: '00000000-0000-7000-8000-000000000160',
      request_scope: { prunePlanId: 'private-plan', snapshotId: 'private-snapshot' },
      request: { confirm_snapshot_id: 'private-snapshot' },
    })
    expect(settlePendingMutationFailure(prune, new TypeError('network loss'))).toBe('retained')

    expect(diagnosticStore.all()).toEqual(expect.arrayContaining([
      expect.objectContaining({
        category: 'MUTATION_RECOVERY',
        operation: 'RESTORE_PLAN_CREATE',
        code: 'MUTATION_OUTCOME_UNCERTAIN',
      }),
      expect.objectContaining({
        category: 'MUTATION_RECOVERY',
        operation: 'PRUNE_EXECUTE',
        code: 'MUTATION_OUTCOME_UNCERTAIN',
      }),
    ]))
    const serialized = JSON.stringify(diagnosticStore.all())
    for (const secret of [
      '00000000-0000-7000-8000-000000000159',
      '00000000-0000-7000-8000-000000000160',
      'confirm_snapshot_id',
      'private-snapshot',
      'Private destination',
      'private-plan',
    ]) {
      expect(serialized).not.toContain(secret)
    }
    expect(sessionStorage.getItem(DIAGNOSTICS_STORAGE_KEY)).not.toContain('idempotency_key')
  })

  it('coalesces repeated auth recovery failures without recording principal data', () => {
    const error = new ApiRequestError(503, errorPayload({
      code: 'internal_dependency_unavailable',
      message: 'user_id=private-user email=private@example.test Cookie=private-cookie',
      request_id: 'auth-R1',
      retryable: true,
    }))
    recordAuthRecoveryFailure(error)
    recordAuthRecoveryFailure(error)

    expect(diagnosticStore.all()).toHaveLength(1)
    expect(diagnosticStore.all()[0]).toMatchObject({
      category: 'AUTH',
      operation: 'AUTH_RECOVERY',
      code: 'AUTH_RECOVERY_FAILED',
      server_request_id: 'auth-R1',
      occurrences: 2,
    })
    const serialized = JSON.stringify(diagnosticStore.all())
    expect(serialized).not.toContain('private-user')
    expect(serialized).not.toContain('private@example.test')
    expect(serialized).not.toContain('private-cookie')
  })

  it('keeps the API error path intact when diagnostic storage itself throws', async () => {
    const diagnosticWrite = vi.spyOn(diagnosticStore, 'record').mockImplementation(() => {
      throw new Error('diagnostic storage failure')
    })
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(503, { error: errorPayload() }),
    )
    const client = new ApiClient({ fetchImpl })

    try {
      await expect(client.get('/api/v1/backups/sets', {
        diagnosticOperation: 'BACKUP_SET_LIST',
      })).rejects.toBeInstanceOf(ApiRequestError)
      expect(fetchImpl).toHaveBeenCalledOnce()
    } finally {
      diagnosticWrite.mockRestore()
    }
  })
})
