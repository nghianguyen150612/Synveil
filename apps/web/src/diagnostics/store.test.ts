import { describe, expect, it } from 'vitest'

import { serializeDiagnosticsBundle } from './format'
import {
  DiagnosticStore,
  DIAGNOSTICS_STORAGE_KEY,
  MAX_DIAGNOSTIC_EVENTS,
} from './store'

function memoryStorage(): Storage {
  const values = new Map<string, string>()
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
    clear: () => values.clear(),
    key: (index) => [...values.keys()][index] ?? null,
    get length(): number {
      return values.size
    },
  }
}

function throwingStorage(): Storage {
  return {
    getItem: () => { throw new Error('storage blocked') },
    setItem: () => { throw new Error('storage blocked') },
    removeItem: () => { throw new Error('storage blocked') },
    clear: () => { throw new Error('storage blocked') },
    key: () => { throw new Error('storage blocked') },
    get length(): number {
      throw new Error('storage blocked')
    },
  }
}

function input(operation: 'SNAPSHOT_LIST' | 'RESTORE_PLAN_CREATE' = 'SNAPSHOT_LIST') {
  return {
    severity: 'ERROR' as const,
    category: 'API' as const,
    operation,
    outcome: 'FAILED' as const,
    code: 'API_DEPENDENCY_UNAVAILABLE' as const,
    http_method: 'GET' as const,
    http_status: 503,
    api_error_code: 'dependency_unavailable',
    server_request_id: 'R1',
    retryable: true,
  }
}

function eventId(sequence: number): string {
  return `00000000-0000-7000-8000-${String(sequence).padStart(12, '0')}`
}

describe('DiagnosticStore', () => {
  it('stores a typed bounded event, deduplicates nearby failures, and persists only the safe envelope', () => {
    const storage = memoryStorage()
    let sequence = 0
    let now = 0
    const store = new DiagnosticStore({
      storage,
      now: () => new Date(Date.UTC(2026, 0, 1, 0, 0, now++)).toISOString(),
      createEventId: () => eventId(++sequence),
    })
    store.bindPrincipal('user-a')

    store.record(input())
    const second = store.record(input())

    expect(store.all()).toHaveLength(1)
    expect(second).toMatchObject({
      operation: 'SNAPSHOT_LIST',
      http_status: 503,
      server_request_id: 'R1',
      occurrences: 2,
    })
    expect(JSON.parse(storage.getItem(DIAGNOSTICS_STORAGE_KEY) ?? '{}')).toMatchObject({
      schema_version: 1,
      events: [{ operation: 'SNAPSHOT_LIST', server_request_id: 'R1' }],
    })
  })

  it('evicts oldest entries at the configured bound', () => {
    const store = new DiagnosticStore({
      storage: memoryStorage(),
      maxEvents: 3,
      now: (() => {
        let second = 0
        return () => new Date(Date.UTC(2026, 0, 1, 0, 0, second++ * 6)).toISOString()
      })(),
      createEventId: (() => {
        let sequence = 0
        return () => eventId(++sequence)
      })(),
    })
    store.bindPrincipal('user-a')

    for (let index = 0; index < 5; index += 1) {
      store.record(input())
    }

    expect(MAX_DIAGNOSTIC_EVENTS).toBe(100)
    expect(store.maxEvents).toBe(3)
    expect(store.all()).toHaveLength(3)
    expect(store.all().map((event) => event.event_id)).toEqual([
      eventId(5),
      eventId(4),
      eventId(3),
    ])
  })

  it('fails closed for malformed and future storage schemas', () => {
    const storage = memoryStorage()
    storage.setItem(DIAGNOSTICS_STORAGE_KEY, '{not-json')
    const malformed = new DiagnosticStore({ storage })
    expect(malformed.all()).toEqual([])
    expect(storage.getItem(DIAGNOSTICS_STORAGE_KEY)).toBeNull()

    storage.setItem(DIAGNOSTICS_STORAGE_KEY, JSON.stringify({ schema_version: 999, events: [] }))
    const future = new DiagnosticStore({ storage })
    expect(future.all()).toEqual([])
    expect(storage.getItem(DIAGNOSTICS_STORAGE_KEY)).toBeNull()

    storage.setItem(DIAGNOSTICS_STORAGE_KEY, JSON.stringify({
      schema_version: 1,
      events: [{
        schema_version: 1,
        event_id: eventId(1),
        occurred_at: '2026-01-01T00:00:00.000Z',
        severity: 'ERROR',
        category: 'API',
        operation: 'SNAPSHOT_LIST',
        outcome: 'FAILED',
        code: 'API_INTERNAL_ERROR',
        retryable: true,
        secret_body: 'must be rejected',
      }],
    }))
    const invalidEvent = new DiagnosticStore({ storage })
    expect(invalidEvent.all()).toEqual([])
    expect(storage.getItem(DIAGNOSTICS_STORAGE_KEY)).not.toContain('secret_body')
  })

  it('removes an oversized storage envelope before parsing it', () => {
    const storage = memoryStorage()
    storage.setItem(DIAGNOSTICS_STORAGE_KEY, 'x'.repeat(1024))

    const store = new DiagnosticStore({ storage, maxStorageCharacters: 128 })

    expect(store.all()).toEqual([])
    expect(storage.getItem(DIAGNOSTICS_STORAGE_KEY)).toBeNull()
  })

  it('keeps product-safe in-memory diagnostics when sessionStorage is unavailable', () => {
    const store = new DiagnosticStore({ storage: throwingStorage(), createEventId: () => eventId(1) })
    store.bindPrincipal('user-a')

    expect(() => store.record(input())).not.toThrow()
    expect(store.all()).toHaveLength(1)
    expect(store.isStorageAvailable).toBe(false)
  })

  it('clears the prior browser buffer when the authenticated principal changes', () => {
    const store = new DiagnosticStore({ storage: memoryStorage(), createEventId: () => eventId(1) })
    store.bindPrincipal('user-a')
    store.record(input())
    expect(store.all()).toHaveLength(1)

    store.bindPrincipal('user-b')
    expect(store.all()).toHaveLength(0)
    store.record(input('RESTORE_PLAN_CREATE'))
    store.bindPrincipal('user-b')
    expect(store.all()).toHaveLength(1)
  })

  it('produces a copy bundle from an explicit safe event allowlist', () => {
    const store = new DiagnosticStore({ storage: memoryStorage(), createEventId: () => eventId(1) })
    store.bindPrincipal('user-a')
    store.record(input())

    const copied = serializeDiagnosticsBundle(store.all(), 'DEVELOPMENT_HEALTH')
    expect(copied).toContain('SNAPSHOT_LIST')
    expect(copied).toContain('R1')
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
  })
})
