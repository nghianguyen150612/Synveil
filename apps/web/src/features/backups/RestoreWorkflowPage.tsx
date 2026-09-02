import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type RefObject,
  type ReactNode,
} from 'react'
import { useNavigate, useParams } from 'react-router-dom'

import {
  backupApi as defaultBackupApi,
  createUuidV7,
  type BackupApi,
  type BackupOperationDetail,
  type BackupRestoreExecution,
  type BackupRestorePlan,
  type BackupSnapshot,
  type CreateBackupRestorePlanRequest,
} from '../../api/backups'
import { isApiRequestError } from '../../api/errors'
import {
  findPendingMutation,
  mutationRecoveryStore,
  settlePendingMutationFailure,
  type PendingMutationRecord,
} from '../../api/mutationRecovery'
import {
  fileMetadataApi as defaultFileMetadataApi,
  type FileMetadataApi,
  type LiveLibraryResource,
  type LiveNodeResource,
} from '../../api/files'
import { useAuth } from '../../auth/useAuth'
import { StatusPill, type StatusTone } from '../../components/ui/StatusPill'
import {
  type DestinationSelection,
  RestoreDestinationPicker,
} from './RestoreDestinationPicker'
import { MutationRecoveryNotice } from './MutationRecoveryNotice'

const NAMESPACE_PAGE_LIMIT = 50
const MAX_LIBRARY_LOOKUP_PAGES = 100
const MAX_DIRECTORY_DEPTH = 100
const MAX_DESTINATION_NAME_LENGTH = 1024

type LoadStatus = 'idle' | 'loading' | 'success' | 'error'
type WorkflowStep = 'DESTINATION' | 'REVIEW' | 'COMPLETE'

interface DataState<T> {
  readonly status: LoadStatus
  readonly data?: T
  readonly error?: unknown
}

interface MutationAction {
  readonly idempotencyKey: string
  readonly request?: CreateBackupRestorePlanRequest
  readonly phase: 'submitting' | 'failed'
}

interface DestinationDetails {
  readonly status: LoadStatus
  readonly libraryName?: string
  readonly folderName?: string
  readonly folderPath?: string
  readonly error?: unknown
}

export interface RestoreWorkflowPageProps {
  readonly backupApi?: BackupApi
  readonly fileApi?: FileMetadataApi
}

function isAbortError(error: unknown): boolean {
  return typeof DOMException !== 'undefined' && error instanceof DOMException && error.name === 'AbortError'
}

function pendingRestorePlanRequest(
  record: PendingMutationRecord,
): CreateBackupRestorePlanRequest | undefined {
  const targetLibraryId = record.request.target_library_id
  const targetParentNodeId = record.request.target_parent_node_id
  const pendingDestinationName = record.request.destination_name
  if (
    typeof targetLibraryId !== 'string' ||
    typeof targetParentNodeId !== 'string' ||
    typeof pendingDestinationName !== 'string'
  ) {
    return undefined
  }
  return {
    target_library_id: targetLibraryId,
    target_parent_node_id: targetParentNodeId,
    destination_name: pendingDestinationName,
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

function summaryCount(directoryCount: string, fileCount: string): string {
  if (/^(0|[1-9][0-9]*)$/.test(directoryCount) && /^(0|[1-9][0-9]*)$/.test(fileCount)) {
    try {
      return formatInteger((BigInt(directoryCount) + BigInt(fileCount)).toString())
    } catch {
      return `${formatInteger(directoryCount)} directories and ${formatInteger(fileCount)} files`
    }
  }
  return `${formatInteger(directoryCount)} directories and ${formatInteger(fileCount)} files`
}

function destinationNameByteLength(value: string): number {
  return typeof TextEncoder === 'undefined' ? value.length : new TextEncoder().encode(value).length
}

function planStateLabel(state: BackupRestorePlan['state']): string {
  switch (state) {
    case 'PLANNED':
      return 'Ready to restore'
    case 'STALE':
      return 'Stale'
    case 'EXECUTED':
      return 'Completed'
  }
}

function planStateTone(state: BackupRestorePlan['state']): StatusTone {
  switch (state) {
    case 'PLANNED':
      return 'planned'
    case 'STALE':
      return 'quiet'
    case 'EXECUTED':
      return 'info'
  }
}

function snapshotStateLabel(snapshot: BackupSnapshot): string {
  switch (snapshot.state) {
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

function snapshotStateTone(snapshot: BackupSnapshot): StatusTone {
  switch (snapshot.state) {
    case 'COMPLETED':
      return 'info'
    case 'EXPIRED':
    case 'BUILDING':
      return 'planned'
    case 'FAILED':
      return 'quiet'
  }
}

function loadErrorMessage(error: unknown, subject: string): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again to continue.'
    }
    if (error.status === 404) {
      return subject === 'restore plan'
        ? 'Restore plan not available.'
        : 'This snapshot is no longer available for restore.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Backup data is temporarily unavailable. Please try again.'
    }
  }
  return `We could not load ${subject}. Please try again.`
}

function planCreateErrorMessage(error: unknown): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again before creating a plan.'
    }
    if (error.status === 403) {
      return 'This request could not be verified. Refresh the page and try again.'
    }
    if (error.status === 404) {
      return 'The selected snapshot or destination is no longer available for restore.'
    }
    if (error.status === 409) {
      return 'That name is already in use in the selected folder, or the destination changed. Choose another name or folder.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Restore planning is temporarily unavailable. Please try again.'
    }
    if (error.status === 400) {
      return 'Check the destination details and try again.'
    }
  }
  return "We couldn't confirm the result. Retrying with the same request is safe."
}

function executeErrorMessage(error: unknown): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again before restoring.'
    }
    if (error.status === 403) {
      return 'This request could not be verified. Refresh the page and try again.'
    }
    if (error.status === 404) {
      return 'Restore plan not available.'
    }
    if (error.status === 409) {
      return 'The destination changed after this restore plan was created. Create a new plan before restoring.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Restore execution is temporarily unavailable. We could not confirm the result.'
    }
  }
  return "We couldn't confirm the result. Retrying with the same request is safe."
}

function destinationErrorMessage(error: unknown): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again before continuing.'
    }
    if (error.status === 404) {
      return 'The destination folder is no longer available.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'The destination details are temporarily unavailable. Please try again.'
    }
  }
  return 'The destination folder could not be confirmed. Refresh before restoring.'
}

function TechnicalDetails({ children }: { readonly children: ReactNode }) {
  return (
    <details className="restore-technical-details">
      <summary>Technical details</summary>
      <div>{children}</div>
    </details>
  )
}

function LoadingBlock({ label }: { readonly label: string }) {
  return (
    <p className="restore-loading" role="status" aria-busy="true">
      {label}
    </p>
  )
}

function SummaryMetric({
  label,
  children,
}: {
  readonly label: string
  readonly children: ReactNode
}) {
  return (
    <div className="restore-summary-metric">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  )
}

function WorkflowSteps({ step }: { readonly step: WorkflowStep }) {
  const steps: readonly { readonly id: WorkflowStep; readonly label: string }[] = [
    { id: 'DESTINATION', label: 'Destination' },
    { id: 'REVIEW', label: 'Review' },
    { id: 'COMPLETE', label: 'Complete' },
  ]
  const activeIndex = steps.findIndex((item) => item.id === step)

  return (
    <ol className="restore-workflow-steps" aria-label="Restore workflow steps">
      {steps.map((item, index) => (
        <li
          className={item.id === step ? 'restore-workflow-step restore-workflow-step--active' : 'restore-workflow-step'}
          key={item.id}
          aria-current={item.id === step ? 'step' : undefined}
        >
          <span aria-hidden="true">{index + 1}</span>
          <strong>{item.label}</strong>
          {index < activeIndex && <small>Done</small>}
        </li>
      ))}
    </ol>
  )
}

function WorkflowHeader({
  step,
  headingRef,
}: {
  readonly step: WorkflowStep
  readonly headingRef: RefObject<HTMLHeadingElement | null>
}) {
  const title = step === 'DESTINATION' ? 'Restore a snapshot' : step === 'REVIEW' ? 'Review restore plan' : 'Restore completed'
  const subtitle =
    step === 'DESTINATION'
      ? 'Choose where a new copy of this historical snapshot should appear.'
      : step === 'REVIEW'
        ? 'Review the persisted destination and planned contents before explicitly restoring.'
        : 'The server-confirmed restore receipt is shown below.'

  return (
    <>
      <section className="backup-page-heading restore-page-heading" aria-labelledby="restore-workflow-title">
        <div>
          <p className="eyebrow">Protected copies · Restore</p>
          <h1 id="restore-workflow-title" ref={headingRef} tabIndex={-1}>{title}</h1>
          <p>{subtitle}</p>
        </div>
      </section>
      <WorkflowSteps step={step} />
    </>
  )
}

function SourceSummary({
  snapshot,
  snapshotId,
}: {
  readonly snapshot?: BackupSnapshot
  readonly snapshotId: string
}) {
  return (
    <section className="panel restore-source-summary" aria-labelledby="restore-source-title">
      <div className="restore-section-heading">
        <div>
          <p className="eyebrow">Historical source</p>
          <h2 id="restore-source-title">Restore from</h2>
          <p>
            This snapshot is the read-only source. Planning and restoring do not consume or change it.
          </p>
        </div>
        {snapshot && <StatusPill tone={snapshotStateTone(snapshot)}>{snapshotStateLabel(snapshot)}</StatusPill>}
      </div>
      <dl className="restore-summary-grid">
        <SummaryMetric label="Snapshot">
          {snapshot ? <Timestamp value={snapshot.committed_at ?? snapshot.created_at} /> : 'Loading…'}
        </SummaryMetric>
        <SummaryMetric label="Logical items">
          {snapshot ? formatInteger(snapshot.logical_node_count) : 'Loading…'}
        </SummaryMetric>
        <SummaryMetric label="Content references">
          {snapshot ? formatInteger(snapshot.content_reference_count) : 'Loading…'}
        </SummaryMetric>
        <SummaryMetric label="Source snapshot ID">
          <code>{snapshotId}</code>
        </SummaryMetric>
      </dl>
    </section>
  )
}

function RestoreReceipt({
  plan,
  execution,
  destinationDetails,
  targetDirectoryState,
}: {
  readonly plan: BackupRestorePlan
  readonly execution: BackupRestoreExecution
  readonly destinationDetails: DestinationDetails
  readonly targetDirectoryState: DataState<readonly LiveNodeResource[]>
}) {
  const destinationLibrary = destinationDetails.libraryName ?? 'Selected destination library'
  const destinationFolder = destinationDetails.folderPath ?? destinationDetails.folderName ?? 'Selected destination folder'

  return (
    <section className="panel restore-receipt" aria-labelledby="restore-receipt-title">
      <div className="restore-section-heading">
        <div>
          <p className="eyebrow">Server-confirmed receipt</p>
          <h2 id="restore-receipt-title">Restore completed</h2>
          <p role="status" aria-live="polite">
            Restored successfully. Existing destination content was left unchanged.
          </p>
        </div>
        <StatusPill tone="info">Executed</StatusPill>
      </div>
      <dl className="restore-summary-grid">
        <SummaryMetric label="Destination library">{destinationLibrary}</SummaryMetric>
        <SummaryMetric label="Destination folder">{destinationFolder}</SummaryMetric>
        <SummaryMetric label="Created items">
          {summaryCount(execution.created_directory_count, execution.created_file_count)}
        </SummaryMetric>
        <SummaryMetric label="Executed"><Timestamp value={execution.executed_at} /></SummaryMetric>
        <SummaryMetric label="Created directories">{formatInteger(execution.created_directory_count)}</SummaryMetric>
        <SummaryMetric label="Created files">{formatInteger(execution.created_file_count)}</SummaryMetric>
      </dl>
      <TargetDirectoryContents state={targetDirectoryState} />
      <p className="restore-muted-copy">
        The source snapshot remains available as historical backup data.
      </p>
      <TechnicalDetails>
        <dl className="restore-technical-list">
          <div><dt>Restore plan ID</dt><dd><code>{plan.restore_plan_id}</code></dd></div>
          <div><dt>Restore execution ID</dt><dd><code>{execution.restore_execution_id}</code></dd></div>
        </dl>
      </TechnicalDetails>
    </section>
  )
}

function TargetDirectoryContents({
  state,
}: {
  readonly state: DataState<readonly LiveNodeResource[]>
}) {
  if (state.status === 'loading') {
    return <LoadingBlock label="Refreshing the destination folder…" />
  }
  if (state.status !== 'success' || !state.data) {
    return null
  }
  return (
    <section className="restore-target-contents" aria-labelledby="restore-target-contents-title">
      <div>
        <h3 id="restore-target-contents-title">Destination folder after restore</h3>
        <p>These children were read again from the live namespace after the server confirmed execution.</p>
      </div>
      {state.data.length === 0 ? (
        <p className="restore-empty-copy">The destination folder is empty.</p>
      ) : (
        <ul>
          {state.data.map((node) => (
            <li key={node.id}>
              <strong>{node.attributes.name}</strong>
              <span>{node.attributes.kind === 'DIRECTORY' ? 'Directory' : 'File'}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

function ReviewField({
  label,
  children,
}: {
  readonly label: string
  readonly children: ReactNode
}) {
  return (
    <div className="restore-review-field">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  )
}

async function findLibrary(
  fileApi: FileMetadataApi,
  libraryId: string,
  signal: AbortSignal,
): Promise<LiveLibraryResource | undefined> {
  let cursor: string | undefined
  for (let page = 0; page < MAX_LIBRARY_LOOKUP_PAGES; page += 1) {
    const response = await fileApi.listLibraries({
      cursor,
      limit: NAMESPACE_PAGE_LIMIT,
      signal,
    })
    const found = response.data.find((library) => library.id === libraryId)
    if (found) {
      return found
    }
    if (!response.page.has_more || !response.page.next_cursor) {
      return undefined
    }
    cursor = response.page.next_cursor
  }
  return undefined
}

async function liveDirectoryPath(
  fileApi: FileMetadataApi,
  library: LiveLibraryResource,
  node: LiveNodeResource,
  signal: AbortSignal,
): Promise<string> {
  const names: string[] = []
  const visited = new Set<string>()
  let current = node

  for (let depth = 0; depth < MAX_DIRECTORY_DEPTH; depth += 1) {
    if (visited.has(current.id)) {
      throw new Error('destination parent chain contains a cycle')
    }
    visited.add(current.id)
    if (
      current.attributes.library_id !== library.id ||
      current.attributes.kind !== 'DIRECTORY' ||
      current.attributes.state !== 'ACTIVE'
    ) {
      throw new Error('destination is not an active directory')
    }
    names.unshift(current.attributes.name)
    const parentId = current.attributes.parent_id
    if (!parentId) {
      return [library.attributes.name, ...names].join(' / ')
    }
    current = (await fileApi.getLiveNode(parentId, { signal })).data
  }

  throw new Error('destination parent chain exceeds the supported depth')
}

export function RestoreWorkflowPage({
  backupApi = defaultBackupApi,
  fileApi = defaultFileMetadataApi,
}: RestoreWorkflowPageProps) {
  const { refresh: refreshAuth } = useAuth()
  const navigate = useNavigate()
  const { backupSetId, snapshotId, restorePlanId } = useParams<{
    backupSetId: string
    snapshotId: string
    restorePlanId: string
  }>()
  const [snapshotState, setSnapshotState] = useState<DataState<BackupSnapshot>>({ status: 'idle' })
  const [planState, setPlanState] = useState<DataState<BackupRestorePlan>>({ status: 'idle' })
  const [executionState, setExecutionState] = useState<DataState<BackupRestoreExecution>>({ status: 'idle' })
  const [destinationDetails, setDestinationDetails] = useState<DestinationDetails>({ status: 'idle' })
  const [destination, setDestination] = useState<DestinationSelection>()
  const [destinationName, setDestinationName] = useState('')
  const [planCreateAction, setPlanCreateAction] = useState<MutationAction | null>(null)
  const [planCreateError, setPlanCreateError] = useState<string>()
  const [setupValidationError, setSetupValidationError] = useState<string>()
  const [executionAction, setExecutionAction] = useState<MutationAction | null>(null)
  const [executionError, setExecutionError] = useState<string>()
  const [pendingPlanCreate, setPendingPlanCreate] = useState<PendingMutationRecord | undefined>(() =>
    findPendingMutation(
      'create_restore_plan',
      (scope) => scope.backupSetId === backupSetId && scope.snapshotId === snapshotId,
    ),
  )
  const [pendingExecution, setPendingExecution] = useState<PendingMutationRecord | undefined>(() =>
    findPendingMutation(
      'execute_restore_plan',
      (scope) => scope.backupSetId === backupSetId && scope.restorePlanId === restorePlanId,
    ),
  )
  const [targetRefreshStatus, setTargetRefreshStatus] = useState<LoadStatus>('idle')
  const [targetDirectoryState, setTargetDirectoryState] = useState<DataState<readonly LiveNodeResource[]>>({ status: 'idle' })
  const [planRefreshVersion, setPlanRefreshVersion] = useState(0)

  const mounted = useRef(true)
  const snapshotRequestId = useRef(0)
  const planRequestId = useRef(0)
  const executionRequestId = useRef(0)
  const destinationRequestId = useRef(0)
  const snapshotController = useRef<AbortController | undefined>(undefined)
  const planController = useRef<AbortController | undefined>(undefined)
  const executionController = useRef<AbortController | undefined>(undefined)
  const destinationController = useRef<AbortController | undefined>(undefined)
  const targetRefreshController = useRef<AbortController | undefined>(undefined)
  const planCreateController = useRef<AbortController | undefined>(undefined)
  const planCreateRef = useRef<MutationAction | null>(null)
  const executionActionRef = useRef<MutationAction | null>(null)
  const targetRefreshStartedRef = useRef(false)
  const headingRef = useRef<HTMLHeadingElement>(null)
  const destinationNameRef = useRef<HTMLInputElement>(null)

  const recoverSession = useCallback(
    (error: unknown) => {
      if (isApiRequestError(error) && error.status === 401) {
        void refreshAuth().catch(() => undefined)
      }
    },
    [refreshAuth],
  )

  const refreshSession = useCallback(() => {
    void refreshAuth().catch(() => undefined)
  }, [refreshAuth])

  useEffect(() => {
    return () => {
      mounted.current = false
      snapshotController.current?.abort()
      planController.current?.abort()
      executionController.current?.abort()
      destinationController.current?.abort()
      targetRefreshController.current?.abort()
      planCreateController.current?.abort()
    }
  }, [])

  useEffect(() => {
    setPendingPlanCreate(findPendingMutation(
      'create_restore_plan',
      (scope) => scope.backupSetId === backupSetId && scope.snapshotId === snapshotId,
    ))
    setPendingExecution(findPendingMutation(
      'execute_restore_plan',
      (scope) => scope.backupSetId === backupSetId && scope.restorePlanId === restorePlanId,
    ))
  }, [backupSetId, restorePlanId, snapshotId])

  const loadSnapshot = useCallback(
    async (requestedSnapshotId: string) => {
      snapshotController.current?.abort()
      const controller = new AbortController()
      snapshotController.current = controller
      const requestId = snapshotRequestId.current + 1
      snapshotRequestId.current = requestId
      setSnapshotState({ status: 'loading' })
      try {
        const response = await backupApi.getBackupSnapshot(requestedSnapshotId, { signal: controller.signal })
        if (!mounted.current || controller.signal.aborted || snapshotRequestId.current !== requestId) {
          return
        }
        if (backupSetId && response.data.backup_set_id !== backupSetId) {
          setSnapshotState({ status: 'error', error: new Error('snapshot scope mismatch') })
          return
        }
        setSnapshotState({ status: 'success', data: response.data })
      } catch (error) {
        if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
          return
        }
        setSnapshotState({ status: 'error', error })
        recoverSession(error)
      }
    },
    [backupApi, backupSetId, recoverSession],
  )

  const readPlan = useCallback(
    async (showLoading: boolean) => {
      if (!restorePlanId) {
        return
      }
      planController.current?.abort()
      const controller = new AbortController()
      planController.current = controller
      const requestId = planRequestId.current + 1
      planRequestId.current = requestId
      if (showLoading) {
        setPlanState({ status: 'loading' })
        setExecutionState({ status: 'idle' })
      }
      try {
        const response = await backupApi.getBackupRestorePlan(restorePlanId, { signal: controller.signal })
        if (!mounted.current || controller.signal.aborted || planRequestId.current !== requestId) {
          return
        }
        if (
          response.data.restore_plan_id !== restorePlanId ||
          (backupSetId && response.data.source_backup_set_id !== backupSetId)
        ) {
          setPlanState({ status: 'error', error: new Error('restore plan scope mismatch') })
          return
        }
        setPlanState({ status: 'success', data: response.data })
      } catch (error) {
        if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
          return
        }
        setPlanState({ status: 'error', error })
        recoverSession(error)
      }
    },
    [backupApi, backupSetId, recoverSession, restorePlanId],
  )

  useEffect(() => {
    if (!restorePlanId && snapshotId) {
      void loadSnapshot(snapshotId)
    }
  }, [loadSnapshot, restorePlanId, snapshotId])

  useEffect(() => {
    if (restorePlanId) {
      void readPlan(true)
    }
  }, [readPlan, restorePlanId, planRefreshVersion])

  const sourceSnapshotId = planState.data?.source_snapshot_id ?? snapshotId

  useEffect(() => {
    if (restorePlanId && sourceSnapshotId) {
      void loadSnapshot(sourceSnapshotId)
    }
  }, [loadSnapshot, restorePlanId, sourceSnapshotId])

  const loadDestinationDetails = useCallback(
    async (plan: BackupRestorePlan) => {
      destinationController.current?.abort()
      const controller = new AbortController()
      destinationController.current = controller
      const requestId = destinationRequestId.current + 1
      destinationRequestId.current = requestId
      setDestinationDetails({ status: 'loading' })
      try {
        const [library, nodeResponse] = await Promise.all([
          findLibrary(fileApi, plan.target_library_id, controller.signal),
          fileApi.getLiveNode(plan.target_parent_node_id, { signal: controller.signal }),
        ])
        const node = nodeResponse.data
        if (
          !mounted.current ||
          controller.signal.aborted ||
          destinationRequestId.current !== requestId
        ) {
          return
        }
        if (
          !library ||
          node.id !== plan.target_parent_node_id ||
          node.attributes.library_id !== plan.target_library_id ||
          node.attributes.kind !== 'DIRECTORY' ||
          node.attributes.state !== 'ACTIVE'
        ) {
          setDestinationDetails({ status: 'error', error: new Error('destination is not a directory') })
          return
        }
        const folderPath = await liveDirectoryPath(fileApi, library, node, controller.signal)
        if (
          !mounted.current ||
          controller.signal.aborted ||
          destinationRequestId.current !== requestId
        ) {
          return
        }
        setDestinationDetails({
          status: 'success',
          libraryName: library.attributes.name,
          folderName: node.attributes.name,
          folderPath,
        })
      } catch (error) {
        if (
          !mounted.current ||
          controller.signal.aborted ||
          isAbortError(error) ||
          destinationRequestId.current !== requestId
        ) {
          return
        }
        setDestinationDetails({ status: 'error', error })
        recoverSession(error)
      }
    },
    [fileApi, recoverSession],
  )

  useEffect(() => {
    if (planState.data) {
      void loadDestinationDetails(planState.data)
    } else if (restorePlanId && planState.status !== 'loading') {
      setDestinationDetails({ status: 'idle' })
    }
  }, [loadDestinationDetails, planState.data, planState.status, restorePlanId])

  const loadExecutionReceipt = useCallback(
    async (plan: BackupRestorePlan) => {
      executionController.current?.abort()
      const controller = new AbortController()
      executionController.current = controller
      const requestId = executionRequestId.current + 1
      executionRequestId.current = requestId
      setExecutionState({ status: 'loading' })
      try {
        const operationResponse = await backupApi.getBackupOperation('RESTORE', plan.restore_plan_id, {
          signal: controller.signal,
        })
        const operation: BackupOperationDetail = operationResponse.data
        if (operation.operation_kind !== 'RESTORE' || !operation.restore_execution_id) {
          throw new Error('restore execution receipt is not available')
        }
        const response = await backupApi.getBackupRestoreExecution(operation.restore_execution_id, {
          signal: controller.signal,
        })
        if (
          !mounted.current ||
          controller.signal.aborted ||
          executionRequestId.current !== requestId
        ) {
          return
        }
        if (response.data.restore_plan_id !== plan.restore_plan_id) {
          setExecutionState({ status: 'error', error: new Error('restore receipt scope mismatch') })
          return
        }
        if (pendingExecution?.request_scope.restorePlanId === plan.restore_plan_id) {
          mutationRecoveryStore.remove(pendingExecution.action_id)
          setPendingExecution(undefined)
        }
        setExecutionState({ status: 'success', data: response.data })
      } catch (error) {
        if (
          !mounted.current ||
          controller.signal.aborted ||
          isAbortError(error) ||
          executionRequestId.current !== requestId
        ) {
          return
        }
        setExecutionState({ status: 'error', error })
        recoverSession(error)
      }
    },
    [backupApi, pendingExecution, recoverSession],
  )

  useEffect(() => {
    const plan = planState.data
    if (plan?.state === 'EXECUTED' && executionState.status === 'idle') {
      void loadExecutionReceipt(plan)
    }
  }, [executionState.status, loadExecutionReceipt, planState.data])

  useEffect(() => {
    if (planState.data?.state === 'STALE') {
      executionActionRef.current = null
      setExecutionAction(null)
      if (pendingExecution) {
        mutationRecoveryStore.remove(pendingExecution.action_id)
        setPendingExecution(undefined)
        setExecutionError('The restore plan is stale, so its impossible local retry was cleared. Nothing was restored.')
      }
    }
  }, [pendingExecution, planState.data?.state])

  useEffect(() => {
    const shouldFocus =
      (!restorePlanId && snapshotState.status === 'success') ||
      (restorePlanId && planState.status === 'success') ||
      executionState.status === 'success'
    if (shouldFocus) {
      headingRef.current?.focus()
    }
  }, [executionState.status, planState.status, restorePlanId, snapshotState.status])

  useEffect(() => {
    if (snapshotState.status === 'error' || planState.status === 'error') {
      const heading = document.querySelector<HTMLElement>('.restore-error-panel h1, .restore-error-panel h2')
      if (heading) {
        heading.setAttribute('tabindex', '-1')
        heading.focus()
      }
    }
  }, [planState.status, snapshotState.status])

  useEffect(() => {
    if (!restorePlanId && (planCreateError || setupValidationError)) {
      destinationNameRef.current?.focus()
    }
  }, [planCreateError, restorePlanId, setupValidationError])

  const refreshRestoreOperations = useCallback(
    async (requestedBackupSetId: string) => {
      try {
        await backupApi.listBackupOperations(requestedBackupSetId, {
          kind: 'RESTORE',
          limit: NAMESPACE_PAGE_LIMIT,
        })
      } catch (error) {
        recoverSession(error)
      }
    },
    [backupApi, recoverSession],
  )

  const refreshTargetNamespace = useCallback(
    async (plan: BackupRestorePlan) => {
      if (targetRefreshStartedRef.current) {
        return
      }
      targetRefreshStartedRef.current = true
      targetRefreshController.current?.abort()
      const controller = new AbortController()
      targetRefreshController.current = controller
      setTargetRefreshStatus('loading')
      setTargetDirectoryState({ status: 'loading' })
      try {
        const response = await fileApi.listLibraryChildren(plan.target_library_id, {
          parentId: plan.target_parent_node_id,
          limit: NAMESPACE_PAGE_LIMIT,
          signal: controller.signal,
        })
        if (!mounted.current || controller.signal.aborted) {
          return
        }
        setTargetDirectoryState({ status: 'success', data: response.data })
        setTargetRefreshStatus('success')
      } catch (error) {
        if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
          return
        }
        setTargetDirectoryState({ status: 'error', error })
        setTargetRefreshStatus('error')
        recoverSession(error)
      }
    },
    [fileApi, recoverSession],
  )

  useEffect(() => {
    const plan = planState.data
    if (
      plan?.state === 'EXECUTED' &&
      executionState.status === 'success' &&
      targetRefreshStatus === 'idle'
    ) {
      void refreshTargetNamespace(plan)
    }
  }, [executionState.status, planState.data, refreshTargetNamespace, targetRefreshStatus])

  function invalidateFailedPlanAction() {
    if (planCreateRef.current?.phase === 'failed') {
      planCreateRef.current = null
      setPlanCreateAction(null)
      setPlanCreateError(undefined)
    }
  }

  const handleDestinationChange = useCallback((selection: DestinationSelection | undefined) => {
    setDestination(selection)
    setSetupValidationError(undefined)
    invalidateFailedPlanAction()
  }, [])

  function handleDestinationNameChange(value: string) {
    setDestinationName(value)
    setSetupValidationError(undefined)
    invalidateFailedPlanAction()
  }

  async function submitPlan(
    requestOverride?: CreateBackupRestorePlanRequest,
    retryKey?: string,
  ) {
    if (
      !snapshotId ||
      (!requestOverride && !destination) ||
      planCreateRef.current?.phase === 'submitting'
    ) {
      return
    }
    const requestedName = requestOverride?.destination_name ?? destinationName
    if (requestedName.trim().length === 0) {
      setSetupValidationError('Enter a destination name.')
      destinationNameRef.current?.focus()
      return
    }
    if (destinationNameByteLength(requestedName) > MAX_DESTINATION_NAME_LENGTH) {
      setSetupValidationError('Destination name is too long.')
      destinationNameRef.current?.focus()
      return
    }
    const request: CreateBackupRestorePlanRequest = requestOverride ?? {
      target_library_id: destination!.targetLibraryId,
      target_parent_node_id: destination!.targetParentNodeId,
      destination_name: destinationName,
    }
    let idempotencyKey = retryKey
    if (!idempotencyKey) {
      try {
        idempotencyKey = createUuidV7()
      } catch {
        setPlanCreateError('Secure browser randomness is unavailable. The restore plan was not created.')
        return
      }
    }
    const action: MutationAction = { idempotencyKey, request, phase: 'submitting' }
    const recoveryRecord = mutationRecoveryStore.add({
      action_kind: 'create_restore_plan',
      idempotency_key: idempotencyKey,
      request_scope: { backupSetId, snapshotId },
      request: { ...request },
    })
    if (mutationRecoveryStore.isAvailable) {
      setPendingPlanCreate(recoveryRecord)
    }
    planCreateRef.current = action
    setPlanCreateAction(action)
    setPlanCreateError(undefined)
    setSetupValidationError(undefined)
    planCreateController.current?.abort()
    const controller = new AbortController()
    planCreateController.current = controller
    let postConfirmed = false
    try {
      const response = await backupApi.createBackupRestorePlan(snapshotId, request, idempotencyKey, {
        signal: controller.signal,
      })
      postConfirmed = true
      mutationRecoveryStore.remove(recoveryRecord.action_id)
      setPendingPlanCreate((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      if (!mounted.current || controller.signal.aborted) {
        return
      }
      if (
        response.data.source_snapshot_id !== snapshotId ||
        response.data.source_backup_set_id !== backupSetId
      ) {
        throw new Error('restore plan scope mismatch')
      }
      planCreateRef.current = null
      setPlanCreateAction(null)
      void refreshRestoreOperations(response.data.source_backup_set_id)
      navigate(`/backups/${encodeURIComponent(response.data.source_backup_set_id)}/restore/${encodeURIComponent(response.data.restore_plan_id)}`)
    } catch (error) {
      const recoveryOutcome = postConfirmed
        ? 'removed'
        : settlePendingMutationFailure(recoveryRecord, error)
      if (recoveryOutcome === 'removed') {
        setPendingPlanCreate((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      }
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      if (recoveryOutcome === 'retained') {
        const failedAction: MutationAction = { ...action, phase: 'failed' }
        planCreateRef.current = failedAction
        setPlanCreateAction(failedAction)
      } else {
        planCreateRef.current = null
        setPlanCreateAction(null)
      }
      setPlanCreateError(planCreateErrorMessage(error))
      recoverSession(error)
      if (isApiRequestError(error) && error.status === 409) {
        destinationNameRef.current?.focus()
      }
    }
  }

  async function refreshCanonicalPlanPreservingReceipt(plan: BackupRestorePlan) {
    try {
      const response = await backupApi.getBackupRestorePlan(plan.restore_plan_id)
      if (!mounted.current || response.data.source_backup_set_id !== backupSetId) {
        return
      }
      setPlanState({ status: 'success', data: response.data })
    } catch (error) {
      recoverSession(error)
    }
  }

  async function submitExecution(retryKey?: string) {
    const plan = planState.data
    if (
      !plan ||
      plan.state !== 'PLANNED' ||
      executionActionRef.current?.phase === 'submitting' ||
      destinationDetails.status !== 'success'
    ) {
      return
    }
    let idempotencyKey = retryKey
    if (!idempotencyKey) {
      try {
        idempotencyKey = createUuidV7()
      } catch {
        setExecutionError('Secure browser randomness is unavailable. The restore was not executed.')
        return
      }
    }
    const action: MutationAction = { idempotencyKey, phase: 'submitting' }
    const recoveryRecord = mutationRecoveryStore.add({
      action_kind: 'execute_restore_plan',
      idempotency_key: idempotencyKey,
      request_scope: {
        backupSetId: plan.source_backup_set_id,
        snapshotId: plan.source_snapshot_id,
        restorePlanId: plan.restore_plan_id,
      },
      request: {},
    })
    if (mutationRecoveryStore.isAvailable) {
      setPendingExecution(recoveryRecord)
    }
    executionActionRef.current = action
    setExecutionAction(action)
    setExecutionError(undefined)
    executionController.current?.abort()
    const controller = new AbortController()
    executionController.current = controller
    let postConfirmed = false
    try {
      const response = await backupApi.executeBackupRestorePlan(plan.restore_plan_id, idempotencyKey, {
        signal: controller.signal,
      })
      postConfirmed = true
      mutationRecoveryStore.remove(recoveryRecord.action_id)
      setPendingExecution((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      if (!mounted.current || controller.signal.aborted) {
        return
      }
      const receiptResponse = await backupApi.getBackupRestoreExecution(
        response.data.restore_execution_id,
        { signal: controller.signal },
      )
      if (!mounted.current || controller.signal.aborted) {
        return
      }
      if (receiptResponse.data.restore_plan_id !== plan.restore_plan_id) {
        throw new Error('restore receipt scope mismatch')
      }
      setExecutionState({ status: 'success', data: receiptResponse.data })
      executionActionRef.current = null
      setExecutionAction(null)
      setExecutionError(undefined)
      void refreshCanonicalPlanPreservingReceipt(plan)
      void refreshRestoreOperations(plan.source_backup_set_id)
      void refreshTargetNamespace(plan)
    } catch (error) {
      const recoveryOutcome = postConfirmed
        ? 'removed'
        : settlePendingMutationFailure(recoveryRecord, error)
      if (recoveryOutcome === 'removed') {
        setPendingExecution((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      }
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      if (postConfirmed) {
        executionActionRef.current = null
        setExecutionAction(null)
        setExecutionError('The restore was accepted, but its completion receipt could not be loaded. Check current status before taking another action.')
        void readPlan(false)
      } else if (isApiRequestError(error) && error.status === 409) {
        const failedAction: MutationAction = { ...action, phase: 'failed' }
        executionActionRef.current = recoveryOutcome === 'retained' ? failedAction : null
        setExecutionAction(recoveryOutcome === 'retained' ? failedAction : null)
        setExecutionError(executeErrorMessage(error))
        void readPlan(false)
      } else {
        const failedAction = recoveryOutcome === 'retained'
          ? { ...action, phase: 'failed' as const }
          : null
        executionActionRef.current = failedAction
        setExecutionAction(failedAction)
        setExecutionError(executeErrorMessage(error))
      }
      recoverSession(error)
    }
  }

  function createNewPlan() {
    const sourceId = planState.data?.source_snapshot_id ?? snapshotId
    if (!backupSetId || !sourceId) {
      navigate('/backups')
      return
    }
    navigate(`/backups/${encodeURIComponent(backupSetId)}/snapshots/${encodeURIComponent(sourceId)}/restore`)
  }

  function submitSetup(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    void submitPlan()
  }

  function discardPendingPlanCreate() {
    if (!pendingPlanCreate) {
      return
    }
    mutationRecoveryStore.remove(pendingPlanCreate.action_id)
    if (planCreateRef.current?.idempotencyKey === pendingPlanCreate.idempotency_key) {
      planCreateRef.current = null
      setPlanCreateAction(null)
      setPlanCreateError(undefined)
    }
    setPendingPlanCreate(undefined)
  }

  function discardPendingExecution() {
    if (!pendingExecution) {
      return
    }
    mutationRecoveryStore.remove(pendingExecution.action_id)
    if (executionActionRef.current?.idempotencyKey === pendingExecution.idempotency_key) {
      executionActionRef.current = null
      setExecutionAction(null)
      setExecutionError(undefined)
    }
    setPendingExecution(undefined)
  }

  const planCreateSubmitting = planCreateAction?.phase === 'submitting'
  const executionSubmitting = executionAction?.phase === 'submitting'
  const setupSnapshot = snapshotState.data
  const plan = planState.data
  const workflowStep: WorkflowStep =
    !restorePlanId ? 'DESTINATION' : plan?.state === 'EXECUTED' || executionState.status === 'success' ? 'COMPLETE' : 'REVIEW'

  if (!backupSetId || (!snapshotId && !restorePlanId)) {
    return (
      <div className="backup-page restore-workflow-page">
        <WorkflowHeader step="DESTINATION" headingRef={headingRef} />
        <section className="panel restore-error-panel" role="alert">
          <h2>Restore workflow is not available</h2>
          <p>The requested restore source could not be identified.</p>
          <button type="button" className="button--secondary" onClick={() => navigate('/backups')}>
            Back to backups
          </button>
        </section>
      </div>
    )
  }

  if (!restorePlanId && snapshotState.status === 'error') {
    return (
      <div className="backup-page restore-workflow-page">
        <WorkflowHeader step="DESTINATION" headingRef={headingRef} />
        <section className="panel restore-error-panel" role="alert">
          <h2>Snapshot unavailable</h2>
          <p>{loadErrorMessage(snapshotState.error, 'snapshot')}</p>
          <button type="button" className="button--secondary" onClick={() => navigate('/backups')}>
            Back to backups
          </button>
        </section>
      </div>
    )
  }

  if (!restorePlanId && (snapshotState.status === 'loading' || !setupSnapshot)) {
    return (
      <div className="backup-page restore-workflow-page">
        <WorkflowHeader step="DESTINATION" headingRef={headingRef} />
        <SourceSummary snapshot={undefined} snapshotId={snapshotId ?? 'unknown'} />
        <section className="panel" aria-labelledby="restore-loading-title">
          <h2 id="restore-loading-title">Checking snapshot eligibility</h2>
          <LoadingBlock label="Confirming that this completed snapshot can be restored…" />
        </section>
      </div>
    )
  }

  if (!restorePlanId && setupSnapshot && setupSnapshot.state !== 'COMPLETED') {
    return (
      <div className="backup-page restore-workflow-page">
        <WorkflowHeader step="DESTINATION" headingRef={headingRef} />
        <SourceSummary snapshot={setupSnapshot} snapshotId={setupSnapshot.snapshot_id} />
        <section className="panel restore-error-panel" role="alert">
          <h2>This snapshot is not available for restore</h2>
          <p>This snapshot is no longer in a completed state that can begin a restore.</p>
          <button type="button" className="button--secondary" onClick={() => navigate('/backups')}>
            Back to backups
          </button>
        </section>
      </div>
    )
  }

  if (restorePlanId && planState.status === 'loading') {
    return (
      <div className="backup-page restore-workflow-page">
        <WorkflowHeader step="REVIEW" headingRef={headingRef} />
        <section className="panel" aria-labelledby="restore-plan-loading-title">
          <h2 id="restore-plan-loading-title">Loading restore plan</h2>
          <LoadingBlock label="Loading the persisted restore plan…" />
        </section>
      </div>
    )
  }

  if (restorePlanId && planState.status === 'error') {
    return (
      <div className="backup-page restore-workflow-page">
        <WorkflowHeader step="REVIEW" headingRef={headingRef} />
        <section className="panel restore-error-panel" role="alert">
          <h2>Restore plan not available</h2>
          <p>{loadErrorMessage(planState.error, 'restore plan')}</p>
          <button
            type="button"
            className="button--secondary"
            onClick={() => setPlanRefreshVersion((current) => current + 1)}
          >
            Refresh restore plan
          </button>
          <button type="button" className="button--secondary" onClick={() => navigate('/backups')}>
            Back to backups
          </button>
        </section>
      </div>
    )
  }

  if (restorePlanId && !plan) {
    return (
      <div className="backup-page restore-workflow-page">
        <WorkflowHeader step="REVIEW" headingRef={headingRef} />
        <section className="panel restore-error-panel" role="alert">
          <h2>Restore plan not available</h2>
          <p>Restore plan not available.</p>
          <button type="button" className="button--secondary" onClick={() => navigate('/backups')}>
            Back to backups
          </button>
        </section>
      </div>
    )
  }

  if (!restorePlanId) {
    return (
      <div className="backup-page restore-workflow-page">
        <WorkflowHeader step="DESTINATION" headingRef={headingRef} />
        {pendingPlanCreate && pendingRestorePlanRequest(pendingPlanCreate) && (
          <MutationRecoveryNotice
            record={pendingPlanCreate}
            title="Finish creating this restore plan"
            busy={planCreateSubmitting}
            onRetry={() => {
              const request = pendingRestorePlanRequest(pendingPlanCreate)
              if (request) {
                void submitPlan(request, pendingPlanCreate.idempotency_key)
              }
            }}
            onDiscard={discardPendingPlanCreate}
          />
        )}
        <SourceSummary snapshot={setupSnapshot} snapshotId={setupSnapshot?.snapshot_id ?? snapshotId ?? 'unknown'} />
        <form className="restore-setup-form" onSubmit={submitSetup} noValidate>
          <RestoreDestinationPicker
            fileApi={fileApi}
            disabled={planCreateSubmitting}
            onDestinationChange={handleDestinationChange}
            onUnauthorized={refreshSession}
          />
          <section className="panel restore-name-panel" aria-labelledby="restore-name-title">
            <div className="restore-section-heading">
              <div>
                <p className="eyebrow">Destination name</p>
                <h2 id="restore-name-title">Name the restored item</h2>
                <p>
                  This name is used exactly as entered in the selected folder. Existing items are not changed.
                </p>
              </div>
            </div>
            <div className="field">
              <label htmlFor="restore-destination-name">Destination name</label>
              <input
                id="restore-destination-name"
                ref={destinationNameRef}
                value={destinationName}
                onChange={(event) => handleDestinationNameChange(event.currentTarget.value)}
                disabled={planCreateSubmitting}
                maxLength={MAX_DESTINATION_NAME_LENGTH}
                aria-invalid={Boolean(setupValidationError || planCreateError)}
                aria-describedby={
                  setupValidationError || planCreateError
                    ? 'restore-name-help restore-name-error'
                    : 'restore-name-help'
                }
              />
              <p id="restore-name-help" className="restore-field-help">
                Choose a name that is not already used in the selected folder. Names are never changed automatically.
              </p>
            </div>
            {(setupValidationError || planCreateError) && (
              <div id="restore-name-error" className="restore-inline-error" role="alert" aria-live="assertive">
                <p>{setupValidationError ?? planCreateError}</p>
                {planCreateAction?.phase === 'failed' && (
                  <button type="button" className="button--secondary" onClick={() => void submitPlan(planCreateAction.request, planCreateAction.idempotencyKey)}>
                    Try the request again
                  </button>
                )}
              </div>
            )}
            <div className="restore-action-row">
              <button
                type="submit"
                disabled={
                  planCreateSubmitting ||
                  planCreateAction?.phase === 'failed' ||
                  Boolean(pendingPlanCreate) ||
                  !destination ||
                  destinationName.trim().length === 0 ||
                  destinationNameByteLength(destinationName) > MAX_DESTINATION_NAME_LENGTH
                }
              >
                {planCreateSubmitting ? 'Creating restore plan…' : 'Create restore plan'}
              </button>
              <button type="button" className="button--secondary" onClick={() => navigate('/backups')} disabled={planCreateSubmitting}>
                Cancel
              </button>
            </div>
          </section>
        </form>
      </div>
    )
  }

  if (!plan) {
    return (
      <div className="backup-page restore-workflow-page">
        <WorkflowHeader step="REVIEW" headingRef={headingRef} />
        <section className="panel restore-error-panel" role="alert">
          <h2>Restore plan not available</h2>
          <p>Restore plan not available.</p>
          <button type="button" className="button--secondary" onClick={() => setPlanRefreshVersion((current) => current + 1)}>
            Refresh restore plan
          </button>
          <button type="button" className="button--secondary" onClick={() => navigate('/backups')}>
            Back to backups
          </button>
        </section>
      </div>
    )
  }

  const sourceId = plan.source_snapshot_id
  const showReceipt = Boolean(executionState.data) && executionState.status === 'success'
  const destinationLookupError = destinationDetails.status === 'error'

  return (
    <div className="backup-page restore-workflow-page">
      <WorkflowHeader step={workflowStep} headingRef={headingRef} />
      {pendingExecution && plan.state === 'PLANNED' && (
        <MutationRecoveryNotice
          record={pendingExecution}
          title="Finish this restore"
          busy={executionSubmitting}
          onCheckStatus={() => void readPlan(false)}
          onRetry={() => void submitExecution(pendingExecution.idempotency_key)}
          onDiscard={discardPendingExecution}
        />
      )}
      <SourceSummary snapshot={snapshotState.data} snapshotId={sourceId} />
      {showReceipt && executionState.data ? (
        <>
          <RestoreReceipt
            plan={plan}
            execution={executionState.data}
            destinationDetails={destinationDetails}
            targetDirectoryState={targetDirectoryState}
          />
          {targetRefreshStatus === 'success' && (
            <p className="restore-refresh-notice" role="status">The destination folder was refreshed from the server.</p>
          )}
          {targetRefreshStatus === 'error' && (
            <p className="restore-refresh-notice restore-refresh-notice--warning" role="status">
              Restore completed. The destination view could not be refreshed; reload it when ready.
            </p>
          )}
        </>
      ) : (
        <section className="panel restore-review-panel" aria-labelledby="restore-review-title">
          <div className="restore-section-heading">
            <div>
              <p className="eyebrow">Step 2</p>
              <h2 id="restore-review-title">Review restore plan</h2>
              <p>
                The fields below come from the persisted plan. They are read-only until you explicitly create a new plan.
              </p>
            </div>
            <StatusPill tone={planStateTone(plan.state)}>{planStateLabel(plan.state)}</StatusPill>
          </div>

          <div className="restore-review-sections">
            <section aria-labelledby="restore-review-source-title">
              <h3 id="restore-review-source-title">Restore from</h3>
              <dl className="restore-review-list">
                <ReviewField label="Snapshot">{sourceId}</ReviewField>
                <ReviewField label="Backup set">{plan.source_backup_set_id}</ReviewField>
                <ReviewField label="Snapshot state">
                  {snapshotState.data ? snapshotState.data.state : 'Server-confirmed source'}
                </ReviewField>
              </dl>
            </section>
            <section aria-labelledby="restore-review-destination-title">
              <h3 id="restore-review-destination-title">Restore to</h3>
              <dl className="restore-review-list">
                <ReviewField label="Destination library">
                  {destinationDetails.libraryName ?? 'Selected destination library'}
                </ReviewField>
                <ReviewField label="Destination folder">
                  {destinationDetails.folderPath ?? destinationDetails.folderName ?? 'Selected destination folder'}
                </ReviewField>
                <ReviewField label="Destination name">{plan.destination_name}</ReviewField>
              </dl>
            </section>
          </div>

          {destinationDetails.status === 'loading' && <LoadingBlock label="Confirming destination details…" />}
          {destinationLookupError && (
            <div className="restore-inline-error" role="alert">
              <p>{destinationErrorMessage(destinationDetails.error)}</p>
              <button type="button" className="button--secondary" onClick={() => void loadDestinationDetails(plan)}>
                Refresh destination details
              </button>
            </div>
          )}

          <dl className="restore-summary-grid">
            <SummaryMetric label="Planned items">{formatInteger(plan.planned_entry_count)}</SummaryMetric>
            <SummaryMetric label="Planned directories">{formatInteger(plan.planned_directory_count)}</SummaryMetric>
            <SummaryMetric label="Planned files">{formatInteger(plan.planned_file_count)}</SummaryMetric>
            <SummaryMetric label="Plan created"><Timestamp value={plan.created_at} /></SummaryMetric>
          </dl>

          {plan.state === 'STALE' && (
            <div className="restore-inline-error restore-inline-error--terminal" role="alert">
              <p>The destination changed after this restore plan was created. Create a new plan before restoring.</p>
              <p>Nothing was restored.</p>
              <button type="button" className="button--secondary" onClick={createNewPlan}>
                Create a new plan
              </button>
            </div>
          )}

          {plan.state === 'EXECUTED' && executionState.status === 'loading' && (
            <LoadingBlock label="Loading the canonical restore receipt…" />
          )}
          {plan.state === 'EXECUTED' && executionState.status === 'error' && (
            <div className="restore-inline-error" role="alert">
              <p>The plan has already been executed. The completion receipt could not be loaded.</p>
              <button type="button" className="button--secondary" onClick={() => void loadExecutionReceipt(plan)}>
                Load completion receipt
              </button>
            </div>
          )}

          {plan.state === 'PLANNED' && (
            <div className="restore-explicit-action">
              <p>
                Ready to restore. Nothing will be written until you choose the explicit action below.
              </p>
              <button
                type="button"
                onClick={() => void submitExecution()}
                disabled={
                  executionSubmitting ||
                  executionAction?.phase === 'failed' ||
                  Boolean(pendingExecution) ||
                  destinationDetails.status !== 'success'
                }
              >
                {executionSubmitting ? 'Restoring…' : 'Restore to this folder'}
              </button>
              {executionAction?.phase === 'failed' && (
                <button
                  type="button"
                  className="button--secondary"
                  onClick={() => void submitExecution(executionAction.idempotencyKey)}
                >
                  Try the request again
                </button>
              )}
              {executionError && (
                <div className="restore-inline-error" role="alert" aria-live="assertive">
                  <p>{executionError}</p>
                  {executionAction?.phase === 'failed' && (
                    <p>Retrying with the same request key is safe while the result is unconfirmed.</p>
                  )}
                </div>
              )}
            </div>
          )}

          <div className="restore-review-actions">
            {plan.state === 'PLANNED' && (
              <button type="button" className="button--secondary" onClick={createNewPlan} disabled={executionSubmitting}>
                Create a new plan
              </button>
            )}
            <button type="button" className="button--secondary" onClick={() => navigate('/backups')} disabled={executionSubmitting}>
              Back to backups
            </button>
          </div>
          <TechnicalDetails>
            <dl className="restore-technical-list">
              <div><dt>Restore plan ID</dt><dd><code>{plan.restore_plan_id}</code></dd></div>
              <div><dt>Source snapshot ID</dt><dd><code>{plan.source_snapshot_id}</code></dd></div>
            </dl>
          </TechnicalDetails>
        </section>
      )}
    </div>
  )
}
