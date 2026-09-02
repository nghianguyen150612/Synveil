import { describe, expect, it, beforeEach, afterEach } from 'vitest'
import {
  bindMutationRecoveryPrincipal,
  LEGACY_PENDING_MUTATIONS_KEY,
  mutationRecoveryStore,
  makePendingCreateRecord,
  makePendingExecuteRecord,
  PENDING_MUTATIONS_KEY,
  PENDING_SCHEMA_VERSION,
} from './mutationRecovery'

const OWNER_A = '00000000-0000-7000-8000-000000000001'
const OWNER_B = '00000000-0000-7000-8000-000000000002'

describe('mutationRecoveryStore', () => {
  beforeEach(() => {
    mutationRecoveryStore.clear()
    bindMutationRecoveryPrincipal(OWNER_A)
  })

  afterEach(() => {
    mutationRecoveryStore.clear()
    bindMutationRecoveryPrincipal(null)
  })

  it('is available in the jsdom environment', () => {
    expect(mutationRecoveryStore.isAvailable).toBe(true)
  })

  it('adds a record and reads it back', () => {
    const record = mutationRecoveryStore.add({
      action_kind: 'create_backup_set',
      idempotency_key: '01925000-0000-7000-8000-000000000001',
      request_scope: {},
      request: { library_id: 'lib-1', name: 'Test' },
    })

    expect(record.schema_version).toBe(PENDING_SCHEMA_VERSION)
    expect(record.status).toBe('uncertain')
    expect(record.action_kind).toBe('create_backup_set')
    expect(record.owner_id).toBe(OWNER_A)
    expect(record.idempotency_key).toBe('01925000-0000-7000-8000-000000000001')
    expect(record.request).toEqual({ library_id: 'lib-1', name: 'Test' })

    const all = mutationRecoveryStore.all()
    expect(all).toHaveLength(1)
    expect(all[0]?.action_id).toBe(record.action_id)
  })

  it('removes a record by action_id', () => {
    const record = mutationRecoveryStore.add({
      action_kind: 'create_maintenance_run',
      idempotency_key: '01925000-0000-7000-8000-000000000002',
      request_scope: { backupSetId: 'set-1' },
      request: {},
    })

    expect(mutationRecoveryStore.all()).toHaveLength(1)
    mutationRecoveryStore.remove(record.action_id)
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('clears all records', () => {
    mutationRecoveryStore.add({
      action_kind: 'create_backup_set',
      idempotency_key: '01925000-0000-7000-8000-000000000003',
      request_scope: {},
      request: {},
    })
    mutationRecoveryStore.add({
      action_kind: 'configure_retention_policy',
      idempotency_key: '01925000-0000-7000-8000-000000000004',
      request_scope: { backupSetId: 'set-1' },
      request: {},
    })

    expect(mutationRecoveryStore.all()).toHaveLength(2)
    mutationRecoveryStore.clear()
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('supports multiple pending mutations', () => {
    const record1 = mutationRecoveryStore.add({
      action_kind: 'configure_retention_policy',
      idempotency_key: '01925000-0000-7000-8000-000000000005',
      request_scope: { backupSetId: 'set-1' },
      request: { keep_latest_completed: 3, expire_after_seconds: 86400 },
    })
    const record2 = mutationRecoveryStore.add({
      action_kind: 'create_maintenance_run',
      idempotency_key: '01925000-0000-7000-8000-000000000006',
      request_scope: { backupSetId: 'set-2' },
      request: {},
    })

    const all = mutationRecoveryStore.all()
    expect(all).toHaveLength(2)
    expect(all.some((r) => r.action_id === record1.action_id)).toBe(true)
    expect(all.some((r) => r.action_id === record2.action_id)).toBe(true)
  })

  it('reuses one persisted record for the same action kind and idempotency key', () => {
    const first = mutationRecoveryStore.add({
      action_kind: 'create_restore_plan',
      idempotency_key: '01925000-0000-7000-8000-00000000000e',
      request_scope: { snapshotId: 'snapshot-1' },
      request: { destination_name: 'Recovered' },
    })
    const retry = mutationRecoveryStore.add({
      action_kind: 'create_restore_plan',
      idempotency_key: '01925000-0000-7000-8000-00000000000e',
      request_scope: { snapshotId: 'snapshot-1' },
      request: { destination_name: 'Recovered' },
    })

    expect(retry.action_id).toBe(first.action_id)
    expect(mutationRecoveryStore.all()).toHaveLength(1)
  })

  it('ignores invalid/malformed storage contents', () => {
    sessionStorage.setItem(PENDING_MUTATIONS_KEY, 'not-valid-json{{{')
    const all = mutationRecoveryStore.all()
    expect(all).toEqual([])
  })

  it('ignores records with unknown schema version', () => {
    sessionStorage.setItem(PENDING_MUTATIONS_KEY, JSON.stringify({
      schema_version: 999,
      records: [],
    }))
    const all = mutationRecoveryStore.all()
    expect(all).toEqual([])
  })

  it('ignores records with invalid idempotency key format', () => {
    sessionStorage.setItem(PENDING_MUTATIONS_KEY, JSON.stringify({
      schema_version: PENDING_SCHEMA_VERSION,
      records: [{
        schema_version: PENDING_SCHEMA_VERSION,
        action_id: 'valid-uuidv7',
        action_kind: 'create_backup_set',
        idempotency_key: 'not-a-real-uuidv7',
        method: 'POST',
        request_scope: {},
        request: {},
        created_at: '2026-01-01T00:00:00Z',
        status: 'uncertain',
      }],
    }))
    const all = mutationRecoveryStore.all()
    expect(all).toEqual([])
  })

  it('ignores records with unknown action_kind', () => {
    sessionStorage.setItem(PENDING_MUTATIONS_KEY, JSON.stringify({
      schema_version: PENDING_SCHEMA_VERSION,
      records: [{
        schema_version: PENDING_SCHEMA_VERSION,
        action_id: 'valid-id',
        action_kind: 'unknown_action_kind',
        idempotency_key: '01925000-0000-7000-8000-000000000007',
        method: 'POST',
        request_scope: {},
        request: {},
        created_at: '2026-01-01T00:00:00Z',
        status: 'uncertain',
      }],
    }))
    const all = mutationRecoveryStore.all()
    expect(all).toEqual([])
  })

  it('clears the storage key when the last record is removed', () => {
    const record = mutationRecoveryStore.add({
      action_kind: 'create_backup_set',
      idempotency_key: '01925000-0000-7000-8000-000000000008',
      request_scope: {},
      request: {},
    })

    expect(sessionStorage.getItem(PENDING_MUTATIONS_KEY)).not.toBeNull()
    mutationRecoveryStore.remove(record.action_id)
    expect(sessionStorage.getItem(PENDING_MUTATIONS_KEY)).toBeNull()
  })

  it('persists before send: the record exists in storage even if no POST was made', () => {
    const record = mutationRecoveryStore.add({
      action_kind: 'execute_restore_plan',
      idempotency_key: '01925000-0000-7000-8000-000000000009',
      request_scope: { restorePlanId: 'plan-1' },
      request: {},
    })

    const stored = JSON.parse(sessionStorage.getItem(PENDING_MUTATIONS_KEY) ?? '{}')
    expect(stored.schema_version).toBe(PENDING_SCHEMA_VERSION)
    expect(stored.records).toHaveLength(1)
    expect(stored.records[0].action_id).toBe(record.action_id)
    expect(stored.records[0].idempotency_key).toBe('01925000-0000-7000-8000-000000000009')
  })

  it('filters retry discovery by the active principal and exposes only a generic foreign set', () => {
    const record = mutationRecoveryStore.add({
      action_kind: 'execute_restore_plan',
      idempotency_key: '01925000-0000-7000-8000-000000000010',
      request_scope: { restorePlanId: 'owner-a-plan' },
      request: {},
    })

    bindMutationRecoveryPrincipal(OWNER_B)
    expect(mutationRecoveryStore.forCurrentPrincipal()).toEqual([])
    expect(mutationRecoveryStore.foreignForCurrentPrincipal().map((item) => item.action_id))
      .toEqual([record.action_id])
  })

  it('fails closed for ambiguous v1 records until they are explicitly discarded', () => {
    sessionStorage.setItem(LEGACY_PENDING_MUTATIONS_KEY, JSON.stringify({
      schema_version: 1,
      records: [{ action_kind: 'execute_prune_plan' }],
    }))

    expect(mutationRecoveryStore.hasAmbiguousLegacyRecords).toBe(true)
    expect(mutationRecoveryStore.forCurrentPrincipal()).toEqual([])
    mutationRecoveryStore.discardAmbiguousLegacyRecords()
    expect(mutationRecoveryStore.hasAmbiguousLegacyRecords).toBe(false)
  })
})

describe('makePendingCreateRecord', () => {
  beforeEach(() => { mutationRecoveryStore.clear(); bindMutationRecoveryPrincipal(OWNER_A) })
  afterEach(() => { mutationRecoveryStore.clear(); bindMutationRecoveryPrincipal(null) })

  it.each([
    'create_backup_set',
    'create_maintenance_run',
    'create_restore_plan',
    'create_prune_plan',
  ] as const)('creates a %s record', (kind) => {
    const record = makePendingCreateRecord(kind, '01925000-0000-7000-8000-00000000000a', {}, {})
    expect(record.action_kind).toBe(kind)
    expect(mutationRecoveryStore.all()).toHaveLength(1)
  })

  it('preserves the idempotency key exactly for retry', () => {
    const key = '01925000-0000-7000-8000-00000000000b'
    const record = makePendingCreateRecord('create_backup_set', key, {}, { name: 'test' })
    expect(record.idempotency_key).toBe(key)
    expect(record.request).toEqual({ name: 'test' })
  })
})

describe('makePendingExecuteRecord', () => {
  beforeEach(() => { mutationRecoveryStore.clear(); bindMutationRecoveryPrincipal(OWNER_A) })
  afterEach(() => { mutationRecoveryStore.clear(); bindMutationRecoveryPrincipal(null) })

  it.each([
    'execute_restore_plan',
    'execute_prune_plan',
  ] as const)('creates a %s record', (kind) => {
    const record = makePendingExecuteRecord(kind, '01925000-0000-7000-8000-00000000000c', {}, {})
    expect(record.action_kind).toBe(kind)
    expect(mutationRecoveryStore.all()).toHaveLength(1)
  })

  it('preserves confirm_snapshot_id in the request for prune execution retry', () => {
    const key = '01925000-0000-7000-8000-00000000000d'
    const snapshotId = 'snapshot-bound-to-plan'
    const record = makePendingExecuteRecord('execute_prune_plan', key, { prunePlanId: 'plan-1' }, {
      confirm_snapshot_id: snapshotId,
    })

    expect(record.idempotency_key).toBe(key)
    expect(record.request.confirm_snapshot_id).toBe(snapshotId)
  })
})
