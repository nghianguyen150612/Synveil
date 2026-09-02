import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'

import {
  backupApi as defaultBackupApi,
  createUuidV7,
  type BackupApi,
  type BackupPruneExecution,
  type BackupPrunePlan,
  type BackupSet,
  type BackupSnapshot,
} from '../../api/backups'
import { isApiRequestError } from '../../api/errors'
import {
  findPendingMutation,
  mutationRecoveryStore,
  settlePendingMutationFailure,
  type PendingMutationRecord,
} from '../../api/mutationRecovery'
import { useAuth } from '../../auth/useAuth'
import { StatusPill } from '../../components/ui/StatusPill'
import { MutationRecoveryNotice } from './MutationRecoveryNotice'

type LoadStatus = 'idle' | 'loading' | 'success' | 'error'

interface DataState<T> {
  readonly status: LoadStatus
  readonly data?: T
  readonly error?: unknown
}

interface MutationAction {
  readonly idempotencyKey: string
  readonly phase: 'submitting' | 'failed'
}

export interface PruneWorkflowPageProps {
  readonly backupApi?: BackupApi
}

function isAbortError(error: unknown): boolean {
  return typeof DOMException !== 'undefined' && error instanceof DOMException && error.name === 'AbortError'
}

function pendingConfirmSnapshotId(record: PendingMutationRecord): string | undefined {
  const snapshotId = record.request.confirm_snapshot_id
  return typeof snapshotId === 'string' ? snapshotId : undefined
}

function formatInteger(value: string | number): string {
  try {
    return new Intl.NumberFormat().format(BigInt(value))
  } catch {
    return String(value)
  }
}

function formatTimestamp(value?: string): string {
  if (!value) {
    return 'Not recorded'
  }
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(date)
}

function Timestamp({ value }: { readonly value?: string }) {
  return value ? <time dateTime={value} title={value}>{formatTimestamp(value)}</time> : <span>Not recorded</span>
}

function SummaryMetric({ label, children }: { readonly label: string; readonly children: ReactNode }) {
  return <div className="prune-summary-metric"><dt>{label}</dt><dd>{children}</dd></div>
}

function loadErrorMessage(error: unknown, subject: 'snapshot' | 'plan'): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again to continue.'
    }
    if (error.status === 404) {
      return subject === 'plan' ? 'Prune plan not available.' : 'Snapshot not available.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Backup data is temporarily unavailable. Please try again.'
    }
  }
  return subject === 'plan' ? 'The prune plan could not be loaded.' : 'The snapshot could not be loaded.'
}

function planningErrorMessage(error: unknown): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again before creating a plan.'
    }
    if (error.status === 403) {
      return 'This request could not be verified. Refresh the page and try again.'
    }
    if (error.status === 404) {
      return 'This snapshot is no longer available.'
    }
    if (error.status === 409) {
      return error.code === 'snapshot_already_pruned'
        ? 'This backup content has already been released.'
        : 'A current prune plan already exists, or the snapshot is not eligible. Refresh its activity before continuing.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Prune planning is temporarily unavailable. Please try again.'
    }
  }
  return 'We could not confirm the result. Retry this same request safely.'
}

function executionErrorMessage(error: unknown): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again before continuing.'
    }
    if (error.status === 403) {
      return 'This request could not be verified. Refresh the page and try again.'
    }
    if (error.status === 404) {
      return 'Prune plan not available.'
    }
    if (error.status === 409) {
      return 'This prune plan is no longer valid because the underlying backup state changed. Create a new plan to continue safely.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Synveil could not confirm the result. Retry the same request or refresh the plan.'
    }
  }
  return 'We could not confirm the result. Retry this same request safely.'
}

type PruneStep = 'plan' | 'confirm' | 'complete'

function PruneSteps({ current }: { readonly current: PruneStep }) {
  return (
    <ol className="prune-workflow-steps" aria-label="Prune workflow steps">
      <li className={current === 'plan' ? 'prune-workflow-step prune-workflow-step--active' : 'prune-workflow-step'} aria-current={current === 'plan' ? 'step' : undefined}>
        <span aria-hidden="true">1</span><strong>Plan</strong><small>Review impact</small>
      </li>
      <li className={current === 'confirm' ? 'prune-workflow-step prune-workflow-step--active' : 'prune-workflow-step'} aria-current={current === 'confirm' ? 'step' : undefined}>
        <span aria-hidden="true">2</span><strong>Confirm</strong><small>Explicit action</small>
      </li>
      <li className={current === 'complete' ? 'prune-workflow-step prune-workflow-step--active' : 'prune-workflow-step'} aria-current={current === 'complete' ? 'step' : undefined}>
        <span aria-hidden="true">3</span><strong>Complete</strong>
      </li>
    </ol>
  )
}

interface ConfirmPruneDialogProps {
  readonly plan: BackupPrunePlan
  readonly snapshot: BackupSnapshot
  readonly backupSetName: string
  readonly action: MutationAction | null
  readonly error?: string
  readonly onClose: () => void
  readonly onExecute: (retryKey?: string) => void
}

function ConfirmPruneDialog({
  plan,
  snapshot,
  backupSetName,
  action,
  error,
  onClose,
  onExecute,
}: ConfirmPruneDialogProps) {
  const [confirmed, setConfirmed] = useState(false)
  const dialogRef = useRef<HTMLDivElement>(null)
  const headingRef = useRef<HTMLHeadingElement>(null)
  const previousFocus = useRef<HTMLElement | null>(null)
  const submitting = action?.phase === 'submitting'

  useEffect(() => {
    previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    headingRef.current?.focus()
    return () => {
      document.body.style.overflow = previousOverflow
      previousFocus.current?.focus()
    }
  }, [])

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === 'Escape' && !submitting) {
      event.preventDefault()
      onClose()
      return
    }
    if (event.key !== 'Tab') {
      return
    }
    const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), [href], [tabindex]:not([tabindex="-1"])',
    )
    if (!focusable || focusable.length === 0) {
      return
    }
    const first = focusable[0]
    const last = focusable[focusable.length - 1]
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault()
      last?.focus()
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault()
      first?.focus()
    }
  }

  return (
    <div className="prune-dialog-backdrop">
      <div
        className="prune-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-prune-title"
        aria-describedby="confirm-prune-description"
        ref={dialogRef}
        onKeyDown={handleKeyDown}
      >
        <div className="prune-dialog-heading">
          <div>
            <p className="eyebrow">Explicit confirmation</p>
            <h2 id="confirm-prune-title" ref={headingRef} tabIndex={-1}>Release retained backup content?</h2>
          </div>
          <button type="button" className="button--quiet" onClick={onClose} disabled={submitting} aria-label="Close confirmation dialog">×</button>
        </div>
        <p id="confirm-prune-description">
          This releases the selected expired snapshot&apos;s authority to retain its content. The snapshot and its historical tree remain visible.
        </p>
        <dl className="prune-dialog-summary">
          <SummaryMetric label="Backup set">{backupSetName}</SummaryMetric>
          <SummaryMetric label="Snapshot"><Timestamp value={snapshot.committed_at ?? snapshot.created_at} /></SummaryMetric>
          <SummaryMetric label="Snapshot items">{formatInteger(plan.entry_count)}</SummaryMetric>
          <SummaryMetric label="Retained elsewhere">{formatInteger(plan.retained_by_other_reference_count)}</SummaryMetric>
          <SummaryMetric label="Handed to internal cleanup">{formatInteger(plan.would_become_unreferenced_count)}</SummaryMetric>
        </dl>
        <p className="prune-boundary-copy">
          Items that become unreferenced are handed to Synveil&apos;s internal cleanup process. This does not mean physical storage has already been deleted or reclaimed.
        </p>
        <label className="prune-confirm-checkbox">
          <input
            type="checkbox"
            checked={confirmed}
            disabled={submitting}
            onChange={(event) => setConfirmed(event.currentTarget.checked)}
          />
          <span>I understand this releases this backup&apos;s retained content.</span>
        </label>
        {error && <div className="prune-inline-error" role="alert" aria-live="assertive"><p>{error}</p></div>}
        <div className="prune-dialog-actions">
          <button
            type="button"
            className="button--danger"
            disabled={!confirmed || action !== null}
            onClick={() => onExecute()}
          >
            {submitting ? 'Submitting…' : 'Release retained content'}
          </button>
          {action?.phase === 'failed' && (
            <button type="button" className="button--secondary" onClick={() => onExecute(action.idempotencyKey)}>
              Try the same request again
            </button>
          )}
          <button type="button" className="button--secondary" onClick={onClose} disabled={submitting}>Cancel</button>
        </div>
      </div>
    </div>
  )
}

export function PruneWorkflowPage({ backupApi = defaultBackupApi }: PruneWorkflowPageProps) {
  const navigate = useNavigate()
  const { refresh: refreshAuth } = useAuth()
  const { backupSetId, snapshotId, prunePlanId } = useParams<{
    backupSetId: string
    snapshotId: string
    prunePlanId: string
  }>()
  const [snapshotState, setSnapshotState] = useState<DataState<BackupSnapshot>>({ status: 'idle' })
  const [backupSetState, setBackupSetState] = useState<DataState<BackupSet>>({ status: 'idle' })
  const [planState, setPlanState] = useState<DataState<BackupPrunePlan>>({ status: 'idle' })
  const [executionState, setExecutionState] = useState<DataState<BackupPruneExecution>>({ status: 'idle' })
  const [createAction, setCreateAction] = useState<MutationAction | null>(null)
  const [createError, setCreateError] = useState<string>()
  const [executeAction, setExecuteAction] = useState<MutationAction | null>(null)
  const [executeError, setExecuteError] = useState<string>()
  const [dialogOpen, setDialogOpen] = useState(false)
  const [planRefreshVersion, setPlanRefreshVersion] = useState(0)
  const [pendingCreate, setPendingCreate] = useState<PendingMutationRecord | undefined>(() =>
    findPendingMutation(
      'create_prune_plan',
      (scope) => scope.backupSetId === backupSetId && scope.snapshotId === snapshotId,
    ),
  )
  const [pendingExecution, setPendingExecution] = useState<PendingMutationRecord | undefined>(() =>
    findPendingMutation(
      'execute_prune_plan',
      (scope) => scope.backupSetId === backupSetId && scope.prunePlanId === prunePlanId,
    ),
  )

  const mounted = useRef(true)
  const loadController = useRef<AbortController | undefined>(undefined)
  const receiptController = useRef<AbortController | undefined>(undefined)
  const createRef = useRef<MutationAction | null>(null)
  const executeRef = useRef<MutationAction | null>(null)
  const completionHeadingRef = useRef<HTMLHeadingElement>(null)

  const recoverSession = useCallback((error: unknown) => {
    if (isApiRequestError(error) && error.status === 401) {
      void refreshAuth().catch(() => undefined)
    }
  }, [refreshAuth])

  useEffect(() => () => {
    mounted.current = false
    loadController.current?.abort()
    receiptController.current?.abort()
  }, [])

  useEffect(() => {
    setPendingCreate(findPendingMutation(
      'create_prune_plan',
      (scope) => scope.backupSetId === backupSetId && scope.snapshotId === snapshotId,
    ))
    setPendingExecution(findPendingMutation(
      'execute_prune_plan',
      (scope) => scope.backupSetId === backupSetId && scope.prunePlanId === prunePlanId,
    ))
  }, [backupSetId, prunePlanId, snapshotId])

  const loadSnapshotAndSet = useCallback(async (requestedSnapshotId: string, expectedSetId: string, signal: AbortSignal) => {
    const [snapshotResponse, setResponse] = await Promise.all([
      backupApi.getBackupSnapshot(requestedSnapshotId, { signal }),
      backupApi.getBackupSet(expectedSetId, { signal }),
    ])
    if (
      snapshotResponse.data.snapshot_id !== requestedSnapshotId ||
      snapshotResponse.data.backup_set_id !== expectedSetId ||
      setResponse.data.backup_set_id !== expectedSetId
    ) {
      throw new Error('backup scope mismatch')
    }
    return { snapshot: snapshotResponse.data, backupSet: setResponse.data }
  }, [backupApi])

  const readWorkflow = useCallback(async () => {
    if (!backupSetId || (!snapshotId && !prunePlanId)) {
      return
    }
    loadController.current?.abort()
    const controller = new AbortController()
    loadController.current = controller
    setSnapshotState({ status: 'loading' })
    setBackupSetState({ status: 'loading' })
    if (prunePlanId) {
      setPlanState({ status: 'loading' })
    }
    try {
      let plan: BackupPrunePlan | undefined
      let requestedSnapshotId = snapshotId
      if (prunePlanId) {
        const response = await backupApi.getBackupPrunePlan(prunePlanId, { signal: controller.signal })
        if (response.data.prune_plan_id !== prunePlanId || response.data.backup_set_id !== backupSetId) {
          throw new Error('prune plan scope mismatch')
        }
        plan = response.data
        requestedSnapshotId = plan.snapshot_id
      }
      if (!requestedSnapshotId) {
        throw new Error('snapshot identity missing')
      }
      const details = await loadSnapshotAndSet(requestedSnapshotId, backupSetId, controller.signal)
      if (!mounted.current || controller.signal.aborted) {
        return
      }
      setSnapshotState({ status: 'success', data: details.snapshot })
      setBackupSetState({ status: 'success', data: details.backupSet })
      if (plan) {
        setPlanState({ status: 'success', data: plan })
      }
    } catch (error) {
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      if (prunePlanId) {
        setPlanState({ status: 'error', error })
      }
      setSnapshotState({ status: 'error', error })
      setBackupSetState({ status: 'error', error })
      recoverSession(error)
    }
  }, [backupApi, backupSetId, loadSnapshotAndSet, prunePlanId, recoverSession, snapshotId])

  useEffect(() => {
    void readWorkflow()
  }, [planRefreshVersion, readWorkflow])

  const loadExecutionReceipt = useCallback(async (plan: BackupPrunePlan) => {
    receiptController.current?.abort()
    const controller = new AbortController()
    receiptController.current = controller
    setExecutionState({ status: 'loading' })
    try {
      const operationResponse = await backupApi.getBackupOperation('PRUNE', plan.prune_plan_id, { signal: controller.signal })
      const operation = operationResponse.data
      if (operation.operation_kind !== 'PRUNE' || !operation.prune_execution_id) {
        throw new Error('prune receipt unavailable')
      }
      const response = await backupApi.getBackupPruneExecution(operation.prune_execution_id, { signal: controller.signal })
      if (
        !mounted.current ||
        controller.signal.aborted ||
        response.data.prune_plan_id !== plan.prune_plan_id ||
        response.data.snapshot_id !== plan.snapshot_id
      ) {
        return
      }
      if (pendingExecution?.request_scope.prunePlanId === plan.prune_plan_id) {
        mutationRecoveryStore.remove(pendingExecution.action_id)
        setPendingExecution(undefined)
      }
      setExecutionState({ status: 'success', data: response.data })
    } catch (error) {
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      setExecutionState({ status: 'error', error })
      recoverSession(error)
    }
  }, [backupApi, pendingExecution, recoverSession])

  useEffect(() => {
    const plan = planState.data
    if (plan?.state === 'EXECUTED' && executionState.status === 'idle') {
      void loadExecutionReceipt(plan)
    }
  }, [executionState.status, loadExecutionReceipt, planState.data])

  useEffect(() => {
    const plan = planState.data
    if (!plan || !pendingExecution) {
      return
    }
    const confirmedSnapshotId = pendingConfirmSnapshotId(pendingExecution)
    if (plan.state === 'STALE' || confirmedSnapshotId !== plan.snapshot_id) {
      mutationRecoveryStore.remove(pendingExecution.action_id)
      setPendingExecution(undefined)
      setDialogOpen(false)
      setExecuteError(plan.state === 'STALE'
        ? 'The prune plan is stale, so its impossible local retry was cleared. No content was released.'
        : 'The local retry did not match the loaded prune plan and was cleared. No content was released.')
    }
  }, [pendingExecution, planState.data])

  useEffect(() => {
    if (executionState.status === 'success') {
      completionHeadingRef.current?.focus()
    }
  }, [executionState.status])

  useEffect(() => {
    if (snapshotState.status === 'error' || planState.status === 'error') {
      const heading = document.querySelector<HTMLElement>('.prune-error-panel h1, .prune-error-panel h2')
      if (heading) {
        heading.setAttribute('tabindex', '-1')
        heading.focus()
      }
    }
  }, [planState.status, snapshotState.status])

  async function submitCreate(retryKey?: string) {
    const snapshot = snapshotState.data
    if (!snapshot || snapshot.state !== 'EXPIRED' || createRef.current?.phase === 'submitting') {
      return
    }
    let idempotencyKey = retryKey
    if (!idempotencyKey) {
      try {
        idempotencyKey = createUuidV7()
      } catch {
        setCreateError('Secure browser randomness is unavailable. The prune plan was not created.')
        return
      }
    }
    const action: MutationAction = { idempotencyKey, phase: 'submitting' }
    const recoveryRecord = mutationRecoveryStore.add({
      action_kind: 'create_prune_plan',
      idempotency_key: idempotencyKey,
      request_scope: { backupSetId, snapshotId: snapshot.snapshot_id },
      request: {},
    })
    if (mutationRecoveryStore.isAvailable) {
      setPendingCreate(recoveryRecord)
    }
    createRef.current = action
    setCreateAction(action)
    setCreateError(undefined)
    let postConfirmed = false
    try {
      const response = await backupApi.createBackupPrunePlan(snapshot.snapshot_id, idempotencyKey)
      postConfirmed = true
      mutationRecoveryStore.remove(recoveryRecord.action_id)
      setPendingCreate((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      if (!mounted.current) {
        return
      }
      if (
        response.data.snapshot_id !== snapshot.snapshot_id ||
        response.data.backup_set_id !== backupSetId
      ) {
        throw new Error('prune plan scope mismatch')
      }
      createRef.current = null
      setCreateAction(null)
      void backupApi.listBackupOperations(response.data.backup_set_id, { kind: 'PRUNE', limit: 50 }).catch(recoverSession)
      navigate(`/backups/${encodeURIComponent(response.data.backup_set_id)}/prune/${encodeURIComponent(response.data.prune_plan_id)}`)
    } catch (error) {
      const recoveryOutcome = postConfirmed
        ? 'removed'
        : settlePendingMutationFailure(recoveryRecord, error)
      if (recoveryOutcome === 'removed') {
        setPendingCreate((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      }
      if (!mounted.current) {
        return
      }
      if (recoveryOutcome === 'retained') {
        const failed: MutationAction = { ...action, phase: 'failed' }
        createRef.current = failed
        setCreateAction(failed)
      } else {
        createRef.current = null
        setCreateAction(null)
      }
      setCreateError(planningErrorMessage(error))
      recoverSession(error)
    }
  }

  async function submitExecution(retryKey?: string) {
    const plan = planState.data
    if (!plan || plan.state !== 'PLANNED' || executeRef.current?.phase === 'submitting') {
      return
    }
    let idempotencyKey = retryKey ?? pendingExecution?.idempotency_key
    if (!idempotencyKey) {
      try {
        idempotencyKey = createUuidV7()
      } catch {
        setExecuteError('Secure browser randomness is unavailable. No content was released.')
        return
      }
    }
    const action: MutationAction = { idempotencyKey, phase: 'submitting' }
    const recoveryRecord = mutationRecoveryStore.add({
      action_kind: 'execute_prune_plan',
      idempotency_key: idempotencyKey,
      request_scope: {
        backupSetId: plan.backup_set_id,
        snapshotId: plan.snapshot_id,
        prunePlanId: plan.prune_plan_id,
      },
      request: { confirm_snapshot_id: plan.snapshot_id },
    })
    if (mutationRecoveryStore.isAvailable) {
      setPendingExecution(recoveryRecord)
    }
    executeRef.current = action
    setExecuteAction(action)
    setExecuteError(undefined)
    let postConfirmed = false
    try {
      // Confirmation identity is deliberately taken only from the persisted loaded plan.
      const response = await backupApi.executeBackupPrunePlan(
        plan.prune_plan_id,
        plan.snapshot_id,
        idempotencyKey,
      )
      postConfirmed = true
      mutationRecoveryStore.remove(recoveryRecord.action_id)
      setPendingExecution((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      const receiptResponse = await backupApi.getBackupPruneExecution(response.data.prune_execution_id)
      if (!mounted.current) {
        return
      }
      if (
        receiptResponse.data.prune_plan_id !== plan.prune_plan_id ||
        receiptResponse.data.snapshot_id !== plan.snapshot_id
      ) {
        throw new Error('prune receipt scope mismatch')
      }
      executeRef.current = null
      setExecuteAction(null)
      setDialogOpen(false)
      setExecutionState({ status: 'success', data: receiptResponse.data })
      setPlanState({ status: 'success', data: { ...plan, state: 'EXECUTED' } })
      const [snapshotRefresh] = await Promise.allSettled([
        backupApi.getBackupSnapshot(plan.snapshot_id),
        backupApi.listBackupOperations(plan.backup_set_id, { kind: 'PRUNE', limit: 50 }),
      ])
      if (mounted.current && snapshotRefresh.status === 'fulfilled') {
        setSnapshotState({ status: 'success', data: snapshotRefresh.value.data })
      }
    } catch (error) {
      const recoveryOutcome = postConfirmed
        ? 'removed'
        : settlePendingMutationFailure(recoveryRecord, error)
      if (recoveryOutcome === 'removed') {
        setPendingExecution((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      }
      if (!mounted.current) {
        return
      }
      if (postConfirmed) {
        executeRef.current = null
        setExecuteAction(null)
        setDialogOpen(false)
        setExecuteError('The retention change was accepted, but its completion receipt could not be loaded. Check current status before taking another action.')
        setPlanRefreshVersion((current) => current + 1)
      } else if (isApiRequestError(error) && error.status === 409) {
        executeRef.current = null
        setExecuteAction(null)
        setDialogOpen(false)
        setExecuteError(executionErrorMessage(error))
        setPlanState({ status: 'success', data: { ...plan, state: 'STALE', stale_at: new Date().toISOString() } })
        void backupApi.getBackupPrunePlan(plan.prune_plan_id).then((response) => {
          if (mounted.current && response.data.state !== 'PLANNED') {
            setPlanState({ status: 'success', data: response.data })
          }
        }).catch(recoverSession)
      } else {
        const failed = recoveryOutcome === 'retained'
          ? { ...action, phase: 'failed' as const }
          : null
        executeRef.current = failed
        setExecuteAction(failed)
        setExecuteError(executionErrorMessage(error))
      }
      recoverSession(error)
    }
  }

  function createNewPlan() {
    const sourceId = planState.data?.snapshot_id ?? snapshotState.data?.snapshot_id
    if (!backupSetId || !sourceId) {
      navigate('/backups')
      return
    }
    navigate(`/backups/${encodeURIComponent(backupSetId)}/snapshots/${encodeURIComponent(sourceId)}/prune`)
  }

  function discardPendingCreate() {
    if (!pendingCreate) {
      return
    }
    mutationRecoveryStore.remove(pendingCreate.action_id)
    if (createRef.current?.idempotencyKey === pendingCreate.idempotency_key) {
      createRef.current = null
      setCreateAction(null)
      setCreateError(undefined)
    }
    setPendingCreate(undefined)
  }

  function discardPendingExecution() {
    if (!pendingExecution) {
      return
    }
    mutationRecoveryStore.remove(pendingExecution.action_id)
    if (executeRef.current?.idempotencyKey === pendingExecution.idempotency_key) {
      executeRef.current = null
      setExecuteAction(null)
      setExecuteError(undefined)
    }
    setDialogOpen(false)
    setPendingExecution(undefined)
  }

  if (!backupSetId || (!snapshotId && !prunePlanId)) {
    return <section className="panel prune-error-panel" role="alert"><h1>Prune workflow not available</h1><p>The requested backup could not be identified.</p><Link className="button-link" to="/backups">Back to backups</Link></section>
  }

  const sourceSnapshot = snapshotState.data
  const backupSetName = backupSetState.data?.name ?? 'Selected backup set'
  const plan = planState.data
  const receipt = executionState.data
  const completed = Boolean(receipt) || plan?.state === 'EXECUTED'

  return (
    <div className="backup-page prune-workflow-page">
      <section className="backup-page-heading" aria-labelledby="prune-page-title">
        <div>
          <p className="eyebrow">Protected copies · Retention</p>
          <h1 id="prune-page-title">Release retained backup content</h1>
          <p>Plan and review the retention impact before making an explicit change.</p>
        </div>
        <Link className="button-link button--secondary" to={`/backups/${encodeURIComponent(backupSetId)}`}>Back to backup set</Link>
      </section>

      {!prunePlanId && pendingCreate && (
        <MutationRecoveryNotice
          record={pendingCreate}
          title="Finish creating this prune plan"
          busy={createAction?.phase === 'submitting'}
          onRetry={() => void submitCreate(pendingCreate.idempotency_key)}
          onDiscard={discardPendingCreate}
        />
      )}

      {prunePlanId && pendingExecution && plan?.state === 'PLANNED' && (
        <MutationRecoveryNotice
          record={pendingExecution}
          title="Finish the pending retention release"
          retryLabel="Retry safely"
          busy={executeAction?.phase === 'submitting'}
          onCheckStatus={() => setPlanRefreshVersion((value) => value + 1)}
          onRetry={() => {
            setExecuteError(undefined)
            setDialogOpen(true)
          }}
          onDiscard={discardPendingExecution}
        />
      )}

      <PruneSteps current={completed ? 'complete' : plan ? 'confirm' : 'plan'} />

      {(snapshotState.status === 'loading' || (prunePlanId && planState.status === 'loading')) && (
        <section className="panel" aria-labelledby="prune-loading-title"><h2 id="prune-loading-title">Loading retention details</h2><p role="status" aria-busy="true">Loading the persisted backup information…</p></section>
      )}

      {(snapshotState.status === 'error' || planState.status === 'error') && (
        <section className="panel prune-error-panel" role="alert">
          <h2>{prunePlanId ? 'Prune plan not available' : 'Snapshot not available'}</h2>
          <p>{loadErrorMessage(planState.error ?? snapshotState.error, prunePlanId ? 'plan' : 'snapshot')}</p>
          <button type="button" className="button--secondary" onClick={() => setPlanRefreshVersion((value) => value + 1)}>Try again</button>
        </section>
      )}

      {sourceSnapshot && (
        <section className="panel prune-source-panel" aria-labelledby="prune-source-title">
          <div className="backup-section-header backup-section-header--compact">
            <div><p className="eyebrow">Historical snapshot</p><h2 id="prune-source-title">{backupSetName}</h2></div>
            <StatusPill tone="planned">{sourceSnapshot.state === 'EXPIRED' ? 'Expired' : sourceSnapshot.state}</StatusPill>
          </div>
          <dl className="prune-summary-grid">
            <SummaryMetric label="Snapshot date"><Timestamp value={sourceSnapshot.committed_at ?? sourceSnapshot.created_at} /></SummaryMetric>
            <SummaryMetric label="Logical items">{formatInteger(sourceSnapshot.logical_node_count)}</SummaryMetric>
            <SummaryMetric label="State">{sourceSnapshot.state === 'EXPIRED' ? 'Expired' : sourceSnapshot.state}</SummaryMetric>
          </dl>
          <p>This historical snapshot and its logical tree remain visible after retained content is released.</p>
        </section>
      )}

      {!prunePlanId && sourceSnapshot && sourceSnapshot.state !== 'EXPIRED' && (
        <section className="panel prune-error-panel" role="alert">
          <h2>This snapshot is not eligible</h2>
          <p>Only an expired snapshot can begin a prune plan. No retention change was made.</p>
        </section>
      )}

      {!prunePlanId && sourceSnapshot?.state === 'EXPIRED' && (
        <section className="panel prune-plan-start" aria-labelledby="prune-plan-start-title">
          <p className="eyebrow">Step 1</p>
          <h2 id="prune-plan-start-title">Create a retention-impact plan</h2>
          <p>Planning is read-only. It calculates how this snapshot&apos;s retained content is referenced without releasing anything.</p>
          <button type="button" onClick={() => void submitCreate()} disabled={createAction !== null || Boolean(pendingCreate)}>
            {createAction?.phase === 'submitting' ? 'Creating plan…' : 'Create prune plan'}
          </button>
          {createAction?.phase === 'failed' && (
            <button type="button" className="button--secondary" onClick={() => void submitCreate(createAction.idempotencyKey)}>Try the same request again</button>
          )}
          {createError && <p className="prune-inline-error" role="alert">{createError}</p>}
        </section>
      )}

      {plan && !receipt && plan.state !== 'EXECUTED' && (
        <section className="panel prune-review-panel" aria-labelledby="prune-review-title">
          <div className="backup-section-header backup-section-header--compact">
            <div>
              <p className="eyebrow">Persisted plan</p>
              <h2 id="prune-review-title">Review retention impact</h2>
              <p className="backup-section-subtitle">These counts come from the exact saved plan.</p>
            </div>
            <StatusPill tone={plan.state === 'STALE' ? 'quiet' : 'planned'}>{plan.state === 'STALE' ? 'Stale' : 'Planned'}</StatusPill>
          </div>
          <dl className="prune-impact-grid">
            <SummaryMetric label="Snapshot entries">{formatInteger(plan.entry_count)}</SummaryMetric>
            <SummaryMetric label="Distinct logical content">{formatInteger(plan.distinct_content_count)}</SummaryMetric>
            <SummaryMetric label="Retained by another reference">{formatInteger(plan.retained_by_other_reference_count)}</SummaryMetric>
            <SummaryMetric label="Would become unreferenced">{formatInteger(plan.would_become_unreferenced_count)}</SummaryMetric>
          </dl>
          <div className="prune-boundary-copy">
            <p>Items that become unreferenced are handed to Synveil&apos;s internal cleanup process.</p>
            <p>This does not mean physical storage has already been deleted or reclaimed.</p>
          </div>
          {plan.state === 'PLANNED' && (
            <div className="prune-explicit-action">
              <p>Nothing is released until you review a final confirmation and deliberately approve it.</p>
              <button type="button" className="button--danger-outline" onClick={() => {
                setExecuteError(undefined)
                setDialogOpen(true)
              }}>Review and confirm</button>
            </div>
          )}
          {plan.state === 'STALE' && (
            <div className="prune-inline-error" role="alert">
              <p>This operation is no longer valid because the underlying backup state changed. Create a new plan to continue safely.</p>
              <button type="button" className="button--secondary" onClick={createNewPlan}>Create a new plan</button>
            </div>
          )}
          {executeError && plan.state === 'STALE' && <p className="prune-inline-error" role="alert">{executeError}</p>}
        </section>
      )}

      {plan?.state === 'EXECUTED' && executionState.status === 'loading' && (
        <section className="panel"><h2>Loading completion receipt</h2><p role="status" aria-busy="true">Reading the server-confirmed result…</p></section>
      )}

      {plan?.state === 'EXECUTED' && executionState.status === 'error' && (
        <section className="panel prune-error-panel" role="alert"><h2>Completion receipt unavailable</h2><p>The retention change is recorded, but its receipt could not be loaded.</p><button type="button" className="button--secondary" onClick={() => void loadExecutionReceipt(plan)}>Try again</button></section>
      )}

      {plan && receipt && (
        <section className="panel prune-success-panel" aria-labelledby="prune-success-title">
          <div className="backup-section-header backup-section-header--compact">
            <div><p className="eyebrow">Server-confirmed result</p><h2 id="prune-success-title" ref={completionHeadingRef} tabIndex={-1}>Backup retention released</h2></div>
            <StatusPill tone="info">Executed</StatusPill>
          </div>
          <p role="status">Content no longer referenced by other backups was handed to Synveil&apos;s cleanup process.</p>
          <p>Physical storage cleanup is a separate internal process. The expired snapshot remains in backup history.</p>
          <dl className="prune-summary-grid">
            <SummaryMetric label="Released references">{formatInteger(receipt.released_content_reference_count)}</SummaryMetric>
            <SummaryMetric label="Retained elsewhere">{formatInteger(receipt.retained_elsewhere_count)}</SummaryMetric>
            <SummaryMetric label="Handed to internal cleanup">{formatInteger(receipt.gc_handoff_count)}</SummaryMetric>
            <SummaryMetric label="Completed"><Timestamp value={receipt.executed_at} /></SummaryMetric>
          </dl>
          <Link className="button-link" to={`/backups/${encodeURIComponent(plan.backup_set_id)}`}>Return to backup history</Link>
        </section>
      )}

      {dialogOpen && plan?.state === 'PLANNED' && sourceSnapshot && (
        <ConfirmPruneDialog
          plan={plan}
          snapshot={sourceSnapshot}
          backupSetName={backupSetName}
          action={executeAction}
          error={executeError}
          onClose={() => {
            if (executeAction?.phase !== 'submitting') {
              executeRef.current = null
              setExecuteAction(null)
              setExecuteError(undefined)
              setDialogOpen(false)
            }
          }}
          onExecute={(retryKey) => void submitExecution(retryKey)}
        />
      )}
    </div>
  )
}
