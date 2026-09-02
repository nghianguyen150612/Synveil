import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react'
import { Link, useNavigate } from 'react-router-dom'

import {
  backupApi as defaultBackupApi,
  createUuidV7,
  type BackupApi,
  type BackupOperationSummary,
  type BackupRetentionPolicy,
  type BackupSet,
  type BackupSnapshot,
  type CreateBackupSetRequest,
} from '../../api/backups'
import { isApiRequestError } from '../../api/errors'
import {
  findPendingMutation,
  mutationRecoveryStore,
  settlePendingMutationFailure,
  type PendingMutationRecord,
} from '../../api/mutationRecovery'
import {
  fileMetadataApi as defaultFileApi,
  type FileMetadataApi,
  type LiveLibraryResource,
} from '../../api/files'
import { useAuth } from '../../auth/useAuth'
import { StatusPill } from '../../components/ui/StatusPill'
import { MutationRecoveryNotice } from './MutationRecoveryNotice'

const PAGE_LIMIT = 50
const MAX_NAME_LENGTH = 1024

type LoadStatus = 'loading' | 'success' | 'error'

interface SetCollection {
  readonly status: LoadStatus
  readonly items: readonly BackupSet[]
  readonly nextCursor?: string
  readonly hasMore: boolean
  readonly error?: unknown
}

interface BackupCardSummary {
  readonly status: 'loading' | 'success'
  readonly policy?: BackupRetentionPolicy['data'] | null
  readonly latestSnapshot?: BackupSnapshot
  readonly latestOperation?: BackupOperationSummary
  readonly unavailable?: boolean
}

interface CreateAction {
  readonly idempotencyKey: string
  readonly request: CreateBackupSetRequest
  readonly phase: 'submitting' | 'failed'
}

export interface BackupOverviewPageProps {
  readonly backupApi?: BackupApi
  readonly fileApi?: FileMetadataApi
}

function isAbortError(error: unknown): boolean {
  return typeof DOMException !== 'undefined' && error instanceof DOMException && error.name === 'AbortError'
}

function formatTimestamp(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(date)
}

function Timestamp({ value }: { readonly value: string }) {
  return <time dateTime={value} title={value}>{formatTimestamp(value)}</time>
}

function formatInteger(value: string | number): string {
  try {
    return new Intl.NumberFormat().format(BigInt(value))
  } catch {
    return String(value)
  }
}

function formatRetention(policy?: BackupRetentionPolicy['data'] | null): string {
  if (policy === undefined) {
    return 'Loading retention settings…'
  }
  if (policy === null) {
    return 'Retention settings not configured'
  }
  return `Keep at least ${formatInteger(policy.keep_latest_completed)} completed backup${policy.keep_latest_completed === '1' ? '' : 's'}`
}

function snapshotLabel(snapshot: BackupSnapshot): string {
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

function operationLabel(operation: BackupOperationSummary): string {
  const kind = operation.operation_kind === 'MAINTENANCE'
    ? 'Maintenance'
    : operation.operation_kind === 'RESTORE'
      ? 'Restore'
      : 'Prune'
  return `${kind} · ${operation.progress.completed_steps} of ${operation.progress.total_steps} steps completed`
}

function loadErrorMessage(error: unknown): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again to view backups.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Backup data is temporarily unavailable. Please try again.'
    }
  }
  return 'Backups could not be loaded. Please try again.'
}

function createErrorMessage(error: unknown): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again before creating a backup set.'
    }
    if (error.status === 403) {
      return 'This request could not be verified. Refresh the page and try again.'
    }
    if (error.status === 404) {
      return 'The selected library is no longer available.'
    }
    if (error.status === 409) {
      return 'A backup set already uses this library and name. Choose a different name or library.'
    }
    if (error.status === 400 || error.status === 413) {
      return 'Check the backup name and library, then try again.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Backup setup is temporarily unavailable. Please try again.'
    }
  }
  return 'We could not confirm the result. Retry this same request safely.'
}

function mergeUniqueSets(current: readonly BackupSet[], incoming: readonly BackupSet[]): BackupSet[] {
  const seen = new Set(current.map((item) => item.backup_set_id))
  return [...current, ...incoming.filter((item) => !seen.has(item.backup_set_id))]
}

function pendingCreateRequest(record: PendingMutationRecord): CreateBackupSetRequest | undefined {
  const libraryId = record.request.library_id
  const pendingName = record.request.name
  if (typeof libraryId !== 'string' || typeof pendingName !== 'string') {
    return undefined
  }
  return { library_id: libraryId, name: pendingName }
}

export function BackupOverviewPage({
  backupApi = defaultBackupApi,
  fileApi = defaultFileApi,
}: BackupOverviewPageProps) {
  const navigate = useNavigate()
  const { refresh: refreshAuth } = useAuth()
  const [sets, setSets] = useState<SetCollection>({ status: 'loading', items: [], hasMore: false })
  const [summaries, setSummaries] = useState<Readonly<Record<string, BackupCardSummary>>>({})
  const [refreshVersion, setRefreshVersion] = useState(0)
  const [loadingMore, setLoadingMore] = useState(false)
  const [showCreate, setShowCreate] = useState(false)
  const [libraries, setLibraries] = useState<readonly LiveLibraryResource[]>([])
  const [librariesStatus, setLibrariesStatus] = useState<'idle' | LoadStatus>('idle')
  const [librariesCursor, setLibrariesCursor] = useState<string>()
  const [librariesHaveMore, setLibrariesHaveMore] = useState(false)
  const [librariesLoadingMore, setLibrariesLoadingMore] = useState(false)
  const [name, setName] = useState('')
  const [libraryId, setLibraryId] = useState('')
  const [validationError, setValidationError] = useState<string>()
  const [createError, setCreateError] = useState<string>()
  const [createAction, setCreateAction] = useState<CreateAction | null>(null)
  const [pendingCreate, setPendingCreate] = useState<PendingMutationRecord | undefined>(() =>
    findPendingMutation('create_backup_set'),
  )

  const mounted = useRef(true)
  const setsRequestId = useRef(0)
  const summaryRequestId = useRef(0)
  const libraryRequestId = useRef(0)
  const controllers = useRef(new Set<AbortController>())
  const createActionRef = useRef<CreateAction | null>(null)

  const recoverSession = useCallback((error: unknown) => {
    if (isApiRequestError(error) && error.status === 401) {
      void refreshAuth().catch(() => undefined)
    }
  }, [refreshAuth])

  const trackedController = useCallback(() => {
    const controller = new AbortController()
    controllers.current.add(controller)
    return controller
  }, [])

  const releaseController = useCallback((controller: AbortController) => {
    controllers.current.delete(controller)
  }, [])

  useEffect(() => () => {
    mounted.current = false
    for (const controller of controllers.current) {
      controller.abort()
    }
  }, [])

  const loadSummaries = useCallback(async (items: readonly BackupSet[]) => {
    if (items.length === 0) {
      return
    }
    const requestId = summaryRequestId.current + 1
    summaryRequestId.current = requestId
    const controller = trackedController()
    setSummaries((current) => {
      const next = { ...current }
      for (const item of items) {
        next[item.backup_set_id] = { status: 'loading' }
      }
      return next
    })

    await Promise.all(items.map(async (item) => {
      const [policyResult, snapshotResult, operationResult] = await Promise.allSettled([
        backupApi.getBackupRetentionPolicy(item.backup_set_id, { signal: controller.signal }),
        backupApi.listBackupSnapshots(item.backup_set_id, { limit: 1, signal: controller.signal }),
        backupApi.listBackupOperations(item.backup_set_id, { limit: 1, signal: controller.signal }),
      ])
      if (!mounted.current || controller.signal.aborted || summaryRequestId.current !== requestId) {
        return
      }
      let policy: BackupRetentionPolicy['data'] | null | undefined
      if (policyResult.status === 'fulfilled') {
        policy = policyResult.value.data.backup_set_id === item.backup_set_id
          ? policyResult.value.data
          : undefined
      } else {
        policy = isApiRequestError(policyResult.reason) && policyResult.reason.status === 404
          ? null
          : undefined
      }
      const snapshotCandidate = snapshotResult.status === 'fulfilled' ? snapshotResult.value.data[0] : undefined
      const operationCandidate = operationResult.status === 'fulfilled' ? operationResult.value.data[0] : undefined
      const latestSnapshot = snapshotCandidate?.backup_set_id === item.backup_set_id ? snapshotCandidate : undefined
      const latestOperation = operationCandidate?.backup_set_id === item.backup_set_id ? operationCandidate : undefined
      const unavailable =
        (policyResult.status === 'rejected' && policy === undefined) ||
        (policyResult.status === 'fulfilled' && policy === undefined) ||
        snapshotResult.status === 'rejected' ||
        operationResult.status === 'rejected' ||
        (snapshotCandidate !== undefined && latestSnapshot === undefined) ||
        (operationCandidate !== undefined && latestOperation === undefined)
      for (const result of [policyResult, snapshotResult, operationResult]) {
        if (result.status === 'rejected') {
          recoverSession(result.reason)
        }
      }
      setSummaries((current) => ({
        ...current,
        [item.backup_set_id]: {
          status: 'success',
          policy,
          latestSnapshot,
          latestOperation,
          unavailable,
        },
      }))
    }))
    releaseController(controller)
  }, [backupApi, recoverSession, releaseController, trackedController])

  useEffect(() => {
    const controller = trackedController()
    const requestId = setsRequestId.current + 1
    setsRequestId.current = requestId
    summaryRequestId.current += 1
    setSets({ status: 'loading', items: [], hasMore: false })
    setSummaries({})

    void backupApi.listBackupSets({ limit: PAGE_LIMIT, signal: controller.signal }).then((response) => {
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
      if (items.length === 0) {
        setShowCreate(true)
      }
      void loadSummaries(items)
    }).catch((error: unknown) => {
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      setSets({ status: 'error', items: [], hasMore: false, error })
      recoverSession(error)
    }).finally(() => releaseController(controller))

    return () => controller.abort()
  }, [backupApi, loadSummaries, recoverSession, refreshVersion, releaseController, trackedController])

  const loadLibraries = useCallback(async (cursor?: string, append = false) => {
    const controller = trackedController()
    const requestId = libraryRequestId.current + 1
    libraryRequestId.current = requestId
    if (append) {
      setLibrariesLoadingMore(true)
    } else {
      setLibrariesStatus('loading')
      setLibraries([])
    }
    try {
      const response = await fileApi.listLibraries({ cursor, limit: PAGE_LIMIT, signal: controller.signal })
      if (!mounted.current || controller.signal.aborted || libraryRequestId.current !== requestId) {
        return
      }
      setLibraries((current) => append ? [...current, ...response.data] : [...response.data])
      setLibrariesCursor(response.page.next_cursor)
      setLibrariesHaveMore(response.page.has_more)
      setLibrariesStatus('success')
      setLibraryId((current) => current || response.data[0]?.id || '')
    } catch (error) {
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      setLibrariesStatus('error')
      recoverSession(error)
    } finally {
      releaseController(controller)
      if (mounted.current) {
        setLibrariesLoadingMore(false)
      }
    }
  }, [fileApi, recoverSession, releaseController, trackedController])

  useEffect(() => {
    if (showCreate && librariesStatus === 'idle') {
      void loadLibraries()
    }
  }, [librariesStatus, loadLibraries, showCreate])

  function invalidateFailedCreate() {
    if (createActionRef.current?.phase === 'failed') {
      createActionRef.current = null
      setCreateAction(null)
      setCreateError(undefined)
    }
  }

  async function submitCreate(requestOverride?: CreateBackupSetRequest, retryKey?: string) {
    if (createActionRef.current?.phase === 'submitting') {
      return
    }
    const request = requestOverride ?? { library_id: libraryId, name }
    if (!request.library_id) {
      setValidationError('Choose a source library.')
      return
    }
    if (request.name.trim().length === 0) {
      setValidationError('Enter a backup set name.')
      return
    }
    if (request.name.length > MAX_NAME_LENGTH) {
      setValidationError('The backup set name is too long.')
      return
    }
    let idempotencyKey = retryKey
    if (!idempotencyKey) {
      try {
        idempotencyKey = createUuidV7()
      } catch {
        setCreateError('Secure browser randomness is unavailable. The backup set was not created.')
        return
      }
    }
    const action: CreateAction = { idempotencyKey, request, phase: 'submitting' }
    const recoveryRecord = mutationRecoveryStore.add({
      action_kind: 'create_backup_set',
      idempotency_key: idempotencyKey,
      request_scope: {},
      request: { ...request },
    })
    if (mutationRecoveryStore.isAvailable) {
      setPendingCreate(recoveryRecord)
    }
    createActionRef.current = action
    setCreateAction(action)
    setValidationError(undefined)
    setCreateError(undefined)
    let postConfirmed = false
    try {
      const response = await backupApi.createBackupSet(request, idempotencyKey)
      postConfirmed = true
      mutationRecoveryStore.remove(recoveryRecord.action_id)
      setPendingCreate((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      if (!mounted.current) {
        return
      }
      if (response.data.library_id !== request.library_id) {
        throw new Error('backup set scope mismatch')
      }
      createActionRef.current = null
      setCreateAction(null)
      navigate(`/backups/${encodeURIComponent(response.data.backup_set_id)}`)
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
        const failed: CreateAction = { ...action, phase: 'failed' }
        createActionRef.current = failed
        setCreateAction(failed)
      } else {
        createActionRef.current = null
        setCreateAction(null)
      }
      setCreateError(createErrorMessage(error))
      recoverSession(error)
    }
  }

  async function loadMoreSets() {
    if (!sets.nextCursor || !sets.hasMore || loadingMore) {
      return
    }
    const controller = trackedController()
    setLoadingMore(true)
    try {
      const response = await backupApi.listBackupSets({
        cursor: sets.nextCursor,
        limit: PAGE_LIMIT,
        signal: controller.signal,
      })
      if (!mounted.current || controller.signal.aborted) {
        return
      }
      setSets((current) => ({
        status: 'success',
        items: mergeUniqueSets(current.items, response.data),
        nextCursor: response.page.next_cursor,
        hasMore: response.page.has_more,
      }))
      void loadSummaries(response.data)
    } catch (error) {
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      setSets((current) => ({ ...current, error }))
      recoverSession(error)
    } finally {
      releaseController(controller)
      if (mounted.current) {
        setLoadingMore(false)
      }
    }
  }

  function submitForm(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    void submitCreate()
  }

  return (
    <div className="backup-page backup-control-center">
      <section className="backup-page-heading" aria-labelledby="backup-control-center-title">
        <div>
          <p className="eyebrow">Protected copies</p>
          <h1 id="backup-control-center-title">Backup Control Center</h1>
          <p>Review protected file history and choose when Synveil performs backup actions.</p>
        </div>
        <div className="backup-heading-actions">
          <button type="button" className="button--secondary" onClick={() => setRefreshVersion((value) => value + 1)}>
            Refresh
          </button>
          <button type="button" onClick={() => setShowCreate((value) => !value)} aria-expanded={showCreate}>
            {showCreate ? 'Close setup' : 'Create backup set'}
          </button>
        </div>
      </section>

      {pendingCreate && pendingCreateRequest(pendingCreate) && (
        <MutationRecoveryNotice
          record={pendingCreate}
          title="Finish creating this backup set"
          busy={createAction?.phase === 'submitting'}
          onRetry={() => {
            const request = pendingCreateRequest(pendingCreate)
            if (request) {
              void submitCreate(request, pendingCreate.idempotency_key)
            }
          }}
          onDiscard={() => {
            mutationRecoveryStore.remove(pendingCreate.action_id)
            if (createActionRef.current?.idempotencyKey === pendingCreate.idempotency_key) {
              createActionRef.current = null
              setCreateAction(null)
              setCreateError(undefined)
            }
            setPendingCreate(undefined)
          }}
        />
      )}

      {showCreate && (
        <section className="panel backup-create-panel" aria-labelledby="create-backup-set-title">
          <div className="backup-section-header backup-section-header--compact">
            <div>
              <p className="eyebrow">New protected collection</p>
              <h2 id="create-backup-set-title">Create a backup set</h2>
              <p className="backup-section-subtitle">Choose the library to protect and give this backup set a recognizable name.</p>
            </div>
          </div>
          {librariesStatus === 'loading' && <p role="status" aria-busy="true">Loading your libraries…</p>}
          {librariesStatus === 'error' && (
            <div className="backup-inline-error" role="alert">
              <p>Libraries are temporarily unavailable.</p>
              <button type="button" className="button--secondary" onClick={() => void loadLibraries()}>Try again</button>
            </div>
          )}
          {librariesStatus === 'success' && libraries.length === 0 && (
            <p className="backup-empty-copy">No source libraries are available for a backup set.</p>
          )}
          {librariesStatus === 'success' && libraries.length > 0 && (
            <form className="backup-create-form" onSubmit={submitForm} noValidate>
              <div className="field">
                <label htmlFor="backup-source-library">Source library</label>
                <select
                  id="backup-source-library"
                  value={libraryId}
                  disabled={createAction?.phase === 'submitting'}
                  onChange={(event) => {
                    setLibraryId(event.currentTarget.value)
                    setValidationError(undefined)
                    invalidateFailedCreate()
                  }}
                >
                  {libraries.map((library) => (
                    <option value={library.id} key={library.id}>{library.attributes.name}</option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label htmlFor="backup-set-name">Backup set name</label>
                <input
                  id="backup-set-name"
                  value={name}
                  maxLength={MAX_NAME_LENGTH}
                  disabled={createAction?.phase === 'submitting'}
                  aria-invalid={Boolean(validationError || createError)}
                  onChange={(event) => {
                    setName(event.currentTarget.value)
                    setValidationError(undefined)
                    invalidateFailedCreate()
                  }}
                />
              </div>
              {librariesHaveMore && (
                <button type="button" className="button--secondary" disabled={librariesLoadingMore} onClick={() => void loadLibraries(librariesCursor, true)}>
                  {librariesLoadingMore ? 'Loading more libraries…' : 'Load more libraries'}
                </button>
              )}
              {(validationError || createError) && <p className="backup-inline-error" role="alert">{validationError ?? createError}</p>}
              <div className="backup-form-actions">
                <button type="submit" disabled={createAction !== null || Boolean(pendingCreate) || !libraryId || name.trim().length === 0}>
                  {createAction?.phase === 'submitting' ? 'Creating…' : 'Create backup set'}
                </button>
                {createAction?.phase === 'failed' && (
                  <button type="button" className="button--secondary" onClick={() => void submitCreate(createAction.request, createAction.idempotencyKey)}>
                    Try the request again
                  </button>
                )}
              </div>
            </form>
          )}
        </section>
      )}

      {sets.status === 'loading' && (
        <section className="panel backup-overview-loading" aria-labelledby="backup-overview-loading-title">
          <h2 id="backup-overview-loading-title">Loading backup sets</h2>
          <p role="status" aria-busy="true">Finding your protected collections…</p>
        </section>
      )}

      {sets.status === 'error' && (
        <section className="panel" aria-labelledby="backup-overview-error-title">
          <h2 id="backup-overview-error-title">Backup sets unavailable</h2>
          <div className="backup-inline-error" role="alert">
            <p>{loadErrorMessage(sets.error)}</p>
            <button type="button" className="button--secondary" onClick={() => setRefreshVersion((value) => value + 1)}>Try again</button>
          </div>
        </section>
      )}

      {sets.status === 'success' && sets.items.length === 0 && (
        <section className="panel backup-empty-state" aria-labelledby="no-backups-title">
          <span className="backup-empty-icon" aria-hidden="true">⌁</span>
          <p className="eyebrow">Backup overview</p>
          <h2 id="no-backups-title">No backups yet.</h2>
          <p>Create a backup set to start protecting your files.</p>
        </section>
      )}

      {sets.status === 'success' && sets.items.length > 0 && (
        <section aria-labelledby="your-backup-sets-title">
          <div className="backup-list-heading">
            <div>
              <p className="eyebrow">Overview</p>
              <h2 id="your-backup-sets-title">Your backup sets</h2>
            </div>
          </div>
          {sets.error !== undefined && <p className="backup-inline-error" role="alert">More backup sets could not be loaded. Refresh to try again.</p>}
          <div className="backup-card-grid">
            {sets.items.map((set) => {
              const summary = summaries[set.backup_set_id]
              return (
                <article className="panel backup-set-card" key={set.backup_set_id}>
                  <div className="backup-set-card-header">
                    <div>
                      <h3>{set.name}</h3>
                      <p>Created <Timestamp value={set.created_at} /></p>
                    </div>
                    <StatusPill tone={set.state === 'DISABLED' ? 'planned' : 'info'}>
                      {set.state === 'DISABLED' ? 'Disabled' : set.state === 'ACTIVE' ? 'Active' : 'Ready'}
                    </StatusPill>
                  </div>
                  <dl className="backup-set-card-details">
                    <div><dt>Retention</dt><dd>{formatRetention(summary?.policy)}</dd></div>
                    <div>
                      <dt>Latest snapshot</dt>
                      <dd>{summary?.latestSnapshot
                        ? <>{snapshotLabel(summary.latestSnapshot)} · <Timestamp value={summary.latestSnapshot.committed_at ?? summary.latestSnapshot.created_at} /></>
                        : summary?.status === 'success' ? 'No snapshots yet' : 'Loading…'}</dd>
                    </div>
                    <div>
                      <dt>Recent activity</dt>
                      <dd>{summary?.latestOperation ? operationLabel(summary.latestOperation) : summary?.status === 'success' ? 'No activity yet' : 'Loading…'}</dd>
                    </div>
                  </dl>
                  {summary?.unavailable && <p className="backup-card-warning">Some summary details are temporarily unavailable.</p>}
                  <Link className="button-link" to={`/backups/${encodeURIComponent(set.backup_set_id)}`}>
                    Open backup set
                  </Link>
                </article>
              )
            })}
          </div>
          {sets.hasMore && (
            <button type="button" className="button--secondary backup-load-more" disabled={loadingMore} onClick={() => void loadMoreSets()}>
              {loadingMore ? 'Loading more backup sets…' : 'Load more backup sets'}
            </button>
          )}
        </section>
      )}
    </div>
  )
}
