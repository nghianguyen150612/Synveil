import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AuthApi, AuthSessionResponse } from '../../api/auth'
import {
  createBackupSet as sendCreateBackupSet,
  type BackupApi,
} from '../../api/backups'
import { ApiClient } from '../../api/client'
import type { BootstrapApi, BootstrapStatusResponse } from '../../api/bootstrap'
import { ApiRequestError } from '../../api/errors'
import {
  bindMutationRecoveryPrincipal,
  mutationRecoveryStore,
} from '../../api/mutationRecovery'
import type { FileMetadataApi, LiveLibraryResource } from '../../api/files'
import { App } from '../../app/App'

const backupSet = {
  backup_set_id: 'set-public-1',
  name: 'Family files',
  library_id: 'library-1',
  state: 'ACTIVE' as const,
  created_at: '2026-08-01T09:00:00Z',
  updated_at: '2026-09-01T09:00:00Z',
}

const snapshot = {
  snapshot_id: 'snapshot-public-1',
  backup_set_id: backupSet.backup_set_id,
  library_id: backupSet.library_id,
  state: 'COMPLETED' as const,
  created_at: '2026-08-31T09:00:00Z',
  committed_at: '2026-08-31T09:01:00Z',
  logical_node_count: '8',
  content_reference_count: '5',
  object_id: 'physical-object-must-not-render',
  storage_key: 'physical-storage-key-must-not-render',
}

const operation = {
  operation_kind: 'MAINTENANCE' as const,
  operation_id: 'operation-1',
  backup_set_id: backupSet.backup_set_id,
  state: 'COMPLETED' as const,
  phase: 'COMPLETED' as const,
  progress: { completed_steps: 3, total_steps: 3 },
  terminal: true,
  next_action: 'NONE' as const,
  created_at: '2026-08-31T09:00:00Z',
  last_transition_at: '2026-08-31T09:03:00Z',
}

const policy = {
  data: {
    policy_revision_id: 'policy-1',
    backup_set_id: backupSet.backup_set_id,
    revision_number: '1',
    keep_latest_completed: '4',
    expire_after_seconds: '2592000',
    created_at: '2026-08-01T09:00:00Z',
  },
  meta: { request_id: 'policy' },
}

const library: LiveLibraryResource = {
  id: 'library-1',
  type: 'library',
  revision: '1',
  attributes: {
    name: 'Personal library',
    root_node_id: 'root-1',
    status: 'ACTIVE',
    created_at: '2026-08-01T09:00:00Z',
    updated_at: '2026-08-01T09:00:00Z',
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

const secondUserSession: AuthSessionResponse = {
  data: {
    ...session.data,
    user_id: '00000000-0000-7000-8000-000000000099',
    session_id: '00000000-0000-7000-8000-000000000098',
  },
  meta: { request_id: 'session-user-b' },
}

const bootstrap: BootstrapStatusResponse = {
  data: { setup_required: false },
  meta: { request_id: 'bootstrap' },
}

function page<T>(data: readonly T[], hasMore = false, nextCursor?: string) {
  return {
    data,
    page: { has_more: hasMore, ...(nextCursor ? { next_cursor: nextCursor } : {}) },
    meta: { request_id: 'page' },
  }
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

function makeBackupApi(overrides: Partial<BackupApi> = {}): BackupApi {
  return {
    listBackupSets: vi.fn().mockResolvedValue(page([backupSet])),
    getBackupSet: vi.fn().mockResolvedValue({ data: backupSet, meta: { request_id: 'set' } }),
    createBackupSet: vi.fn().mockResolvedValue({ data: backupSet, meta: { request_id: 'create' } }),
    listBackupSnapshots: vi.fn().mockResolvedValue(page([snapshot])),
    getBackupSnapshot: vi.fn().mockResolvedValue({ data: snapshot, meta: { request_id: 'snapshot' } }),
    listBackupSnapshotNodes: vi.fn().mockResolvedValue(page([])),
    getBackupRetentionPolicy: vi.fn().mockResolvedValue(policy),
    configureBackupRetentionPolicy: vi.fn().mockResolvedValue(policy),
    listBackupOperations: vi.fn().mockResolvedValue(page([operation])),
    getBackupOperation: vi.fn().mockRejectedValue(new Error('unused')),
    listBackupMaintenanceRuns: vi.fn().mockResolvedValue(page([])),
    getBackupMaintenanceRun: vi.fn().mockRejectedValue(new Error('unused')),
    createBackupMaintenanceRun: vi.fn().mockRejectedValue(new Error('unused')),
    advanceBackupMaintenanceRun: vi.fn().mockRejectedValue(new Error('unused')),
    createBackupRestorePlan: vi.fn().mockRejectedValue(new Error('unused')),
    getBackupRestorePlan: vi.fn().mockRejectedValue(new Error('unused')),
    executeBackupRestorePlan: vi.fn().mockRejectedValue(new Error('unused')),
    getBackupRestoreExecution: vi.fn().mockRejectedValue(new Error('unused')),
    createBackupPrunePlan: vi.fn().mockRejectedValue(new Error('unused')),
    getBackupPrunePlan: vi.fn().mockRejectedValue(new Error('unused')),
    executeBackupPrunePlan: vi.fn().mockRejectedValue(new Error('unused')),
    getBackupPruneExecution: vi.fn().mockRejectedValue(new Error('unused')),
    ...overrides,
  }
}

function makeFileApi(): FileMetadataApi {
  return {
    listLibraries: vi.fn().mockResolvedValue(page([library])),
    listLibraryChildren: vi.fn().mockResolvedValue(page([])),
    getLiveNode: vi.fn().mockRejectedValue(new Error('unused')),
  }
}

function renderOverview(backupApi: BackupApi, fileApi = makeFileApi()) {
  const auth = authApis()
  return render(
    <App
      router="memory"
      initialEntries={['/backups']}
      authApi={auth.authApi}
      bootstrapApi={auth.bootstrapApi}
      backupApi={backupApi}
      fileApi={fileApi}
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

describe('Backup Control Center overview', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    mutationRecoveryStore.clear()
    document.cookie = 'synveil_csrf=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/'
  })

  it('renders user-facing set cards with factual summaries and no physical identity', async () => {
    renderOverview(makeBackupApi())

    expect(await screen.findByRole('heading', { name: 'Backup Control Center', level: 1 })).toBeInTheDocument()
    expect(await screen.findByRole('heading', { name: 'Family files' })).toBeInTheDocument()
    expect(screen.getByText('Keep at least 4 completed backups')).toBeInTheDocument()
    expect(screen.getByText(/Maintenance · 3 of 3 steps completed/)).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Open backup set' })).toHaveAttribute('href', '/backups/set-public-1')
    expect(document.body).not.toHaveTextContent('physical-object-must-not-render')
    expect(document.body).not.toHaveTextContent('physical-storage-key-must-not-render')
    expect(document.body).not.toHaveTextContent(/%|ETA|time remaining/i)
  })

  it('shows the friendly empty state and creates a set once despite a rapid double click', async () => {
    const pending = deferred<Awaited<ReturnType<BackupApi['createBackupSet']>>>()
    const createSet = vi.fn<BackupApi['createBackupSet']>().mockReturnValue(pending.promise)
    const api = makeBackupApi({
      listBackupSets: vi.fn().mockResolvedValue(page([])),
      createBackupSet: createSet,
    })
    renderOverview(api)

    expect(await screen.findByRole('heading', { name: 'No backups yet.' })).toBeInTheDocument()
    await screen.findByRole('option', { name: 'Personal library' })
    fireEvent.change(screen.getByLabelText('Backup set name'), { target: { value: 'Laptop files' } })
    const createButton = screen.getByRole('button', { name: 'Create backup set' })
    fireEvent.click(createButton)
    fireEvent.click(createButton)

    expect(createSet).toHaveBeenCalledOnce()
    expect(createSet.mock.calls[0]?.[0]).toEqual({ library_id: 'library-1', name: 'Laptop files' })
    expect(createSet.mock.calls[0]?.[1]).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    expect(createButton).toBeDisabled()

    pending.resolve({ data: { ...backupSet, backup_set_id: 'new-set', name: 'Laptop files' }, meta: { request_id: 'create' } })
    expect(await screen.findByRole('heading', { name: 'Backup set details' })).toBeInTheDocument()
  })

  it('retries an ambiguous create response with the same UUIDv7 key', async () => {
    const created = { data: { ...backupSet, backup_set_id: 'new-set' }, meta: { request_id: 'create' } }
    const createSet = vi
      .fn<BackupApi['createBackupSet']>()
      .mockRejectedValueOnce(new Error('response lost'))
      .mockResolvedValueOnce(created)
    const api = makeBackupApi({ listBackupSets: vi.fn().mockResolvedValue(page([])), createBackupSet: createSet })
    renderOverview(api)
    await screen.findByRole('option', { name: 'Personal library' })
    fireEvent.change(screen.getByLabelText('Backup set name'), { target: { value: 'Laptop files' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create backup set' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Try the request again' }))

    await waitFor(() => expect(createSet).toHaveBeenCalledTimes(2))
    expect(createSet.mock.calls[0]?.[1]).toBe(createSet.mock.calls[1]?.[1])
  })

  it('[recovery] persists before create-set POST, survives remount without auto-POST, and clears after canonical retry', async () => {
    let firstCall = true
    let capturedKey: string | undefined
    const createSet = vi.fn<BackupApi['createBackupSet']>().mockImplementation(async (request, key) => {
      const pending = mutationRecoveryStore.all().find((record) => record.action_kind === 'create_backup_set')
      expect(pending).toBeDefined()
      expect(pending?.request).toEqual(request)
      expect(pending?.idempotency_key).toBe(key)
      capturedKey = key
      if (firstCall) {
        firstCall = false
        throw new Error('response lost after send')
      }
      return { data: { ...backupSet, backup_set_id: 'canonical-set', name: request.name }, meta: { request_id: 'create' } }
    })
    const api = makeBackupApi({ listBackupSets: vi.fn().mockResolvedValue(page([])), createBackupSet: createSet })
    const firstRender = renderOverview(api)
    await screen.findByRole('option', { name: 'Personal library' })
    fireEvent.change(screen.getByLabelText('Backup set name'), { target: { value: 'Laptop files' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create backup set' }))

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Finish creating this backup set' })).toBeInTheDocument()
    })
    expect(createSet).toHaveBeenCalledOnce()
    expect(mutationRecoveryStore.all()).toHaveLength(1)
    firstRender.unmount()

    renderOverview(api)
    expect(await screen.findByRole('heading', { name: 'Finish creating this backup set' })).toBeInTheDocument()
    expect(createSet).toHaveBeenCalledOnce()
    fireEvent.click(screen.getByRole('button', { name: 'Retry safely' }))

    expect(await screen.findByRole('heading', { name: 'Backup set details' })).toBeInTheDocument()
    expect(createSet).toHaveBeenCalledTimes(2)
    expect(createSet.mock.calls[1]?.[0]).toEqual({ library_id: 'library-1', name: 'Laptop files' })
    expect(createSet.mock.calls[1]?.[1]).toBe(capturedKey)
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[recovery] discards only the local create retry and performs no POST', async () => {
    mutationRecoveryStore.add({
      action_kind: 'create_backup_set',
      idempotency_key: '00000000-0000-7000-8000-000000000057',
      request_scope: {},
      request: { library_id: 'library-1', name: 'Possibly created' },
    })
    const createSet = vi.fn<BackupApi['createBackupSet']>()
    renderOverview(makeBackupApi({ createBackupSet: createSet }))

    expect(await screen.findByText(/Discarding this retry does not prove/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Discard local retry' }))
    expect(createSet).not.toHaveBeenCalled()
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[recovery] remains usable without sessionStorage and does not claim reload recovery', async () => {
    const availableStorage = globalThis.sessionStorage
    const storageError = () => {
      throw new DOMException('storage denied', 'SecurityError')
    }
    const unavailableStorage = {
      get length() { return 0 },
      clear: storageError,
      getItem: storageError,
      key: () => null,
      removeItem: storageError,
      setItem: storageError,
    } satisfies Storage
    Object.defineProperty(globalThis, 'sessionStorage', {
      configurable: true,
      value: unavailableStorage,
    })
    try {
      expect(mutationRecoveryStore.isAvailable).toBe(false)
      const createSet = vi
        .fn<BackupApi['createBackupSet']>()
        .mockRejectedValueOnce(new Error('response lost'))
        .mockResolvedValueOnce({
          data: { ...backupSet, backup_set_id: 'storage-free-set' },
          meta: { request_id: 'create' },
        })
      renderOverview(makeBackupApi({ listBackupSets: vi.fn().mockResolvedValue(page([])), createBackupSet: createSet }))
      await screen.findByRole('option', { name: 'Personal library' })
      fireEvent.change(screen.getByLabelText('Backup set name'), { target: { value: 'Still works' } })
      fireEvent.click(screen.getByRole('button', { name: 'Create backup set' }))

      const localRetry = await screen.findByRole('button', { name: 'Try the request again' })
      expect(screen.queryByRole('heading', { name: 'Finish creating this backup set' })).not.toBeInTheDocument()
      fireEvent.click(localRetry)
      expect(await screen.findByRole('heading', { name: 'Backup set details' })).toBeInTheDocument()
      expect(createSet).toHaveBeenCalledTimes(2)
      expect(createSet.mock.calls[0]?.[1]).toBe(createSet.mock.calls[1]?.[1])
      expect(screen.queryByText(/browser did not receive the result/)).not.toBeInTheDocument()
    } finally {
      Object.defineProperty(globalThis, 'sessionStorage', {
        configurable: true,
        value: availableStorage,
      })
    }
  })

  it('[recovery] retains one key across temporary 401 and reads a fresh CSRF cookie on retry', async () => {
    const requests: RequestInit[] = []
    const fetchImpl = vi.fn<typeof fetch>().mockImplementation(async (_input, init) => {
      requests.push(init ?? {})
      if (requests.length === 1) {
        return new Response(JSON.stringify({
          error: {
            code: 'unauthorized',
            message: 'expired',
            request_id: 'first',
            retryable: false,
          },
        }), { status: 401, headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({
        data: { ...backupSet, backup_set_id: 'csrf-set', name: 'CSRF retry' },
        meta: { request_id: 'second' },
      }), { status: 201, headers: { 'Content-Type': 'application/json' } })
    })
    const client = new ApiClient({ fetchImpl })
    const createSet: BackupApi['createBackupSet'] = (request, key, options) =>
      sendCreateBackupSet(request, key, options, client)
    document.cookie = 'synveil_csrf=csrf-old; path=/'
    renderOverview(makeBackupApi({ listBackupSets: vi.fn().mockResolvedValue(page([])), createBackupSet: createSet }))
    await screen.findByRole('option', { name: 'Personal library' })
    fireEvent.change(screen.getByLabelText('Backup set name'), { target: { value: 'CSRF retry' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create backup set' }))
    await screen.findByRole('heading', { name: 'Finish creating this backup set' })

    const pending = mutationRecoveryStore.all()[0]
    expect(pending).toBeDefined()
    expect(JSON.stringify(pending)).not.toContain('csrf-old')
    document.cookie = 'synveil_csrf=csrf-new; path=/'
    fireEvent.click(screen.getByRole('button', { name: 'Retry safely' }))
    expect(await screen.findByRole('heading', { name: 'Backup set details' })).toBeInTheDocument()

    const firstHeaders = new Headers(requests[0]?.headers)
    const secondHeaders = new Headers(requests[1]?.headers)
    expect(firstHeaders.get('Idempotency-Key')).toBe(secondHeaders.get('Idempotency-Key'))
    expect(firstHeaders.get('X-CSRF-Token')).toBe('csrf-old')
    expect(secondHeaders.get('X-CSRF-Token')).toBe('csrf-new')
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[auth] treats CSRF-specific 403 as recoverable without logout or automatic POST replay', async () => {
    const csrfFailure = new ApiRequestError(403, {
      code: 'permission_denied',
      message: 'stale csrf',
      request_id: 'csrf-failure',
      retryable: false,
    })
    const createSet = vi
      .fn<BackupApi['createBackupSet']>()
      .mockRejectedValueOnce(csrfFailure)
      .mockResolvedValueOnce({ data: { ...backupSet, backup_set_id: 'csrf-recovered' }, meta: { request_id: 'created' } })
    const api = makeBackupApi({
      listBackupSets: vi.fn().mockResolvedValue(page([])),
      createBackupSet: createSet,
    })
    const apis = authApis()
    const getCurrentSession = vi.fn().mockResolvedValue(session)
    const authApi: AuthApi = { ...apis.authApi, getCurrentSession }
    render(
      <App
        router="memory"
        initialEntries={['/backups']}
        authApi={authApi}
        bootstrapApi={apis.bootstrapApi}
        backupApi={api}
        fileApi={makeFileApi()}
      />,
    )

    await screen.findByRole('option', { name: 'Personal library' })
    fireEvent.change(screen.getByLabelText('Backup set name'), { target: { value: 'CSRF retry' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create backup set' }))

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Finish creating this backup set' })).toBeInTheDocument()
    })
    expect(createSet).toHaveBeenCalledOnce()
    const originalKey = createSet.mock.calls[0]?.[1]
    expect(getCurrentSession).toHaveBeenCalledTimes(2)
    expect(authApi.getCsrfToken).toHaveBeenCalledOnce()
    expect(screen.queryByRole('heading', { name: 'Sign in' })).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Retry safely' }))
    expect(await screen.findByRole('heading', { name: 'Backup set details' })).toBeInTheDocument()
    expect(createSet).toHaveBeenCalledTimes(2)
    expect(createSet.mock.calls[1]?.[1]).toBe(originalKey)
  })

  it('shows a clear authentication-required state for a backup read failure', async () => {
    const payload = {
      code: 'unauthorized',
      message: 'raw server message',
      request_id: 'request-1',
      retryable: false,
    }
    renderOverview(makeBackupApi({
      listBackupSets: vi.fn().mockRejectedValue(new ApiRequestError(401, payload)),
    }))

    expect(await screen.findByRole('heading', { name: /sign in/i })).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent(/session has ended/i)
    expect(screen.queryByText('raw server message')).not.toBeInTheDocument()
  })

  it('[auth] single-flights concurrent summary 401s and reconstructs once from GETs', async () => {
    const secondSet = {
      ...backupSet,
      backup_set_id: 'set-public-2',
      name: 'Work files',
      library_id: 'library-2',
    }
    const unauthorized = () => new ApiRequestError(401, {
      code: 'authentication_failed',
      message: 'expired',
      request_id: 'summary-auth',
      retryable: false,
    })
    const getPolicy = vi
      .fn<BackupApi['getBackupRetentionPolicy']>()
      .mockRejectedValueOnce(unauthorized())
      .mockRejectedValueOnce(unauthorized())
      .mockResolvedValue(policy)
    const listSnapshots = vi
      .fn<BackupApi['listBackupSnapshots']>()
      .mockRejectedValueOnce(unauthorized())
      .mockRejectedValueOnce(unauthorized())
      .mockResolvedValue(page([snapshot]))
    const listOperations = vi
      .fn<BackupApi['listBackupOperations']>()
      .mockRejectedValueOnce(unauthorized())
      .mockRejectedValueOnce(unauthorized())
      .mockResolvedValue(page([operation]))
    const api = makeBackupApi({
      listBackupSets: vi.fn().mockResolvedValue(page([backupSet, secondSet])),
      getBackupRetentionPolicy: getPolicy,
      listBackupSnapshots: listSnapshots,
      listBackupOperations: listOperations,
    })
    const apis = authApis()
    const getCurrentSession = vi.fn().mockResolvedValue(session)
    const authApi: AuthApi = { ...apis.authApi, getCurrentSession }
    render(
      <App
        router="memory"
        initialEntries={['/backups']}
        authApi={authApi}
        bootstrapApi={apis.bootstrapApi}
        backupApi={api}
        fileApi={makeFileApi()}
      />,
    )

    expect(await screen.findByRole('heading', { name: 'Work files' })).toBeInTheDocument()
    expect(getCurrentSession).toHaveBeenCalledTimes(2)
    expect(authApi.getCsrfToken).toHaveBeenCalledOnce()
    expect(api.listBackupSets).toHaveBeenCalledTimes(2)
    expect(getPolicy).toHaveBeenCalledTimes(4)
    expect(listSnapshots).toHaveBeenCalledTimes(4)
    expect(listOperations).toHaveBeenCalledTimes(4)
  })

  it('[auth] suspends protected content on recovery network failure and resumes on user retry', async () => {
    const unauthorized = new ApiRequestError(401, {
      code: 'authentication_failed',
      message: 'expired',
      request_id: 'network-recovery-auth',
      retryable: false,
    })
    const listBackupSets = vi
      .fn<BackupApi['listBackupSets']>()
      .mockRejectedValueOnce(unauthorized)
      .mockResolvedValueOnce(page([]))
    const api = makeBackupApi({ listBackupSets })
    const apis = authApis()
    const getCurrentSession = vi
      .fn<AuthApi['getCurrentSession']>()
      .mockResolvedValueOnce(session)
      .mockRejectedValueOnce(new TypeError('network unavailable'))
      .mockResolvedValueOnce(session)
    const authApi: AuthApi = { ...apis.authApi, getCurrentSession }
    render(
      <App
        router="memory"
        initialEntries={['/backups']}
        authApi={authApi}
        bootstrapApi={apis.bootstrapApi}
        backupApi={api}
        fileApi={makeFileApi()}
      />,
    )

    expect(await screen.findByRole('heading', { name: /session could not be verified/i })).toHaveFocus()
    expect(screen.queryByRole('heading', { name: 'Backup Control Center' })).not.toBeInTheDocument()
    const retry = screen.getByRole('button', { name: /try session recovery again/i })
    fireEvent.click(retry)

    expect(await screen.findByRole('heading', { name: 'Backup Control Center' })).toHaveFocus()
    expect(getCurrentSession).toHaveBeenCalledTimes(3)
    expect(authApi.getCsrfToken).toHaveBeenCalledOnce()
    expect(listBackupSets).toHaveBeenCalledTimes(2)
  })

  it('[auth] blocks and discards a retry owned by another authenticated principal', async () => {
    bindMutationRecoveryPrincipal(session.data.user_id)
    mutationRecoveryStore.add({
      action_kind: 'execute_restore_plan',
      idempotency_key: '01925000-0000-7000-8000-000000000099',
      request_scope: {
        backupSetId: 'owner-a-set',
        restorePlanId: 'owner-a-plan',
      },
      request: { private_marker: 'owner-a-private-marker' },
    })
    const getRestorePlan = vi.fn<BackupApi['getBackupRestorePlan']>()
    const executeRestorePlan = vi.fn<BackupApi['executeBackupRestorePlan']>()
    const api = makeBackupApi({
      listBackupSets: vi.fn().mockResolvedValue(page([])),
      getBackupRestorePlan: getRestorePlan,
      executeBackupRestorePlan: executeRestorePlan,
    })
    const apis = authApis()
    const authApi: AuthApi = {
      ...apis.authApi,
      getCurrentSession: vi.fn().mockResolvedValue(secondUserSession),
    }
    render(
      <App
        router="memory"
        initialEntries={['/backups']}
        authApi={authApi}
        bootstrapApi={apis.bootstrapApi}
        backupApi={api}
        fileApi={makeFileApi()}
      />,
    )

    const warning = await screen.findByRole('heading', {
      name: /pending request cannot be used with this account/i,
    })
    expect(warning).toHaveFocus()
    expect(executeRestorePlan).not.toHaveBeenCalled()
    expect(getRestorePlan).not.toHaveBeenCalled()
    expect(document.body).not.toHaveTextContent('owner-a-private-marker')
    expect(document.body).not.toHaveTextContent('owner-a-plan')

    fireEvent.click(screen.getByRole('button', { name: /discard blocked local retry/i }))
    await waitFor(() => {
      expect(screen.queryByRole('heading', {
        name: /pending request cannot be used with this account/i,
      })).not.toBeInTheDocument()
    })
    expect(mutationRecoveryStore.all()).toHaveLength(0)
  })

  it('[auth] ignores an old principal GET that resolves after logout and a new login', async () => {
    const oldResponse = deferred<Awaited<ReturnType<BackupApi['listBackupSets']>>>()
    const newSet = {
      ...backupSet,
      backup_set_id: 'user-b-set',
      library_id: 'user-b-library',
      name: 'User B files',
    }
    const listBackupSets = vi
      .fn<BackupApi['listBackupSets']>()
      .mockReturnValueOnce(oldResponse.promise)
      .mockResolvedValueOnce(page([newSet]))
    const api = makeBackupApi({ listBackupSets })
    const apis = authApis()
    const authApi: AuthApi = {
      ...apis.authApi,
      getCurrentSession: vi.fn().mockResolvedValue(session),
      login: vi.fn().mockResolvedValue(secondUserSession),
    }
    render(
      <App
        router="memory"
        initialEntries={['/backups']}
        authApi={authApi}
        bootstrapApi={apis.bootstrapApi}
        backupApi={api}
        fileApi={makeFileApi()}
      />,
    )

    await screen.findByRole('heading', { name: 'Backup Control Center' })
    fireEvent.click(screen.getByRole('button', { name: 'Logout' }))
    await screen.findByRole('heading', { name: 'Sign in' })
    fireEvent.change(screen.getByLabelText('Login identifier'), { target: { value: 'user-b' } })
    fireEvent.change(screen.getByLabelText('Login key'), { target: { value: 'user-b-key' } })
    fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'password' } })
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }))

    expect(await screen.findByRole('heading', { name: 'User B files' })).toBeInTheDocument()
    oldResponse.resolve(page([backupSet]))
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'Family files' })).not.toBeInTheDocument()
      expect(screen.getByRole('heading', { name: 'User B files' })).toBeInTheDocument()
    })
    expect(listBackupSets).toHaveBeenCalledTimes(2)
  })
})
