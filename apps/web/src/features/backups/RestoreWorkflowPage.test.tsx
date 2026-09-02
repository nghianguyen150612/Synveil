import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AuthApi, AuthSessionResponse } from '../../api/auth'
import type { BootstrapApi, BootstrapStatusResponse } from '../../api/bootstrap'
import {
  type BackupApi,
  type BackupRestoreExecutionResponse,
  type BackupRestorePlan,
  type BackupRestorePlanResponse,
  type BackupRestoreOperation,
  type BackupSnapshot,
} from '../../api/backups'
import { ApiRequestError, type ApiErrorPayload } from '../../api/errors'
import { mutationRecoveryStore } from '../../api/mutationRecovery'
import type {
  FileMetadataApi,
  LiveLibraryResource,
  LiveNodeResource,
  LiveNodeResponse,
} from '../../api/files'
import { App } from '../../app/App'

const snapshot: BackupSnapshot = {
  snapshot_id: 'snapshot-1',
  backup_set_id: 'set-1',
  library_id: 'source-library-1',
  state: 'COMPLETED',
  created_at: '2026-08-30T09:00:00Z',
  committed_at: '2026-08-30T09:01:00Z',
  logical_node_count: '3',
  content_reference_count: '2',
}

const plannedPlan: BackupRestorePlan = {
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
  created_at: '2026-08-31T09:00:00Z',
}

const executedPlan: BackupRestorePlan = { ...plannedPlan, state: 'EXECUTED' }
const stalePlan: BackupRestorePlan = {
  ...plannedPlan,
  state: 'STALE',
  stale_at: '2026-08-31T09:02:00Z',
}

const planResponse: BackupRestorePlanResponse = {
  data: plannedPlan,
  meta: { request_id: 'plan' },
}

const executionResponse: BackupRestoreExecutionResponse = {
  data: {
    restore_execution_id: 'execution-1',
    restore_plan_id: 'plan-1',
    source_snapshot_id: 'snapshot-1',
    target_library_id: 'library-1',
    created_directory_count: '1',
    created_file_count: '2',
    journal_start_sequence: '10',
    journal_end_sequence: '12',
    executed_at: '2026-08-31T09:03:00Z',
  },
  meta: { request_id: 'execution' },
}

const libraryOne: LiveLibraryResource = {
  id: 'library-1',
  type: 'library',
  revision: '1',
  attributes: {
    name: 'Personal files',
    root_node_id: 'root-1',
    status: 'ACTIVE',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
}

const libraryTwo: LiveLibraryResource = {
  ...libraryOne,
  id: 'library-2',
  attributes: { ...libraryOne.attributes, name: 'Work files', root_node_id: 'root-2' },
}

const rootNode: LiveNodeResource = {
  id: 'root-1',
  type: 'node',
  revision: '1',
  attributes: {
    library_id: 'library-1',
    name: 'Library root',
    kind: 'DIRECTORY',
    state: 'ACTIVE',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
}

const folderNode: LiveNodeResource = {
  ...rootNode,
  id: 'folder-1',
  attributes: {
    ...rootNode.attributes,
    parent_id: 'root-1',
    name: 'Documents',
  },
}

const fileNode: LiveNodeResource = {
  ...rootNode,
  id: 'file-1',
  attributes: {
    ...rootNode.attributes,
    parent_id: 'root-1',
    name: 'readme.txt',
    kind: 'FILE',
  },
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

function operationForPlan(plan: BackupRestorePlan): BackupRestoreOperation {
  return {
    operation_kind: 'RESTORE',
    operation_id: 'operation-1',
    backup_set_id: 'set-1',
    snapshot_id: 'snapshot-1',
    source_snapshot_id: 'snapshot-1',
    restore_plan_id: plan.restore_plan_id,
    target_library_id: plan.target_library_id,
    target_parent_node_id: plan.target_parent_node_id,
    destination_name: plan.destination_name,
    state: plan.state,
    phase: plan.state === 'EXECUTED' ? 'COMPLETED' : plan.state === 'STALE' ? 'STALE' : 'AWAITING_EXECUTION',
    progress: { completed_steps: plan.state === 'EXECUTED' ? 2 : 1, total_steps: 2 },
    terminal: plan.state !== 'PLANNED',
    next_action: plan.state === 'PLANNED' ? 'EXECUTE' : 'NONE',
    created_at: plan.created_at,
    last_transition_at: '2026-08-31T09:03:00Z',
    planned_entry_count: plan.planned_entry_count,
    planned_directory_count: plan.planned_directory_count,
    planned_file_count: plan.planned_file_count,
    ...(plan.state === 'EXECUTED' ? { restore_execution_id: 'execution-1', executed_at: executionResponse.data.executed_at } : {}),
  }
}

function makeBackupApi(overrides: Partial<BackupApi> = {}): BackupApi {
  const operation = operationForPlan(plannedPlan)
  return {
    listBackupSets: vi.fn().mockResolvedValue(page([])),
    getBackupSet: vi.fn().mockResolvedValue({ data: { backup_set_id: 'set-1' }, meta: { request_id: 'set' } }),
    createBackupSet: vi.fn().mockRejectedValue(new Error('unused')),
    listBackupSnapshots: vi.fn().mockResolvedValue(page([snapshot])),
    getBackupSnapshot: vi.fn().mockResolvedValue({ data: snapshot, meta: { request_id: 'snapshot' } }),
    listBackupSnapshotNodes: vi.fn().mockResolvedValue(page([])),
    getBackupRetentionPolicy: vi.fn().mockResolvedValue({ data: {}, meta: { request_id: 'policy' } }),
    configureBackupRetentionPolicy: vi.fn().mockRejectedValue(new Error('unused')),
    listBackupOperations: vi.fn().mockResolvedValue(page([operation])),
    getBackupOperation: vi.fn<BackupApi['getBackupOperation']>().mockResolvedValue({
      data: { ...operation, restore_execution_id: 'execution-1', state: 'EXECUTED', phase: 'COMPLETED', terminal: true, next_action: 'NONE' },
      meta: { request_id: 'operation' },
    }),
    listBackupMaintenanceRuns: vi.fn().mockResolvedValue(page([])),
    getBackupMaintenanceRun: vi.fn().mockRejectedValue(new Error('unused')),
    createBackupMaintenanceRun: vi.fn().mockRejectedValue(new Error('unused')),
    advanceBackupMaintenanceRun: vi.fn().mockRejectedValue(new Error('unused')),
    createBackupRestorePlan: vi.fn().mockResolvedValue(planResponse),
    getBackupRestorePlan: vi.fn().mockResolvedValue(planResponse),
    executeBackupRestorePlan: vi.fn().mockResolvedValue(executionResponse),
    getBackupRestoreExecution: vi.fn().mockResolvedValue(executionResponse),
    createBackupPrunePlan: vi.fn().mockRejectedValue(new Error('unused')),
    getBackupPrunePlan: vi.fn().mockRejectedValue(new Error('unused')),
    executeBackupPrunePlan: vi.fn().mockRejectedValue(new Error('unused')),
    getBackupPruneExecution: vi.fn().mockRejectedValue(new Error('unused')),
    ...overrides,
  }
}

function makeFileApi(overrides: Partial<FileMetadataApi> = {}): FileMetadataApi {
  const getNode = vi.fn<FileMetadataApi['getLiveNode']>().mockImplementation(async (nodeId) => {
    const node = nodeId === rootNode.id ? rootNode : folderNode
    const response: LiveNodeResponse = { data: node, meta: { request_id: 'node' } }
    return response
  })
  return {
    listLibraries: vi.fn().mockResolvedValue(page([libraryOne, libraryTwo])),
    listLibraryChildren: vi.fn().mockResolvedValue(page([folderNode, fileNode])),
    getLiveNode: getNode,
    ...overrides,
  }
}

function renderRestore(
  path: string,
  backupApi: BackupApi,
  fileApi = makeFileApi(),
  apis = authenticatedApis(),
) {
  return render(
    <App
      router="memory"
      initialEntries={[path]}
      authApi={apis.authApi}
      bootstrapApi={apis.bootstrapApi}
      backupApi={backupApi}
      fileApi={fileApi}
    />,
  )
}

function conflictError(code = 'destination_conflict', status = 409): ApiRequestError {
  const payload: ApiErrorPayload = {
    code,
    message: 'internal conflict detail',
    request_id: 'restore-conflict',
    retryable: false,
  }
  return new ApiRequestError(status, payload)
}

function planPath(): string {
  return '/backups/set-1/restore/plan-1'
}

describe('restore workflow', () => {
  beforeEach(() => mutationRecoveryStore.clear())

  it('[auth] restores an unauthenticated deep link by GET without flashing or creating', async () => {
    const getPlan = vi.fn<BackupApi['getBackupRestorePlan']>().mockResolvedValue(planResponse)
    const createPlan = vi.fn<BackupApi['createBackupRestorePlan']>().mockResolvedValue(planResponse)
    const executePlan = vi.fn<BackupApi['executeBackupRestorePlan']>().mockResolvedValue(executionResponse)
    const backupApi = makeBackupApi({
      getBackupRestorePlan: getPlan,
      createBackupRestorePlan: createPlan,
      executeBackupRestorePlan: executePlan,
    })
    const authenticated = authenticatedApis()
    let rejectSession!: (error: unknown) => void
    const delayedSession = new Promise<AuthSessionResponse>((_resolve, reject) => {
      rejectSession = reject
    })
    const getCurrentSession = vi.fn(() => delayedSession)
    const authApi: AuthApi = {
      ...authenticated.authApi,
      getCurrentSession,
      login: vi.fn().mockResolvedValue(session),
    }
    render(
      <App
        router="memory"
        initialEntries={[`${planPath()}?kind=RESTORE#activity`]}
        authApi={authApi}
        bootstrapApi={authenticated.bootstrapApi}
        backupApi={backupApi}
        fileApi={makeFileApi()}
      />,
    )

    expect(await screen.findByRole('heading', { name: 'Checking your session' })).toBeInTheDocument()
    await waitFor(() => expect(getCurrentSession).toHaveBeenCalledOnce())
    expect(screen.queryByRole('heading', { name: 'Review restore plan' })).not.toBeInTheDocument()
    expect(getPlan).not.toHaveBeenCalled()
    await act(async () => {
      rejectSession(new ApiRequestError(401, {
        code: 'authentication_failed',
        message: 'expired',
        request_id: 'bootstrap-auth',
        retryable: false,
      }))
    })
    expect(await screen.findByRole('heading', { name: 'Sign in' })).toHaveFocus()
    fireEvent.change(screen.getByLabelText('Login identifier'), { target: { value: 'alice' } })
    fireEvent.change(screen.getByLabelText('Login key'), { target: { value: 'alice-key' } })
    fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'password' } })
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }))

    expect(await screen.findByRole('heading', { name: 'Review restore plan' })).toHaveFocus()
    expect(getPlan).toHaveBeenCalled()
    expect(createPlan).not.toHaveBeenCalled()
    expect(executePlan).not.toHaveBeenCalled()
  })

  it.each(['EXPIRED', 'BUILDING', 'FAILED'] as const)('does not offer a workflow for %s sources', async (state) => {
    const unavailableSnapshot: BackupSnapshot = { ...snapshot, state }
    const createPlan = vi.fn<BackupApi['createBackupRestorePlan']>().mockResolvedValue(planResponse)
    const backupApi = makeBackupApi({
      getBackupSnapshot: vi.fn().mockResolvedValue({ data: unavailableSnapshot, meta: { request_id: 'snapshot' } }),
      createBackupRestorePlan: createPlan,
    })
    renderRestore('/backups/set-1/snapshots/snapshot-1/restore', backupApi)

    expect(await screen.findByRole('heading', { name: 'This snapshot is not available for restore' })).toBeInTheDocument()
    expect(createPlan).not.toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: 'Create restore plan' })).not.toBeInTheDocument()
  })

  it('does not create a plan on render and uses the submitted destination to enter review', async () => {
    const createPlan = vi.fn<BackupApi['createBackupRestorePlan']>().mockResolvedValue(planResponse)
    const getPlan = vi.fn<BackupApi['getBackupRestorePlan']>().mockResolvedValue(planResponse)
    const backupApi = makeBackupApi({ createBackupRestorePlan: createPlan, getBackupRestorePlan: getPlan })
    renderRestore('/backups/set-1/snapshots/snapshot-1/restore', backupApi)

    expect(await screen.findByRole('heading', { name: 'Restore a snapshot' })).toBeInTheDocument()
    await screen.findByRole('option', { name: 'Personal files' })
    expect(createPlan).not.toHaveBeenCalled()

    fireEvent.change(screen.getByLabelText('Destination name'), { target: { value: 'Restored files' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create restore plan' }))

    await waitFor(() => expect(createPlan).toHaveBeenCalledOnce())
    expect(createPlan.mock.calls[0]?.[0]).toBe('snapshot-1')
    expect(createPlan.mock.calls[0]?.[1]).toEqual({
      target_library_id: 'library-1',
      target_parent_node_id: 'root-1',
      destination_name: 'Restored files',
    })
    expect(createPlan.mock.calls[0]?.[2]).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    expect(await screen.findByRole('heading', { name: 'Review restore plan', level: 2 })).toBeInTheDocument()
    expect(getPlan).toHaveBeenCalledWith('plan-1', expect.objectContaining({ signal: expect.any(AbortSignal) }))
    expect(backupApi.executeBackupRestorePlan).not.toHaveBeenCalled()
    expect(screen.getByRole('heading', { name: 'Restore from', level: 3 })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Restore to', level: 3 })).toBeInTheDocument()
    expect(screen.getByText('Restored files')).toBeInTheDocument()
  })

  it('[auth] preserves a restore-plan key across 401 recovery and requires explicit retry', async () => {
    const expired = new ApiRequestError(401, {
      code: 'authentication_failed',
      message: 'expired',
      request_id: 'restore-auth',
      retryable: false,
    })
    const createPlan = vi
      .fn<BackupApi['createBackupRestorePlan']>()
      .mockRejectedValueOnce(expired)
      .mockResolvedValueOnce(planResponse)
    const backupApi = makeBackupApi({ createBackupRestorePlan: createPlan })
    const auth = authenticatedApis()
    const getCurrentSession = vi.fn().mockResolvedValue(session)
    const authApi: AuthApi = { ...auth.authApi, getCurrentSession }
    renderRestore(
      '/backups/set-1/snapshots/snapshot-1/restore',
      backupApi,
      makeFileApi(),
      { ...auth, authApi },
    )

    await screen.findByRole('option', { name: 'Personal files' })
    fireEvent.change(screen.getByLabelText('Destination name'), {
      target: { value: 'Recovered restore' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Create restore plan' }))

    await waitFor(() => expect(getCurrentSession).toHaveBeenCalledTimes(2))
    const retrySafely = await screen.findByRole('button', { name: 'Retry safely' })
    expect(createPlan).toHaveBeenCalledOnce()
    const originalKey = createPlan.mock.calls[0]?.[2]
    expect(authApi.getCsrfToken).toHaveBeenCalledOnce()

    fireEvent.click(retrySafely)
    expect(await screen.findByRole('heading', { name: 'Review restore plan', level: 1 })).toBeInTheDocument()
    expect(createPlan).toHaveBeenCalledTimes(2)
    expect(createPlan.mock.calls[1]?.[2]).toBe(originalKey)
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('validates the explicit destination name before planning', async () => {
    const createPlan = vi.fn<BackupApi['createBackupRestorePlan']>().mockResolvedValue(planResponse)
    const backupApi = makeBackupApi({ createBackupRestorePlan: createPlan })
    renderRestore('/backups/set-1/snapshots/snapshot-1/restore', backupApi)
    await screen.findByRole('heading', { name: 'Restore a snapshot' })
    await screen.findByRole('option', { name: 'Personal files' })

    const nameInput = screen.getByLabelText('Destination name')
    const createButton = screen.getByRole('button', { name: 'Create restore plan' })
    expect(createButton).toBeDisabled()

    fireEvent.change(nameInput, { target: { value: '   ' } })
    expect(createButton).toBeDisabled()
    fireEvent.change(nameInput, { target: { value: 'x'.repeat(1_025) } })
    expect(createButton).toBeDisabled()
    expect(createPlan).not.toHaveBeenCalled()
  })

  it('retries an ambiguous plan request with the same key and a changed flow gets a fresh key', async () => {
    const createPlan = vi
      .fn<BackupApi['createBackupRestorePlan']>()
      .mockRejectedValueOnce(new Error('response lost'))
      .mockResolvedValueOnce(planResponse)
      .mockResolvedValueOnce({ ...planResponse, data: { ...plannedPlan, restore_plan_id: 'plan-2' } })
    const backupApi = makeBackupApi({ createBackupRestorePlan: createPlan })
    renderRestore('/backups/set-1/snapshots/snapshot-1/restore', backupApi)
    await screen.findByRole('heading', { name: 'Restore a snapshot' })
    await screen.findByRole('option', { name: 'Personal files' })

    fireEvent.change(screen.getByLabelText('Destination name'), { target: { value: 'Restored files' } })
    const createButton = screen.getByRole('button', { name: 'Create restore plan' })
    await waitFor(() => expect(createButton).toBeEnabled())
    fireEvent.click(createButton)
    fireEvent.click(await screen.findByRole('button', { name: 'Try the request again' }))
    await waitFor(() => expect(createPlan).toHaveBeenCalledTimes(2))
    expect(createPlan.mock.calls[0]?.[2]).toBe(createPlan.mock.calls[1]?.[2])
    expect(await screen.findByRole('heading', { name: 'Review restore plan', level: 2 })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Create a new plan' }))
    expect(await screen.findByRole('heading', { name: 'Restore a snapshot' })).toBeInTheDocument()
    await screen.findByRole('option', { name: 'Personal files' })
    fireEvent.change(screen.getByLabelText('Destination name'), { target: { value: 'Another restore' } })
    const newCreateButton = screen.getByRole('button', { name: 'Create restore plan' })
    await waitFor(() => expect(newCreateButton).toBeEnabled())
    fireEvent.click(newCreateButton)
    await waitFor(() => expect(createPlan).toHaveBeenCalledTimes(3))
    expect(createPlan.mock.calls[1]?.[2]).not.toBe(createPlan.mock.calls[2]?.[2])
  })

  it('[recovery] recovers a lost restore-plan response end to end after remount with the exact request and key', async () => {
    let firstAttempt = true
    let originalKey: string | undefined
    const createPlan = vi.fn<BackupApi['createBackupRestorePlan']>().mockImplementation(async (sourceId, request, key) => {
      const pending = mutationRecoveryStore.all().find((record) => record.action_kind === 'create_restore_plan')
      expect(pending?.request_scope).toEqual({ backupSetId: 'set-1', snapshotId: 'snapshot-1' })
      expect(pending?.request).toEqual(request)
      expect(pending?.idempotency_key).toBe(key)
      expect(sourceId).toBe('snapshot-1')
      originalKey = key
      if (firstAttempt) {
        firstAttempt = false
        throw new Error('connection ended after send')
      }
      return planResponse
    })
    const backupApi = makeBackupApi({ createBackupRestorePlan: createPlan })
    const firstRender = renderRestore('/backups/set-1/snapshots/snapshot-1/restore', backupApi)
    await screen.findByRole('option', { name: 'Personal files' })
    fireEvent.change(screen.getByLabelText('Destination name'), { target: { value: 'Restored files' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create restore plan' }))

    expect(await screen.findByRole('heading', { name: 'Finish creating this restore plan' })).toBeInTheDocument()
    expect(createPlan).toHaveBeenCalledOnce()
    expect(mutationRecoveryStore.all()).toHaveLength(1)
    firstRender.unmount()

    renderRestore('/backups/set-1/snapshots/snapshot-1/restore', backupApi)
    expect(await screen.findByRole('heading', { name: 'Finish creating this restore plan' })).toBeInTheDocument()
    expect(createPlan).toHaveBeenCalledOnce()
    fireEvent.click(screen.getByRole('button', { name: 'Retry safely' }))

    expect(await screen.findByRole('heading', { name: 'Review restore plan', level: 2 })).toBeInTheDocument()
    expect(createPlan).toHaveBeenCalledTimes(2)
    expect(createPlan.mock.calls[1]?.[1]).toEqual(createPlan.mock.calls[0]?.[1])
    expect(createPlan.mock.calls[1]?.[2]).toBe(originalKey)
    expect(backupApi.getBackupRestorePlan).toHaveBeenCalledWith(
      'plan-1',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    )
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('keeps a collision in setup and never issues execution', async () => {
    const createPlan = vi.fn<BackupApi['createBackupRestorePlan']>().mockRejectedValue(conflictError())
    const executePlan = vi.fn<BackupApi['executeBackupRestorePlan']>().mockResolvedValue(executionResponse)
    const backupApi = makeBackupApi({ createBackupRestorePlan: createPlan, executeBackupRestorePlan: executePlan })
    renderRestore('/backups/set-1/snapshots/snapshot-1/restore', backupApi)
    await screen.findByRole('heading', { name: 'Restore a snapshot' })
    await screen.findByRole('option', { name: 'Personal files' })

    fireEvent.change(screen.getByLabelText('Destination name'), { target: { value: 'Existing name' } })
    const createButton = screen.getByRole('button', { name: 'Create restore plan' })
    await waitFor(() => expect(createButton).toBeEnabled())
    fireEvent.click(createButton)

    expect(await screen.findByRole('alert')).toHaveTextContent(/already in use/i)
    expect(screen.getByRole('heading', { name: 'Choose a destination' })).toBeInTheDocument()
    expect(executePlan).not.toHaveBeenCalled()
    await waitFor(() => expect(screen.getByLabelText('Destination name')).toHaveFocus())
  })

  it('[recovery] keeps review read-only, requires explicit execution, and prevents double-click requests', async () => {
    let resolveExecution: ((response: BackupRestoreExecutionResponse) => void) | undefined
    const executePlan = vi.fn<BackupApi['executeBackupRestorePlan']>().mockImplementation(
      (planId, key) => new Promise((resolve) => {
        const pending = mutationRecoveryStore.all().find((record) => record.action_kind === 'execute_restore_plan')
        expect(pending?.request_scope).toEqual({
          backupSetId: 'set-1',
          snapshotId: 'snapshot-1',
          restorePlanId: 'plan-1',
        })
        expect(pending?.request).toEqual({})
        expect(pending?.idempotency_key).toBe(key)
        expect(planId).toBe('plan-1')
        resolveExecution = resolve
      }),
    )
    const getPlan = vi
      .fn<BackupApi['getBackupRestorePlan']>()
      .mockResolvedValueOnce(planResponse)
      .mockResolvedValue({ ...planResponse, data: executedPlan })
    const backupApi = makeBackupApi({ executeBackupRestorePlan: executePlan, getBackupRestorePlan: getPlan })
    renderRestore(planPath(), backupApi)

    await screen.findByRole('heading', { name: 'Review restore plan', level: 2 })
    const destinationName = screen.getByText('Restored files')
    expect(destinationName.tagName).not.toBe('INPUT')
    const executeButton = await screen.findByRole('button', { name: 'Restore to this folder' })
    await waitFor(() => expect(executeButton).toBeEnabled())
    expect(executePlan).not.toHaveBeenCalled()

    fireEvent.click(executeButton)
    fireEvent.click(executeButton)
    expect(executePlan).toHaveBeenCalledOnce()
    expect(executePlan.mock.calls[0]?.[0]).toBe('plan-1')
    expect(executePlan.mock.calls[0]?.[1]).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    resolveExecution?.(executionResponse)

    expect(await screen.findByRole('heading', { name: 'Restore completed', level: 2 })).toBeInTheDocument()
    expect(backupApi.getBackupRestoreExecution).toHaveBeenCalledWith(
      'execution-1',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    )
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('retries an ambiguous execution with the same key and recovers the canonical receipt', async () => {
    const executePlan = vi
      .fn<BackupApi['executeBackupRestorePlan']>()
      .mockRejectedValueOnce(new Error('response lost'))
      .mockResolvedValueOnce(executionResponse)
    const backupApi = makeBackupApi({ executeBackupRestorePlan: executePlan })
    renderRestore(planPath(), backupApi)
    const executeButton = await screen.findByRole('button', { name: 'Restore to this folder' })
    await waitFor(() => expect(executeButton).toBeEnabled())

    fireEvent.click(executeButton)
    expect(await screen.findByRole('button', { name: 'Try the request again' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Restore to this folder' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Try the request again' }))
    await waitFor(() => expect(executePlan).toHaveBeenCalledTimes(2))
    expect(executePlan.mock.calls[0]?.[1]).toBe(executePlan.mock.calls[1]?.[1])
    expect(await screen.findByRole('heading', { name: 'Restore completed', level: 2 })).toBeInTheDocument()
  })

  it('[recovery] GET-reconciles a planned execution retry and only POSTs after explicit same-key retry', async () => {
    const key = '00000000-0000-7000-8000-000000000157'
    mutationRecoveryStore.add({
      action_kind: 'execute_restore_plan',
      idempotency_key: key,
      request_scope: { backupSetId: 'set-1', snapshotId: 'snapshot-1', restorePlanId: 'plan-1' },
      request: {},
    })
    const executePlan = vi.fn<BackupApi['executeBackupRestorePlan']>().mockImplementation(async (planId, requestKey) => {
      expect(mutationRecoveryStore.all().some((record) => record.idempotency_key === requestKey)).toBe(true)
      expect(planId).toBe('plan-1')
      return executionResponse
    })
    const backupApi = makeBackupApi({ executeBackupRestorePlan: executePlan })
    renderRestore(planPath(), backupApi)

    expect(await screen.findByRole('heading', { name: 'Finish this restore' })).toBeInTheDocument()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Retry safely' })).toBeEnabled())
    expect(backupApi.getBackupRestorePlan).toHaveBeenCalled()
    expect(executePlan).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Retry safely' }))

    expect(await screen.findByRole('heading', { name: 'Restore completed', level: 2 })).toBeInTheDocument()
    expect(executePlan).toHaveBeenCalledWith('plan-1', key, expect.objectContaining({ signal: expect.any(AbortSignal) }))
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('treats a late collision or target-basis change as stale and offers no automatic replan', async () => {
    const executePlan = vi.fn<BackupApi['executeBackupRestorePlan']>().mockRejectedValue(conflictError('target_changed'))
    const getPlan = vi
      .fn<BackupApi['getBackupRestorePlan']>()
      .mockResolvedValueOnce(planResponse)
      .mockResolvedValue({ data: stalePlan, meta: { request_id: 'stale-plan' } })
    const backupApi = makeBackupApi({ executeBackupRestorePlan: executePlan, getBackupRestorePlan: getPlan })
    renderRestore(planPath(), backupApi)
    const executeButton = await screen.findByRole('button', { name: 'Restore to this folder' })
    await waitFor(() => expect(executeButton).toBeEnabled())

    fireEvent.click(executeButton)

    expect(await screen.findByText(/destination changed after this restore plan/i)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Restore to this folder' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Create a new plan' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Restore completed', level: 2 })).not.toBeInTheDocument()
  })

  it('[recovery] clears an impossible reload retry when GET says the restore plan is STALE', async () => {
    mutationRecoveryStore.add({
      action_kind: 'execute_restore_plan',
      idempotency_key: '00000000-0000-7000-8000-000000000657',
      request_scope: { backupSetId: 'set-1', snapshotId: 'snapshot-1', restorePlanId: 'plan-1' },
      request: {},
    })
    const executePlan = vi.fn<BackupApi['executeBackupRestorePlan']>()
    renderRestore(planPath(), makeBackupApi({
      getBackupRestorePlan: vi.fn().mockResolvedValue({ data: stalePlan, meta: { request_id: 'stale' } }),
      executeBackupRestorePlan: executePlan,
    }))

    expect(await screen.findByText(/destination changed after this restore plan/i)).toBeInTheDocument()
    expect(executePlan).not.toHaveBeenCalled()
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[recovery] reloads an executed plan through the canonical operation and receipt', async () => {
    mutationRecoveryStore.add({
      action_kind: 'execute_restore_plan',
      idempotency_key: '00000000-0000-7000-8000-000000000257',
      request_scope: { backupSetId: 'set-1', snapshotId: 'snapshot-1', restorePlanId: 'plan-1' },
      request: {},
    })
    const operation: BackupRestoreOperation = operationForPlan(executedPlan)
    const getOperation = vi.fn<BackupApi['getBackupOperation']>().mockResolvedValue({
      data: operation,
      meta: { request_id: 'operation' },
    })
    const getPlan = vi.fn<BackupApi['getBackupRestorePlan']>().mockResolvedValue({
      data: executedPlan,
      meta: { request_id: 'executed-plan' },
    })
    const getExecution = vi.fn<BackupApi['getBackupRestoreExecution']>().mockResolvedValue(executionResponse)
    const backupApi = makeBackupApi({ getBackupRestorePlan: getPlan, getBackupOperation: getOperation, getBackupRestoreExecution: getExecution })
    renderRestore(planPath(), backupApi)

    expect(await screen.findByRole('heading', { name: 'Restore completed', level: 2 })).toBeInTheDocument()
    expect(getOperation).toHaveBeenCalledWith('RESTORE', 'plan-1', expect.objectContaining({ signal: expect.any(AbortSignal) }))
    expect(getExecution).toHaveBeenCalledWith('execution-1', expect.objectContaining({ signal: expect.any(AbortSignal) }))
    expect(backupApi.executeBackupRestorePlan).not.toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: 'Restore to this folder' })).not.toBeInTheDocument()
    expect(await screen.findByText(/The source snapshot remains available/i)).toBeInTheDocument()
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('maps a concealed plan 404 to safe copy without exposing ownership details', async () => {
    const missingPlan = vi.fn<BackupApi['getBackupRestorePlan']>().mockRejectedValue(conflictError('not_found', 404))
    const backupApi = makeBackupApi({ getBackupRestorePlan: missingPlan })
    renderRestore(planPath(), backupApi)

    expect(await screen.findByRole('heading', { name: 'Restore plan not available' })).toBeInTheDocument()
    expect(screen.getByRole('alert')).toHaveTextContent('Restore plan not available.')
    expect(screen.queryByText(/owner|foreign|belongs/i)).not.toBeInTheDocument()
  })

  it('uses the server-confirmed target refresh after a successful execution', async () => {
    const restoredRootItem: LiveNodeResource = {
      ...folderNode,
      id: 'restored-root-item',
      attributes: { ...folderNode.attributes, name: 'Restored copy (server-confirmed)' },
    }
    const listTargetChildren = vi.fn().mockResolvedValue(page([restoredRootItem, fileNode]))
    const listOperations = vi.fn().mockResolvedValue(page([]))
    const backupApi = makeBackupApi({ listBackupOperations: listOperations })
    const fileApi = makeFileApi({ listLibraryChildren: listTargetChildren })
    renderRestore(planPath(), backupApi, fileApi)
    const executeButton = await screen.findByRole('button', { name: 'Restore to this folder' })
    await waitFor(() => expect(executeButton).toBeEnabled())

    fireEvent.click(executeButton)
    expect(await screen.findByRole('heading', { name: 'Restore completed', level: 2 })).toBeInTheDocument()
    expect(await screen.findByRole('heading', { name: 'Destination folder after restore' })).toBeInTheDocument()
    expect(screen.getByText('Restored copy (server-confirmed)')).toBeInTheDocument()
    await waitFor(() => expect(listTargetChildren).toHaveBeenCalledWith(
      'library-1',
      expect.objectContaining({ parentId: 'root-1', limit: 50, signal: expect.any(AbortSignal) }),
    ))
    expect(listOperations).toHaveBeenCalledWith('set-1', { kind: 'RESTORE', limit: 50 })
  })
})
