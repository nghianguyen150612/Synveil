import { safeServerRequestId } from '../api/errors'
import { createUuidV7 } from '../api/uuid'
import {
  DIAGNOSTIC_SCHEMA_VERSION,
  type DiagnosticCategory,
  type DiagnosticCode,
  type DiagnosticEvent,
  type DiagnosticEventInput,
  type DiagnosticHttpMethod,
  type DiagnosticOperation,
  type DiagnosticOutcome,
  type DiagnosticRouteCategory,
  type DiagnosticSeverity,
} from './types'

export const DIAGNOSTICS_STORAGE_KEY = 'synveil.diagnostics.events.v1'
export const MAX_DIAGNOSTIC_EVENTS = 100
export const MAX_DIAGNOSTICS_STORAGE_CHARACTERS = 48 * 1024
export const DIAGNOSTIC_DEDUPLICATION_WINDOW_MS = 5_000

const UUID_V7_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
const SAFE_API_ERROR_CODE_PATTERN = /^[a-z][a-z0-9_.-]{0,63}$/
const DIAGNOSTIC_CATEGORIES: ReadonlySet<DiagnosticCategory> = new Set([
  'NETWORK',
  'API',
  'AUTH',
  'MUTATION_RECOVERY',
  'CLIENT_RENDER',
  'CLIENT_RUNTIME',
])
const DIAGNOSTIC_CODES: ReadonlySet<DiagnosticCode> = new Set([
  'NETWORK_UNAVAILABLE',
  'REQUEST_ABORTED',
  'API_AUTH_REQUIRED',
  'API_FORBIDDEN',
  'API_NOT_FOUND',
  'API_CONFLICT',
  'API_VALIDATION',
  'API_DEPENDENCY_UNAVAILABLE',
  'API_INTERNAL_ERROR',
  'AUTH_RECOVERY_FAILED',
  'AUTH_RECOVERED',
  'MUTATION_OUTCOME_UNCERTAIN',
  'MUTATION_RECONCILED',
  'CLIENT_RENDER_FAILURE',
  'CLIENT_RUNTIME_FAILURE',
])
const DIAGNOSTIC_OPERATIONS: ReadonlySet<DiagnosticOperation> = new Set([
  'BACKUP_SET_LIST',
  'BACKUP_SET_GET',
  'BACKUP_SET_CREATE',
  'SNAPSHOT_LIST',
  'SNAPSHOT_GET',
  'SNAPSHOT_TREE',
  'RETENTION_GET',
  'RETENTION_UPDATE',
  'MAINTENANCE_LIST',
  'MAINTENANCE_GET',
  'MAINTENANCE_CREATE',
  'MAINTENANCE_ADVANCE',
  'OPERATION_LIST',
  'OPERATION_GET',
  'RESTORE_PLAN_CREATE',
  'RESTORE_PLAN_GET',
  'RESTORE_EXECUTE',
  'RESTORE_EXECUTION_GET',
  'PRUNE_PLAN_CREATE',
  'PRUNE_PLAN_GET',
  'PRUNE_EXECUTE',
  'PRUNE_EXECUTION_GET',
  'LIBRARY_LIST',
  'LIBRARY_CHILDREN',
  'NODE_GET',
  'AUTH_BOOTSTRAP',
  'AUTH_BOOTSTRAP_CREATE',
  'AUTH_LOGIN',
  'AUTH_SESSION_CHECK',
  'AUTH_CSRF',
  'AUTH_RECOVERY',
  'AUTH_LOGOUT',
  'UPLOAD_SESSION_CREATE',
  'UPLOAD_SESSION_GET',
  'UPLOAD_CHUNK_APPEND',
  'UPLOAD_COMPLETE',
  'UPLOAD_ABORT',
  'CLIENT_RENDER',
  'CLIENT_RUNTIME',
])
const DIAGNOSTIC_SEVERITIES: ReadonlySet<DiagnosticSeverity> = new Set([
  'INFO',
  'WARNING',
  'ERROR',
])
const DIAGNOSTIC_OUTCOMES: ReadonlySet<DiagnosticOutcome> = new Set([
  'FAILED',
  'RECOVERED',
])
const DIAGNOSTIC_METHODS: ReadonlySet<DiagnosticHttpMethod> = new Set([
  'GET',
  'POST',
  'PUT',
  'PATCH',
  'DELETE',
])
const DIAGNOSTIC_ROUTE_CATEGORIES: ReadonlySet<DiagnosticRouteCategory> = new Set([
  'HOME',
  'BACKUP_OVERVIEW',
  'BACKUP_DETAIL',
  'BACKUP_RESTORE',
  'BACKUP_PRUNE',
  'DEVELOPMENT_HEALTH',
  'AUTH',
  'NOT_FOUND',
  'UNKNOWN',
])
const ENVELOPE_KEYS = new Set(['schema_version', 'events'])
const EVENT_KEYS = new Set([
  'schema_version',
  'event_id',
  'occurred_at',
  'severity',
  'category',
  'operation',
  'outcome',
  'code',
  'http_method',
  'http_status',
  'api_error_code',
  'server_request_id',
  'retryable',
  'route_category',
  'occurrences',
])

export interface DiagnosticStoreOptions {
  readonly storage?: Storage | null
  readonly maxEvents?: number
  readonly maxStorageCharacters?: number
  readonly now?: () => string
  readonly createEventId?: () => string
}

function resolveSessionStorage(): Storage | undefined {
  try {
    return globalThis.sessionStorage
  } catch {
    return undefined
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function isValidTimestamp(value: unknown): value is string {
  if (typeof value !== 'string' || value.length > 64) {
    return false
  }
  const parsed = Date.parse(value)
  return Number.isFinite(parsed) && new Date(parsed).toISOString() === value
}

export function sanitizeApiErrorCode(value: unknown): string | undefined {
  return typeof value === 'string' && SAFE_API_ERROR_CODE_PATTERN.test(value)
    ? value
    : undefined
}

function isValidHttpStatus(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 100 && value <= 599
}

function isValidOccurrences(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 1 && value <= 1_000_000
}

function hasOnlyEventKeys(value: Record<string, unknown>): boolean {
  return Object.keys(value).every((key) => EVENT_KEYS.has(key))
}

function decodeEvent(value: unknown): DiagnosticEvent | undefined {
  if (!isRecord(value) || !hasOnlyEventKeys(value)) {
    return undefined
  }
  if (
    value.schema_version !== DIAGNOSTIC_SCHEMA_VERSION ||
    typeof value.event_id !== 'string' ||
    !UUID_V7_PATTERN.test(value.event_id) ||
    !isValidTimestamp(value.occurred_at) ||
    typeof value.severity !== 'string' ||
    !DIAGNOSTIC_SEVERITIES.has(value.severity as DiagnosticSeverity) ||
    typeof value.category !== 'string' ||
    !DIAGNOSTIC_CATEGORIES.has(value.category as DiagnosticCategory) ||
    typeof value.operation !== 'string' ||
    !DIAGNOSTIC_OPERATIONS.has(value.operation as DiagnosticOperation) ||
    typeof value.outcome !== 'string' ||
    !DIAGNOSTIC_OUTCOMES.has(value.outcome as DiagnosticOutcome) ||
    typeof value.code !== 'string' ||
    !DIAGNOSTIC_CODES.has(value.code as DiagnosticCode) ||
    typeof value.retryable !== 'boolean'
  ) {
    return undefined
  }

  if (value.http_method !== undefined &&
      (typeof value.http_method !== 'string' ||
        !DIAGNOSTIC_METHODS.has(value.http_method as DiagnosticHttpMethod))) {
    return undefined
  }
  if (value.http_status !== undefined && !isValidHttpStatus(value.http_status)) {
    return undefined
  }
  if (value.api_error_code !== undefined &&
      (typeof value.api_error_code !== 'string' ||
        sanitizeApiErrorCode(value.api_error_code) === undefined)) {
    return undefined
  }
  if (value.server_request_id !== undefined &&
      (typeof value.server_request_id !== 'string' ||
        safeServerRequestId(value.server_request_id) === undefined)) {
    return undefined
  }
  if (value.route_category !== undefined &&
      (typeof value.route_category !== 'string' ||
        !DIAGNOSTIC_ROUTE_CATEGORIES.has(value.route_category as DiagnosticRouteCategory))) {
    return undefined
  }
  if (value.occurrences !== undefined && !isValidOccurrences(value.occurrences)) {
    return undefined
  }

  return {
    schema_version: DIAGNOSTIC_SCHEMA_VERSION,
    event_id: value.event_id,
    occurred_at: value.occurred_at,
    severity: value.severity as DiagnosticSeverity,
    category: value.category as DiagnosticCategory,
    operation: value.operation as DiagnosticOperation,
    outcome: value.outcome as DiagnosticOutcome,
    code: value.code as DiagnosticCode,
    ...(value.http_method ? { http_method: value.http_method as DiagnosticHttpMethod } : {}),
    ...(value.http_status !== undefined ? { http_status: value.http_status } : {}),
    ...(value.api_error_code ? { api_error_code: value.api_error_code } : {}),
    ...(value.server_request_id ? { server_request_id: value.server_request_id } : {}),
    retryable: value.retryable,
    ...(value.route_category ? { route_category: value.route_category as DiagnosticRouteCategory } : {}),
    ...(value.occurrences !== undefined ? { occurrences: value.occurrences } : {}),
  }
}

interface DecodedEnvelope {
  readonly events: readonly DiagnosticEvent[]
  readonly hadInvalidEvents: boolean
}

function decodeEnvelope(value: unknown): DecodedEnvelope | undefined {
  if (
    !isRecord(value) ||
    !Object.keys(value).every((key) => ENVELOPE_KEYS.has(key)) ||
    value.schema_version !== DIAGNOSTIC_SCHEMA_VERSION ||
    !Array.isArray(value.events)
  ) {
    return undefined
  }
  const decodedEvents = value.events.map(decodeEvent)
  return {
    events: decodedEvents.filter((event): event is DiagnosticEvent => event !== undefined),
    hadInvalidEvents: decodedEvents.some((event) => event === undefined),
  }
}

function deduplicationKey(event: DiagnosticEvent): string {
  return [event.category, event.operation, event.code, event.http_status ?? ''].join('\u0000')
}

function eventTime(event: DiagnosticEvent): number {
  return Date.parse(event.occurred_at)
}

function compareNewestFirst(left: DiagnosticEvent, right: DiagnosticEvent): number {
  const timeDifference = eventTime(right) - eventTime(left)
  return timeDifference !== 0 ? timeDifference : right.event_id.localeCompare(left.event_id)
}

export class DiagnosticStore {
  readonly storageKey = DIAGNOSTICS_STORAGE_KEY
  readonly maxEvents: number
  readonly maxStorageCharacters: number

  private readonly storage: Storage | undefined
  private readonly now: () => string
  private readonly createEventId: () => string
  private readonly listeners = new Set<() => void>()
  private events: DiagnosticEvent[]
  private storageAvailable: boolean
  private activePrincipalId: string | null = null
  private principalBound = false

  constructor(options: DiagnosticStoreOptions = {}) {
    this.storage = options.storage === undefined
      ? resolveSessionStorage()
      : (options.storage ?? undefined)
    this.maxEvents = Math.min(MAX_DIAGNOSTIC_EVENTS, Math.max(1, options.maxEvents ?? MAX_DIAGNOSTIC_EVENTS))
    this.maxStorageCharacters = Math.max(1, options.maxStorageCharacters ?? MAX_DIAGNOSTICS_STORAGE_CHARACTERS)
    this.now = options.now ?? (() => new Date().toISOString())
    this.createEventId = options.createEventId ?? createUuidV7
    this.storageAvailable = this.storage !== undefined
    this.events = [...this.readStoredEvents()]
  }

  get isStorageAvailable(): boolean {
    return this.storageAvailable
  }

  all(): readonly DiagnosticEvent[] {
    return this.events
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  bindPrincipal(principalId: string | null): void {
    const changed = !this.principalBound || this.activePrincipalId !== principalId
    this.principalBound = true
    this.activePrincipalId = principalId
    if (changed) {
      // Principal identity is intentionally not persisted beside diagnostics.
      // Clear the prior browser buffer before a new authenticated view renders.
      this.events = []
      this.removeStored()
      this.notify()
    }
  }

  record(input: DiagnosticEventInput): DiagnosticEvent | undefined {
    if (!DIAGNOSTIC_SEVERITIES.has(input.severity) ||
        !DIAGNOSTIC_CATEGORIES.has(input.category) ||
        !DIAGNOSTIC_OPERATIONS.has(input.operation) ||
        !DIAGNOSTIC_OUTCOMES.has(input.outcome) ||
        !DIAGNOSTIC_CODES.has(input.code) ||
        typeof input.retryable !== 'boolean') {
      return undefined
    }

    const httpMethod = input.http_method && DIAGNOSTIC_METHODS.has(input.http_method)
      ? input.http_method
      : undefined
    const httpStatus = input.http_status !== undefined && isValidHttpStatus(input.http_status)
      ? input.http_status
      : undefined
    const apiErrorCode = sanitizeApiErrorCode(input.api_error_code)
    const serverRequestId = safeServerRequestId(input.server_request_id)
    const routeCategory = input.route_category && DIAGNOSTIC_ROUTE_CATEGORIES.has(input.route_category)
      ? input.route_category
      : undefined
    const event = decodeEvent({
      schema_version: DIAGNOSTIC_SCHEMA_VERSION,
      event_id: this.createEventId(),
      occurred_at: this.now(),
      severity: input.severity,
      category: input.category,
      operation: input.operation,
      outcome: input.outcome,
      code: input.code,
      ...(httpMethod ? { http_method: httpMethod } : {}),
      ...(httpStatus !== undefined ? { http_status: httpStatus } : {}),
      ...(apiErrorCode ? { api_error_code: apiErrorCode } : {}),
      ...(serverRequestId ? { server_request_id: serverRequestId } : {}),
      retryable: input.retryable,
      ...(routeCategory ? { route_category: routeCategory } : {}),
    })
    if (!event) {
      return undefined
    }

    const duplicateIndex = this.events.findIndex((candidate) =>
      deduplicationKey(candidate) === deduplicationKey(event) &&
      Math.abs(eventTime(event) - eventTime(candidate)) <= DIAGNOSTIC_DEDUPLICATION_WINDOW_MS,
    )
    let storedEvent = event
    if (duplicateIndex >= 0) {
      const existing = this.events[duplicateIndex]
      storedEvent = {
        ...existing,
        occurred_at: event.occurred_at,
        ...(existing.api_error_code || !event.api_error_code
          ? {}
          : { api_error_code: event.api_error_code }),
        ...(existing.server_request_id || !event.server_request_id
          ? {}
          : { server_request_id: event.server_request_id }),
        occurrences: Math.min(1_000_000, (existing.occurrences ?? 1) + 1),
      }
      this.events = [storedEvent, ...this.events.filter((_, index) => index !== duplicateIndex)]
    } else {
      this.events = [event, ...this.events]
    }
    this.events = this.events.slice(0, this.maxEvents)
    this.persist()
    this.notify()
    return storedEvent
  }

  clear(): void {
    this.events = []
    this.removeStored()
    this.notify()
  }

  /** Re-read the bounded storage envelope; useful for recovery after a tab storage change. */
  reloadFromStorage(): void {
    this.events = [...this.readStoredEvents()]
    this.notify()
  }

  private readStoredEvents(): readonly DiagnosticEvent[] {
    if (!this.storage) {
      return []
    }
    let raw: string | null
    try {
      raw = this.storage.getItem(this.storageKey)
    } catch {
      this.storageAvailable = false
      return []
    }
    if (raw === null) {
      return []
    }
    if (raw.length > this.maxStorageCharacters) {
      this.removeStored()
      return []
    }
    try {
      const decoded = decodeEnvelope(JSON.parse(raw) as unknown)
      if (decoded === undefined) {
        this.removeStored()
        return []
      }
      const bounded = [...decoded.events]
        .sort(compareNewestFirst)
        .slice(0, this.maxEvents)
      if (bounded.length !== decoded.events.length || decoded.hadInvalidEvents || decoded.events.length === 0) {
        this.writeStored(bounded)
      }
      return bounded
    } catch {
      this.removeStored()
      return []
    }
  }

  private persist(): void {
    if (!this.storage) {
      return
    }
    let bounded = [...this.events]
    while (bounded.length > 0) {
      const serialized = JSON.stringify({
        schema_version: DIAGNOSTIC_SCHEMA_VERSION,
        events: bounded,
      })
      if (serialized.length <= this.maxStorageCharacters) {
        this.events = bounded
        this.writeStored(bounded)
        return
      }
      bounded = bounded.slice(0, -1)
    }
    this.events = []
    this.removeStored()
  }

  private writeStored(events: readonly DiagnosticEvent[]): void {
    if (!this.storage) {
      return
    }
    try {
      this.storage.setItem(this.storageKey, JSON.stringify({
        schema_version: DIAGNOSTIC_SCHEMA_VERSION,
        events,
      }))
      this.storageAvailable = true
    } catch {
      // Browser storage is an optimization only; keep the bounded in-memory copy.
      this.storageAvailable = false
    }
  }

  private removeStored(): void {
    if (!this.storage) {
      return
    }
    try {
      this.storage.removeItem(this.storageKey)
      this.storageAvailable = true
    } catch {
      this.storageAvailable = false
    }
  }

  private notify(): void {
    for (const listener of this.listeners) {
      try {
        listener()
      } catch {
        // A diagnostic subscriber must never change product behavior.
      }
    }
  }
}

export const diagnosticStore = new DiagnosticStore()

/** Best-effort boundary used by production code so diagnostics cannot fail an API or UI action. */
export function safeRecordDiagnostic(input: DiagnosticEventInput): DiagnosticEvent | undefined {
  try {
    return diagnosticStore.record(input)
  } catch {
    return undefined
  }
}

export function bindDiagnosticsPrincipal(principalId: string | null): void {
  try {
    diagnosticStore.bindPrincipal(principalId)
  } catch {
    // Principal transitions remain authoritative even if browser storage is broken.
  }
}
