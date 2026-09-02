import { describe, expect, it, vi } from 'vitest'

import { ApiClient } from './client'
import {
  advanceBackupMaintenanceRun,
  configureBackupRetentionPolicy,
  createBackupPrunePlan,
  createBackupSet,
  createBackupMaintenanceRun,
  createBackupRestorePlan,
  createUuidV7,
  executeBackupRestorePlan,
  executeBackupPrunePlan,
  getBackupOperation,
  getBackupRestoreExecution,
  getBackupRestorePlan,
  getBackupPruneExecution,
  getBackupPrunePlan,
  listBackupSnapshotNodes,
  listBackupSnapshots,
} from './backups'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

function errorBody(code = 'backup_error') {
  return {
    error: {
      code,
      message: 'safe server message',
      request_id: 'backup-request',
      retryable: false,
    },
  }
}

describe('backup API helpers', () => {
  it('uses bounded cursor queries and the snapshot root contract', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockImplementation(async () =>
      jsonResponse({
        data: [],
        page: { has_more: false },
        meta: { request_id: 'snapshot-page' },
      }),
    )
    const client = new ApiClient({ fetchImpl })

    await listBackupSnapshots('set-1', { cursor: 'next page', limit: 50 }, client)
    await listBackupSnapshotNodes('snapshot-1', { limit: 50 }, client)

    expect(fetchImpl.mock.calls[0]?.[0]).toBe(
      '/api/v1/backups/sets/set-1/snapshots?cursor=next+page&limit=50',
    )
    expect(fetchImpl.mock.calls[1]?.[0]).toBe(
      '/api/v1/backups/snapshots/snapshot-1/nodes?limit=50',
    )
  })

  it('keeps the unified operation path kind-qualified', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockImplementation(async () =>
      jsonResponse({
        data: {
          operation_kind: 'RESTORE',
          operation_id: 'operation-1',
          backup_set_id: 'set-1',
          snapshot_id: 'snapshot-1',
          source_snapshot_id: 'snapshot-1',
          restore_plan_id: 'plan-1',
          target_library_id: 'library-1',
          target_parent_node_id: 'node-1',
          destination_name: 'Restored files',
          state: 'EXECUTED',
          phase: 'COMPLETED',
          progress: { completed_steps: 2, total_steps: 2 },
          terminal: true,
          next_action: 'NONE',
          planned_entry_count: '2',
          planned_directory_count: '1',
          planned_file_count: '1',
          created_at: '2026-08-31T00:00:00Z',
          last_transition_at: '2026-08-31T00:01:00Z',
        },
        meta: { request_id: 'operation-detail' },
      }),
    )
    const client = new ApiClient({ fetchImpl })

    await getBackupOperation('RESTORE', 'operation-1', undefined, client)

    expect(fetchImpl.mock.calls[0]?.[0]).toBe(
      '/api/v1/backups/operations/RESTORE/operation-1',
    )
  })

  it.each([
    [401, 'unauthorized'],
    [404, 'not_found'],
    [409, 'active_run'],
    [503, 'dependency_unavailable'],
  ])('preserves safe status handling for %s responses', async (status, code) => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(errorBody(code), status))
    const client = new ApiClient({ fetchImpl })

    await expect(listBackupSnapshots('set-1', {}, client)).rejects.toMatchObject({
      status,
      code,
    })
    expect(fetchImpl).toHaveBeenCalledOnce()
  })

  it('sends CSRF through the shared client and one caller-supplied idempotency key', async () => {
    document.cookie = 'synveil_csrf=csrf-proof; path=/'
    const fetchImpl = vi.fn<typeof fetch>().mockImplementation(async () =>
      jsonResponse({
        data: {
          maintenance_run_id: 'run-1',
          backup_set_id: 'set-1',
          state: 'CREATED',
          policy_revision_id: 'policy-1',
          policy_revision_number: '1',
          created_at: '2026-08-31T00:00:00Z',
        },
        meta: { request_id: 'mutation' },
      }, 201),
    )
    const client = new ApiClient({ fetchImpl })
    const key = createUuidV7(1_725_000_000_000)

    await createBackupMaintenanceRun('set-1', key, undefined, client)
    await advanceBackupMaintenanceRun('run-1', key, undefined, client)

    const firstHeaders = new Headers(fetchImpl.mock.calls[0]?.[1]?.headers)
    const secondHeaders = new Headers(fetchImpl.mock.calls[1]?.[1]?.headers)
    expect(firstHeaders.get('Idempotency-Key')).toBe(key)
    expect(secondHeaders.get('Idempotency-Key')).toBe(key)
    expect(firstHeaders.get('X-CSRF-Token')).toBe('csrf-proof')
    expect(secondHeaders.get('X-CSRF-Token')).toBe('csrf-proof')
  })

  it('uses the durable restore-plan and exact-plan execution contracts', async () => {
    document.cookie = 'synveil_csrf=csrf-proof; path=/'
    const fetchImpl = vi.fn<typeof fetch>().mockImplementation(async (input) => {
      const path = String(input)
      if (path.endsWith('/execute')) {
        return jsonResponse({
          data: {
            restore_execution_id: 'execution-1',
            restore_plan_id: 'plan-1',
            source_snapshot_id: 'snapshot-1',
            target_library_id: 'library-1',
            created_directory_count: '1',
            created_file_count: '2',
            journal_start_sequence: '10',
            journal_end_sequence: '12',
            executed_at: '2026-08-31T00:03:00Z',
          },
          meta: { request_id: 'execution' },
        })
      }
      if (path.includes('/restore-executions/')) {
        return jsonResponse({
          data: {
            restore_execution_id: 'execution-1',
            restore_plan_id: 'plan-1',
            source_snapshot_id: 'snapshot-1',
            target_library_id: 'library-1',
            created_directory_count: '1',
            created_file_count: '2',
            journal_start_sequence: '10',
            journal_end_sequence: '12',
            executed_at: '2026-08-31T00:03:00Z',
          },
          meta: { request_id: 'execution-read' },
        })
      }
      return jsonResponse({
        data: {
          restore_plan_id: 'plan-1',
          source_snapshot_id: 'snapshot-1',
          source_backup_set_id: 'set-1',
          target_library_id: 'library-1',
          target_parent_node_id: 'root-1',
          destination_name: 'Restored files',
          state: 'PLANNED',
          planned_entry_count: '3',
          planned_directory_count: '1',
          planned_file_count: '2',
          created_at: '2026-08-31T00:01:00Z',
        },
        meta: { request_id: 'plan' },
      }, 201)
    })
    const client = new ApiClient({ fetchImpl })
    const planKey = createUuidV7(1_725_000_000_000)
    const executionKey = createUuidV7(1_725_000_000_001)

    await createBackupRestorePlan(
      'snapshot-1',
      {
        target_library_id: 'library-1',
        target_parent_node_id: 'root-1',
        destination_name: 'Restored files',
      },
      planKey,
      undefined,
      client,
    )
    await getBackupRestorePlan('plan-1', undefined, client)
    await executeBackupRestorePlan('plan-1', executionKey, undefined, client)
    await getBackupRestoreExecution('execution-1', undefined, client)

    expect(fetchImpl.mock.calls[0]?.[0]).toBe(
      '/api/v1/backups/snapshots/snapshot-1/restore-plans',
    )
    expect(fetchImpl.mock.calls[1]?.[0]).toBe('/api/v1/backups/restore-plans/plan-1')
    expect(fetchImpl.mock.calls[2]?.[0]).toBe('/api/v1/backups/restore-plans/plan-1/execute')
    expect(fetchImpl.mock.calls[3]?.[0]).toBe('/api/v1/backups/restore-executions/execution-1')

    const planHeaders = new Headers(fetchImpl.mock.calls[0]?.[1]?.headers)
    const executeHeaders = new Headers(fetchImpl.mock.calls[2]?.[1]?.headers)
    expect(planHeaders.get('Idempotency-Key')).toBe(planKey)
    expect(executeHeaders.get('Idempotency-Key')).toBe(executionKey)
    expect(planHeaders.get('X-CSRF-Token')).toBe('csrf-proof')
    expect(executeHeaders.get('X-CSRF-Token')).toBe('csrf-proof')
    expect(JSON.parse(String(fetchImpl.mock.calls[0]?.[1]?.body))).toEqual({
      target_library_id: 'library-1',
      target_parent_node_id: 'root-1',
      destination_name: 'Restored files',
    })
    expect(fetchImpl.mock.calls[2]?.[1]?.body).toBeUndefined()
  })

  it('uses exact create-set and append-only retention contracts', async () => {
    document.cookie = 'synveil_csrf=csrf-proof; path=/'
    const fetchImpl = vi.fn<typeof fetch>().mockImplementation(async (input) => {
      if (String(input).endsWith('/retention-policy')) {
        return jsonResponse({
          data: {
            policy_revision_id: 'policy-2',
            backup_set_id: 'set-1',
            revision_number: '2',
            keep_latest_completed: '4',
            expire_after_seconds: '1209600',
            created_at: '2026-09-01T00:01:00Z',
          },
          meta: { request_id: 'policy' },
        }, 201)
      }
      return jsonResponse({
        data: {
          backup_set_id: 'set-1',
          name: 'Personal files',
          library_id: 'library-1',
          state: 'CREATED',
          created_at: '2026-09-01T00:00:00Z',
          updated_at: '2026-09-01T00:00:00Z',
        },
        meta: { request_id: 'set' },
      }, 201)
    })
    const client = new ApiClient({ fetchImpl })
    const createKey = createUuidV7(1_725_000_000_010)
    const policyKey = createUuidV7(1_725_000_000_011)

    await createBackupSet({ library_id: 'library-1', name: 'Personal files' }, createKey, undefined, client)
    await configureBackupRetentionPolicy(
      'set-1',
      { keep_latest_completed: 4, expire_after_seconds: 1_209_600 },
      policyKey,
      undefined,
      client,
    )

    expect(fetchImpl.mock.calls[0]?.[0]).toBe('/api/v1/backups/sets')
    expect(fetchImpl.mock.calls[1]?.[0]).toBe('/api/v1/backups/sets/set-1/retention-policy')
    expect(JSON.parse(String(fetchImpl.mock.calls[0]?.[1]?.body))).toEqual({
      library_id: 'library-1',
      name: 'Personal files',
    })
    expect(JSON.parse(String(fetchImpl.mock.calls[1]?.[1]?.body))).toEqual({
      keep_latest_completed: 4,
      expire_after_seconds: 1_209_600,
    })
    for (const [index, key] of [[0, createKey], [1, policyKey]] as const) {
      const headers = new Headers(fetchImpl.mock.calls[index]?.[1]?.headers)
      expect(headers.get('Idempotency-Key')).toBe(key)
      expect(headers.get('X-CSRF-Token')).toBe('csrf-proof')
    }
  })

  it('keeps prune planning read-only and binds execution to the caller-supplied plan snapshot', async () => {
    document.cookie = 'synveil_csrf=csrf-proof; path=/'
    const plan = {
      prune_plan_id: 'prune-plan-1',
      snapshot_id: 'snapshot-bound-to-plan',
      backup_set_id: 'set-1',
      state: 'PLANNED',
      entry_count: '4',
      distinct_content_count: '3',
      retained_by_other_reference_count: '2',
      would_become_unreferenced_count: '1',
      impact_summary: [
        { impact: 'RETAINED_BY_OTHER_REFERENCE', content_count: '2' },
        { impact: 'WOULD_BECOME_UNREFERENCED', content_count: '1' },
      ],
      created_at: '2026-09-01T00:00:00Z',
    }
    const receipt = {
      prune_execution_id: 'prune-execution-1',
      prune_plan_id: 'prune-plan-1',
      snapshot_id: 'snapshot-bound-to-plan',
      backup_set_id: 'set-1',
      released_content_reference_count: '4',
      distinct_content_count: '3',
      retained_elsewhere_count: '2',
      gc_handoff_count: '1',
      executed_at: '2026-09-01T00:02:00Z',
    }
    const fetchImpl = vi.fn<typeof fetch>().mockImplementation(async (input) => {
      const path = String(input)
      if (path.endsWith('/execute') || path.includes('/prune-executions/')) {
        return jsonResponse({ data: receipt, meta: { request_id: 'receipt' } })
      }
      return jsonResponse({ data: plan, meta: { request_id: 'plan' } }, path.endsWith('/prune-plans') ? 201 : 200)
    })
    const client = new ApiClient({ fetchImpl })
    const planKey = createUuidV7(1_725_000_000_020)
    const executeKey = createUuidV7(1_725_000_000_021)

    await createBackupPrunePlan('snapshot-bound-to-plan', planKey, undefined, client)
    await getBackupPrunePlan('prune-plan-1', undefined, client)
    await executeBackupPrunePlan(
      'prune-plan-1',
      'snapshot-bound-to-plan',
      executeKey,
      undefined,
      client,
    )
    await getBackupPruneExecution('prune-execution-1', undefined, client)

    expect(fetchImpl.mock.calls.map((call) => call[0])).toEqual([
      '/api/v1/backups/snapshots/snapshot-bound-to-plan/prune-plans',
      '/api/v1/backups/prune-plans/prune-plan-1',
      '/api/v1/backups/prune-plans/prune-plan-1/execute',
      '/api/v1/backups/prune-executions/prune-execution-1',
    ])
    expect(fetchImpl.mock.calls[0]?.[1]?.body).toBeUndefined()
    expect(JSON.parse(String(fetchImpl.mock.calls[2]?.[1]?.body))).toEqual({
      confirm_snapshot_id: 'snapshot-bound-to-plan',
    })
    expect(new Headers(fetchImpl.mock.calls[0]?.[1]?.headers).get('Idempotency-Key')).toBe(planKey)
    expect(new Headers(fetchImpl.mock.calls[2]?.[1]?.headers).get('Idempotency-Key')).toBe(executeKey)
    expect(new Headers(fetchImpl.mock.calls[0]?.[1]?.headers).get('X-CSRF-Token')).toBe('csrf-proof')
    expect(new Headers(fetchImpl.mock.calls[2]?.[1]?.headers).get('X-CSRF-Token')).toBe('csrf-proof')
  })

  it('creates lowercase UUIDv7 values with the RFC version and variant bits', () => {
    const key = createUuidV7(1_725_000_000_000)

    expect(key).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
  })
})
