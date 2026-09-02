import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import type { AuthApi, AuthSessionResponse } from '../../api/auth'
import type { BackupApi, BackupOperationSummary } from '../../api/backups'
import type { BootstrapApi, BootstrapStatusResponse } from '../../api/bootstrap'
import { App } from '../../app/App'
import type { FileMetadataApi, LiveLibraryResource } from '../../api/files'
import { ApiRequestError } from '../../api/errors'
import {
  bindMutationRecoveryPrincipal,
  mutationRecoveryStore,
} from '../../api/mutationRecovery'
import {
  AuthLoadingScreen,
  AuthRecoveringScreen,
  AuthRecoveryErrorScreen,
} from '../../auth/AuthStatusScreens'

const backupSet = {
  backup_set_id: 'set-1',
  name: 'Family files',
  library_id: 'library-1',
  state: 'ACTIVE' as const,
  created_at: '2026-08-01T09:00:00Z',
  updated_at: '2026-09-01T09:00:00Z',
}

const snapshot = {
  snapshot_id: 'snapshot-completed',
  backup_set_id: backupSet.backup_set_id,
  library_id: backupSet.library_id,
  state: 'COMPLETED' as const,
  created_at: '2026-08-30T09:00:00Z',
  committed_at: '2026-08-30T09:01:00Z',
  logical_node_count: '5',
  content_reference_count: '3',
}

const expiredSnapshot = {
  ...snapshot,
  snapshot_id: 'snapshot-expired',
  state: 'EXPIRED' as const,
  expired_at: '2026-08-31T09:01:00Z',
}

const maintenanceOperation: BackupOperationSummary = {
  operation_kind: 'MAINTENANCE',
  operation_id: 'run-1',
  backup_set_id: backupSet.backup_set_id,
  state: 'COMPLETED',
  phase: 'COMPLETED',
  progress: { completed_steps: 3, total_steps: 3 },
  terminal: true,
  next_action: 'NONE',
  created_at: '2026-08-31T09:00:00Z',
  last_transition_at: '2026-08-31T09:03:00Z',
}

const policy = {
  data: {
    policy_revision_id: 'policy-1',
    backup_set_id: backupSet.backup_set_id,
    revision_number: '1',
    keep_latest_completed: '3',
    expire_after_seconds: String(30 * 86_400),
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

function authApis(): { authApi: AuthApi; bootstrapApi: BootstrapApi } {
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
    listBackupSnapshots: vi.fn().mockResolvedValue(page([snapshot, expiredSnapshot])),
    getBackupSnapshot: vi.fn().mockResolvedValue({ data: snapshot, meta: { request_id: 'snapshot' } }),
    listBackupSnapshotNodes: vi.fn().mockResolvedValue(page([])),
    getBackupRetentionPolicy: vi.fn().mockResolvedValue(policy),
    configureBackupRetentionPolicy: vi.fn().mockResolvedValue(policy),
    listBackupOperations: vi.fn().mockResolvedValue(page([maintenanceOperation])),
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

async function checkA11y(container: HTMLElement): Promise<void> {
  // Import axe-core at runtime to avoid ESM/CJS issues
  const axe = await import('axe-core')
  const results = await axe.default.run(container, {
    rules: {
      'color-contrast': { enabled: false }, // known jsdom limitation: no visual color testing
    },
  })
  const criticalViolations = results.violations.filter((v) => v.impact === 'critical' || v.impact === 'serious')
  if (criticalViolations.length > 0) {
    const messages = criticalViolations.map((v) => `  [${v.impact}] ${v.id}: ${v.description}\n${v.nodes.map((n) => `    - ${n.html}`).join('\n')}`)
    expect.fail(`Found ${criticalViolations.length} serious/critical a11y violations:\n${messages.join('\n')}`)
  }
}

function renderPage(path: string, backupApi: BackupApi, fileApi: FileMetadataApi = makeFileApi()) {
  const auth = authApis()
  return render(
    <App
      router="memory"
      initialEntries={[path]}
      authApi={auth.authApi}
      bootstrapApi={auth.bootstrapApi}
      backupApi={backupApi}
      fileApi={fileApi}
    />,
  )
}

beforeEach(() => mutationRecoveryStore.clear())

describe('accessibility — backup overview', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('renders with no serious/critical a11y violations', async () => {
    const { container } = renderPage('/backups', makeBackupApi())
    await screen.findByRole('heading', { name: 'Backup Control Center', level: 1 })
    await screen.findByText('Keep at least 3 completed backups')
    await screen.findByText(/Maintenance · 3 of 3 steps completed/)
    await checkA11y(container)
  })

  it('renders empty state with no serious/critical a11y violations', async () => {
    const { container } = renderPage('/backups', makeBackupApi({ listBackupSets: vi.fn().mockResolvedValue(page([])) }))
    await screen.findByRole('heading', { name: 'No backups yet.' })
    await checkA11y(container)
  })

  it('renders a representative mutation error with no serious/critical a11y violations', async () => {
    const createError = new ApiRequestError(409, {
      code: 'backup_set_conflict',
      message: 'server detail',
      request_id: 'create-conflict',
      retryable: false,
    })
    const api = makeBackupApi({
      listBackupSets: vi.fn().mockResolvedValue(page([])),
      createBackupSet: vi.fn().mockRejectedValue(createError),
    })
    const { container } = renderPage('/backups', api)
    await screen.findByRole('option', { name: 'Personal library' })
    fireEvent.change(screen.getByLabelText('Backup set name'), { target: { value: 'Family files' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create backup set' }))
    await screen.findByRole('alert')
    expect(mutationRecoveryStore.all()).toHaveLength(0)
    await checkA11y(container)
  })
})

describe('accessibility — backup set detail', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('renders with no serious/critical a11y violations', async () => {
    const { container } = renderPage('/backups/set-1', makeBackupApi())
    await screen.findByRole('heading', { name: 'Backup set details', level: 1 })
    await checkA11y(container)
  })
})

describe('accessibility — backup set 404', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('renders not-found with no serious/critical a11y violations', async () => {
    const { container } = renderPage('/backups/missing-or-foreign', makeBackupApi({
      getBackupSet: vi.fn().mockRejectedValue(new ApiRequestError(404, {
        code: 'not_found',
        message: 'safe server message',
        request_id: 'missing-set',
        retryable: false,
      })),
    }))
    await screen.findByRole('heading', { name: 'Backup set not available' })
    await checkA11y(container)
  })
})

describe('accessibility — restore workflow entry', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('renders with no serious/critical a11y violations', async () => {
    const { container } = renderPage('/backups/set-1/snapshots/snapshot-completed/restore', makeBackupApi())
    await screen.findByRole('heading', { name: 'Restore a snapshot' })
    await screen.findByRole('option', { name: 'Personal library' })
    await checkA11y(container)
  })
})

describe('accessibility — restore plan review', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('renders with no serious/critical a11y violations', async () => {
    const plannedPlan = {
      restore_plan_id: 'plan-1',
      source_snapshot_id: 'snapshot-completed',
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
    const planResponse = { data: plannedPlan, meta: { request_id: 'plan' } }
    const operation = {
      operation_kind: 'RESTORE',
      operation_id: 'operation-1',
      backup_set_id: 'set-1',
      snapshot_id: 'snapshot-completed',
      state: 'PLANNED',
      phase: 'AWAITING_EXECUTION',
      progress: { completed_steps: 1, total_steps: 2 },
      terminal: false,
      next_action: 'EXECUTE',
      created_at: '2026-08-31T09:00:00Z',
      last_transition_at: '2026-08-31T09:01:00Z',
      restore_plan_id: 'plan-1',
      source_snapshot_id: 'snapshot-completed',
      target_library_id: 'library-1',
      target_parent_node_id: 'root-1',
      destination_name: 'Restored files',
      planned_entry_count: '3',
      planned_directory_count: '1',
      planned_file_count: '2',
    }
    const restoreDetail = { data: operation, meta: { request_id: 'operation' } }
    const getPlan = vi.fn().mockResolvedValue(planResponse)
    const getOperation = vi.fn().mockResolvedValue(restoreDetail)
    const fileApi: FileMetadataApi = {
      listLibraries: vi.fn().mockResolvedValue(page([library])),
      listLibraryChildren: vi.fn().mockResolvedValue(page([])),
      getLiveNode: vi.fn().mockImplementation(async (nodeId: string) => ({
        data: {
          id: nodeId,
          type: 'node',
          revision: '1',
          attributes: {
            library_id: 'library-1',
            parent_id: undefined,
            name: 'Library root',
            kind: 'DIRECTORY',
            state: 'ACTIVE',
            created_at: '2026-01-01T00:00:00Z',
            updated_at: '2026-01-01T00:00:00Z',
          },
        },
        meta: { request_id: 'node' },
      })),
    }
    const api = makeBackupApi({ getBackupRestorePlan: getPlan, getBackupOperation: getOperation })
    const { container } = renderPage('/backups/set-1/restore/plan-1', api, fileApi)
    await screen.findByRole('heading', { name: 'Review restore plan', level: 2 })
    await checkA11y(container)
  })
})

describe('accessibility — auth/session hardening states', () => {
  it('renders auth bootstrap with no serious/critical a11y violations', async () => {
    const { container } = render(<AuthLoadingScreen />)
    await checkA11y(container)
  })

  it('renders session recovery with no serious/critical a11y violations', async () => {
    const { container } = render(<AuthRecoveringScreen />)
    await checkA11y(container)
  })

  it('renders recovery failure with no serious/critical a11y violations', async () => {
    const { container } = render(
      <AuthRecoveryErrorScreen
        message="Unable to verify your session right now."
        onRetry={() => undefined}
      />,
    )
    await checkA11y(container)
  })

  it('renders auth-required login with no serious/critical a11y violations', async () => {
    const apis = authApis()
    const authApi: AuthApi = {
      ...apis.authApi,
      getCurrentSession: vi.fn().mockRejectedValue(new ApiRequestError(401, {
        code: 'authentication_failed',
        message: 'expired',
        request_id: 'a11y-auth-required',
        retryable: false,
      })),
    }
    const { container } = render(
      <App
        router="memory"
        initialEntries={['/backups']}
        authApi={authApi}
        bootstrapApi={apis.bootstrapApi}
        backupApi={makeBackupApi()}
        fileApi={makeFileApi()}
      />,
    )
    await screen.findByRole('heading', { name: 'Sign in' })
    await checkA11y(container)
  })

  it('renders cross-account retry blocking with no serious/critical a11y violations', async () => {
    bindMutationRecoveryPrincipal(session.data.user_id)
    mutationRecoveryStore.add({
      action_kind: 'execute_prune_plan',
      idempotency_key: '01925000-0000-7000-8000-000000000099',
      request_scope: { prunePlanId: 'private-plan' },
      request: { confirm_snapshot_id: 'private-snapshot' },
    })
    const apis = authApis()
    const otherSession: AuthSessionResponse = {
      data: {
        ...session.data,
        user_id: '00000000-0000-7000-8000-000000000099',
        session_id: '00000000-0000-7000-8000-000000000098',
      },
      meta: { request_id: 'other-session' },
    }
    const authApi: AuthApi = {
      ...apis.authApi,
      getCurrentSession: vi.fn().mockResolvedValue(otherSession),
    }
    const { container } = render(
      <App
        router="memory"
        initialEntries={['/backups']}
        authApi={authApi}
        bootstrapApi={apis.bootstrapApi}
        backupApi={makeBackupApi()}
        fileApi={makeFileApi()}
      />,
    )
    await screen.findByRole('heading', {
      name: /pending request cannot be used with this account/i,
    })
    await checkA11y(container)
  })
})

describe('accessibility — prune workflow', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('renders prune review with no serious/critical a11y violations', async () => {
    const plan = {
      prune_plan_id: 'prune-plan-1',
      snapshot_id: 'snapshot-expired',
      backup_set_id: 'set-1',
      state: 'PLANNED',
      entry_count: '7',
      distinct_content_count: '5',
      retained_by_other_reference_count: '3',
      would_become_unreferenced_count: '2',
      impact_summary: [
        { impact: 'RETAINED_BY_OTHER_REFERENCE', content_count: '3' },
        { impact: 'WOULD_BECOME_UNREFERENCED', content_count: '2' },
      ],
      created_at: '2026-09-01T00:00:00Z',
    }
    const api = makeBackupApi({
      getBackupSnapshot: vi.fn().mockResolvedValue({ data: expiredSnapshot, meta: { request_id: 'snapshot' } }),
      getBackupPrunePlan: vi.fn().mockResolvedValue({ data: plan, meta: { request_id: 'plan' } }),
    })
    const { container } = renderPage('/backups/set-1/prune/prune-plan-1', api)
    await screen.findByRole('heading', { name: 'Review retention impact' })
    await checkA11y(container)
  })

  it('audits the populated prune confirmation dialog with no serious/critical violations', async () => {
    const plan = {
      prune_plan_id: 'prune-plan-dialog',
      snapshot_id: 'snapshot-expired',
      backup_set_id: 'set-1',
      state: 'PLANNED',
      entry_count: '7',
      distinct_content_count: '5',
      retained_by_other_reference_count: '3',
      would_become_unreferenced_count: '2',
      impact_summary: [
        { impact: 'RETAINED_BY_OTHER_REFERENCE', content_count: '3' },
        { impact: 'WOULD_BECOME_UNREFERENCED', content_count: '2' },
      ],
      created_at: '2026-09-01T00:00:00Z',
    }
    const api = makeBackupApi({
      getBackupSnapshot: vi.fn().mockResolvedValue({ data: expiredSnapshot, meta: { request_id: 'snapshot' } }),
      getBackupPrunePlan: vi.fn().mockResolvedValue({ data: plan, meta: { request_id: 'plan' } }),
    })
    const { container } = renderPage('/backups/set-1/prune/prune-plan-dialog', api)
    fireEvent.click(await screen.findByRole('button', { name: 'Review and confirm' }))
    const dialog = await screen.findByRole('dialog', { name: 'Release retained backup content?' })
    expect(dialog).toHaveTextContent('Handed to internal cleanup')
    expect(dialog).toHaveTextContent('2')
    await checkA11y(container)
  })
})

describe('accessibility — operation detail', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('renders with no serious/critical a11y violations', async () => {
    const completedOp = {
      ...maintenanceOperation,
      operation_kind: 'MAINTENANCE',
      maintenance_run_id: 'run-1',
      policy_revision_id: 'policy-1',
      policy_revision_number: '1',
    }
    const api = makeBackupApi({
      getBackupOperation: vi.fn().mockResolvedValue({ data: completedOp, meta: { request_id: 'operation' } }),
    })
    const { container } = renderPage('/backups/set-1', api)
    await screen.findByRole('heading', { name: 'Backup set details', level: 1 })
    // Wait for the activity feed to render and present a maintenance operation
    await waitFor(() => expect(screen.getByText(/3 of 3 steps completed/)).toBeInTheDocument())
    await checkA11y(container)
  })
})
