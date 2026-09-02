import { useEffect, useRef, useState } from 'react'

import { mutationRecoveryStore } from '../../api/mutationRecovery'
import { useAuth } from '../../auth/useAuth'

/**
 * Account-bound retries are announced without rendering their action kind,
 * resource IDs, request body, prior principal ID, or any other owner data.
 */
export function PendingMutationAccountWarning() {
  const { generation } = useAuth()
  const [revision, setRevision] = useState(0)
  const headingRef = useRef<HTMLHeadingElement>(null)
  const foreign = mutationRecoveryStore.foreignForCurrentPrincipal()
  const legacy = mutationRecoveryStore.hasAmbiguousLegacyRecords

  useEffect(() => {
    if (foreign.length > 0 || legacy) {
      headingRef.current?.focus()
    }
  }, [foreign.length, generation, legacy, revision])

  if (foreign.length === 0 && !legacy) {
    return null
  }

  function discardBlockedRetries() {
    for (const record of foreign) {
      mutationRecoveryStore.remove(record.action_id)
    }
    if (legacy) {
      mutationRecoveryStore.discardAmbiguousLegacyRecords()
    }
    setRevision((value) => value + 1)
  }

  return (
    <section
      className="panel backup-recovery-notice"
      aria-labelledby="cross-account-retry-title"
    >
      <p className="eyebrow">Request recovery blocked</p>
      <h1 id="cross-account-retry-title" ref={headingRef} tabIndex={-1}>
        A pending request cannot be used with this account
      </h1>
      <p>
        This pending request belongs to another signed-in account or could not
        be safely matched to the current account.
      </p>
      <p>No request was sent. Sign in with the original account to review it, or discard the local retry.</p>
      <button type="button" className="button--secondary" onClick={discardBlockedRetries}>
        Discard blocked local retry
      </button>
    </section>
  )
}
