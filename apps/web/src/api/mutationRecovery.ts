import { createUuidV7 } from './backups'
import { isApiRequestError } from './errors'
import { recordMutationOutcomeUncertain } from '../diagnostics/record'

/**
 * Minimal, non-secret metadata persisted locally so a browser tab can safely
 * retry a mutation whose network outcome was uncertain (response lost,
 * navigation aborted, transport dropped).
 *
 * Only what is needed to replay the exact semantic request is stored.
 * No auth tokens, no CSRF tokens, no server secrets, no full payloads like
 * trees or operation history.
 */
export const LEGACY_PENDING_MUTATIONS_KEY = 'synveil.backup.pendingMutations.v1'
export const PENDING_MUTATIONS_KEY = 'synveil.backup.pendingMutations.v2'
export const PENDING_SCHEMA_VERSION = 2

export type MutationActionKind =
  | 'create_backup_set'
  | 'configure_retention_policy'
  | 'create_maintenance_run'
  | 'advance_maintenance_run'
  | 'create_restore_plan'
  | 'execute_restore_plan'
  | 'create_prune_plan'
  | 'execute_prune_plan'

export interface PendingResourceScope {
  readonly backupSetId?: string
  readonly snapshotId?: string
  readonly restorePlanId?: string
  readonly prunePlanId?: string
  readonly maintenanceRunId?: string
}

export interface PendingMutationRecord {
  readonly schema_version: typeof PENDING_SCHEMA_VERSION
  readonly action_id: string
  readonly action_kind: MutationActionKind
  readonly owner_id: string
  readonly idempotency_key: string
  readonly method: 'POST'
  readonly request_scope: PendingResourceScope
  readonly request: Record<string, unknown>
  readonly created_at: string
  readonly status: 'uncertain'
}

interface StoredEnvelope {
  readonly schema_version: number
  readonly records: readonly PendingMutationRecord[]
}

type StorageReadResult =
  | { readonly status: 'available'; readonly value: unknown }
  | { readonly status: 'unavailable' }

let memoryRecords: PendingMutationRecord[] = []
let memoryFallbackActive = false
let activePrincipalId: string | null = null

const UUID_V7_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
const ACTION_KINDS: ReadonlySet<MutationActionKind> = new Set([
  'create_backup_set',
  'configure_retention_policy',
  'create_maintenance_run',
  'advance_maintenance_run',
  'create_restore_plan',
  'execute_restore_plan',
  'create_prune_plan',
  'execute_prune_plan',
])

function isValidActionKind(value: unknown): value is MutationActionKind {
  return typeof value === 'string' && ACTION_KINDS.has(value as MutationActionKind)
}

function isValidScope(value: unknown): value is PendingResourceScope {
  if (!value || typeof value !== 'object') {
    return value === undefined
  }
  const record = value as Record<string, unknown>
  for (const key of Object.keys(record)) {
    if (typeof record[key] !== 'string') {
      return false
    }
  }
  return true
}

function isStoredRecord(value: unknown): value is PendingMutationRecord {
  if (!value || typeof value !== 'object') {
    return false
  }
  const record = value as Record<string, unknown>
  return (
    typeof record.schema_version === 'number' &&
    record.schema_version === PENDING_SCHEMA_VERSION &&
    typeof record.action_id === 'string' &&
    isValidActionKind(record.action_kind) &&
    typeof record.owner_id === 'string' &&
    record.owner_id.length > 0 &&
    typeof record.idempotency_key === 'string' &&
    UUID_V7_REGEX.test(record.idempotency_key) &&
    record.method === 'POST' &&
    isValidScope(record.request_scope) &&
    typeof record.request === 'object' &&
    record.request !== null &&
    typeof record.created_at === 'string' &&
    record.status === 'uncertain'
  )
}

function isStoredEnvelope(value: unknown): value is StoredEnvelope {
  if (!value || typeof value !== 'object') {
    return false
  }
  const envelope = value as Record<string, unknown>
  if (typeof envelope.schema_version !== 'number' || envelope.schema_version !== PENDING_SCHEMA_VERSION) {
    return false
  }
  if (!Array.isArray(envelope.records)) {
    return false
  }
  return envelope.records.every(isStoredRecord)
}

function readRaw(key = PENDING_MUTATIONS_KEY): StorageReadResult {
  try {
    const item = sessionStorage.getItem(key)
    if (item === null) {
      return { status: 'available', value: undefined }
    }
    return { status: 'available', value: JSON.parse(item) }
  } catch {
    return { status: 'unavailable' }
  }
}

function writeRaw(value: StoredEnvelope, key = PENDING_MUTATIONS_KEY): boolean {
  try {
    sessionStorage.setItem(key, JSON.stringify(value))
    memoryFallbackActive = false
    return true
  } catch {
    memoryFallbackActive = true
    return false
  }
}

function clearRaw(key = PENDING_MUTATIONS_KEY): void {
  try {
    sessionStorage.removeItem(key)
    memoryFallbackActive = false
  } catch {
    memoryFallbackActive = true
    // storage unavailable — nothing to clear
  }
}

function nowIso(): string {
  return new Date().toISOString()
}

function availableRecords(): readonly PendingMutationRecord[] {
  const stored = readRaw()
  if (stored.status === 'unavailable') {
    return memoryRecords
  }
  if (memoryFallbackActive) {
    return memoryRecords
  }
  if (!isStoredEnvelope(stored.value)) {
    if (stored.value === undefined) {
      memoryRecords = []
    }
    return []
  }
  memoryRecords = [...stored.value.records]
  return stored.value.records
}

export interface MutationRecoveryStore {
  readonly isAvailable: boolean
  add(record: Omit<PendingMutationRecord, 'schema_version' | 'action_id' | 'created_at' | 'status' | 'method' | 'owner_id'> & { readonly method?: 'POST' }): PendingMutationRecord
  remove(actionId: string): void
  clear(): void
  all(): readonly PendingMutationRecord[]
  forCurrentPrincipal(): readonly PendingMutationRecord[]
  foreignForCurrentPrincipal(): readonly PendingMutationRecord[]
  readonly hasAmbiguousLegacyRecords: boolean
  discardAmbiguousLegacyRecords(): void
}

/** Bind same-tab retry discovery to the currently authenticated public user ID. */
export function bindMutationRecoveryPrincipal(principalId: string | null): void {
  activePrincipalId = principalId
}

export const mutationRecoveryStore: MutationRecoveryStore = {
  get isAvailable(): boolean {
    try {
      const testKey = `${PENDING_MUTATIONS_KEY}.probe`
      sessionStorage.setItem(testKey, '1')
      sessionStorage.removeItem(testKey)
      if (memoryFallbackActive) {
        if (memoryRecords.length === 0) {
          sessionStorage.removeItem(PENDING_MUTATIONS_KEY)
        } else {
          sessionStorage.setItem(PENDING_MUTATIONS_KEY, JSON.stringify({
            schema_version: PENDING_SCHEMA_VERSION,
            records: memoryRecords,
          }))
        }
        memoryFallbackActive = false
      }
      return true
    } catch {
      return false
    }
  },

  add(partial): PendingMutationRecord {
    if (!activePrincipalId) {
      throw new Error('An authenticated principal is required for mutation recovery.')
    }
    const existingRecords = availableRecords()
    const existingRecord = existingRecords.find((record) =>
      record.action_kind === partial.action_kind &&
      record.owner_id === activePrincipalId &&
      record.idempotency_key === partial.idempotency_key
    )
    if (existingRecord) {
      writeRaw({ schema_version: PENDING_SCHEMA_VERSION, records: existingRecords })
      return existingRecord
    }
    const method = partial.method ?? 'POST'
    const record: PendingMutationRecord = {
      schema_version: PENDING_SCHEMA_VERSION,
      action_id: createUuidV7(),
      owner_id: activePrincipalId,
      idempotency_key: partial.idempotency_key,
      method,
      request_scope: partial.request_scope,
      request: partial.request,
      created_at: nowIso(),
      status: 'uncertain',
      action_kind: partial.action_kind,
    }
    const records = [...existingRecords]
    records.push(record)
    memoryRecords = records
    writeRaw({ schema_version: PENDING_SCHEMA_VERSION, records })
    return record
  },

  remove(actionId): void {
    const records = availableRecords().filter((record) => record.action_id !== actionId)
    memoryRecords = [...records]
    if (records.length === 0) {
      clearRaw()
    } else {
      writeRaw({ schema_version: PENDING_SCHEMA_VERSION, records })
    }
  },

  clear(): void {
    memoryRecords = []
    clearRaw()
    clearRaw(LEGACY_PENDING_MUTATIONS_KEY)
  },

  all(): readonly PendingMutationRecord[] {
    return availableRecords()
  },

  forCurrentPrincipal(): readonly PendingMutationRecord[] {
    if (!activePrincipalId) {
      return []
    }
    return availableRecords().filter((record) => record.owner_id === activePrincipalId)
  },

  foreignForCurrentPrincipal(): readonly PendingMutationRecord[] {
    if (!activePrincipalId) {
      return []
    }
    return availableRecords().filter((record) => record.owner_id !== activePrincipalId)
  },

  get hasAmbiguousLegacyRecords(): boolean {
    const legacy = readRaw(LEGACY_PENDING_MUTATIONS_KEY)
    return legacy.status === 'available' && legacy.value !== undefined
  },

  discardAmbiguousLegacyRecords(): void {
    clearRaw(LEGACY_PENDING_MUTATIONS_KEY)
  },
}

/**
 * HTTP responses below 500 are definitive except temporary authentication or
 * CSRF failures. Transport failures, aborts, 401/403, and server failures can
 * all leave delivery uncertain, so their exact retry identity is retained.
 */
export function isDefinitiveMutationFailure(error: unknown): boolean {
  return isApiRequestError(error) &&
    error.status >= 400 &&
    error.status < 500 &&
    error.status !== 401 &&
    error.status !== 403
}

/** Remove a retry only when the server outcome is definitive. */
export function settlePendingMutationFailure(
  record: PendingMutationRecord,
  error: unknown,
): 'removed' | 'retained' {
  if (isDefinitiveMutationFailure(error)) {
    mutationRecoveryStore.remove(record.action_id)
    return 'removed'
  }
  recordMutationOutcomeUncertain(record.action_kind)
  return 'retained'
}

export function findPendingMutation(
  actionKind: MutationActionKind,
  matchesScope: (scope: PendingResourceScope) => boolean = () => true,
): PendingMutationRecord | undefined {
  if (!mutationRecoveryStore.isAvailable) {
    return undefined
  }
  return [...mutationRecoveryStore.forCurrentPrincipal()]
    .reverse()
    .find((record) => record.action_kind === actionKind && matchesScope(record.request_scope))
}

/**
 * Build a safe retry record for a create-style mutation where the canonical
 * resource identity is only known from the (possibly lost) server response.
 * Recovery relies on idempotency-key replay with the exact same request.
 */
export function makePendingCreateRecord(
  actionKind: 'create_backup_set' | 'create_maintenance_run' | 'create_restore_plan' | 'create_prune_plan',
  idempotencyKey: string,
  scope: PendingResourceScope,
  request: Record<string, unknown>,
): PendingMutationRecord {
  return mutationRecoveryStore.add({ action_kind: actionKind, idempotency_key: idempotencyKey, method: 'POST', request_scope: scope, request })
}

/**
 * Build a safe retry record for an execute plan mutation. The plan IDs used
 * here are the durable canonical server IDs already known to the UI.
 */
export function makePendingExecuteRecord(
  actionKind: 'execute_restore_plan' | 'execute_prune_plan',
  idempotencyKey: string,
  scope: PendingResourceScope,
  request: Record<string, unknown>,
): PendingMutationRecord {
  return mutationRecoveryStore.add({ action_kind: actionKind, idempotency_key: idempotencyKey, method: 'POST', request_scope: scope, request })
}
