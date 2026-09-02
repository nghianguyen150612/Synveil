import { useEffect, useRef, useState, type FormEvent } from 'react'

import {
  createUuidV7,
  type BackupApi,
  type BackupRetentionPolicy,
  type ConfigureBackupRetentionPolicyRequest,
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

type RetentionData = BackupRetentionPolicy['data']
type ExpiryUnit = 'seconds' | 'hours' | 'days' | 'weeks'

interface PolicyAction {
  readonly idempotencyKey: string
  readonly request: ConfigureBackupRetentionPolicyRequest
  readonly phase: 'submitting' | 'failed'
}

export interface RetentionPolicyEditorProps {
  readonly backupSetId: string
  readonly policy: RetentionData | null
  readonly backupApi: BackupApi
  readonly onUpdated: (policy: RetentionData) => void
}

const UNIT_SECONDS: Readonly<Record<ExpiryUnit, bigint>> = {
  seconds: 1n,
  hours: 3_600n,
  days: 86_400n,
  weeks: 604_800n,
}

function formatInteger(value: string): string {
  try {
    return new Intl.NumberFormat().format(BigInt(value))
  } catch {
    return value
  }
}

function durationParts(seconds: string): { readonly value: string; readonly unit: ExpiryUnit } {
  if (!/^[1-9][0-9]*$/.test(seconds)) {
    return { value: seconds, unit: 'seconds' }
  }
  const total = BigInt(seconds)
  for (const unit of ['weeks', 'days', 'hours'] as const) {
    if (total % UNIT_SECONDS[unit] === 0n) {
      return { value: String(total / UNIT_SECONDS[unit]), unit }
    }
  }
  return { value: seconds, unit: 'seconds' }
}

function formatDuration(seconds: string): string {
  const parts = durationParts(seconds)
  const count = parts.value
  const singular = count === '1'
  return `${formatInteger(count)} ${parts.unit.slice(0, singular ? -1 : undefined)}`
}

function mutationErrorMessage(error: unknown): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again before saving retention settings.'
    }
    if (error.status === 403) {
      return 'This request could not be verified. Refresh the page and try again.'
    }
    if (error.status === 404) {
      return 'This backup set is no longer available.'
    }
    if (error.status === 409) {
      return 'Retention settings changed before this request completed. Refresh before starting a different update.'
    }
    if (error.status === 400 || error.status === 413) {
      return 'Check the retention values and try again.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'Retention settings are temporarily unavailable. Please try again.'
    }
  }
  return 'We could not confirm the result. Retry this same request safely.'
}

function pendingPolicyRequest(
  record: PendingMutationRecord,
): ConfigureBackupRetentionPolicyRequest | undefined {
  const keepLatest = record.request.keep_latest_completed
  const expireAfter = record.request.expire_after_seconds
  if (
    typeof keepLatest !== 'number' ||
    !Number.isSafeInteger(keepLatest) ||
    typeof expireAfter !== 'number' ||
    !Number.isSafeInteger(expireAfter)
  ) {
    return undefined
  }
  return { keep_latest_completed: keepLatest, expire_after_seconds: expireAfter }
}

export function RetentionPolicyEditor({
  backupSetId,
  policy,
  backupApi,
  onUpdated,
}: RetentionPolicyEditorProps) {
  const { refresh: refreshAuth } = useAuth()
  const [editing, setEditing] = useState(false)
  const [keepLatest, setKeepLatest] = useState(policy?.keep_latest_completed ?? '3')
  const initialDuration = durationParts(policy?.expire_after_seconds ?? String(30 * 86_400))
  const [expiryValue, setExpiryValue] = useState(initialDuration.value)
  const [expiryUnit, setExpiryUnit] = useState<ExpiryUnit>(initialDuration.unit)
  const [validationError, setValidationError] = useState<string>()
  const [action, setAction] = useState<PolicyAction | null>(null)
  const [requestError, setRequestError] = useState<string>()
  const [notice, setNotice] = useState<string>()
  const [pendingPolicy, setPendingPolicy] = useState<PendingMutationRecord | undefined>(() =>
    findPendingMutation(
      'configure_retention_policy',
      (scope) => scope.backupSetId === backupSetId,
    ),
  )
  const [checkingRecovery, setCheckingRecovery] = useState(false)
  const mounted = useRef(true)
  const actionRef = useRef<PolicyAction | null>(null)

  useEffect(() => () => {
    mounted.current = false
  }, [])

  useEffect(() => {
    if (editing) {
      return
    }
    const parts = durationParts(policy?.expire_after_seconds ?? String(30 * 86_400))
    setKeepLatest(policy?.keep_latest_completed ?? '3')
    setExpiryValue(parts.value)
    setExpiryUnit(parts.unit)
  }, [editing, policy])

  useEffect(() => {
    setPendingPolicy(findPendingMutation(
      'configure_retention_policy',
      (scope) => scope.backupSetId === backupSetId,
    ))
  }, [backupSetId])

  function invalidateFailedAction() {
    if (pendingPolicy) {
      mutationRecoveryStore.remove(pendingPolicy.action_id)
      setPendingPolicy(undefined)
      setNotice('Changed values will be saved as a new request with a new request key.')
    }
    if (actionRef.current?.phase === 'failed') {
      actionRef.current = null
      setAction(null)
      setRequestError(undefined)
    }
  }

  async function checkPendingPolicy() {
    const record = pendingPolicy
    const request = record ? pendingPolicyRequest(record) : undefined
    if (!record || !request || checkingRecovery) {
      return
    }
    setCheckingRecovery(true)
    setRequestError(undefined)
    try {
      const response = await backupApi.getBackupRetentionPolicy(backupSetId)
      if (!mounted.current) {
        return
      }
      onUpdated(response.data)
      if (
        response.data.keep_latest_completed === String(request.keep_latest_completed) &&
        response.data.expire_after_seconds === String(request.expire_after_seconds)
      ) {
        mutationRecoveryStore.remove(record.action_id)
        setPendingPolicy(undefined)
        setNotice('The current retention settings already include the previous request.')
      } else {
        setNotice('The current retention settings do not match the pending request. You can retry it safely or discard the local retry.')
      }
    } catch (error) {
      if (mounted.current) {
        setRequestError('Current retention settings could not be checked. The safe retry is still available.')
      }
      if (isApiRequestError(error) && error.status === 401) {
        void refreshAuth().catch(() => undefined)
      }
    } finally {
      if (mounted.current) {
        setCheckingRecovery(false)
      }
    }
  }

  function buildRequest(): ConfigureBackupRetentionPolicyRequest | undefined {
    if (!/^[1-9][0-9]*$/.test(keepLatest) || !/^[1-9][0-9]*$/.test(expiryValue)) {
      setValidationError('Enter whole numbers greater than zero.')
      return undefined
    }
    const keep = BigInt(keepLatest)
    const seconds = BigInt(expiryValue) * UNIT_SECONDS[expiryUnit]
    if (keep > BigInt(Number.MAX_SAFE_INTEGER) || seconds > BigInt(Number.MAX_SAFE_INTEGER)) {
      setValidationError('These values are too large for this browser form.')
      return undefined
    }
    return {
      keep_latest_completed: Number(keep),
      expire_after_seconds: Number(seconds),
    }
  }

  async function submitPolicy(requestOverride?: ConfigureBackupRetentionPolicyRequest, retryKey?: string) {
    if (actionRef.current?.phase === 'submitting') {
      return
    }
    const request = requestOverride ?? buildRequest()
    if (!request) {
      return
    }
    let idempotencyKey = retryKey
    if (!idempotencyKey) {
      try {
        idempotencyKey = createUuidV7()
      } catch {
        setRequestError('Secure browser randomness is unavailable. Retention settings were not updated.')
        return
      }
    }
    const nextAction: PolicyAction = { idempotencyKey, request, phase: 'submitting' }
    const recoveryRecord = mutationRecoveryStore.add({
      action_kind: 'configure_retention_policy',
      idempotency_key: idempotencyKey,
      request_scope: { backupSetId },
      request: { ...request },
    })
    if (mutationRecoveryStore.isAvailable) {
      setPendingPolicy(recoveryRecord)
    }
    actionRef.current = nextAction
    setAction(nextAction)
    setValidationError(undefined)
    setRequestError(undefined)
    setNotice(undefined)
    let postConfirmed = false
    try {
      const response = await backupApi.configureBackupRetentionPolicy(
        backupSetId,
        request,
        idempotencyKey,
      )
      postConfirmed = true
      mutationRecoveryStore.remove(recoveryRecord.action_id)
      setPendingPolicy((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      if (!mounted.current) {
        return
      }
      if (response.data.backup_set_id !== backupSetId) {
        throw new Error('retention policy scope mismatch')
      }
      actionRef.current = null
      setAction(null)
      setEditing(false)
      setNotice('Retention settings updated.')
      onUpdated(response.data)
    } catch (error) {
      const recoveryOutcome = postConfirmed
        ? 'removed'
        : settlePendingMutationFailure(recoveryRecord, error)
      if (recoveryOutcome === 'removed') {
        setPendingPolicy((current) => current?.action_id === recoveryRecord.action_id ? undefined : current)
      }
      if (!mounted.current) {
        return
      }
      if (recoveryOutcome === 'retained') {
        const failed: PolicyAction = { ...nextAction, phase: 'failed' }
        actionRef.current = failed
        setAction(failed)
      } else {
        actionRef.current = null
        setAction(null)
      }
      setRequestError(mutationErrorMessage(error))
      if (isApiRequestError(error) && error.status === 401) {
        void refreshAuth().catch(() => undefined)
      }
    }
  }

  function submitForm(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    void submitPolicy()
  }

  function cancelEditing() {
    actionRef.current = null
    setAction(null)
    setRequestError(undefined)
    setValidationError(undefined)
    setEditing(false)
  }

  return (
    <section className="backup-retention" aria-labelledby="retention-title">
      <div className="backup-subsection-heading">
        <div><p className="eyebrow">Policy</p><h3 id="retention-title">Retention</h3></div>
        {policy && <StatusPill tone="info">Configured</StatusPill>}
      </div>

      {pendingPolicy && pendingPolicyRequest(pendingPolicy) && (
        <MutationRecoveryNotice
          record={pendingPolicy}
          title="Finish saving retention settings"
          busy={checkingRecovery || action?.phase === 'submitting'}
          onCheckStatus={() => void checkPendingPolicy()}
          onRetry={() => {
            const request = pendingPolicyRequest(pendingPolicy)
            if (request) {
              void submitPolicy(request, pendingPolicy.idempotency_key)
            }
          }}
          onDiscard={() => {
            mutationRecoveryStore.remove(pendingPolicy.action_id)
            if (actionRef.current?.idempotencyKey === pendingPolicy.idempotency_key) {
              actionRef.current = null
              setAction(null)
              setRequestError(undefined)
            }
            setPendingPolicy(undefined)
          }}
        />
      )}

      {!editing && policy && (
        <>
          <p>Keep at least <strong>{formatInteger(policy.keep_latest_completed)}</strong> latest completed backup{policy.keep_latest_completed === '1' ? '' : 's'}.</p>
          <p>Backups older than <strong>{formatDuration(policy.expire_after_seconds)}</strong> can expire.</p>
        </>
      )}
      {!editing && !policy && <p className="backup-empty-copy">No retention settings configured.</p>}
      {!editing && (
        <button type="button" className="button--secondary" onClick={() => {
          setNotice(undefined)
          setEditing(true)
        }}>{policy ? 'Change retention settings' : 'Configure retention settings'}</button>
      )}

      {editing && (
        <form className="retention-form" onSubmit={submitForm} noValidate>
          <div className="field">
            <label htmlFor="retention-keep-latest">Completed backups to always keep</label>
            <input
              id="retention-keep-latest"
              type="number"
              min="1"
              step="1"
              inputMode="numeric"
              value={keepLatest}
              disabled={action?.phase === 'submitting'}
              onChange={(event) => {
                setKeepLatest(event.currentTarget.value)
                setValidationError(undefined)
                invalidateFailedAction()
              }}
            />
          </div>
          <div className="retention-expiry-fields">
            <div className="field">
              <label htmlFor="retention-expiry-value">Expire backups older than</label>
              <input
                id="retention-expiry-value"
                type="number"
                min="1"
                step="1"
                inputMode="numeric"
                value={expiryValue}
                disabled={action?.phase === 'submitting'}
                onChange={(event) => {
                  setExpiryValue(event.currentTarget.value)
                  setValidationError(undefined)
                  invalidateFailedAction()
                }}
              />
            </div>
            <div className="field">
              <label htmlFor="retention-expiry-unit">Time unit</label>
              <select
                id="retention-expiry-unit"
                value={expiryUnit}
                disabled={action?.phase === 'submitting'}
                onChange={(event) => {
                  setExpiryUnit(event.currentTarget.value as ExpiryUnit)
                  setValidationError(undefined)
                  invalidateFailedAction()
                }}
              >
                <option value="weeks">Weeks</option>
                <option value="days">Days</option>
                <option value="hours">Hours</option>
                <option value="seconds">Seconds</option>
              </select>
            </div>
          </div>
          {(validationError || requestError) && <p className="backup-inline-error" role="alert">{validationError ?? requestError}</p>}
          <div className="backup-form-actions">
            <button type="submit" disabled={action !== null || Boolean(pendingPolicy)}>{action?.phase === 'submitting' ? 'Saving…' : 'Save retention settings'}</button>
            {action?.phase === 'failed' && (
              <button type="button" className="button--secondary" onClick={() => void submitPolicy(action.request, action.idempotencyKey)}>Try the same request again</button>
            )}
            <button type="button" className="button--secondary" onClick={cancelEditing} disabled={action?.phase === 'submitting'}>Cancel</button>
          </div>
        </form>
      )}
      {notice && <p className="backup-mutation-message backup-mutation-message--success" role="status">{notice}</p>}
    </section>
  )
}
