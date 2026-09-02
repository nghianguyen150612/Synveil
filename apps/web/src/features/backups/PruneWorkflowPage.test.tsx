import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AuthApi, AuthSessionResponse } from '../../api/auth'
import type {
  BackupApi,
  BackupPruneExecutionResponse,
  BackupPruneOperation,
  BackupPrunePlanResponse,
} from '../../api/backups'
import type { BootstrapApi, BootstrapStatusResponse } from '../../api/bootstrap'
import { ApiRequestError } from '../../api/errors'
import { mutationRecoveryStore } from '../../api/mutationRecovery'
import { App } from '../../app/App'

const set = {
  backup_set_id: 'set-1',
  name: 'Family files',
  library_id: 'library-1',
  state: 'ACTIVE' as const,
  created_at: '2026-08-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
}

const expiredSnapshot = {
  snapshot_id: 'snapshot-bound-to-plan',
  backup_set_id: set.backup_set_id,
  library_id: set.library_id,
  state: 'EXPIRED' as const,
  created_at: '2026-07-01T00:00:00Z',
  committed_at: '2026-07-01T00:01:00Z',
  expired_at: '2026-08-01T00:00:00Z',
  logical_node_count: '7',
  content_reference_count: '5',
}

const completedSnapshot = {
  ...expiredSnapshot,
  snapshot_id: 'snapshot-completed',
  state: 'COMPLETED' as const,
  expired_at: undefined,
}

const plannedPlan = {
  prune_plan_id: 'prune-plan-1',
  snapshot_id: expiredSnapshot.snapshot_id,
  backup_set_id: set.backup_set_id,
  state: 'PLANNED' as const,
  entry_count: '7',
  distinct_content_count: '5',
  retained_by_other_reference_count: '3',
  would_become_unreferenced_count: '2',
  impact_summary: [
    { impact: 'RETAINED_BY_OTHER_REFERENCE' as const, content_count: '3' },
    { impact: 'WOULD_BECOME_UNREFERENCED' as const, content_count: '2' },
  ],
  created_at: '2026-09-01T00:00:00Z',
}

const planResponse: BackupPrunePlanResponse = {
  data: plannedPlan,
  meta: { request_id: 'plan' },
}

const receiptResponse: BackupPruneExecutionResponse = {
  data: {
    prune_execution_id: 'prune-execution-1',
    prune_plan_id: plannedPlan.prune_plan_id,
    snapshot_id: plannedPlan.snapshot_id,
    backup_set_id: plannedPlan.backup_set_id,
    released_content_reference_count: '7',
    distinct_content_count: '5',
    retained_elsewhere_count: '3',
    gc_handoff_count: '2',
    executed_at: '2026-09-01T00:02:00Z',
  },
  meta: { request_id: 'receipt' },
}

const session: AuthSessionResponse = {
  data: {
    authenticated: true,
    user_id: '00000000-0000-7000-8000-000000000001',
    is_instance_admin: false,
    session_id: '00000000-0000-7000-8000-000000000002',
  },
  meta: { request_id: 'session' },
}

const bootstrap: BootstrapStatusResponse = {
  data: { setup_required: false },
  meta: { request_id: 'bootstrap' },
}

function page<T>(data: readonly T[]) {
  return { data, page: { has_more: false }, meta: { request_id: 'page' } }
}

function authApis(): { readonly authApi: AuthApi; readonly bootstrapApi: BootstrapApi } {
  return {
    authApi: {
      login: vi.fn().mockResolvedValue(session),
      getCurrentSession: vi.fn().mockResolvedValue(session),
      getCsrfToken: vi.fn().mockResolvedValue({ data: { csrf_token: '0'.repeat(128) }, meta: { request_id: 'csrf' } }),
      logout: vi.fn().mockResolvedValue(undefined),
    },
    bootstrapApi: {
      getStatus: vi.fn().mockResolvedValue(bootstrap),
      createFirstAdmin: vi.fn().mockResolvedValue(bootstrap),
    },
  }
}

function pruneOperation(state: 'PLANNED' | 'STALE' | 'EXECUTED'): BackupPruneOperation {
  return {
    operation_kind: 'PRUNE',
    operation_id: plannedPlan.prune_plan_id,
    prune_plan_id: plannedPlan.prune_plan_id,
    backup_set_id: plannedPlan.backup_set_id,
    snapshot_id: plannedPlan.snapshot_id,
    state,
    phase: state === 'PLANNED' ? 'AWAITING_CONFIRMATION' : state === 'STALE' ? 'STALE' : 'COMPLETED',
    progress: { completed_steps: state === 'PLANNED' ? 1 : 2, total_steps: 2 },
    terminal: state !== 'PLANNED',
    next_action: state === 'PLANNED' ? 'EXECUTE' : state === 'STALE' ? 'CREATE_NEW_PLAN' : 'NONE',
    planned_pin_release_count: '7',
    distinct_content_count: '5',
    retained_by_other_reference_count: '3',
    would_become_unreferenced_count: '2',
    created_at: plannedPlan.created_at,
    last_transition_at: '2026-09-01T00:02:00Z',
    ...(state === 'EXECUTED' ? {
      prune_execution_id: receiptResponse.data.prune_execution_id,
      released_content_reference_count: '7',
      gc_handoff_count: '2',
      executed_at: receiptResponse.data.executed_at,
      completed_at: receiptResponse.data.executed_at,
    } : {}),
  }
}

function makeBackupApi(overrides: Partial<BackupApi> = {}): BackupApi {
  return {
    listBackupSets: vi.fn().mockResolvedValue(page([set])),
    getBackupSet: vi.fn().mockResolvedValue({ data: set, meta: { request_id: 'set' } }),
    createBackupSet: vi.fn().mockRejectedValue(new Error('unused')),
    listBackupSnapshots: vi.fn().mockResolvedValue(page([expiredSnapshot])),
    getBackupSnapshot: vi.fn().mockResolvedValue({ data: expiredSnapshot, meta: { request_id: 'snapshot' } }),
    listBackupSnapshotNodes: vi.fn().mockResolvedValue(page([])),
    getBackupRetentionPolicy: vi.fn().mockRejectedValue(new Error('unused')),
    configureBackupRetentionPolicy: vi.fn().mockRejectedValue(new Error('unused')),
    listBackupOperations: vi.fn().mockResolvedValue(page([pruneOperation('PLANNED')])),
    getBackupOperation: vi.fn().mockResolvedValue({ data: pruneOperation('EXECUTED'), meta: { request_id: 'operation' } }),
    listBackupMaintenanceRuns: vi.fn().mockResolvedValue(page([])),
    getBackupMaintenanceRun: vi.fn().mockRejectedValue(new Error('unused')),
    createBackupMaintenanceRun: vi.fn().mockRejectedValue(new Error('unused')),
    advanceBackupMaintenanceRun: vi.fn().mockRejectedValue(new Error('unused')),
    createBackupRestorePlan: vi.fn().mockRejectedValue(new Error('unused')),
    getBackupRestorePlan: vi.fn().mockRejectedValue(new Error('unused')),
    executeBackupRestorePlan: vi.fn().mockRejectedValue(new Error('unused')),
    getBackupRestoreExecution: vi.fn().mockRejectedValue(new Error('unused')),
    createBackupPrunePlan: vi.fn().mockResolvedValue(planResponse),
    getBackupPrunePlan: vi.fn().mockResolvedValue(planResponse),
    executeBackupPrunePlan: vi.fn().mockResolvedValue(receiptResponse),
    getBackupPruneExecution: vi.fn().mockResolvedValue(receiptResponse),
    ...overrides,
  }
}

function renderPrune(
  path: string,
  backupApi: BackupApi,
  auth = authApis(),
) {
  return render(
    <App
      router="memory"
      initialEntries={[path]}
      authApi={auth.authApi}
      bootstrapApi={auth.bootstrapApi}
      backupApi={backupApi}
    />,
  )
}

function planPath(): string {
  return `/backups/${set.backup_set_id}/prune/${plannedPlan.prune_plan_id}`
}

function conflictError(code = 'prune_plan_stale', status = 409): ApiRequestError {
  return new ApiRequestError(status, {
    code,
    message: 'raw server diagnostic',
    request_id: 'request-1',
    retryable: false,
  })
}

describe('guarded prune workflow', () => {
  beforeEach(() => mutationRecoveryStore.clear())

  it('offers planning only for an expired snapshot', async () => {
    const api = makeBackupApi()
    renderPrune(`/backups/${set.backup_set_id}/snapshots/${expiredSnapshot.snapshot_id}/prune`, api)

    expect(await screen.findByRole('button', { name: 'Create prune plan' })).toBeEnabled()
    expect(screen.getAllByText('Expired').length).toBeGreaterThan(0)
    expect(api.createBackupPrunePlan).not.toHaveBeenCalled()
  })

  it('fails closed for a completed snapshot and never creates a plan', async () => {
    const api = makeBackupApi({
      getBackupSnapshot: vi.fn().mockResolvedValue({ data: completedSnapshot, meta: { request_id: 'snapshot' } }),
    })
    renderPrune(`/backups/${set.backup_set_id}/snapshots/${completedSnapshot.snapshot_id}/prune`, api)

    expect(await screen.findByRole('heading', { name: 'This snapshot is not eligible' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Create prune plan' })).not.toBeInTheDocument()
    expect(api.createBackupPrunePlan).not.toHaveBeenCalled()
  })

  it('[recovery] creates a plan without executing and presents the persisted impact review', async () => {
    const createPlan = vi.fn<BackupApi['createBackupPrunePlan']>().mockImplementation(async (snapshotId, key) => {
      const pending = mutationRecoveryStore.all().find((record) => record.action_kind === 'create_prune_plan')
      expect(pending?.request_scope).toEqual({ backupSetId: 'set-1', snapshotId: expiredSnapshot.snapshot_id })
      expect(pending?.request).toEqual({})
      expect(pending?.idempotency_key).toBe(key)
      expect(snapshotId).toBe(expiredSnapshot.snapshot_id)
      return planResponse
    })
    const executePlan = vi.fn<BackupApi['executeBackupPrunePlan']>().mockResolvedValue(receiptResponse)
    const api = makeBackupApi({ createBackupPrunePlan: createPlan, executeBackupPrunePlan: executePlan })
    renderPrune(`/backups/${set.backup_set_id}/snapshots/${expiredSnapshot.snapshot_id}/prune`, api)

    fireEvent.click(await screen.findByRole('button', { name: 'Create prune plan' }))
    expect(await screen.findByRole('heading', { name: 'Review retention impact' })).toBeInTheDocument()
    expect(screen.getByText('Retained by another reference')).toBeInTheDocument()
    expect(screen.getByText('Would become unreferenced')).toBeInTheDocument()
    expect(createPlan.mock.calls[0]?.[0]).toBe(expiredSnapshot.snapshot_id)
    expect(createPlan.mock.calls[0]?.[1]).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    expect(executePlan).not.toHaveBeenCalled()
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('retries an unconfirmed plan creation with the same idempotency key', async () => {
    const createPlan = vi
      .fn<BackupApi['createBackupPrunePlan']>()
      .mockRejectedValueOnce(new Error('response lost'))
      .mockResolvedValueOnce(planResponse)
    const api = makeBackupApi({ createBackupPrunePlan: createPlan })
    renderPrune(`/backups/${set.backup_set_id}/snapshots/${expiredSnapshot.snapshot_id}/prune`, api)

    fireEvent.click(await screen.findByRole('button', { name: 'Create prune plan' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Try the same request again' }))

    await waitFor(() => expect(createPlan).toHaveBeenCalledTimes(2))
    expect(createPlan.mock.calls[0]?.[1]).toBe(createPlan.mock.calls[1]?.[1])
    expect(await screen.findByRole('heading', { name: 'Review retention impact' })).toBeInTheDocument()
    expect(api.executeBackupPrunePlan).not.toHaveBeenCalled()
  })

  it('requires checkbox confirmation, binds the exact plan snapshot, and prevents double execution', async () => {
    let resolveExecution: ((value: BackupPruneExecutionResponse) => void) | undefined
    const executePlan = vi.fn<BackupApi['executeBackupPrunePlan']>().mockImplementation(() => new Promise((resolve) => {
      resolveExecution = resolve
    }))
    const api = makeBackupApi({ executeBackupPrunePlan: executePlan })
    renderPrune(planPath(), api)

    const reviewButton = await screen.findByRole('button', { name: 'Review and confirm' })
    fireEvent.click(reviewButton)
    const dialog = screen.getByRole('dialog', { name: 'Release retained backup content?' })
    const executeButton = screen.getByRole('button', { name: 'Release retained content' })
    expect(executeButton).toBeDisabled()
    expect(dialog).toHaveTextContent('Family files')
    expect(dialog).toHaveTextContent('7')

    fireEvent.click(screen.getByRole('checkbox', { name: /I understand this releases/i }))
    expect(executeButton).toBeEnabled()
    fireEvent.click(executeButton)
    fireEvent.click(executeButton)
    expect(executePlan).toHaveBeenCalledOnce()
    expect(executePlan.mock.calls[0]?.[0]).toBe(plannedPlan.prune_plan_id)
    expect(executePlan.mock.calls[0]?.[1]).toBe(plannedPlan.snapshot_id)
    expect(executePlan.mock.calls[0]?.[2]).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)

    resolveExecution?.(receiptResponse)
    expect(await screen.findByRole('heading', { name: 'Backup retention released' })).toBeInTheDocument()
    expect(screen.getAllByText('Expired').length).toBeGreaterThan(0)
    expect(screen.getByText(/expired snapshot remains in backup history/i)).toBeInTheDocument()
    expect(screen.queryByText(/will delete|permanently deleted|guaranteed space|free [0-9]/i)).not.toBeInTheDocument()
  })

  it('retries a lost execution response with the same idempotency key', async () => {
    const executePlan = vi
      .fn<BackupApi['executeBackupPrunePlan']>()
      .mockRejectedValueOnce(new Error('response lost'))
      .mockResolvedValueOnce(receiptResponse)
    const api = makeBackupApi({ executeBackupPrunePlan: executePlan })
    renderPrune(planPath(), api)
    fireEvent.click(await screen.findByRole('button', { name: 'Review and confirm' }))
    fireEvent.click(screen.getByRole('checkbox', { name: /I understand this releases/i }))
    fireEvent.click(screen.getByRole('button', { name: 'Release retained content' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Try the same request again' }))

    await waitFor(() => expect(executePlan).toHaveBeenCalledTimes(2))
    expect(executePlan.mock.calls[0]?.[2]).toBe(executePlan.mock.calls[1]?.[2])
    expect(executePlan.mock.calls[1]?.[1]).toBe(plannedPlan.snapshot_id)
  })

  it('[recovery] requires fresh confirmation after reload and replays a PLANNED execution with the same key and loaded snapshot', async () => {
    let firstAttempt = true
    let originalKey: string | undefined
    const executePlan = vi.fn<BackupApi['executeBackupPrunePlan']>().mockImplementation(async (planId, confirmSnapshotId, key) => {
      const pending = mutationRecoveryStore.all().find((record) => record.action_kind === 'execute_prune_plan')
      expect(pending?.request).toEqual({ confirm_snapshot_id: plannedPlan.snapshot_id })
      expect(pending?.idempotency_key).toBe(key)
      expect(planId).toBe(plannedPlan.prune_plan_id)
      expect(confirmSnapshotId).toBe(plannedPlan.snapshot_id)
      originalKey = key
      if (firstAttempt) {
        firstAttempt = false
        throw new Error('connection lost after send')
      }
      return receiptResponse
    })
    const getPlan = vi.fn<BackupApi['getBackupPrunePlan']>().mockResolvedValue(planResponse)
    const api = makeBackupApi({ executeBackupPrunePlan: executePlan, getBackupPrunePlan: getPlan })
    const firstRender = renderPrune(planPath(), api)
    fireEvent.click(await screen.findByRole('button', { name: 'Review and confirm' }))
    fireEvent.click(screen.getByRole('checkbox', { name: /I understand this releases/i }))
    fireEvent.click(screen.getByRole('button', { name: 'Release retained content' }))
    expect(await screen.findByRole('heading', { name: 'Finish the pending retention release' })).toBeInTheDocument()
    expect(executePlan).toHaveBeenCalledOnce()
    firstRender.unmount()

    renderPrune(planPath(), api)
    expect(await screen.findByRole('heading', { name: 'Finish the pending retention release' })).toBeInTheDocument()
    expect(executePlan).toHaveBeenCalledOnce()
    expect(getPlan.mock.calls.length).toBeGreaterThanOrEqual(2)
    fireEvent.click(screen.getByRole('button', { name: 'Retry safely' }))

    const checkbox = await screen.findByRole('checkbox', { name: /I understand this releases/i })
    expect(checkbox).not.toBeChecked()
    expect(screen.getByRole('button', { name: 'Release retained content' })).toBeDisabled()
    fireEvent.click(checkbox)
    fireEvent.click(screen.getByRole('button', { name: 'Release retained content' }))

    expect(await screen.findByRole('heading', { name: 'Backup retention released' })).toBeInTheDocument()
    expect(executePlan).toHaveBeenCalledTimes(2)
    expect(executePlan.mock.calls[1]?.[2]).toBe(originalKey)
    expect(executePlan.mock.calls[1]?.[1]).toBe(plannedPlan.snapshot_id)
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[auth] recovers a prune 401 without replay, reloads GET state, and resets confirmation', async () => {
    const expired = new ApiRequestError(401, {
      code: 'authentication_failed',
      message: 'expired',
      request_id: 'prune-auth',
      retryable: false,
    })
    const executePlan = vi
      .fn<BackupApi['executeBackupPrunePlan']>()
      .mockRejectedValueOnce(expired)
      .mockResolvedValueOnce(receiptResponse)
    const getPlan = vi.fn<BackupApi['getBackupPrunePlan']>().mockResolvedValue(planResponse)
    const api = makeBackupApi({ executeBackupPrunePlan: executePlan, getBackupPrunePlan: getPlan })
    const auth = authApis()
    const getCurrentSession = vi.fn().mockResolvedValue(session)
    const authApi: AuthApi = { ...auth.authApi, getCurrentSession }
    renderPrune(planPath(), api, { ...auth, authApi })

    fireEvent.click(await screen.findByRole('button', { name: 'Review and confirm' }))
    fireEvent.click(screen.getByRole('checkbox', { name: /I understand this releases/i }))
    fireEvent.click(screen.getByRole('button', { name: 'Release retained content' }))

    await waitFor(() => expect(getPlan.mock.calls.length).toBeGreaterThanOrEqual(2))
    const recoveredRetry = await screen.findByRole('button', { name: 'Retry safely' })
    expect(executePlan).toHaveBeenCalledOnce()
    const originalKey = executePlan.mock.calls[0]?.[2]
    expect(getCurrentSession).toHaveBeenCalledTimes(2)
    expect(authApi.getCsrfToken).toHaveBeenCalledOnce()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()

    fireEvent.click(recoveredRetry)
    const checkbox = await screen.findByRole('checkbox', { name: /I understand this releases/i })
    expect(checkbox).not.toBeChecked()
    expect(screen.getByRole('button', { name: 'Release retained content' })).toBeDisabled()
    fireEvent.click(checkbox)
    fireEvent.click(screen.getByRole('button', { name: 'Release retained content' }))

    expect(await screen.findByRole('heading', { name: 'Backup retention released' })).toBeInTheDocument()
    expect(executePlan).toHaveBeenCalledTimes(2)
    expect(executePlan.mock.calls[1]?.[2]).toBe(originalKey)
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('closes confirmation with Escape, clears consent, and restores focus', async () => {
    renderPrune(planPath(), makeBackupApi())
    const reviewButton = await screen.findByRole('button', { name: 'Review and confirm' })
    reviewButton.focus()
    fireEvent.click(reviewButton)
    fireEvent.click(screen.getByRole('checkbox', { name: /I understand this releases/i }))
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(reviewButton).toHaveFocus()
    fireEvent.click(reviewButton)
    expect(screen.getByRole('checkbox', { name: /I understand this releases/i })).not.toBeChecked()
    expect(screen.getByRole('button', { name: 'Release retained content' })).toBeDisabled()
  })

  it('marks a conflicting plan stale, offers a new plan, and never force-executes', async () => {
    const executePlan = vi.fn<BackupApi['executeBackupPrunePlan']>().mockRejectedValue(conflictError())
    const staleResponse: BackupPrunePlanResponse = {
      data: { ...plannedPlan, state: 'STALE', stale_at: '2026-09-01T00:03:00Z' },
      meta: { request_id: 'stale' },
    }
    const getPlan = vi.fn<BackupApi['getBackupPrunePlan']>()
      .mockResolvedValueOnce(planResponse)
      .mockResolvedValue(staleResponse)
    const api = makeBackupApi({ executeBackupPrunePlan: executePlan, getBackupPrunePlan: getPlan })
    renderPrune(planPath(), api)
    fireEvent.click(await screen.findByRole('button', { name: 'Review and confirm' }))
    fireEvent.click(screen.getByRole('checkbox', { name: /I understand this releases/i }))
    fireEvent.click(screen.getByRole('button', { name: 'Release retained content' }))

    expect(await screen.findByText(/operation is no longer valid/i)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Create a new plan' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /force/i })).not.toBeInTheDocument()
    expect(executePlan).toHaveBeenCalledOnce()
  })

  it('[recovery] clears an impossible reload retry when GET says the prune plan is STALE', async () => {
    mutationRecoveryStore.add({
      action_kind: 'execute_prune_plan',
      idempotency_key: '00000000-0000-7000-8000-000000000757',
      request_scope: {
        backupSetId: plannedPlan.backup_set_id,
        snapshotId: plannedPlan.snapshot_id,
        prunePlanId: plannedPlan.prune_plan_id,
      },
      request: { confirm_snapshot_id: plannedPlan.snapshot_id },
    })
    const staleResponse: BackupPrunePlanResponse = {
      data: { ...plannedPlan, state: 'STALE', stale_at: '2026-09-01T00:03:00Z' },
      meta: { request_id: 'stale' },
    }
    const executePlan = vi.fn<BackupApi['executeBackupPrunePlan']>()
    renderPrune(planPath(), makeBackupApi({
      getBackupPrunePlan: vi.fn().mockResolvedValue(staleResponse),
      executeBackupPrunePlan: executePlan,
    }))

    expect(await screen.findByText(/operation is no longer valid/i)).toBeInTheDocument()
    expect(await screen.findByText(/impossible local retry was cleared/i)).toBeInTheDocument()
    expect(executePlan).not.toHaveBeenCalled()
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[recovery] reloads an executed plan through its one operation and receipt', async () => {
    mutationRecoveryStore.add({
      action_kind: 'execute_prune_plan',
      idempotency_key: '00000000-0000-7000-8000-000000000357',
      request_scope: {
        backupSetId: plannedPlan.backup_set_id,
        snapshotId: plannedPlan.snapshot_id,
        prunePlanId: plannedPlan.prune_plan_id,
      },
      request: { confirm_snapshot_id: plannedPlan.snapshot_id },
    })
    const executedPlan: BackupPrunePlanResponse = {
      data: { ...plannedPlan, state: 'EXECUTED' },
      meta: { request_id: 'executed-plan' },
    }
    const getOperation = vi.fn<BackupApi['getBackupOperation']>().mockResolvedValue({
      data: pruneOperation('EXECUTED'),
      meta: { request_id: 'operation' },
    })
    const getReceipt = vi.fn<BackupApi['getBackupPruneExecution']>().mockResolvedValue(receiptResponse)
    const api = makeBackupApi({ getBackupPrunePlan: vi.fn().mockResolvedValue(executedPlan), getBackupOperation: getOperation, getBackupPruneExecution: getReceipt })
    renderPrune(planPath(), api)

    expect(await screen.findByRole('heading', { name: 'Backup retention released' })).toBeInTheDocument()
    expect(getOperation).toHaveBeenCalledWith('PRUNE', plannedPlan.prune_plan_id, expect.objectContaining({ signal: expect.any(AbortSignal) }))
    expect(getReceipt).toHaveBeenCalledWith(receiptResponse.data.prune_execution_id, expect.objectContaining({ signal: expect.any(AbortSignal) }))
    expect(api.executeBackupPrunePlan).not.toHaveBeenCalled()
    expect(screen.queryByRole('checkbox', { name: /I understand this releases/i })).not.toBeInTheDocument()
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('conceals missing and foreign plans behind one generic 404 state', async () => {
    const api = makeBackupApi({ getBackupPrunePlan: vi.fn().mockRejectedValue(conflictError('not_found', 404)) })
    renderPrune(planPath(), api)

    expect(await screen.findByRole('heading', { name: 'Prune plan not available' })).toBeInTheDocument()
    expect(screen.getByRole('alert')).toHaveTextContent('Prune plan not available.')
    expect(screen.queryByText(/owner|foreign|belongs/i)).not.toBeInTheDocument()
  })
})
