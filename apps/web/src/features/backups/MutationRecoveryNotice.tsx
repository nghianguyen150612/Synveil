import type { PendingMutationRecord } from '../../api/mutationRecovery'
import { useAuth } from '../../auth/useAuth'

export interface MutationRecoveryNoticeProps {
  readonly record: PendingMutationRecord
  readonly title: string
  readonly retryLabel?: string
  readonly busy?: boolean
  readonly canRetry?: boolean
  readonly onCheckStatus?: () => void
  readonly onRetry: () => void
  readonly onDiscard: () => void
}

/** Contextual, user-driven recovery for one persisted backup mutation. */
export function MutationRecoveryNotice({
  record,
  title,
  retryLabel = 'Retry safely',
  busy = false,
  canRetry = true,
  onCheckStatus,
  onRetry,
  onDiscard,
}: MutationRecoveryNoticeProps) {
  const { session } = useAuth()
  if (record.owner_id !== session?.user_id) {
    return null
  }

  return (
    <section
      className="panel backup-recovery-notice"
      aria-labelledby={`backup-recovery-${record.action_id}`}
    >
      <p className="eyebrow">Request recovery</p>
      <h2 id={`backup-recovery-${record.action_id}`}>{title}</h2>
      <p>The previous request may have reached Synveil, but this browser did not receive the result.</p>
      <p>Retrying uses the exact same request and request key.</p>
      <div className="backup-form-actions">
        {onCheckStatus && (
          <button type="button" className="button--secondary" onClick={onCheckStatus} disabled={busy}>
            Check current status
          </button>
        )}
        {canRetry && (
          <button type="button" onClick={onRetry} disabled={busy}>
            {busy ? 'Checking…' : retryLabel}
          </button>
        )}
        <button type="button" className="button--secondary" onClick={onDiscard} disabled={busy}>
          Discard local retry
        </button>
      </div>
      <p className="backup-action-explanation">
        Discarding this retry does not prove that the server did not process the previous request.
      </p>
    </section>
  )
}
