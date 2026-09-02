import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AuthApi, AuthSessionResponse } from '../../api/auth'
import type {
  BackupApi,
  BackupOperationDetail,
  BackupOperationSummary,
  BackupRestoreExecutionResponse,
  BackupRestorePlanResponse,
} from '../../api/backups'
import { ApiRequestError, type ApiErrorPayload } from '../../api/errors'
import { mutationRecoveryStore } from '../../api/mutationRecovery'
import type { BootstrapApi, BootstrapStatusResponse } from '../../api/bootstrap'
import { App } from '../../app/App'

const setOne = {
  backup_set_id: 'set-1',
  name: 'Personal files',
  library_id: 'library-1',
  state: 'ACTIVE' as const,
  created_at: '2026-08-01T09:00:00Z',
  updated_at: '2026-08-31T09:00:00Z',
}

const setTwo = {
  ...setOne,
  backup_set_id: 'set-2',
  name: 'Work files',
  library_id: 'library-2',
}

const completedSnapshot = {
  snapshot_id: 'snapshot-completed',
  backup_set_id: 'set-1',
  library_id: 'library-1',
  state: 'COMPLETED' as const,
  created_at: '2026-08-30T09:00:00Z',
  committed_at: '2026-08-30T09:01:00Z',
  logical_node_count: '2',
  content_reference_count: '1',
}

const expiredSnapshot = {
  snapshot_id: 'snapshot-expired',
  backup_set_id: 'set-1',
  library_id: 'library-1',
  state: 'EXPIRED' as const,
  created_at: '2026-07-01T09:00:00Z',
  committed_at: '2026-07-01T09:01:00Z',
  expired_at: '2026-08-15T09:01:00Z',
  logical_node_count: '1',
  content_reference_count: '1',
}

const rootDirectory = {
  snapshot_node_id: 'dir-1',
  name: 'Documents',
  kind: 'DIRECTORY' as const,
  state: 'ACTIVE' as const,
  revision: '1',
  created_at: '2026-08-30T09:00:00Z',
  updated_at: '2026-08-30T09:00:00Z',
}

const rootFile = {
  snapshot_node_id: 'file-1',
  name: 'readme.txt',
  kind: 'FILE' as const,
  state: 'ACTIVE' as const,
  revision: '1',
  file_version_id: 'version-1',
  byte_length: '2048',
  sha256: 'a'.repeat(64),
  created_at: '2026-08-30T09:00:00Z',
  updated_at: '2026-08-30T09:00:00Z',
}

const nestedFile = {
  ...rootFile,
  snapshot_node_id: 'file-2',
  name: 'notes.txt',
}

const maintenanceSummary: BackupOperationSummary = {
  operation_kind: 'MAINTENANCE',
  operation_id: 'run-1',
  backup_set_id: 'set-1',
  state: 'CREATED',
  phase: 'AWAITING_ADVANCE',
  progress: { completed_steps: 0, total_steps: 3 },
  terminal: false,
  next_action: 'ADVANCE',
  created_at: '2026-08-31T09:00:00Z',
  last_transition_at: '2026-08-31T09:00:00Z',
}

const maintenanceDetail: BackupOperationDetail = {
  ...maintenanceSummary,
  operation_kind: 'MAINTENANCE',
  maintenance_run_id: 'run-1',
  policy_revision_id: 'policy-1',
  policy_revision_number: '1',
}

const restoreSummary: BackupOperationSummary = {
  operation_kind: 'RESTORE',
  operation_id: 'restore-plan-1',
  backup_set_id: 'set-1',
  snapshot_id: 'snapshot-completed',
  state: 'EXECUTED',
  phase: 'COMPLETED',
  progress: { completed_steps: 2, total_steps: 2 },
  terminal: true,
  next_action: 'NONE',
  created_at: '2026-08-29T09:00:00Z',
  last_transition_at: '2026-08-29T09:03:00Z',
  completed_at: '2026-08-29T09:03:00Z',
}

const restoreDetail: BackupOperationDetail = {
  ...restoreSummary,
  operation_kind: 'RESTORE',
  snapshot_id: 'snapshot-completed',
  restore_plan_id: 'restore-plan-1',
  source_snapshot_id: 'snapshot-completed',
  target_library_id: 'library-1',
  target_parent_node_id: 'node-1',
  destination_name: 'Restored files',
  planned_entry_count: '2',
  planned_directory_count: '1',
  planned_file_count: '1',
}

const restorePlanResponse: BackupRestorePlanResponse = {
  data: {
    restore_plan_id: 'restore-plan-1',
    source_snapshot_id: 'snapshot-completed',
    source_backup_set_id: 'set-1',
    target_library_id: 'library-1',
    target_parent_node_id: 'node-1',
    destination_name: 'Restored files',
    state: 'EXECUTED',
    planned_entry_count: '2',
    planned_directory_count: '1',
    planned_file_count: '1',
    created_at: '2026-08-29T09:00:00Z',
  },
  meta: { request_id: 'restore-plan' },
}

const restoreExecutionResponse: BackupRestoreExecutionResponse = {
  data: {
    restore_execution_id: 'restore-execution-1',
    restore_plan_id: 'restore-plan-1',
    source_snapshot_id: 'snapshot-completed',
    target_library_id: 'library-1',
    created_directory_count: '1',
    created_file_count: '1',
    journal_start_sequence: '10',
    journal_end_sequence: '11',
    executed_at: '2026-08-29T09:03:00Z',
  },
  meta: { request_id: 'restore-execution' },
}

const pruneSummary: BackupOperationSummary = {
  operation_kind: 'PRUNE',
  operation_id: 'prune-plan-1',
  backup_set_id: 'set-1',
  snapshot_id: 'snapshot-expired',
  state: 'EXECUTED',
  phase: 'COMPLETED',
  progress: { completed_steps: 2, total_steps: 2 },
  terminal: true,
  next_action: 'NONE',
  created_at: '2026-08-28T09:00:00Z',
  last_transition_at: '2026-08-28T09:02:00Z',
  completed_at: '2026-08-28T09:02:00Z',
}

const pruneDetail: BackupOperationDetail = {
  ...pruneSummary,
  operation_kind: 'PRUNE',
  snapshot_id: 'snapshot-expired',
  prune_plan_id: 'prune-plan-1',
  planned_pin_release_count: '1',
  distinct_content_count: '1',
  retained_by_other_reference_count: '0',
  would_become_unreferenced_count: '1',
}

const policy = {
  data: {
    policy_revision_id: 'policy-1',
    backup_set_id: 'set-1',
    revision_number: '1',
    keep_latest_completed: '3',
    expire_after_seconds: String(30 * 24 * 60 * 60),
    created_at: '2026-08-01T09:00:00Z',
  },
  meta: { request_id: 'policy-1' },
}

const maintenanceRunResponse = {
  data: {
    maintenance_run_id: 'run-1',
    backup_set_id: 'set-1',
    state: 'CREATED' as const,
    policy_revision_id: 'policy-1',
    policy_revision_number: '1',
    created_at: '2026-08-31T09:00:00Z',
  },
  meta: { request_id: 'maintenance-run' },
}

const session: AuthSessionResponse = {
  data: {
    authenticated: true,
    user_id: '00000000-0000-7000-8000-000000000001',
    is_instance_admin: true,
    session_id: '00000000-0000-7000-8000-000000000002',
  },
  meta: { request_id: 'session' },
}

const bootstrapStatus: BootstrapStatusResponse = {
  data: { setup_required: false },
  meta: { request_id: 'bootstrap' },
}

function authenticatedApis(): { authApi: AuthApi; bootstrapApi: BootstrapApi } {
  return {
    authApi: {
      login: vi.fn().mockResolvedValue(session),
      getCurrentSession: vi.fn().mockResolvedValue(session),
      getCsrfToken: vi.fn().mockResolvedValue({
        data: { csrf_token: '0'.repeat(128) },
        meta: { request_id: 'csrf' },
      }),
      logout: vi.fn().mockResolvedValue(undefined),
    },
    bootstrapApi: {
      getStatus: vi.fn().mockResolvedValue(bootstrapStatus),
      createFirstAdmin: vi.fn().mockResolvedValue(bootstrapStatus),
    },
  }
}

function page<T>(data: readonly T[], hasMore = false, nextCursor?: string) {
  return {
    data,
    page: { has_more: hasMore, ...(nextCursor ? { next_cursor: nextCursor } : {}) },
    meta: { request_id: 'page' },
  }
}

function makeBackupApi(overrides: Partial<BackupApi> = {}): BackupApi {
  const operations = [maintenanceSummary, restoreSummary, pruneSummary]
  return {
    listBackupSets: vi.fn().mockResolvedValue(page([setOne, setTwo])),
    getBackupSet: vi.fn().mockImplementation(async (backupSetId: string) => ({
      data: backupSetId === setTwo.backup_set_id ? setTwo : setOne,
      meta: { request_id: 'set' },
    })),
    createBackupSet: vi.fn().mockResolvedValue({ data: setOne, meta: { request_id: 'set-create' } }),
    listBackupSnapshots: vi.fn().mockImplementation(async (backupSetId: string) =>
      page(backupSetId === setTwo.backup_set_id ? [] : [completedSnapshot, expiredSnapshot]),
    ),
    getBackupSnapshot: vi.fn().mockResolvedValue({
      data: expiredSnapshot,
      meta: { request_id: 'snapshot' },
    }),
    listBackupSnapshotNodes: vi.fn().mockImplementation(async (_snapshotId: string, options?: { parentId?: string }) =>
      page(options?.parentId === 'dir-1' ? [nestedFile] : [rootDirectory, rootFile]),
    ),
    getBackupRetentionPolicy: vi.fn().mockResolvedValue(policy),
    configureBackupRetentionPolicy: vi.fn().mockResolvedValue(policy),
    listBackupOperations: vi.fn().mockResolvedValue(page(operations)),
    getBackupOperation: vi.fn().mockImplementation(async (kind: string) => ({
      data: kind === 'RESTORE' ? restoreDetail : kind === 'PRUNE' ? pruneDetail : maintenanceDetail,
      meta: { request_id: 'operation' },
    })),
    listBackupMaintenanceRuns: vi.fn().mockResolvedValue(page([])),
    getBackupMaintenanceRun: vi.fn().mockResolvedValue(maintenanceRunResponse),
    createBackupMaintenanceRun: vi.fn().mockResolvedValue(maintenanceRunResponse),
    advanceBackupMaintenanceRun: vi.fn().mockResolvedValue(maintenanceRunResponse),
    createBackupRestorePlan: vi.fn().mockResolvedValue(restorePlanResponse),
    getBackupRestorePlan: vi.fn().mockResolvedValue(restorePlanResponse),
    executeBackupRestorePlan: vi.fn().mockResolvedValue(restoreExecutionResponse),
    getBackupRestoreExecution: vi.fn().mockResolvedValue(restoreExecutionResponse),
    createBackupPrunePlan: vi.fn().mockRejectedValue(new Error('unused')),
    getBackupPrunePlan: vi.fn().mockRejectedValue(new Error('unused')),
    executeBackupPrunePlan: vi.fn().mockRejectedValue(new Error('unused')),
    getBackupPruneExecution: vi.fn().mockRejectedValue(new Error('unused')),
    ...overrides,
  }
}

function renderBackups(
  backupApi: BackupApi,
  path = '/backups/set-1',
  apis = authenticatedApis(),
) {
  return render(
    <App
      router="memory"
      initialEntries={[path]}
      authApi={apis.authApi}
      bootstrapApi={apis.bootstrapApi}
      backupApi={backupApi}
    />,
  )
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve
  })
  return { promise, resolve }
}

describe('backup foundation page', () => {
  beforeEach(() => mutationRecoveryStore.clear())

  it('renders overview, snapshots, tree browsing, activity, and truthful step progress', async () => {
    renderBackups(makeBackupApi())

    expect(await screen.findByRole('heading', { name: 'Backup set details', level: 1 })).toBeInTheDocument()
    expect(await screen.findByLabelText('Backup set')).toHaveValue('set-1')
    expect(await screen.findByText(/Keep at least/)).toBeInTheDocument()
    expect(screen.getByText('3')).toBeInTheDocument()
    const runButton = screen.getByRole('button', { name: 'Run backup now' })
    await waitFor(() => expect(runButton).toBeDisabled())
    expect(screen.getByText('Available for restore')).toBeInTheDocument()
    expect(screen.getByText(/Not available for restore/)).toBeInTheDocument()
    expect(screen.queryByText('1 of 3 steps completed')).not.toBeInTheDocument()
    expect(screen.getByText('0 of 3 steps completed')).toBeInTheDocument()
    expect(screen.queryByText('33%')).not.toBeInTheDocument()
    expect(screen.queryByText(/\bETA\b|remaining/i)).not.toBeInTheDocument()
    expect(screen.getAllByText('Restore').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Prune').length).toBeGreaterThan(0)

    fireEvent.click(screen.getByRole('button', { name: /Expired.*logical items/i }))
    expect(await screen.findByRole('heading', { name: 'Snapshot details' })).toBeInTheDocument()
    expect(await screen.findByRole('heading', { name: 'Logical tree' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Documents.*Open folder/i })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /Documents.*Open folder/i }))
    expect(await screen.findByText('notes.txt')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Snapshot root' })).toBeInTheDocument()
  })

  it('offers the restore entry point only for a completed snapshot', async () => {
    const getSnapshot = vi.fn().mockImplementation(async (snapshotId: string) => ({
      data: snapshotId === completedSnapshot.snapshot_id ? completedSnapshot : expiredSnapshot,
      meta: { request_id: 'snapshot' },
    }))
    const api = makeBackupApi({ getBackupSnapshot: getSnapshot })
    renderBackups(api)

    fireEvent.click(await screen.findByText('Available for restore'))
    expect(await screen.findByRole('button', { name: /^Restore$/i })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Close details' }))
    fireEvent.click(screen.getByRole('button', { name: /Expired.*logical items/i }))
    await screen.findByRole('heading', { name: 'Snapshot details' })
    expect(screen.queryByRole('button', { name: /^Restore$/i })).not.toBeInTheDocument()
  })

  it('provides a compact multiple-set selector and clears old set data on switch', async () => {
    const api = makeBackupApi()
    renderBackups(api)
    await screen.findByRole('button', { name: 'Run backup now' })

    fireEvent.change(screen.getByLabelText('Backup set'), { target: { value: 'set-2' } })

    expect(await screen.findByRole('heading', { name: 'Work files' })).toBeInTheDocument()
    expect(await screen.findByText('No snapshots are available for this backup set yet.')).toBeInTheDocument()
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'Personal files' })).not.toBeInTheDocument()
    })
  })

  it('uses bounded snapshot pagination and resets the activity cursor when filtering', async () => {
    const listSnapshots = vi.fn().mockImplementation(
      async (_backupSetId: string, options?: { readonly cursor?: string }) =>
        options?.cursor ? page([expiredSnapshot]) : page([completedSnapshot], true, 'snapshot-next'),
    )
    const listOperations = vi.fn().mockImplementation(
      async (_backupSetId: string, options?: { readonly kind?: string; readonly cursor?: string }) =>
        options?.kind === 'RESTORE' ? page([restoreSummary]) : page([restoreSummary, pruneSummary]),
    )
    const api = makeBackupApi({ listBackupSnapshots: listSnapshots, listBackupOperations: listOperations })
    renderBackups(api)

    fireEvent.click(await screen.findByRole('button', { name: 'Load more snapshots' }))
    expect(await screen.findByText(/Not available for restore/)).toBeInTheDocument()
    expect(listSnapshots.mock.calls[1]?.[1]).toMatchObject({ cursor: 'snapshot-next', limit: 50 })

    fireEvent.change(screen.getByLabelText('Show'), { target: { value: 'RESTORE' } })
    expect(await screen.findByRole('button', { name: /Restore.*Completed/i })).toBeInTheDocument()
    const lastOperationCall = listOperations.mock.calls.at(-1)
    expect(lastOperationCall?.[1]).toMatchObject({ kind: 'RESTORE', limit: 50 })
    expect(lastOperationCall?.[1]).not.toHaveProperty('cursor')
  })

  it('appends one cursor page without duplicating operation identity', async () => {
    const listOperations = vi.fn().mockImplementation(
      async (_backupSetId: string, options?: { readonly cursor?: string }) =>
        options?.cursor
          ? page([restoreSummary, pruneSummary])
          : page([restoreSummary], true, 'operation-next'),
    )
    const api = makeBackupApi({ listBackupOperations: listOperations })
    renderBackups(api)

    const loadMore = await screen.findByRole('button', { name: 'Load more activity' })
    fireEvent.click(loadMore)
    fireEvent.click(loadMore)

    await waitFor(() => expect(listOperations).toHaveBeenCalledTimes(2))
    expect(listOperations.mock.calls[1]?.[1]).toMatchObject({ cursor: 'operation-next', limit: 50 })
    expect(screen.getAllByRole('button', { name: /Restore.*Completed/i })).toHaveLength(1)
    expect(screen.getAllByRole('button', { name: /Prune.*Completed/i })).toHaveLength(1)
  })

  it('does not let a late first-set response overwrite the selected second set', async () => {
    const firstSnapshotPage = deferred<Awaited<ReturnType<BackupApi['listBackupSnapshots']>>>()
    const listSnapshots = vi.fn().mockImplementation(async (backupSetId: string) => {
      if (backupSetId === setOne.backup_set_id) {
        return firstSnapshotPage.promise
      }
      return page([])
    })
    const api = makeBackupApi({ listBackupSnapshots: listSnapshots })
    renderBackups(api)
    await screen.findByLabelText('Backup set')

    fireEvent.change(screen.getByLabelText('Backup set'), { target: { value: 'set-2' } })
    expect(await screen.findByRole('heading', { name: 'Work files' })).toBeInTheDocument()
    expect(await screen.findByText('No snapshots are available for this backup set yet.')).toBeInTheDocument()

    firstSnapshotPage.resolve(page([completedSnapshot]))
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Work files' })).toBeInTheDocument())
    expect(screen.queryByText('Available for restore')).not.toBeInTheDocument()
  })

  it('renders the friendly no-backup-set state with the supported setup action', async () => {
    const api = makeBackupApi({ listBackupSets: vi.fn().mockResolvedValue(page([])) })
    renderBackups(api, '/backups')

    expect(await screen.findByRole('heading', { name: 'No backups yet.' })).toBeInTheDocument()
    expect(screen.getByText(/Create a backup set to start protecting your files/i)).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Create a backup set' })).toBeInTheDocument()
    expect(api.getBackupSet).not.toHaveBeenCalled()
  })

  it('conceals missing and foreign backup sets behind one generic not-found view', async () => {
    const payload: ApiErrorPayload = {
      code: 'not_found',
      message: 'raw ownership diagnostic',
      request_id: 'missing-set',
      retryable: false,
    }
    const api = makeBackupApi({
      getBackupSet: vi.fn().mockRejectedValue(new ApiRequestError(404, payload)),
    })
    const apis = authenticatedApis()
    const getCurrentSession = vi.fn().mockResolvedValue(session)
    renderBackups(
      api,
      '/backups/missing-or-foreign',
      { ...apis, authApi: { ...apis.authApi, getCurrentSession } },
    )

    expect(await screen.findByRole('heading', { name: 'Backup set not available' })).toHaveFocus()
    expect(screen.getByText('The requested backup set could not be found.')).toBeInTheDocument()
    expect(screen.queryByText(/owner|foreign|raw ownership/i)).not.toBeInTheDocument()
    expect(getCurrentSession).toHaveBeenCalledOnce()
    expect(screen.getByRole('button', { name: 'Logout' })).toBeInTheDocument()
  })

  it('[auth] reconstructs an operation-detail deep link through the centralized protected route', async () => {
    const getOperation = vi.fn<BackupApi['getBackupOperation']>().mockResolvedValue({
      data: maintenanceDetail,
      meta: { request_id: 'operation-deep-link' },
    })
    const api = makeBackupApi({ getBackupOperation: getOperation })
    renderBackups(api, '/backups/set-1/operations/MAINTENANCE/run-1')

    expect(await screen.findByRole('heading', { name: 'Operation details' })).toHaveFocus()
    expect(getOperation).toHaveBeenCalledWith(
      'MAINTENANCE',
      'run-1',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    )
  })

  it('shows no-policy behavior and blocks manual maintenance safely', async () => {
    const notFoundPayload: ApiErrorPayload = {
      code: 'not_found',
      message: 'safe server message',
      request_id: 'policy-missing',
      retryable: false,
    }
    const api = makeBackupApi({
      getBackupRetentionPolicy: vi.fn().mockRejectedValue(new ApiRequestError(404, notFoundPayload)),
      listBackupOperations: vi.fn().mockResolvedValue(page([])),
    })
    renderBackups(api)

    expect(await screen.findByText('No retention settings configured.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Run backup now' })).toBeDisabled()
    expect(screen.getByText(/retention policy is required/i)).toBeInTheDocument()
  })

  it('validates and appends a new retention revision with a stable retry key', async () => {
    const updatedPolicy = {
      data: {
        ...policy.data,
        policy_revision_id: 'policy-2',
        revision_number: '2',
        keep_latest_completed: '5',
        expire_after_seconds: String(2 * 7 * 24 * 60 * 60),
      },
      meta: { request_id: 'policy-2' },
    }
    const configure = vi
      .fn<BackupApi['configureBackupRetentionPolicy']>()
      .mockRejectedValueOnce(new Error('response lost'))
      .mockResolvedValueOnce(updatedPolicy)
    const api = makeBackupApi({ configureBackupRetentionPolicy: configure })
    renderBackups(api)
    fireEvent.click(await screen.findByRole('button', { name: 'Change retention settings' }))

    fireEvent.change(screen.getByLabelText('Completed backups to always keep'), { target: { value: '0' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save retention settings' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(/greater than zero/i)
    expect(configure).not.toHaveBeenCalled()

    fireEvent.change(screen.getByLabelText('Completed backups to always keep'), { target: { value: '5' } })
    fireEvent.change(screen.getByLabelText('Expire backups older than'), { target: { value: '2' } })
    fireEvent.change(screen.getByLabelText('Time unit'), { target: { value: 'weeks' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save retention settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Try the same request again' }))

    await waitFor(() => expect(configure).toHaveBeenCalledTimes(2))
    expect(configure.mock.calls[0]?.[0]).toBe(setOne.backup_set_id)
    expect(configure.mock.calls[0]?.[1]).toEqual({
      keep_latest_completed: 5,
      expire_after_seconds: 1_209_600,
    })
    expect(configure.mock.calls[0]?.[2]).toBe(configure.mock.calls[1]?.[2])
    expect(configure.mock.calls[0]?.[2]).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    expect(await screen.findByRole('status')).toHaveTextContent('Retention settings updated.')
    expect(screen.getByText(/Keep at least/)).toHaveTextContent('5')
  })

  it('[recovery] persists the exact retention request before POST and removes it on success', async () => {
    const updatedPolicy = {
      data: { ...policy.data, policy_revision_id: 'policy-next', revision_number: '2' },
      meta: { request_id: 'policy-next' },
    }
    const configure = vi.fn<BackupApi['configureBackupRetentionPolicy']>().mockImplementation(async (setId, request, key) => {
      const pending = mutationRecoveryStore.all().find((record) => record.action_kind === 'configure_retention_policy')
      expect(pending?.request_scope).toEqual({ backupSetId: setOne.backup_set_id })
      expect(pending?.request).toEqual(request)
      expect(pending?.idempotency_key).toBe(key)
      expect(setId).toBe(setOne.backup_set_id)
      return updatedPolicy
    })
    renderBackups(makeBackupApi({ configureBackupRetentionPolicy: configure }))
    fireEvent.click(await screen.findByRole('button', { name: 'Change retention settings' }))
    fireEvent.click(screen.getByRole('button', { name: 'Save retention settings' }))

    expect(await screen.findByRole('status')).toHaveTextContent('Retention settings updated.')
    expect(configure).toHaveBeenCalledOnce()
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[recovery] abandons the old retention retry context when values change and uses a new key', async () => {
    const updatedPolicy = {
      data: { ...policy.data, policy_revision_id: 'policy-changed', revision_number: '2', keep_latest_completed: '6' },
      meta: { request_id: 'policy-changed' },
    }
    const configure = vi
      .fn<BackupApi['configureBackupRetentionPolicy']>()
      .mockRejectedValueOnce(new Error('response lost'))
      .mockResolvedValueOnce(updatedPolicy)
    renderBackups(makeBackupApi({ configureBackupRetentionPolicy: configure }))
    fireEvent.click(await screen.findByRole('button', { name: 'Change retention settings' }))
    fireEvent.click(screen.getByRole('button', { name: 'Save retention settings' }))
    await screen.findByRole('heading', { name: 'Finish saving retention settings' })
    const oldKey = configure.mock.calls[0]?.[2]

    fireEvent.change(screen.getByLabelText('Completed backups to always keep'), { target: { value: '6' } })
    expect(screen.queryByRole('heading', { name: 'Finish saving retention settings' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Save retention settings' }))

    expect(await screen.findByRole('status')).toHaveTextContent('Retention settings updated.')
    expect(configure).toHaveBeenCalledTimes(2)
    expect(configure.mock.calls[1]?.[2]).not.toBe(oldKey)
    expect(configure.mock.calls[1]?.[1]).toEqual({
      keep_latest_completed: 6,
      expire_after_seconds: Number(policy.data.expire_after_seconds),
    })
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[recovery] keeps maintenance explicit and retries an ambiguous request with the same key', async () => {
    let firstAttempt = true
    const create = vi.fn<BackupApi['createBackupMaintenanceRun']>().mockImplementation(async (setId, key) => {
      const pending = mutationRecoveryStore.all().find((record) => record.action_kind === 'create_maintenance_run')
      expect(pending?.request_scope).toEqual({ backupSetId: setOne.backup_set_id })
      expect(pending?.request).toEqual({})
      expect(pending?.idempotency_key).toBe(key)
      expect(setId).toBe(setOne.backup_set_id)
      if (firstAttempt) {
        firstAttempt = false
        throw new Error('transport interrupted')
      }
      return maintenanceRunResponse
    })
    const api = makeBackupApi({
      listBackupOperations: vi.fn().mockResolvedValue(page([])),
      createBackupMaintenanceRun: create,
    })
    renderBackups(api)
    const runButton = await screen.findByRole('button', { name: 'Run backup now' })
    await waitFor(() => expect(runButton).toBeEnabled())

    expect(api.advanceBackupMaintenanceRun).not.toHaveBeenCalled()
    fireEvent.click(runButton)
    expect(create).toHaveBeenCalledOnce()
    expect(await screen.findByRole('alert')).toHaveTextContent(/same request will be retried safely/i)

    fireEvent.click(screen.getByRole('button', { name: 'Try the request again' }))
    await waitFor(() => expect(create).toHaveBeenCalledTimes(2))
    expect(create.mock.calls[0]?.[1]).toBe(create.mock.calls[1]?.[1])
    expect(await screen.findByRole('status')).toHaveTextContent(/server-confirmed state/i)
    expect(mutationRecoveryStore.all()).toHaveLength(0)

    const nextRunButton = await screen.findByRole('button', { name: 'Run backup now' })
    fireEvent.click(nextRunButton)
    await waitFor(() => expect(create).toHaveBeenCalledTimes(3))
    expect(create.mock.calls[1]?.[1]).not.toBe(create.mock.calls[2]?.[1])
  })

  it('does not issue two maintenance requests for an immediate double click', async () => {
    let resolveCreate: ((value: typeof maintenanceRunResponse) => void) | undefined
    const create = vi.fn<BackupApi['createBackupMaintenanceRun']>().mockImplementation(
      () => new Promise((resolve) => {
        resolveCreate = resolve
      }),
    )
    const api = makeBackupApi({
      listBackupOperations: vi.fn().mockResolvedValue(page([])),
      createBackupMaintenanceRun: create,
    })
    renderBackups(api)
    const runButton = await screen.findByRole('button', { name: 'Run backup now' })
    await waitFor(() => expect(runButton).toBeEnabled())

    fireEvent.click(runButton)
    fireEvent.click(runButton)
    expect(create).toHaveBeenCalledOnce()
    resolveCreate?.(maintenanceRunResponse)
    await waitFor(() => expect(screen.queryByText('Submitting…')).not.toBeInTheDocument())
  })

  it('[recovery] requires an explicit Continue action for maintenance advancement', async () => {
    const advance = vi.fn<BackupApi['advanceBackupMaintenanceRun']>().mockImplementation(async (runId, key) => {
      const pending = mutationRecoveryStore.all().find((record) => record.action_kind === 'advance_maintenance_run')
      expect(pending?.request_scope).toEqual({ backupSetId: setOne.backup_set_id, maintenanceRunId: 'run-1' })
      expect(pending?.request).toEqual({ expected_state: 'CREATED' })
      expect(pending?.idempotency_key).toBe(key)
      expect(runId).toBe('run-1')
      return maintenanceRunResponse
    })
    const api = makeBackupApi({
      listBackupOperations: vi.fn().mockResolvedValue(page([maintenanceSummary])),
      advanceBackupMaintenanceRun: advance,
    })
    renderBackups(api)

    const operationRow = await screen.findByRole('button', { name: /Backup maintenance.*Ready for next step/i })
    expect(advance).not.toHaveBeenCalled()
    fireEvent.click(operationRow)
    expect(await screen.findByRole('button', { name: 'Continue backup maintenance' })).toBeInTheDocument()
    expect(advance).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Continue backup maintenance' }))
    await waitFor(() => expect(advance).toHaveBeenCalledOnce())
    expect(advance.mock.calls[0]?.[1]).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    )
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[recovery] GET-reconciles an uncertain advance that already moved and never advances another phase', async () => {
    mutationRecoveryStore.add({
      action_kind: 'advance_maintenance_run',
      idempotency_key: '00000000-0000-7000-8000-000000000457',
      request_scope: { backupSetId: 'set-1', maintenanceRunId: 'run-1' },
      request: { expected_state: 'CREATED' },
    })
    const getRun = vi.fn<BackupApi['getBackupMaintenanceRun']>().mockResolvedValue({
      ...maintenanceRunResponse,
      data: { ...maintenanceRunResponse.data, state: 'SNAPSHOT_CAPTURED' },
    })
    const advance = vi.fn<BackupApi['advanceBackupMaintenanceRun']>()
    renderBackups(makeBackupApi({ getBackupMaintenanceRun: getRun, advanceBackupMaintenanceRun: advance }))

    expect(await screen.findByText(/already moved forward/i)).toBeInTheDocument()
    expect(getRun).toHaveBeenCalledWith('run-1')
    expect(advance).not.toHaveBeenCalled()
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[recovery] GET-reconciles an unchanged maintenance phase and requires explicit same-key retry', async () => {
    const key = '00000000-0000-7000-8000-000000000557'
    mutationRecoveryStore.add({
      action_kind: 'advance_maintenance_run',
      idempotency_key: key,
      request_scope: { backupSetId: 'set-1', maintenanceRunId: 'run-1' },
      request: { expected_state: 'CREATED' },
    })
    const getRun = vi.fn<BackupApi['getBackupMaintenanceRun']>().mockResolvedValue(maintenanceRunResponse)
    const advance = vi.fn<BackupApi['advanceBackupMaintenanceRun']>().mockResolvedValue({
      ...maintenanceRunResponse,
      data: { ...maintenanceRunResponse.data, state: 'SNAPSHOT_CAPTURED' },
    })
    renderBackups(makeBackupApi({ getBackupMaintenanceRun: getRun, advanceBackupMaintenanceRun: advance }))

    expect(await screen.findByRole('heading', { name: 'Finish the pending maintenance step' })).toBeInTheDocument()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Retry safely' })).toBeEnabled())
    expect(getRun).toHaveBeenCalledWith('run-1')
    expect(advance).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Retry safely' }))

    await waitFor(() => expect(advance).toHaveBeenCalledWith('run-1', key))
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('keeps one maintenance identity through three separately confirmed durable steps', async () => {
    const stages = [
      { state: 'CREATED', phase: 'AWAITING_ADVANCE', completed: 0, terminal: false, next: 'ADVANCE' },
      { state: 'SNAPSHOT_CAPTURED', phase: 'SNAPSHOT_CAPTURED', completed: 1, terminal: false, next: 'ADVANCE' },
      { state: 'EXPIRY_PLANNED', phase: 'EXPIRY_PLANNED', completed: 2, terminal: false, next: 'ADVANCE' },
      { state: 'COMPLETED', phase: 'COMPLETED', completed: 3, terminal: true, next: 'NONE' },
    ] as const
    let stageIndex = -1
    const summaryAtCurrentStage = (): BackupOperationSummary => {
      const stage = stages[Math.max(0, stageIndex)]
      return {
        ...maintenanceSummary,
        state: stage.state,
        phase: stage.phase,
        progress: { completed_steps: stage.completed, total_steps: 3 },
        terminal: stage.terminal,
        next_action: stage.next,
        last_transition_at: `2026-08-31T09:0${stage.completed}:00Z`,
        ...(stage.terminal ? { completed_at: '2026-08-31T09:03:00Z' } : {}),
      }
    }
    const runAtCurrentStage = () => ({
      ...maintenanceRunResponse,
      data: {
        ...maintenanceRunResponse.data,
        state: stages[Math.max(0, stageIndex)].state,
        ...(stageIndex >= 1 ? { captured_snapshot_id: 'snapshot-new', snapshot_captured_at: '2026-08-31T09:01:00Z' } : {}),
        ...(stageIndex >= 2 ? { expiry_plan_id: 'expiry-plan-1', expiry_planned_at: '2026-08-31T09:02:00Z' } : {}),
        ...(stageIndex >= 3 ? { expiry_execution_id: 'expiry-execution-1', completed_at: '2026-08-31T09:03:00Z' } : {}),
      },
    })
    const create = vi.fn<BackupApi['createBackupMaintenanceRun']>().mockImplementation(async () => {
      stageIndex = 0
      return runAtCurrentStage()
    })
    const advance = vi.fn<BackupApi['advanceBackupMaintenanceRun']>().mockImplementation(async () => {
      stageIndex = Math.min(stageIndex + 1, stages.length - 1)
      return runAtCurrentStage()
    })
    const listOperations = vi.fn<BackupApi['listBackupOperations']>().mockImplementation(async () =>
      page(stageIndex < 0 ? [] : [summaryAtCurrentStage()]),
    )
    const getOperation = vi.fn<BackupApi['getBackupOperation']>().mockImplementation(async () => ({
      data: {
        ...maintenanceDetail,
        ...summaryAtCurrentStage(),
        operation_kind: 'MAINTENANCE',
        maintenance_run_id: 'run-1',
      },
      meta: { request_id: 'maintenance-detail' },
    }))
    const api = makeBackupApi({
      createBackupMaintenanceRun: create,
      advanceBackupMaintenanceRun: advance,
      listBackupOperations: listOperations,
      getBackupOperation: getOperation,
    })
    renderBackups(api)

    const runButton = await screen.findByRole('button', { name: 'Run backup now' })
    await waitFor(() => expect(runButton).toBeEnabled())
    fireEvent.click(runButton)
    expect(advance).not.toHaveBeenCalled()
    await waitFor(() => expect(screen.getAllByText('0 of 3 steps completed').length).toBeGreaterThan(0))

    for (const completedSteps of [1, 2, 3]) {
      const continueButton = await screen.findByRole('button', { name: 'Continue backup maintenance' })
      fireEvent.click(continueButton)
      await waitFor(() => expect(advance).toHaveBeenCalledTimes(completedSteps))
      await waitFor(() => expect(screen.getAllByText(`${completedSteps} of 3 steps completed`).length).toBeGreaterThan(0))
    }

    expect(screen.queryByRole('button', { name: 'Continue backup maintenance' })).not.toBeInTheDocument()
    expect(new Set(advance.mock.calls.map((call) => call[1])).size).toBe(3)
    expect(getOperation.mock.calls.every((call) => call[0] === 'MAINTENANCE' && call[1] === 'run-1')).toBe(true)
  })

  it('presents stale maintenance as terminal and keeps restore/cleanup history read-only', async () => {
    const staleSummary: BackupOperationSummary = {
      ...maintenanceSummary,
      operation_id: 'operation-stale',
      state: 'STALE',
      phase: 'STALE',
      terminal: true,
      next_action: 'CREATE_NEW_RUN',
    }
    const staleDetail: BackupOperationDetail = {
      ...maintenanceDetail,
      ...staleSummary,
      operation_kind: 'MAINTENANCE',
    }
    const api = makeBackupApi({
      listBackupOperations: vi.fn().mockResolvedValue(page([staleSummary, restoreSummary, pruneSummary])),
      getBackupOperation: vi.fn().mockResolvedValue({ data: staleDetail, meta: { request_id: 'stale' } }),
    })
    renderBackups(api)

    const staleRow = await screen.findByRole('button', { name: /Needs a new plan/i })
    fireEvent.click(staleRow)
    expect(await screen.findByText(/This operation can no longer continue/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Continue backup maintenance/i })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /^Restore$/i })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /^Cleanup$/i })).not.toBeInTheDocument()
    expect(screen.queryByText(/physical storage reclamation/i)).not.toBeInTheDocument()
  })
})
