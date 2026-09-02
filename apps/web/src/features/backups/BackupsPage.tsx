import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type ReactNode,
} from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'

import {
  backupApi as defaultBackupApi,
  createUuidV7,
  type BackupApi,
  type BackupMaintenanceOperation,
  type BackupMaintenanceRunState,
  type BackupOperationDetail,
  type BackupOperationKind,
  type BackupOperationNextAction,
  type BackupOperationPhase,
  type BackupOperationState,
  type BackupOperationSummary,
  type BackupRetentionPolicy,
  type BackupSet,
  type BackupSetState,
  type BackupSnapshot,
  type BackupSnapshotNode,
  type BackupSnapshotState,
  type BackupSnapshotNodeState,
} from '../../api/backups'
import { isApiRequestError } from '../../api/errors'
import {
  findPendingMutation,
  mutationRecoveryStore,
  settlePendingMutationFailure,
  type PendingMutationRecord,
} from '../../api/mutationRecovery'
import { StatusPill, type StatusTone } from '../../components/ui/StatusPill'
import { useAuth } from '../../auth/useAuth'
import { RetentionPolicyEditor } from './RetentionPolicyEditor'
import { MutationRecoveryNotice } from './MutationRecoveryNotice'

const SET_PAGE_LIMIT = 50
const SNAPSHOT_PAGE_LIMIT = 50
const OPERATION_PAGE_LIMIT = 50
const TREE_PAGE_LIMIT = 50

type LoadStatus = 'idle' | 'loading' | 'success' | 'error'

interface CollectionState<T> {
  readonly status: LoadStatus
  readonly items: readonly T[]
  readonly nextCursor?: string
  readonly hasMore: boolean
  readonly error?: unknown
}

interface SingleState<T> {
  readonly status: LoadStatus
  readonly data?: T
  readonly error?: unknown
}

interface TreeCrumb {
  readonly nodeId?: string
  readonly name: string
}

interface TreeState {
  readonly status: LoadStatus
  readonly snapshotId?: string
  readonly parentNodeId?: string
  readonly breadcrumbs: readonly TreeCrumb[]
  readonly items: readonly BackupSnapshotNode[]
  readonly nextCursor?: string
  readonly hasMore: boolean
  readonly notAvailable?: boolean
  readonly error?: unknown
}

type OperationFilter = 'ALL' | BackupOperationKind
type MutationKind = 'create' | 'advance'

interface MutationAction {
  readonly kind: MutationKind
  readonly idempotencyKey: string
  readonly maintenanceRunId?: string
  readonly expectedState?: BackupMaintenanceRunState
  readonly phase: 'submitting' | 'failed'
}

const MAINTENANCE_STATES: ReadonlySet<BackupMaintenanceRunState> = new Set([
  'CREATED',
  'SNAPSHOT_CAPTURED',
  'EXPIRY_PLANNED',
  'COMPLETED',
  'STALE',
])

function pendingMaintenanceExpectedState(
  record: PendingMutationRecord,
): BackupMaintenanceRunState | undefined {
  const state = record.request.expected_state
  return typeof state === 'string' && MAINTENANCE_STATES.has(state as BackupMaintenanceRunState)
    ? state as BackupMaintenanceRunState
    : undefined
}

interface OperationSelection {
  readonly operationKind: BackupOperationKind
  readonly operationId: string
}

export interface BackupsPageProps {
  readonly backupApi?: BackupApi
}

const INITIAL_COLLECTION = <T,>(): CollectionState<T> => ({
  status: 'idle',
  items: [],
  hasMore: false,
})

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError'
}

function isNotFound(error: unknown): boolean {
  return isApiRequestError(error) && error.status === 404
}

function sectionErrorMessage(error: unknown, subject: string): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again to view backups.'
    }
    if (error.status === 404) {
      return `${subject} is no longer available.`
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Backup data is temporarily unavailable. Please try again.'
    }
  }
  return `We could not load ${subject.toLowerCase()}. Please try again.`
}

function mutationErrorMessage(kind: MutationKind, error: unknown): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again before continuing.'
    }
    if (error.status === 403) {
      return 'This request could not be verified. Refresh the page and try again.'
    }
    if (error.status === 404) {
      return 'This backup set or maintenance run is no longer available.'
    }
    if (error.status === 409) {
      return kind === 'advance'
        ? 'This maintenance run changed before it could continue. Refresh to see the latest status.'
        : 'A backup run cannot start right now. Refresh to see the latest activity.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Backup maintenance is temporarily unavailable. Please try again.'
    }
  }
  return 'We could not confirm this request. Try again; the same request will be retried safely.'
}

function setStateLabel(state: BackupSetState): string {
  switch (state) {
    case 'ACTIVE':
      return 'Active'
    case 'DISABLED':
      return 'Disabled'
    case 'CREATED':
      return 'Ready'
  }
}

function snapshotStateLabel(state: BackupSnapshotState): string {
  switch (state) {
    case 'COMPLETED':
      return 'Completed'
    case 'EXPIRED':
      return 'Expired'
    case 'BUILDING':
      return 'Preparing'
    case 'FAILED':
      return 'Failed'
  }
}

function snapshotStateTone(state: BackupSnapshotState): StatusTone {
  switch (state) {
    case 'COMPLETED':
      return 'info'
    case 'EXPIRED':
    case 'BUILDING':
      return 'planned'
    case 'FAILED':
      return 'quiet'
  }
}

function nodeStateLabel(state: BackupSnapshotNodeState): string {
  return state === 'TRASHED' ? 'Historical item' : 'In snapshot'
}

function operationKindLabel(kind: BackupOperationKind): string {
  switch (kind) {
    case 'MAINTENANCE':
      return 'Backup maintenance'
    case 'RESTORE':
      return 'Restore'
    case 'PRUNE':
      return 'Prune'
  }
}

function operationStateLabel(state: BackupOperationState): string {
  switch (state) {
    case 'CREATED':
      return 'Created'
    case 'SNAPSHOT_CAPTURED':
      return 'Snapshot captured'
    case 'EXPIRY_PLANNED':
      return 'Retention reviewed'
    case 'PLANNED':
      return 'Planned'
    case 'EXECUTED':
      return 'Executed'
    case 'COMPLETED':
      return 'Completed'
    case 'STALE':
      return 'Stale'
  }
}

function operationPhaseLabel(phase: BackupOperationPhase): string {
  switch (phase) {
    case 'AWAITING_ADVANCE':
      return 'Ready for next step'
    case 'SNAPSHOT_CAPTURED':
      return 'Snapshot captured'
    case 'EXPIRY_PLANNED':
      return 'Retention reviewed'
    case 'AWAITING_EXECUTION':
      return 'Ready to restore'
    case 'AWAITING_CONFIRMATION':
      return 'Waiting for confirmation'
    case 'COMPLETED':
      return 'Completed'
    case 'STALE':
      return 'Needs a new plan'
  }
}

function operationPhaseTone(phase: BackupOperationPhase): StatusTone {
  switch (phase) {
    case 'COMPLETED':
      return 'info'
    case 'STALE':
    case 'AWAITING_ADVANCE':
    case 'AWAITING_EXECUTION':
    case 'AWAITING_CONFIRMATION':
      return 'planned'
    case 'SNAPSHOT_CAPTURED':
    case 'EXPIRY_PLANNED':
      return 'quiet'
  }
}

function nextActionLabel(action: BackupOperationNextAction): string {
  switch (action) {
    case 'ADVANCE':
      return 'Continue available'
    case 'EXECUTE':
      return 'Ready for review'
    case 'CREATE_NEW_RUN':
      return 'Start a new run'
    case 'CREATE_NEW_PLAN':
      return 'Create a new plan later'
    case 'NONE':
      return 'No action needed'
  }
}

function formatInteger(value: string | number): string {
  const text = String(value)
  if (/^(0|[1-9][0-9]*)$/.test(text)) {
    try {
      return new Intl.NumberFormat().format(BigInt(text))
    } catch {
      return text
    }
  }
  return text
}

function formatBytes(value?: string): string {
  if (value === undefined || !/^(0|[1-9][0-9]*)$/.test(value)) {
    return value === undefined ? 'Not recorded' : `${value} bytes`
  }
  const bytes = BigInt(value)
  const units = ['bytes', 'KB', 'MB', 'GB', 'TB', 'PB'] as const
  let scaled = bytes
  let unitIndex = 0
  while (scaled >= 1_000n && unitIndex < units.length - 1) {
    scaled /= 1_000n
    unitIndex += 1
  }
  return `${formatInteger(scaled.toString())} ${units[unitIndex]}`
}

function formatTimestamp(value?: string): string {
  if (!value) {
    return 'Not recorded'
  }
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(date)
}

function Timestamp({ value }: { readonly value?: string }) {
  if (!value) {
    return <span>Not recorded</span>
  }
  return (
    <time dateTime={value} title={value}>
      {formatTimestamp(value)}
    </time>
  )
}

function TechnicalDetails({ children }: { readonly children: ReactNode }) {
  return (
    <details className="backup-technical-details">
      <summary>Technical details</summary>
      <div>{children}</div>
    </details>
  )
}

function LoadingBlock({ label }: { readonly label: string }) {
  return (
    <p className="backup-loading" role="status" aria-busy="true">
      {label}
    </p>
  )
}

function SectionError({
  message,
  onRetry,
}: {
  readonly message: string
  readonly onRetry: () => void
}) {
  return (
    <div className="backup-inline-error" role="alert">
      <p>{message}</p>
      <button type="button" className="button--secondary" onClick={onRetry}>
        Try again
      </button>
    </div>
  )
}

function ProgressText({ progress }: { readonly progress: BackupOperationSummary['progress'] }) {
  const visualStepCount = Math.min(12, Math.max(1, progress.total_steps))
  return (
    <span className="backup-progress" aria-label={`${progress.completed_steps} of ${progress.total_steps} steps completed`}>
      <span aria-hidden="true" className="backup-progress-dots">
        {Array.from({ length: visualStepCount }, (_, index) => (
          <span
            className={index < progress.completed_steps ? 'backup-progress-dot backup-progress-dot--complete' : 'backup-progress-dot'}
            key={index}
          />
        ))}
      </span>
      {progress.completed_steps} of {progress.total_steps} steps completed
    </span>
  )
}

function SnapshotStateCopy({ state }: { readonly state: BackupSnapshotState }) {
  if (state === 'COMPLETED') {
    return <span className="backup-secondary-copy">Available for restore</span>
  }
  if (state === 'EXPIRED') {
    return <span className="backup-secondary-copy">Not available for restore · Eligible for retention release</span>
  }
  return null
}

function SummaryMetric({
  label,
  children,
}: {
  readonly label: string
  readonly children: ReactNode
}) {
  return (
    <div className="backup-summary-metric">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  )
}

function operationKey(operation: BackupOperationSummary): string {
  return `${operation.operation_kind}:${operation.operation_id}`
}

function appendUniqueOperations(
  current: readonly BackupOperationSummary[],
  incoming: readonly BackupOperationSummary[],
): BackupOperationSummary[] {
  const seen = new Set(current.map(operationKey))
  const result = [...current]
  for (const operation of incoming) {
    const key = operationKey(operation)
    if (!seen.has(key)) {
      seen.add(key)
      result.push(operation)
    }
  }
  return result
}

function initialTreeState(): TreeState {
  return {
    status: 'idle',
    breadcrumbs: [{ name: 'Snapshot root' }],
    items: [],
    hasMore: false,
  }
}

export function BackupsPage({ backupApi = defaultBackupApi }: BackupsPageProps) {
  const navigate = useNavigate()
  const {
    backupSetId: routeBackupSetId,
    operationKind: routeOperationKind,
    operationId: routeOperationId,
  } = useParams<{
    backupSetId: string
    operationKind: string
    operationId: string
  }>()
  const { refresh: refreshAuth } = useAuth()
  const routeOperationSelection = useMemo<OperationSelection | undefined>(
    () => routeOperationId &&
      (routeOperationKind === 'MAINTENANCE' ||
        routeOperationKind === 'RESTORE' ||
        routeOperationKind === 'PRUNE')
      ? { operationKind: routeOperationKind, operationId: routeOperationId }
      : undefined,
    [routeOperationId, routeOperationKind],
  )
  const invalidOperationRoute = Boolean(routeOperationId && !routeOperationSelection)
  const [sets, setSets] = useState<CollectionState<BackupSet>>({
    ...INITIAL_COLLECTION<BackupSet>(),
    status: 'loading',
  })
  const [selectedBackupSetId, setSelectedBackupSetId] = useState<string | undefined>(routeBackupSetId)
  const [selectedSet, setSelectedSet] = useState<SingleState<BackupSet>>({ status: 'idle' })
  const [snapshots, setSnapshots] = useState<CollectionState<BackupSnapshot>>(
    INITIAL_COLLECTION<BackupSnapshot>(),
  )
  const [policy, setPolicy] = useState<SingleState<BackupRetentionPolicy['data'] | null>>({
    status: 'idle',
  })
  const [operations, setOperations] = useState<CollectionState<BackupOperationSummary>>(
    INITIAL_COLLECTION<BackupOperationSummary>(),
  )
  const [operationFilter, setOperationFilter] = useState<OperationFilter>('ALL')
  const [selectedOperation, setSelectedOperation] = useState<OperationSelection | undefined>(
    routeOperationSelection,
  )
  const [operationDetail, setOperationDetail] = useState<SingleState<BackupOperationDetail>>({
    status: 'idle',
  })
  const [selectedSnapshotId, setSelectedSnapshotId] = useState<string>()
  const [snapshotDetail, setSnapshotDetail] = useState<SingleState<BackupSnapshot>>({
    status: 'idle',
  })
  const [tree, setTree] = useState<TreeState>(initialTreeState)
  const [setsLoadingMore, setSetsLoadingMore] = useState(false)
  const [snapshotsLoadingMore, setSnapshotsLoadingMore] = useState(false)
  const [operationsLoadingMore, setOperationsLoadingMore] = useState(false)
  const [treeLoadingMore, setTreeLoadingMore] = useState(false)
  const [refreshVersion, setRefreshVersion] = useState(0)
  const [setsRefreshVersion, setSetsRefreshVersion] = useState(0)
  const [snapshotDetailRefreshVersion, setSnapshotDetailRefreshVersion] = useState(0)
  const [mutationAction, setMutationAction] = useState<MutationAction | null>(null)
  const [mutationError, setMutationError] = useState<string>()
  const [mutationNotice, setMutationNotice] = useState<string>()
  const [pendingMaintenance, setPendingMaintenance] = useState<PendingMutationRecord>()
  const [maintenanceRecoveryChecking, setMaintenanceRecoveryChecking] = useState(false)
  const [maintenanceRecoveryReady, setMaintenanceRecoveryReady] = useState(false)

  const mounted = useRef(true)
  const setsRequestId = useRef(0)
  const snapshotsRequestId = useRef(0)
  const operationsRequestId = useRef(0)
  const operationDetailRequestId = useRef(0)
  const snapshotDetailRequestId = useRef(0)
  const treeRequestId = useRef(0)
  const treeController = useRef<AbortController | undefined>(undefined)
  const setsPaginationController = useRef<AbortController | undefined>(undefined)
  const snapshotsPaginationController = useRef<AbortController | undefined>(undefined)
  const operationsPaginationController = useRef<AbortController | undefined>(undefined)
  const operationsLoadingMoreRef = useRef(false)
  const mutationRef = useRef<MutationAction | null>(null)
  const operationDetailHeadingRef = useRef<HTMLHeadingElement>(null)
  const notFoundHeadingRef = useRef<HTMLHeadingElement>(null)
  const treeHeadingRef = useRef<HTMLHeadingElement>(null)

  const currentSet = selectedSet.data ?? sets.items.find((set) => set.backup_set_id === selectedBackupSetId)
  const activeMaintenance = operations.items.some(
    (operation) => operation.operation_kind === 'MAINTENANCE' && !operation.terminal,
  )
  const latestCompletedSnapshot = snapshots.items.find((snapshot) => snapshot.state === 'COMPLETED')
  const latestOperation = operations.items[0]
  const currentMaintenanceOperation =
    operationDetail.data?.operation_kind === 'MAINTENANCE' ? operationDetail.data : undefined
  const mutationSubmitting = mutationAction?.phase === 'submitting'

  const recoverSession = useCallback(
    (error: unknown) => {
      if (isApiRequestError(error) && error.status === 401) {
        void refreshAuth().catch(() => undefined)
      }
    },
    [refreshAuth],
  )

  const reconcilePendingAdvance = useCallback(async (record: PendingMutationRecord) => {
    const runId = record.request_scope.maintenanceRunId
    const expectedState = pendingMaintenanceExpectedState(record)
    if (!runId || !expectedState) {
      mutationRecoveryStore.remove(record.action_id)
      setPendingMaintenance(undefined)
      setMaintenanceRecoveryReady(false)
      setMutationNotice('The local maintenance retry was invalid and was removed. No maintenance step was submitted.')
      return
    }
    setMaintenanceRecoveryChecking(true)
    setMaintenanceRecoveryReady(false)
    try {
      const response = await backupApi.getBackupMaintenanceRun(runId)
      if (!mounted.current) {
        return
      }
      if (
        response.data.maintenance_run_id !== runId ||
        response.data.backup_set_id !== record.request_scope.backupSetId
      ) {
        throw new Error('maintenance recovery scope mismatch')
      }
      if (response.data.state !== expectedState) {
        mutationRecoveryStore.remove(record.action_id)
        setPendingMaintenance(undefined)
        setMutationNotice('The server-confirmed maintenance run has already moved forward. No additional step was submitted.')
        setRefreshVersion((current) => current + 1)
      } else {
        setPendingMaintenance(record)
        setMaintenanceRecoveryReady(true)
        setMutationNotice(undefined)
      }
    } catch (error) {
      if (mounted.current) {
        setMutationError('The current maintenance phase could not be checked. No maintenance step was submitted.')
      }
      recoverSession(error)
    } finally {
      if (mounted.current) {
        setMaintenanceRecoveryChecking(false)
      }
    }
  }, [backupApi, recoverSession])

  useEffect(() => {
    return () => {
      mounted.current = false
      treeController.current?.abort()
      setsPaginationController.current?.abort()
      snapshotsPaginationController.current?.abort()
      operationsPaginationController.current?.abort()
    }
  }, [])

  useEffect(() => {
    if (!selectedBackupSetId) {
      setPendingMaintenance(undefined)
      setMaintenanceRecoveryReady(false)
      return
    }
    const advance = findPendingMutation(
      'advance_maintenance_run',
      (scope) => scope.backupSetId === selectedBackupSetId,
    )
    if (advance) {
      setPendingMaintenance(advance)
      void reconcilePendingAdvance(advance)
      return
    }
    const create = findPendingMutation(
      'create_maintenance_run',
      (scope) => scope.backupSetId === selectedBackupSetId,
    )
    setPendingMaintenance(create)
    setMaintenanceRecoveryReady(Boolean(create))
  }, [reconcilePendingAdvance, selectedBackupSetId])

  useEffect(() => {
    if (routeBackupSetId && routeBackupSetId !== selectedBackupSetId) {
      setSelectedBackupSetId(routeBackupSetId)
    }
  }, [routeBackupSetId, selectedBackupSetId])

  useEffect(() => {
    setSelectedOperation(routeOperationSelection)
  }, [routeOperationSelection])

  useEffect(() => {
    setsPaginationController.current?.abort()
    const controller = new AbortController()
    const requestId = setsRequestId.current + 1
    setsRequestId.current = requestId
    setSets({ ...INITIAL_COLLECTION<BackupSet>(), status: 'loading' })

    void backupApi
      .listBackupSets({ limit: SET_PAGE_LIMIT, signal: controller.signal })
      .then((response) => {
        if (!mounted.current || controller.signal.aborted || setsRequestId.current !== requestId) {
          return
        }
        const items = [...response.data]
        setSets({
          status: 'success',
          items,
          nextCursor: response.page.next_cursor,
          hasMore: response.page.has_more,
        })
        setSelectedBackupSetId((current) => {
          if (routeBackupSetId) {
            return routeBackupSetId
          }
          if (current && items.some((set) => set.backup_set_id === current)) {
            return current
          }
          return items[0]?.backup_set_id
        })
      })
      .catch((error: unknown) => {
        if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
          return
        }
        setSets({ ...INITIAL_COLLECTION<BackupSet>(), status: 'error', error })
        recoverSession(error)
      })

    return () => controller.abort()
  }, [backupApi, recoverSession, routeBackupSetId, setsRefreshVersion])

  useEffect(() => {
    treeController.current?.abort()
    setSelectedSet({ status: 'idle' })
    setSnapshots(INITIAL_COLLECTION<BackupSnapshot>())
    setPolicy({ status: 'idle' })
    setOperations(INITIAL_COLLECTION<BackupOperationSummary>())
    setSelectedSnapshotId(undefined)
    setSnapshotDetail({ status: 'idle' })
    setTree(initialTreeState())
    setSelectedOperation(routeOperationSelection)
    setOperationDetail({ status: 'idle' })
  }, [routeOperationSelection, selectedBackupSetId])

  useEffect(() => {
    snapshotsPaginationController.current?.abort()
    const selectedId = selectedBackupSetId
    if (!selectedId) {
      return
    }

    const controller = new AbortController()
    const requestId = snapshotsRequestId.current + 1
    snapshotsRequestId.current = requestId
    setSelectedSet({ status: 'loading' })
    setSnapshots({ ...INITIAL_COLLECTION<BackupSnapshot>(), status: 'loading' })
    setPolicy({ status: 'loading' })

    void Promise.allSettled([
      backupApi.getBackupSet(selectedId, { signal: controller.signal }),
      backupApi.listBackupSnapshots(selectedId, {
        limit: SNAPSHOT_PAGE_LIMIT,
        signal: controller.signal,
      }),
      backupApi.getBackupRetentionPolicy(selectedId, { signal: controller.signal }),
    ]).then(([setResult, snapshotResult, policyResult]) => {
      if (!mounted.current || controller.signal.aborted || snapshotsRequestId.current !== requestId) {
        return
      }

      if (setResult.status === 'fulfilled' && setResult.value.data.backup_set_id === selectedId) {
        setSelectedSet({ status: 'success', data: setResult.value.data })
      } else {
        const error = setResult.status === 'rejected' ? setResult.reason : new Error('backup set scope mismatch')
        setSelectedSet({ status: 'error', error })
        recoverSession(error)
      }

      if (
        snapshotResult.status === 'fulfilled' &&
        snapshotResult.value.data.every((snapshot) => snapshot.backup_set_id === selectedId)
      ) {
        setSnapshots({
          status: 'success',
          items: [...snapshotResult.value.data],
          nextCursor: snapshotResult.value.page.next_cursor,
          hasMore: snapshotResult.value.page.has_more,
        })
      } else {
        const error = snapshotResult.status === 'rejected' ? snapshotResult.reason : new Error('snapshot scope mismatch')
        setSnapshots({ ...INITIAL_COLLECTION<BackupSnapshot>(), status: 'error', error })
        recoverSession(error)
      }

      if (
        policyResult.status === 'fulfilled' &&
        policyResult.value.data.backup_set_id === selectedId
      ) {
        setPolicy({ status: 'success', data: policyResult.value.data })
      } else {
        const error = policyResult.status === 'rejected' ? policyResult.reason : new Error('retention policy scope mismatch')
        if (isNotFound(error)) {
          setPolicy({ status: 'success', data: null })
        } else {
          setPolicy({ status: 'error', error })
          recoverSession(error)
        }
      }
    })

    return () => controller.abort()
  }, [backupApi, recoverSession, refreshVersion, selectedBackupSetId])

  useEffect(() => {
    operationsPaginationController.current?.abort()
    operationsLoadingMoreRef.current = false
    const selectedId = selectedBackupSetId
    if (!selectedId) {
      return
    }

    const controller = new AbortController()
    const requestId = operationsRequestId.current + 1
    operationsRequestId.current = requestId
    const kind = operationFilter === 'ALL' ? undefined : operationFilter
    setOperations({ ...INITIAL_COLLECTION<BackupOperationSummary>(), status: 'loading' })

    void backupApi
      .listBackupOperations(selectedId, {
        kind,
        limit: OPERATION_PAGE_LIMIT,
        signal: controller.signal,
      })
      .then((response) => {
        if (!mounted.current || controller.signal.aborted || operationsRequestId.current !== requestId) {
          return
        }
        if (!response.data.every((operation) =>
          operation.backup_set_id === selectedId && (!kind || operation.operation_kind === kind)
        )) {
          throw new Error('operation scope mismatch')
        }
        setOperations({
          status: 'success',
          items: [...response.data],
          nextCursor: response.page.next_cursor,
          hasMore: response.page.has_more,
        })
      })
      .catch((error: unknown) => {
        if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
          return
        }
        setOperations({ ...INITIAL_COLLECTION<BackupOperationSummary>(), status: 'error', error })
        recoverSession(error)
      })

    return () => controller.abort()
  }, [backupApi, operationFilter, recoverSession, refreshVersion, selectedBackupSetId])

  const loadTree = useCallback(
    async ({
      snapshotId,
      parentNodeId,
      breadcrumbs,
      cursor,
      append,
    }: {
      readonly snapshotId: string
      readonly parentNodeId?: string
      readonly breadcrumbs: readonly TreeCrumb[]
      readonly cursor?: string
      readonly append: boolean
    }) => {
      const requestId = treeRequestId.current + 1
      treeRequestId.current = requestId
      treeController.current?.abort()
      const controller = new AbortController()
      treeController.current = controller
      if (append) {
        setTree((current) => ({ ...current, status: 'loading', error: undefined }))
      } else {
        setTree({
          status: 'loading',
          snapshotId,
          parentNodeId,
          breadcrumbs,
          items: [],
          hasMore: false,
        })
      }

      try {
        const response = await backupApi.listBackupSnapshotNodes(snapshotId, {
          parentId: parentNodeId,
          cursor,
          limit: TREE_PAGE_LIMIT,
          signal: controller.signal,
        })
        if (
          !mounted.current ||
          controller.signal.aborted ||
          treeRequestId.current !== requestId ||
          selectedSnapshotId !== snapshotId
        ) {
          return
        }
        setTree((current) => ({
          status: 'success',
          snapshotId,
          parentNodeId,
          breadcrumbs,
          items: append ? [...current.items, ...response.data] : [...response.data],
          nextCursor: response.page.next_cursor,
          hasMore: response.page.has_more,
        }))
      } catch (error) {
        if (
          !mounted.current ||
          controller.signal.aborted ||
          isAbortError(error) ||
          treeRequestId.current !== requestId ||
          selectedSnapshotId !== snapshotId
        ) {
          return
        }
        setTree((current) => ({
          ...current,
          status: 'error',
          error,
        }))
        recoverSession(error)
      }
    },
    [backupApi, recoverSession, selectedSnapshotId],
  )

  useEffect(() => {
    const snapshotId = selectedSnapshotId
    if (!snapshotId) {
      return
    }

    const controller = new AbortController()
    const requestId = snapshotDetailRequestId.current + 1
    snapshotDetailRequestId.current = requestId
    setSnapshotDetail({ status: 'loading' })
    setTree(initialTreeState())

    void backupApi
      .getBackupSnapshot(snapshotId, { signal: controller.signal })
      .then((response) => {
        if (
          !mounted.current ||
          controller.signal.aborted ||
          snapshotDetailRequestId.current !== requestId
        ) {
          return
        }
        const snapshot = response.data
        if (
          snapshot.snapshot_id !== snapshotId ||
          snapshot.backup_set_id !== selectedBackupSetId
        ) {
          throw new Error('snapshot scope mismatch')
        }
        setSnapshotDetail({ status: 'success', data: snapshot })
        if (snapshot.state === 'COMPLETED' || snapshot.state === 'EXPIRED') {
          void loadTree({
            snapshotId,
            breadcrumbs: [{ name: 'Snapshot root' }],
            append: false,
          })
        } else {
          setTree({
            status: 'success',
            snapshotId,
            breadcrumbs: [{ name: 'Snapshot root' }],
            items: [],
            hasMore: false,
            notAvailable: true,
          })
        }
      })
      .catch((error: unknown) => {
        if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
          return
        }
        setSnapshotDetail({ status: 'error', error })
        setTree({ ...initialTreeState(), status: 'error', error })
        recoverSession(error)
      })

    return () => controller.abort()
  }, [
    backupApi,
    loadTree,
    recoverSession,
    selectedBackupSetId,
    selectedSnapshotId,
    snapshotDetailRefreshVersion,
  ])

  useEffect(() => {
    const selection = selectedOperation
    if (!selection || !selectedBackupSetId) {
      setOperationDetail({ status: 'idle' })
      return
    }

    const controller = new AbortController()
    const requestId = operationDetailRequestId.current + 1
    operationDetailRequestId.current = requestId
    setOperationDetail({ status: 'loading' })

    void backupApi
      .getBackupOperation(selection.operationKind, selection.operationId, {
        signal: controller.signal,
      })
      .then((response) => {
        if (
          !mounted.current ||
          controller.signal.aborted ||
          operationDetailRequestId.current !== requestId
        ) {
          return
        }
        if (
          response.data.backup_set_id !== selectedBackupSetId ||
          response.data.operation_kind !== selection.operationKind ||
          response.data.operation_id !== selection.operationId
        ) {
          throw new Error('operation scope mismatch')
        }
        setOperationDetail({ status: 'success', data: response.data })
      })
      .catch((error: unknown) => {
        if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
          return
        }
        setOperationDetail({ status: 'error', error })
        recoverSession(error)
      })

    return () => controller.abort()
  }, [backupApi, recoverSession, selectedBackupSetId, selectedOperation])

  useEffect(() => {
    if (selectedOperation && (operationDetail.status === 'success' || operationDetail.status === 'error')) {
      operationDetailHeadingRef.current?.focus()
    }
  }, [operationDetail.status, selectedOperation])

  useEffect(() => {
    if (invalidOperationRoute || (selectedSet.status === 'error' && isNotFound(selectedSet.error))) {
      notFoundHeadingRef.current?.focus()
    }
  }, [invalidOperationRoute, selectedSet.error, selectedSet.status])

  useEffect(() => {
    if (tree.status === 'success' && tree.snapshotId) {
      treeHeadingRef.current?.focus()
    }
  }, [tree.breadcrumbs, tree.snapshotId, tree.status])

  function refreshSelectedData() {
    if (!selectedBackupSetId) {
      return
    }
    mutationRef.current = null
    setMutationAction(null)
    setMutationError(undefined)
    setSelectedSnapshotId(undefined)
    setSelectedOperation(undefined)
    setOperationDetail({ status: 'idle' })
    setSnapshotDetail({ status: 'idle' })
    setTree(initialTreeState())
    setRefreshVersion((current) => current + 1)
  }

  async function loadMoreSets() {
    if (!sets.nextCursor || !sets.hasMore || setsLoadingMore) {
      return
    }
    const requestId = setsRequestId.current + 1
    setsRequestId.current = requestId
    setsPaginationController.current?.abort()
    const controller = new AbortController()
    setsPaginationController.current = controller
    setSetsLoadingMore(true)
    try {
      const response = await backupApi.listBackupSets({
        cursor: sets.nextCursor,
        limit: SET_PAGE_LIMIT,
        signal: controller.signal,
      })
      if (!mounted.current || controller.signal.aborted || setsRequestId.current !== requestId) {
        return
      }
      setSets((current) => ({
        status: 'success',
        items: [...current.items, ...response.data],
        nextCursor: response.page.next_cursor,
        hasMore: response.page.has_more,
      }))
    } catch (error) {
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      setSets((current) => ({ ...current, error }))
      recoverSession(error)
    } finally {
      if (setsPaginationController.current === controller) {
        setsPaginationController.current = undefined
      }
      if (mounted.current) {
        setSetsLoadingMore(false)
      }
    }
  }

  async function loadMoreSnapshots() {
    if (!selectedBackupSetId || !snapshots.nextCursor || !snapshots.hasMore || snapshotsLoadingMore) {
      return
    }
    const selectedId = selectedBackupSetId
    const requestId = snapshotsRequestId.current + 1
    snapshotsRequestId.current = requestId
    snapshotsPaginationController.current?.abort()
    const controller = new AbortController()
    snapshotsPaginationController.current = controller
    setSnapshotsLoadingMore(true)
    try {
      const response = await backupApi.listBackupSnapshots(selectedId, {
        cursor: snapshots.nextCursor,
        limit: SNAPSHOT_PAGE_LIMIT,
        signal: controller.signal,
      })
      if (
        !mounted.current ||
        controller.signal.aborted ||
        snapshotsRequestId.current !== requestId ||
        selectedBackupSetId !== selectedId
      ) {
        return
      }
      if (!response.data.every((snapshot) => snapshot.backup_set_id === selectedId)) {
        throw new Error('snapshot scope mismatch')
      }
      setSnapshots((current) => ({
        status: 'success',
        items: [...current.items, ...response.data],
        nextCursor: response.page.next_cursor,
        hasMore: response.page.has_more,
      }))
    } catch (error) {
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      setSnapshots((current) => ({ ...current, error }))
      recoverSession(error)
    } finally {
      if (snapshotsPaginationController.current === controller) {
        snapshotsPaginationController.current = undefined
      }
      if (mounted.current) {
        setSnapshotsLoadingMore(false)
      }
    }
  }

  async function loadMoreOperations() {
    if (
      !selectedBackupSetId ||
      !operations.nextCursor ||
      !operations.hasMore ||
      operationsLoadingMoreRef.current
    ) {
      return
    }
    const selectedId = selectedBackupSetId
    const selectedFilter = operationFilter
    const requestId = operationsRequestId.current + 1
    operationsRequestId.current = requestId
    operationsPaginationController.current?.abort()
    const controller = new AbortController()
    operationsPaginationController.current = controller
    operationsLoadingMoreRef.current = true
    setOperationsLoadingMore(true)
    try {
      const response = await backupApi.listBackupOperations(selectedId, {
        cursor: operations.nextCursor,
        kind: selectedFilter === 'ALL' ? undefined : selectedFilter,
        limit: OPERATION_PAGE_LIMIT,
        signal: controller.signal,
      })
      if (
        !mounted.current ||
        controller.signal.aborted ||
        operationsRequestId.current !== requestId ||
        selectedBackupSetId !== selectedId ||
        operationFilter !== selectedFilter
      ) {
        return
      }
      if (!response.data.every((operation) =>
        operation.backup_set_id === selectedId &&
        (selectedFilter === 'ALL' || operation.operation_kind === selectedFilter)
      )) {
        throw new Error('operation scope mismatch')
      }
      setOperations((current) => ({
        status: 'success',
        items: appendUniqueOperations(current.items, response.data),
        nextCursor: response.page.next_cursor,
        hasMore: response.page.has_more,
      }))
    } catch (error) {
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      setOperations((current) => ({ ...current, error }))
      recoverSession(error)
    } finally {
      if (operationsPaginationController.current === controller) {
        operationsPaginationController.current = undefined
      }
      if (mounted.current) {
        operationsLoadingMoreRef.current = false
        setOperationsLoadingMore(false)
      }
    }
  }

  async function loadMoreTree() {
    if (!selectedSnapshotId || !tree.nextCursor || !tree.hasMore || treeLoadingMore) {
      return
    }
    setTreeLoadingMore(true)
    try {
      await loadTree({
        snapshotId: selectedSnapshotId,
        parentNodeId: tree.parentNodeId,
        breadcrumbs: tree.breadcrumbs,
        cursor: tree.nextCursor,
        append: true,
      })
    } finally {
      if (mounted.current) {
        setTreeLoadingMore(false)
      }
    }
  }

  function selectBackupSet(event: ChangeEvent<HTMLSelectElement>) {
    const nextId = event.currentTarget.value
    if (nextId === selectedBackupSetId) {
      return
    }
    mutationRef.current = null
    setMutationAction(null)
    setMutationError(undefined)
    setMutationNotice(undefined)
    setOperationFilter('ALL')
    setSelectedSnapshotId(undefined)
    setSelectedOperation(undefined)
    setSelectedBackupSetId(nextId)
    navigate(`/backups/${encodeURIComponent(nextId)}`)
  }

  function selectSnapshot(snapshotId: string) {
    setSelectedSnapshotId((current) => (current === snapshotId ? undefined : snapshotId))
  }

  function selectOperation(operation: BackupOperationSummary) {
    const isCurrent =
      selectedOperation?.operationId === operation.operation_id &&
      selectedOperation.operationKind === operation.operation_kind
    if (isCurrent) {
      setSelectedOperation(undefined)
      navigate(`/backups/${encodeURIComponent(operation.backup_set_id)}`)
      return
    }
    setSelectedOperation({
      operationKind: operation.operation_kind,
      operationId: operation.operation_id,
    })
    navigate(
      `/backups/${encodeURIComponent(operation.backup_set_id)}/operations/${encodeURIComponent(operation.operation_kind)}/${encodeURIComponent(operation.operation_id)}`,
    )
  }

  function openDirectory(node: BackupSnapshotNode) {
    if (node.kind !== 'DIRECTORY' || !selectedSnapshotId) {
      return
    }
    void loadTree({
      snapshotId: selectedSnapshotId,
      parentNodeId: node.snapshot_node_id,
      breadcrumbs: [...tree.breadcrumbs, { nodeId: node.snapshot_node_id, name: node.name }],
      append: false,
    })
  }

  function goToBreadcrumb(index: number) {
    if (!selectedSnapshotId) {
      return
    }
    const breadcrumb = tree.breadcrumbs[index]
    if (!breadcrumb) {
      return
    }
    void loadTree({
      snapshotId: selectedSnapshotId,
      parentNodeId: breadcrumb.nodeId,
      breadcrumbs: tree.breadcrumbs.slice(0, index + 1),
      append: false,
    })
  }

  async function submitCreateMaintenance(idempotencyKey?: string) {
    if (!selectedBackupSetId || mutationRef.current?.phase === 'submitting') {
      return
    }
    let requestKey = idempotencyKey
    if (!requestKey) {
      try {
        requestKey = createUuidV7()
      } catch {
        setMutationError('Secure browser randomness is unavailable. The backup run was not created.')
        return
      }
    }
    const action: MutationAction = {
      kind: 'create',
      idempotencyKey: requestKey,
      phase: 'submitting',
    }
    const recoveryRecord = mutationRecoveryStore.add({
      action_kind: 'create_maintenance_run',
      idempotency_key: requestKey,
      request_scope: { backupSetId: selectedBackupSetId },
      request: {},
    })
    if (mutationRecoveryStore.isAvailable) {
      setPendingMaintenance(recoveryRecord)
      setMaintenanceRecoveryReady(true)
    }
    mutationRef.current = action
    setMutationAction(action)
    setMutationError(undefined)
    setMutationNotice(undefined)
    let postConfirmed = false
    try {
      const response = await backupApi.createBackupMaintenanceRun(selectedBackupSetId, requestKey)
      postConfirmed = true
      mutationRecoveryStore.remove(recoveryRecord.action_id)
      setPendingMaintenance((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      setMaintenanceRecoveryReady(false)
      if (!mounted.current) {
        return
      }
      mutationRef.current = null
      setMutationAction(null)
      setMutationNotice('Backup run created. Recent activity now reflects the server-confirmed state.')
      setSelectedOperation({
        operationKind: 'MAINTENANCE',
        operationId: response.data.maintenance_run_id,
      })
      setRefreshVersion((current) => current + 1)
    } catch (error) {
      const recoveryOutcome = postConfirmed
        ? 'removed'
        : settlePendingMutationFailure(recoveryRecord, error)
      if (recoveryOutcome === 'removed') {
        setPendingMaintenance((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
        setMaintenanceRecoveryReady(false)
      }
      if (!mounted.current) {
        return
      }
      if (recoveryOutcome === 'retained') {
        const failedAction: MutationAction = { ...action, phase: 'failed' }
        mutationRef.current = failedAction
        setMutationAction(failedAction)
      } else {
        mutationRef.current = null
        setMutationAction(null)
      }
      setMutationError(mutationErrorMessage('create', error))
      recoverSession(error)
    }
  }

  async function submitAdvanceMaintenance(
    maintenanceRunId: string,
    idempotencyKey?: string,
    expectedStateOverride?: BackupMaintenanceRunState,
  ) {
    if (mutationRef.current?.phase === 'submitting') {
      return
    }
    let requestKey = idempotencyKey
    if (!requestKey) {
      try {
        requestKey = createUuidV7()
      } catch {
        setMutationError('Secure browser randomness is unavailable. The maintenance step was not submitted.')
        return
      }
    }
    const expectedState = expectedStateOverride ?? (
      currentMaintenanceOperation?.maintenance_run_id === maintenanceRunId &&
      MAINTENANCE_STATES.has(currentMaintenanceOperation.state as BackupMaintenanceRunState)
        ? currentMaintenanceOperation.state as BackupMaintenanceRunState
        : undefined
    )
    if (!expectedState) {
      setMutationError('Refresh the maintenance run before continuing. No maintenance step was submitted.')
      return
    }
    const action: MutationAction = {
      kind: 'advance',
      maintenanceRunId,
      expectedState,
      idempotencyKey: requestKey,
      phase: 'submitting',
    }
    const recoveryRecord = mutationRecoveryStore.add({
      action_kind: 'advance_maintenance_run',
      idempotency_key: requestKey,
      request_scope: { backupSetId: selectedBackupSetId, maintenanceRunId },
      request: { expected_state: expectedState },
    })
    if (mutationRecoveryStore.isAvailable) {
      setPendingMaintenance(recoveryRecord)
      setMaintenanceRecoveryReady(true)
    }
    mutationRef.current = action
    setMutationAction(action)
    setMutationError(undefined)
    setMutationNotice(undefined)
    let postConfirmed = false
    try {
      const response = await backupApi.advanceBackupMaintenanceRun(maintenanceRunId, requestKey)
      postConfirmed = true
      mutationRecoveryStore.remove(recoveryRecord.action_id)
      setPendingMaintenance((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      setMaintenanceRecoveryReady(false)
      if (!mounted.current) {
        return
      }
      mutationRef.current = null
      setMutationAction(null)
      setMutationNotice('The maintenance step was recorded. Activity now reflects the server-confirmed state.')
      setSelectedOperation({
        operationKind: 'MAINTENANCE',
        operationId: response.data.maintenance_run_id,
      })
      setRefreshVersion((current) => current + 1)
    } catch (error) {
      const recoveryOutcome = postConfirmed
        ? 'removed'
        : settlePendingMutationFailure(recoveryRecord, error)
      if (recoveryOutcome === 'removed') {
        setPendingMaintenance((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
        setMaintenanceRecoveryReady(false)
      }
      if (!mounted.current) {
        return
      }
      if (recoveryOutcome === 'retained') {
        const failedAction: MutationAction = { ...action, phase: 'failed' }
        mutationRef.current = failedAction
        setMutationAction(failedAction)
      } else {
        mutationRef.current = null
        setMutationAction(null)
      }
      setMutationError(mutationErrorMessage('advance', error))
      recoverSession(error)
    }
  }

  async function retryMutation() {
    const action = mutationRef.current
    if (!action || action.phase !== 'failed') {
      return
    }
    if (action.kind === 'create') {
      await submitCreateMaintenance(action.idempotencyKey)
    } else if (action.maintenanceRunId) {
      await submitAdvanceMaintenance(action.maintenanceRunId, action.idempotencyKey, action.expectedState)
    }
  }

  function retryPendingMaintenance() {
    const record = pendingMaintenance
    if (!record || !maintenanceRecoveryReady) {
      return
    }
    if (record.action_kind === 'create_maintenance_run') {
      void submitCreateMaintenance(record.idempotency_key)
      return
    }
    const runId = record.request_scope.maintenanceRunId
    const expectedState = pendingMaintenanceExpectedState(record)
    if (runId && expectedState) {
      void submitAdvanceMaintenance(runId, record.idempotency_key, expectedState)
    }
  }

  function discardPendingMaintenance() {
    const record = pendingMaintenance
    if (!record) {
      return
    }
    mutationRecoveryStore.remove(record.action_id)
    if (mutationRef.current?.idempotencyKey === record.idempotency_key) {
      mutationRef.current = null
      setMutationAction(null)
      setMutationError(undefined)
    }
    setPendingMaintenance(undefined)
    setMaintenanceRecoveryReady(false)
  }

  if (invalidOperationRoute) {
    return (
      <div className="backup-page">
        <PageIntro onRefresh={undefined} />
        <section className="panel backup-empty-state" aria-labelledby="operation-route-not-found-title">
          <p className="eyebrow">Operation</p>
          <h2 id="operation-route-not-found-title" ref={notFoundHeadingRef} tabIndex={-1}>Operation not available</h2>
          <p>This item is not available.</p>
          <Link className="button-link button--secondary" to="/backups">Back to Backup Control Center</Link>
        </section>
      </div>
    )
  }

  if (sets.status === 'loading' && !currentSet && selectedSet.status !== 'error') {
    return (
      <div className="backup-page">
        <PageIntro onRefresh={undefined} />
        <section className="panel" aria-labelledby="backup-loading-title">
          <h2 id="backup-loading-title">Loading backups</h2>
          <LoadingBlock label="Finding your backup sets…" />
        </section>
      </div>
    )
  }

  if (selectedSet.status === 'error' && isNotFound(selectedSet.error)) {
    return (
      <div className="backup-page">
        <PageIntro onRefresh={undefined} />
        <section className="panel backup-empty-state" aria-labelledby="backup-not-found-title">
          <p className="eyebrow">Backup set</p>
          <h2 id="backup-not-found-title" ref={notFoundHeadingRef} tabIndex={-1}>Backup set not available</h2>
          <p>The requested backup set could not be found.</p>
          <Link className="button-link button--secondary" to="/backups">Back to Backup Control Center</Link>
        </section>
      </div>
    )
  }

  if (selectedSet.status === 'error' || (sets.status === 'error' && !currentSet)) {
    return (
      <div className="backup-page">
        <PageIntro onRefresh={undefined} />
        <section className="panel" aria-labelledby="backup-load-error-title">
          <h2 id="backup-load-error-title">Backup set could not be loaded</h2>
          <SectionError
            message={sectionErrorMessage(selectedSet.error ?? sets.error, 'this backup set')}
            onRetry={refreshSelectedData}
          />
        </section>
      </div>
    )
  }

  if (!selectedBackupSetId || !currentSet) {
    return (
      <div className="backup-page">
        <PageIntro onRefresh={undefined} />
        <section className="panel" aria-labelledby="backup-selection-title">
          <h2 id="backup-selection-title">Preparing your backup overview</h2>
          <LoadingBlock label="Selecting a backup set…" />
        </section>
      </div>
    )
  }

  const policyData = policy.data
  const canRunMaintenance =
    policy.status === 'success' &&
    policyData !== null &&
    operations.status === 'success' &&
    operationFilter === 'ALL' &&
    !activeMaintenance &&
    currentSet.state !== 'DISABLED' &&
    !mutationAction &&
    !pendingMaintenance
  const latestSnapshotCopy = latestCompletedSnapshot ? (
    <Timestamp value={latestCompletedSnapshot.committed_at ?? latestCompletedSnapshot.created_at} />
  ) : snapshots.hasMore ? (
    'Load more history'
  ) : (
    'None yet'
  )
  const snapshotCount = `${formatInteger(snapshots.items.length)}${snapshots.hasMore ? '+' : ''}`

  return (
    <div className="backup-page">
      <PageIntro onRefresh={refreshSelectedData} />

      {pendingMaintenance && (
        <MutationRecoveryNotice
          record={pendingMaintenance}
          title={pendingMaintenance.action_kind === 'create_maintenance_run'
            ? 'Finish creating this backup run'
            : 'Finish the pending maintenance step'}
          busy={maintenanceRecoveryChecking || mutationAction?.phase === 'submitting'}
          canRetry={maintenanceRecoveryReady}
          onCheckStatus={pendingMaintenance.action_kind === 'advance_maintenance_run'
            ? () => void reconcilePendingAdvance(pendingMaintenance)
            : undefined}
          onRetry={retryPendingMaintenance}
          onDiscard={discardPendingMaintenance}
        />
      )}

      <section className="panel backup-overview" aria-labelledby="backup-overview-title">
        <div className="backup-section-header">
          <div>
            <p className="eyebrow">Selected backup set</p>
            <h2 id="backup-overview-title">{currentSet.name}</h2>
            <p className="backup-section-subtitle">
              Your immutable backup history and the actions that need your attention.
            </p>
          </div>
          <StatusPill tone={currentSet.state === 'DISABLED' ? 'planned' : 'info'}>
            {setStateLabel(currentSet.state)}
          </StatusPill>
        </div>

        {sets.items.length > 1 || sets.hasMore ? (
          <div className="backup-set-selector field">
            <label htmlFor="backup-set-select">Backup set</label>
            <select id="backup-set-select" value={selectedBackupSetId} onChange={selectBackupSet}>
              {!sets.items.some((set) => set.backup_set_id === currentSet.backup_set_id) && (
                <option value={currentSet.backup_set_id}>{currentSet.name}</option>
              )}
              {sets.items.map((set) => (
                <option value={set.backup_set_id} key={set.backup_set_id}>
                  {set.name}
                </option>
              ))}
            </select>
            {sets.hasMore && (
              <button
                type="button"
                className="button--secondary backup-small-action"
                onClick={() => void loadMoreSets()}
                disabled={setsLoadingMore}
              >
                {setsLoadingMore ? 'Loading sets…' : 'Load more sets'}
              </button>
            )}
            {sets.status === 'success' && sets.error !== undefined && (
              <SectionError
                message={sectionErrorMessage(sets.error, 'more backup sets')}
                onRetry={() => setSetsRefreshVersion((current) => current + 1)}
              />
            )}
          </div>
        ) : (
          <p className="backup-selected-set-note">
            Backup set: <strong>{currentSet.name}</strong>
          </p>
        )}

        <dl className="backup-summary-grid">
          <SummaryMetric label="Backup set status">
            <span>{setStateLabel(currentSet.state)}</span>
          </SummaryMetric>
          <SummaryMetric label="Latest completed snapshot">
            {latestSnapshotCopy}
          </SummaryMetric>
          <SummaryMetric label="Snapshots in view">
            {snapshots.status === 'loading' ? 'Loading…' : snapshotCount}
          </SummaryMetric>
          <SummaryMetric label="Latest activity">
            {operations.status === 'loading' ? (
              'Loading…'
            ) : latestOperation ? (
              <span>
                {operationKindLabel(latestOperation.operation_kind)} · {operationPhaseLabel(latestOperation.phase)}
              </span>
            ) : (
              'No activity yet'
            )}
          </SummaryMetric>
        </dl>

        <div className="backup-overview-lower">
          {policy.status === 'loading' && (
            <section className="backup-retention" aria-labelledby="retention-loading-title">
              <h3 id="retention-loading-title">Retention</h3>
              <LoadingBlock label="Checking retention settings…" />
            </section>
          )}
          {policy.status === 'error' && (
            <section className="backup-retention" aria-labelledby="retention-error-title">
              <h3 id="retention-error-title">Retention</h3>
              <SectionError message={sectionErrorMessage(policy.error, 'the retention policy')} onRetry={refreshSelectedData} />
            </section>
          )}
          {policy.status === 'success' && (
            <RetentionPolicyEditor
              backupSetId={selectedBackupSetId}
              policy={policyData ?? null}
              backupApi={backupApi}
              onUpdated={(updatedPolicy) => setPolicy({ status: 'success', data: updatedPolicy })}
            />
          )}

          <section className="backup-maintenance" aria-labelledby="maintenance-title">
            <div className="backup-subsection-heading">
              <div>
                <p className="eyebrow">Manual action</p>
                <h3 id="maintenance-title">Run a backup</h3>
              </div>
              {activeMaintenance && <StatusPill tone="planned">Action needed</StatusPill>}
            </div>
            <p>
              Start a durable backup run for this set. Synveil will show each server-confirmed step
              as you continue it.
            </p>
            <button
              type="button"
              onClick={() => void submitCreateMaintenance()}
              disabled={!canRunMaintenance || mutationSubmitting}
            >
              {mutationSubmitting && mutationAction?.kind === 'create' ? 'Submitting…' : 'Run backup now'}
            </button>
            {mutationAction?.kind === 'create' && mutationAction.phase === 'failed' && (
              <button type="button" className="button--secondary" onClick={() => void retryMutation()}>
                Try the request again
              </button>
            )}
            <p className="backup-action-explanation">
              {policy.status === 'loading'
                ? 'Checking whether this backup set is ready…'
                : policy.status === 'error'
                  ? 'Refresh the retention section before starting a run.'
                  : policyData === null
                    ? 'A retention policy is required before a run can start.'
                    : operations.status === 'loading'
                      ? 'Checking recent activity…'
                      : operationFilter !== 'ALL'
                        ? 'Show All activity to check for a waiting maintenance run.'
                        : activeMaintenance
                        ? 'A backup run is already waiting for its next explicit step.'
                        : currentSet.state === 'DISABLED'
                          ? 'This backup set is disabled. The server will remain the final authority.'
                          : mutationAction
                            ? 'Finish or refresh the current request before starting another one.'
                            : 'This action is explicit and does not run automatically.'}
            </p>
            {mutationError && (
              <div className="backup-mutation-message" role="alert" aria-live="assertive">
                <p>{mutationError}</p>
                <button type="button" className="button--secondary" onClick={refreshSelectedData}>
                  Refresh activity
                </button>
              </div>
            )}
            {mutationNotice && (
              <p className="backup-mutation-message backup-mutation-message--success" role="status">
                {mutationNotice}
              </p>
            )}
          </section>
        </div>
      </section>

      <div className="backup-content-grid">
        <section className="panel backup-section" aria-labelledby="snapshots-title">
          <div className="backup-section-header backup-section-header--compact">
            <div>
              <p className="eyebrow">History</p>
              <h2 id="snapshots-title">Snapshots</h2>
              <p className="backup-section-subtitle">
                Immutable points in time for this backup set. Expired history remains visible.
              </p>
            </div>
          </div>
          {snapshots.status === 'loading' && <LoadingBlock label="Loading snapshots…" />}
          {snapshots.status === 'error' && (
            <SectionError
              message={sectionErrorMessage(snapshots.error, 'snapshots')}
              onRetry={refreshSelectedData}
            />
          )}
          {snapshots.status === 'success' && snapshots.error !== undefined && (
            <SectionError
              message={sectionErrorMessage(snapshots.error, 'more snapshots')}
              onRetry={refreshSelectedData}
            />
          )}
          {snapshots.status === 'success' && snapshots.items.length === 0 && (
            <div className="backup-empty-copy">
              <p>No snapshots are available for this backup set yet.</p>
            </div>
          )}
          {snapshots.status === 'success' && snapshots.items.length > 0 && (
            <ul className="backup-snapshot-list">
              {snapshots.items.map((snapshot) => {
                const isSelected = selectedSnapshotId === snapshot.snapshot_id
                return (
                  <li key={snapshot.snapshot_id}>
                    <button
                      type="button"
                      className={isSelected ? 'backup-list-button backup-list-button--selected' : 'backup-list-button'}
                      onClick={() => selectSnapshot(snapshot.snapshot_id)}
                      aria-expanded={isSelected}
                    >
                      <span className="backup-list-button-main">
                        <span className="backup-list-button-title">
                          <StatusPill tone={snapshotStateTone(snapshot.state)}>
                            {snapshotStateLabel(snapshot.state)}
                          </StatusPill>
                          <Timestamp value={snapshot.committed_at ?? snapshot.created_at} />
                        </span>
                        <span className="backup-list-button-secondary">
                          {formatInteger(snapshot.logical_node_count)} logical items · {formatInteger(snapshot.content_reference_count)} content references
                          {snapshot.expired_at && <> · Expired <Timestamp value={snapshot.expired_at} /></>}
                        </span>
                        <SnapshotStateCopy state={snapshot.state} />
                      </span>
                      <span className="backup-list-chevron" aria-hidden="true">{isSelected ? '−' : '+'}</span>
                    </button>
                  </li>
                )
              })}
            </ul>
          )}
          {snapshots.status === 'success' && snapshots.hasMore && (
            <button
              type="button"
              className="button--secondary backup-load-more"
              onClick={() => void loadMoreSnapshots()}
              disabled={snapshotsLoadingMore}
            >
              {snapshotsLoadingMore ? 'Loading more snapshots…' : 'Load more snapshots'}
            </button>
          )}
        </section>

        <section className="panel backup-section" aria-labelledby="activity-title">
          <div className="backup-section-header backup-section-header--compact">
            <div>
              <p className="eyebrow">Activity</p>
              <h2 id="activity-title">Recent activity</h2>
              <p className="backup-section-subtitle">
                Server-confirmed maintenance, restore, and prune history for this set.
              </p>
            </div>
            <button type="button" className="button--secondary backup-small-action" onClick={refreshSelectedData}>
              Refresh
            </button>
          </div>
          <div className="backup-filter-row">
            <label htmlFor="operation-filter">Show</label>
            <select
              id="operation-filter"
              value={operationFilter}
              onChange={(event) => {
                setSelectedOperation(undefined)
                setOperationDetail({ status: 'idle' })
                setOperationFilter(event.currentTarget.value as OperationFilter)
              }}
            >
              <option value="ALL">All activity</option>
              <option value="MAINTENANCE">Backup maintenance</option>
              <option value="RESTORE">Restore</option>
              <option value="PRUNE">Prune</option>
            </select>
          </div>
          {operations.status === 'loading' && <LoadingBlock label="Loading activity…" />}
          {operations.status === 'error' && (
            <SectionError
              message={sectionErrorMessage(operations.error, 'activity')}
              onRetry={refreshSelectedData}
            />
          )}
          {operations.status === 'success' && operations.error !== undefined && (
            <SectionError
              message={sectionErrorMessage(operations.error, 'latest activity')}
              onRetry={refreshSelectedData}
            />
          )}
          {operations.status === 'success' && operations.items.length === 0 && (
            <div className="backup-empty-copy">
              <p>No activity has been recorded for this backup set yet.</p>
            </div>
          )}
          {operations.status === 'success' && operations.items.length > 0 && (
            <ul className="backup-operation-list">
              {operations.items.map((operation) => {
                const isSelected = selectedOperation?.operationId === operation.operation_id && selectedOperation.operationKind === operation.operation_kind
                return (
                  <li key={operationKey(operation)}>
                    <button
                      type="button"
                      className={isSelected ? 'backup-list-button backup-list-button--selected' : 'backup-list-button'}
                      onClick={() => selectOperation(operation)}
                      aria-expanded={isSelected}
                    >
                      <span className="backup-list-button-main">
                        <span className="backup-list-button-title">
                          <span>{operationKindLabel(operation.operation_kind)}</span>
                          <StatusPill tone={operationPhaseTone(operation.phase)}>
                            {operationPhaseLabel(operation.phase)}
                          </StatusPill>
                        </span>
                        <span className="backup-list-button-secondary">
                          {operationStateLabel(operation.state)} · Created <Timestamp value={operation.created_at} />
                        </span>
                        <span className="backup-operation-progress-line">
                          <ProgressText progress={operation.progress} />
                          <span>Updated <Timestamp value={operation.last_transition_at} /></span>
                        </span>
                      </span>
                      <span className="backup-list-chevron" aria-hidden="true">{isSelected ? '−' : '+'}</span>
                    </button>
                  </li>
                )
              })}
            </ul>
          )}
          {operations.status === 'success' && operations.hasMore && (
            <button
              type="button"
              className="button--secondary backup-load-more"
              onClick={() => void loadMoreOperations()}
              disabled={operationsLoadingMore}
            >
              {operationsLoadingMore ? 'Loading more activity…' : 'Load more activity'}
            </button>
          )}
        </section>
      </div>

      {selectedSnapshotId && (
        <section className="panel backup-detail-panel" aria-labelledby="snapshot-detail-title">
          <div className="backup-section-header backup-section-header--compact">
            <div>
              <p className="eyebrow">Snapshot inspection</p>
              <h2 id="snapshot-detail-title">Snapshot details</h2>
              <p className="backup-section-subtitle">
                Browse the immutable logical tree without opening or downloading file content.
              </p>
            </div>
            <div className="backup-detail-actions">
              {snapshotDetail.data?.state === 'COMPLETED' && (
                <button
                  type="button"
                  onClick={() =>
                    navigate(
                      `/backups/${encodeURIComponent(selectedBackupSetId)}/snapshots/${encodeURIComponent(snapshotDetail.data?.snapshot_id ?? selectedSnapshotId)}/restore`,
                    )
                  }
                >
                  Restore
                </button>
              )}
              {snapshotDetail.data?.state === 'EXPIRED' && (
                <button
                  type="button"
                  className="button--danger-outline"
                  onClick={() =>
                    navigate(
                      `/backups/${encodeURIComponent(selectedBackupSetId)}/snapshots/${encodeURIComponent(snapshotDetail.data?.snapshot_id ?? selectedSnapshotId)}/prune`,
                    )
                  }
                >
                  Release retained content
                </button>
              )}
              <button type="button" className="button--secondary backup-small-action" onClick={() => setSelectedSnapshotId(undefined)}>
                Close details
              </button>
            </div>
          </div>
          {snapshotDetail.status === 'loading' && <LoadingBlock label="Loading snapshot details…" />}
          {snapshotDetail.status === 'error' && (
            <SectionError
              message={sectionErrorMessage(snapshotDetail.error, 'snapshot details')}
              onRetry={() => setSnapshotDetailRefreshVersion((current) => current + 1)}
            />
          )}
          {snapshotDetail.status === 'success' && snapshotDetail.data && (
            <>
              <dl className="backup-detail-grid">
                <SummaryMetric label="State">
                  <span className="backup-detail-value">
                    <StatusPill tone={snapshotStateTone(snapshotDetail.data.state)}>
                      {snapshotStateLabel(snapshotDetail.data.state)}
                    </StatusPill>
                    <SnapshotStateCopy state={snapshotDetail.data.state} />
                  </span>
                </SummaryMetric>
                <SummaryMetric label="Created"><Timestamp value={snapshotDetail.data.created_at} /></SummaryMetric>
                <SummaryMetric label="Committed"><Timestamp value={snapshotDetail.data.committed_at} /></SummaryMetric>
                <SummaryMetric label="Expired"><Timestamp value={snapshotDetail.data.expired_at} /></SummaryMetric>
                <SummaryMetric label="Logical content">
                  {formatInteger(snapshotDetail.data.logical_node_count)} items
                </SummaryMetric>
                <SummaryMetric label="Content references">
                  {formatInteger(snapshotDetail.data.content_reference_count)}
                </SummaryMetric>
              </dl>
              <TechnicalDetails>
                <dl className="backup-technical-list">
                  <div><dt>Snapshot ID</dt><dd><code>{snapshotDetail.data.snapshot_id}</code></dd></div>
                  <div><dt>Backup set ID</dt><dd><code>{snapshotDetail.data.backup_set_id}</code></dd></div>
                  <div><dt>Source library ID</dt><dd><code>{snapshotDetail.data.library_id}</code></dd></div>
                </dl>
              </TechnicalDetails>
              {tree.status === 'loading' && <LoadingBlock label="Loading snapshot tree…" />}
              {tree.status === 'error' && (
                <SectionError
                  message={sectionErrorMessage(tree.error, 'the snapshot tree')}
                  onRetry={() => void loadTree({
                    snapshotId: selectedSnapshotId,
                    parentNodeId: tree.parentNodeId,
                    breadcrumbs: tree.breadcrumbs,
                    append: false,
                  })}
                />
              )}
              {tree.status === 'success' && tree.notAvailable && (
                <p className="backup-muted-copy">The logical tree is available after this snapshot reaches a readable state.</p>
              )}
              {tree.status === 'success' && !tree.notAvailable && (
                <div className="backup-tree" aria-labelledby="snapshot-tree-title">
                  <h3 id="snapshot-tree-title" ref={treeHeadingRef} tabIndex={-1}>Logical tree</h3>
                  <nav className="backup-breadcrumbs" aria-label="Snapshot folders">
                    <ol>
                      {tree.breadcrumbs.map((breadcrumb, index) => {
                        const current = index === tree.breadcrumbs.length - 1
                        return (
                          <li key={breadcrumb.nodeId ?? 'root'}>
                            {current ? (
                              <span aria-current="page">{breadcrumb.name}</span>
                            ) : (
                              <button type="button" className="backup-breadcrumb-button" onClick={() => goToBreadcrumb(index)}>
                                {breadcrumb.name}
                              </button>
                            )}
                          </li>
                        )
                      })}
                    </ol>
                  </nav>
                  {tree.items.length === 0 ? (
                    <p className="backup-empty-copy">This folder is empty.</p>
                  ) : (
                    <ul className="backup-tree-list">
                      {tree.items.map((node) => (
                        <li key={node.snapshot_node_id}>
                          <div className="backup-tree-row">
                            {node.kind === 'DIRECTORY' ? (
                              <button type="button" className="backup-tree-name" onClick={() => openDirectory(node)}>
                                <span className="backup-tree-icon" aria-hidden="true">▸</span>
                                <span>{node.name}</span>
                                <span className="visually-hidden">Open folder</span>
                              </button>
                            ) : (
                              <span className="backup-tree-name backup-tree-name--file">
                                <span className="backup-tree-icon" aria-hidden="true">□</span>
                                <span>{node.name}</span>
                              </span>
                            )}
                            <span className="backup-tree-kind">{node.kind === 'DIRECTORY' ? 'Folder' : 'File'}</span>
                            <span className="backup-tree-size">{node.kind === 'FILE' ? formatBytes(node.byte_length) : '—'}</span>
                            <span className="backup-tree-state">{nodeStateLabel(node.state)}</span>
                          </div>
                          {node.kind === 'FILE' && (
                            <TechnicalDetails>
                              <dl className="backup-technical-list">
                                <div><dt>Logical size</dt><dd>{formatBytes(node.byte_length)}</dd></div>
                                {node.file_version_id && <div><dt>File version ID</dt><dd><code>{node.file_version_id}</code></dd></div>}
                                {node.sha256 && <div><dt>SHA-256</dt><dd><code>{node.sha256}</code></dd></div>}
                              </dl>
                            </TechnicalDetails>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                  {tree.hasMore && (
                    <button type="button" className="button--secondary backup-load-more" onClick={() => void loadMoreTree()} disabled={treeLoadingMore}>
                      {treeLoadingMore ? 'Loading more items…' : 'Load more items'}
                    </button>
                  )}
                </div>
              )}
            </>
          )}
        </section>
      )}

      {selectedOperation && (
        <section className="panel backup-detail-panel" aria-labelledby="operation-detail-title">
          <div className="backup-section-header backup-section-header--compact">
            <div>
              <p className="eyebrow">Activity inspection</p>
              <h2 id="operation-detail-title" ref={operationDetailHeadingRef} tabIndex={-1}>Operation details</h2>
              <p className="backup-section-subtitle">
                Durable state and step progress from the unified activity projection.
              </p>
            </div>
            <button
              type="button"
              className="button--secondary backup-small-action"
              onClick={() => {
                setSelectedOperation(undefined)
                navigate(`/backups/${encodeURIComponent(selectedBackupSetId)}`)
              }}
            >
              Close details
            </button>
          </div>
          {operationDetail.status === 'loading' && <LoadingBlock label="Loading operation details…" />}
          {operationDetail.status === 'error' && (
            <SectionError
              message={sectionErrorMessage(operationDetail.error, 'operation details')}
              onRetry={() => setSelectedOperation({ ...selectedOperation })}
            />
          )}
          {operationDetail.status === 'success' && operationDetail.data && (
            <OperationDetailContent
              operation={operationDetail.data}
              maintenanceOperation={currentMaintenanceOperation}
              mutationAction={mutationAction}
              onAdvance={(runId, state) => void submitAdvanceMaintenance(runId, undefined, state)}
              onRetry={() => void retryMutation()}
            />
          )}
        </section>
      )}
    </div>
  )
}

function PageIntro({ onRefresh }: { readonly onRefresh?: () => void }) {
  return (
    <section className="backup-page-heading" aria-labelledby="backups-page-title">
      <div>
        <p className="eyebrow">Backup Control Center</p>
        <h1 id="backups-page-title">Backup set details</h1>
        <p>
          Inspect snapshots, retention settings, and server-confirmed activity in one place.
        </p>
      </div>
      <div className="backup-heading-actions">
        <Link className="button-link button--secondary" to="/backups">All backup sets</Link>
        {onRefresh && (
          <button type="button" className="button--secondary" onClick={onRefresh}>
            Refresh backup data
          </button>
        )}
      </div>
    </section>
  )
}

function OperationDetailContent({
  operation,
  maintenanceOperation,
  mutationAction,
  onAdvance,
  onRetry,
}: {
  readonly operation: BackupOperationDetail
  readonly maintenanceOperation?: BackupMaintenanceOperation
  readonly mutationAction: MutationAction | null
  readonly onAdvance: (
    maintenanceRunId: string,
    expectedState: BackupMaintenanceRunState,
  ) => void
  readonly onRetry: () => void
}) {
  const navigate = useNavigate()
  const stale = operation.terminal && operation.phase === 'STALE'
  const maintenanceReady =
    maintenanceOperation &&
    maintenanceOperation.next_action === 'ADVANCE' &&
    !maintenanceOperation.terminal

  return (
    <div className="backup-operation-detail">
      <div className="backup-operation-detail-summary">
        <div>
          <p className="eyebrow">{operationKindLabel(operation.operation_kind)}</p>
          <h3>{operationPhaseLabel(operation.phase)}</h3>
        </div>
        <StatusPill tone={operationPhaseTone(operation.phase)}>{operationStateLabel(operation.state)}</StatusPill>
      </div>
      <div className="backup-operation-detail-progress">
        <ProgressText progress={operation.progress} />
        <span>Last changed <Timestamp value={operation.last_transition_at} /></span>
      </div>
      {stale && (
        <p className="backup-detail-callout backup-detail-callout--warning">
          This operation can no longer continue. {operation.operation_kind === 'MAINTENANCE' ? 'Start a new maintenance run when you are ready.' : 'A new plan will be needed for a later action.'}
        </p>
      )}
      {operation.operation_kind === 'MAINTENANCE' && (
        <>
          <dl className="backup-detail-grid">
            <SummaryMetric label="Created"><Timestamp value={operation.created_at} /></SummaryMetric>
            <SummaryMetric label="Next action">{nextActionLabel(operation.next_action)}</SummaryMetric>
            <SummaryMetric label="Policy revision">{formatInteger(operation.policy_revision_number)}</SummaryMetric>
            <SummaryMetric label="Snapshot captured"><Timestamp value={operation.snapshot_captured_at} /></SummaryMetric>
            <SummaryMetric label="Retention reviewed"><Timestamp value={operation.expiry_planned_at} /></SummaryMetric>
            <SummaryMetric label="Completed"><Timestamp value={operation.completed_at} /></SummaryMetric>
          </dl>
          {maintenanceReady && (
            <div className="backup-explicit-action">
              <p>Ready for the next step. Nothing will advance until you choose Continue.</p>
              <button
                type="button"
                onClick={() => onAdvance(
                  operation.maintenance_run_id,
                  operation.state as BackupMaintenanceRunState,
                )}
                disabled={mutationAction?.phase === 'submitting' || mutationAction?.kind === 'advance'}
              >
                {mutationAction?.kind === 'advance' && mutationAction.phase === 'submitting'
                  ? 'Submitting…'
                  : 'Continue backup maintenance'}
              </button>
              {mutationAction?.kind === 'advance' && mutationAction.phase === 'failed' && (
                <button type="button" className="button--secondary" onClick={onRetry}>
                  Try the request again
                </button>
              )}
            </div>
          )}
          <TechnicalDetails>
            <dl className="backup-technical-list">
              <div><dt>Operation ID</dt><dd><code>{operation.operation_id}</code></dd></div>
              <div><dt>Maintenance run ID</dt><dd><code>{operation.maintenance_run_id}</code></dd></div>
              {operation.captured_snapshot_id && <div><dt>Captured snapshot ID</dt><dd><code>{operation.captured_snapshot_id}</code></dd></div>}
            </dl>
          </TechnicalDetails>
        </>
      )}
      {operation.operation_kind === 'RESTORE' && (
        <>
          <dl className="backup-detail-grid">
            <SummaryMetric label="Created"><Timestamp value={operation.created_at} /></SummaryMetric>
            <SummaryMetric label="Next action">{nextActionLabel(operation.next_action)}</SummaryMetric>
            <SummaryMetric label="Planned items">{formatInteger(operation.planned_entry_count)}</SummaryMetric>
            <SummaryMetric label="Planned files">{formatInteger(operation.planned_file_count)}</SummaryMetric>
            <SummaryMetric label="Destination">{operation.destination_name}</SummaryMetric>
            <SummaryMetric label="Completed"><Timestamp value={operation.completed_at} /></SummaryMetric>
          </dl>
          {operation.state === 'PLANNED' && (
            <div className="backup-explicit-action">
              <p>The persisted restore plan is waiting for review. Execution remains an explicit action on the plan.</p>
              <button
                type="button"
                className="button--secondary"
                onClick={() =>
                  navigate(
                    `/backups/${encodeURIComponent(operation.backup_set_id)}/restore/${encodeURIComponent(operation.restore_plan_id)}`,
                  )
                }
              >
                Review restore plan
              </button>
            </div>
          )}
          {operation.state === 'EXECUTED' && (
            <p className="backup-muted-copy">
              Restore completed. This historical receipt is read-only.
            </p>
          )}
          {operation.state === 'STALE' && (
            <p className="backup-detail-callout backup-detail-callout--warning">
              This restore plan is stale. Create a new plan before restoring.
            </p>
          )}
          <TechnicalDetails>
            <dl className="backup-technical-list">
              <div><dt>Operation ID</dt><dd><code>{operation.operation_id}</code></dd></div>
              <div><dt>Source snapshot ID</dt><dd><code>{operation.source_snapshot_id}</code></dd></div>
            </dl>
          </TechnicalDetails>
        </>
      )}
      {operation.operation_kind === 'PRUNE' && (
        <>
          <dl className="backup-detail-grid">
            <SummaryMetric label="Created"><Timestamp value={operation.created_at} /></SummaryMetric>
            <SummaryMetric label="Next action">{nextActionLabel(operation.next_action)}</SummaryMetric>
            <SummaryMetric label="Planned retention references">{formatInteger(operation.planned_pin_release_count)}</SummaryMetric>
            <SummaryMetric label="Retained elsewhere">{formatInteger(operation.retained_by_other_reference_count)}</SummaryMetric>
            <SummaryMetric label="Would become unreferenced">{formatInteger(operation.would_become_unreferenced_count)}</SummaryMetric>
            <SummaryMetric label="Completed"><Timestamp value={operation.completed_at} /></SummaryMetric>
          </dl>
          {operation.state === 'PLANNED' && operation.next_action === 'EXECUTE' && (
            <div className="backup-explicit-action">
              <p>This persisted prune plan must be reviewed and explicitly confirmed before any retention authority is released.</p>
              <button
                type="button"
                className="button--danger-outline"
                onClick={() => navigate(`/backups/${encodeURIComponent(operation.backup_set_id)}/prune/${encodeURIComponent(operation.prune_plan_id)}`)}
              >
                Review prune plan
              </button>
            </div>
          )}
          {operation.state === 'STALE' && (
            <div className="backup-detail-callout backup-detail-callout--warning">
              <p>This operation is no longer valid because the underlying backup state changed. Create a new plan to continue safely.</p>
              <button
                type="button"
                className="button--secondary"
                onClick={() => navigate(`/backups/${encodeURIComponent(operation.backup_set_id)}/snapshots/${encodeURIComponent(operation.snapshot_id)}/prune`)}
              >
                Create a new plan
              </button>
            </div>
          )}
          {operation.state === 'EXECUTED' && (
            <p className="backup-muted-copy">
              Retention release completed. This historical record does not claim physical storage reclamation.
            </p>
          )}
          <TechnicalDetails>
            <dl className="backup-technical-list">
              <div><dt>Operation ID</dt><dd><code>{operation.operation_id}</code></dd></div>
              <div><dt>Snapshot ID</dt><dd><code>{operation.snapshot_id}</code></dd></div>
            </dl>
          </TechnicalDetails>
        </>
      )}
    </div>
  )
}
