import {
  isApiRequestError,
  normalizeApiError,
  safeServerRequestId,
} from '../api/errors'
import type { MutationActionKind } from '../api/mutationRecovery'
import { safeRecordDiagnostic } from './store'
import type {
  DiagnosticCode,
  DiagnosticHttpMethod,
  DiagnosticOperation,
  DiagnosticRouteCategory,
} from './types'

interface ApiFailureInput {
  readonly operation?: DiagnosticOperation
  readonly method: string
  readonly error: unknown
}

function isAbortError(error: unknown): boolean {
  if (typeof DOMException !== 'undefined' && error instanceof DOMException) {
    return error.name === 'AbortError'
  }
  return typeof error === 'object' && error !== null &&
    'name' in error && (error as { readonly name?: unknown }).name === 'AbortError'
}

function diagnosticMethod(method: string): DiagnosticHttpMethod | undefined {
  switch (method.toUpperCase()) {
    case 'GET':
    case 'POST':
    case 'PUT':
    case 'PATCH':
    case 'DELETE':
      return method.toUpperCase() as DiagnosticHttpMethod
    default:
      return undefined
  }
}

function apiFailureCode(status: number | undefined, kind: ReturnType<typeof normalizeApiError>['kind']): DiagnosticCode {
  if (status === 401 || kind === 'AuthenticationRequired') {
    return 'API_AUTH_REQUIRED'
  }
  if (status === 403 || kind === 'ForbiddenError') {
    return 'API_FORBIDDEN'
  }
  if (status === 404 || kind === 'NotFoundError') {
    return 'API_NOT_FOUND'
  }
  if (status === 409 || kind === 'ConflictError') {
    return 'API_CONFLICT'
  }
  if (status === 429 || status === 502 || status === 503 || status === 504 || kind === 'DependencyUnavailable') {
    return 'API_DEPENDENCY_UNAVAILABLE'
  }
  if (status === 400 || status === 413 || status === 422 || kind === 'ValidationError') {
    return 'API_VALIDATION'
  }
  return 'API_INTERNAL_ERROR'
}

/** Record one typed API boundary failure without retaining a URL or payload. */
export function recordApiFailure({ operation, method, error }: ApiFailureInput): void {
  try {
    if (!operation) {
      return
    }
    const httpMethod = diagnosticMethod(method)
    if (!isApiRequestError(error)) {
      const aborted = isAbortError(error)
      safeRecordDiagnostic({
        severity: 'ERROR',
        category: 'NETWORK',
        operation,
        outcome: 'FAILED',
        code: aborted ? 'REQUEST_ABORTED' : 'NETWORK_UNAVAILABLE',
        ...(httpMethod ? { http_method: httpMethod } : {}),
        retryable: !aborted,
      })
      return
    }

    const normalized = normalizeApiError(error)
    const serverRequestId = safeServerRequestId(normalized.requestId)
    safeRecordDiagnostic({
      severity: 'ERROR',
      category: normalized.kind === 'AuthenticationRequired' || normalized.kind === 'ForbiddenError'
        ? 'AUTH'
        : 'API',
      operation,
      outcome: 'FAILED',
      code: apiFailureCode(normalized.status, normalized.kind),
      ...(httpMethod ? { http_method: httpMethod } : {}),
      ...(normalized.status !== undefined ? { http_status: normalized.status } : {}),
      ...(normalized.code ? { api_error_code: normalized.code } : {}),
      ...(serverRequestId ? { server_request_id: serverRequestId } : {}),
      retryable: normalized.retryable,
    })
  } catch {
    // Diagnostics are observational only; an unusual thrown value must not
    // change the API client's normal error path.
  }
}

/** A successful HTTP response with an invalid JSON shape is still a safe API failure. */
export function recordMalformedApiResponse(operation: DiagnosticOperation | undefined, method: string): void {
  try {
    if (!operation) {
      return
    }
    const httpMethod = diagnosticMethod(method)
    safeRecordDiagnostic({
      severity: 'ERROR',
      category: 'API',
      operation,
      outcome: 'FAILED',
      code: 'API_INTERNAL_ERROR',
      ...(httpMethod ? { http_method: httpMethod } : {}),
      retryable: true,
    })
  } catch {
    // Diagnostics are observational only; malformed response reporting is best effort.
  }
}

/** Auth recovery has a stable safe code even when the underlying failure is arbitrary. */
export function recordAuthRecoveryFailure(error: unknown): void {
  try {
    const normalized = isApiRequestError(error) ? normalizeApiError(error) : undefined
    const serverRequestId = normalized ? safeServerRequestId(normalized.requestId) : undefined
    safeRecordDiagnostic({
      severity: 'ERROR',
      category: 'AUTH',
      operation: 'AUTH_RECOVERY',
      outcome: 'FAILED',
      code: 'AUTH_RECOVERY_FAILED',
      ...(normalized?.status !== undefined ? { http_status: normalized.status } : {}),
      ...(normalized?.code ? { api_error_code: normalized.code } : {}),
      ...(serverRequestId ? { server_request_id: serverRequestId } : {}),
      retryable: normalized?.retryable ?? true,
    })
  } catch {
    // Diagnostics are observational only; auth recovery must continue normally.
  }
}

const MUTATION_OPERATIONS: Readonly<Record<MutationActionKind, DiagnosticOperation>> = {
  create_backup_set: 'BACKUP_SET_CREATE',
  configure_retention_policy: 'RETENTION_UPDATE',
  create_maintenance_run: 'MAINTENANCE_CREATE',
  advance_maintenance_run: 'MAINTENANCE_ADVANCE',
  create_restore_plan: 'RESTORE_PLAN_CREATE',
  execute_restore_plan: 'RESTORE_EXECUTE',
  create_prune_plan: 'PRUNE_PLAN_CREATE',
  execute_prune_plan: 'PRUNE_EXECUTE',
}

/** Observe uncertain delivery while deliberately excluding replay identity and request data. */
export function recordMutationOutcomeUncertain(actionKind: MutationActionKind): void {
  safeRecordDiagnostic({
    severity: 'WARNING',
    category: 'MUTATION_RECOVERY',
    operation: MUTATION_OPERATIONS[actionKind],
    outcome: 'FAILED',
    code: 'MUTATION_OUTCOME_UNCERTAIN',
    retryable: true,
  })
}

export function recordClientRenderFailure(routeCategory?: DiagnosticRouteCategory): void {
  safeRecordDiagnostic({
    severity: 'ERROR',
    category: 'CLIENT_RENDER',
    operation: 'CLIENT_RENDER',
    outcome: 'FAILED',
    code: 'CLIENT_RENDER_FAILURE',
    retryable: false,
    ...(routeCategory ? { route_category: routeCategory } : {}),
  })
}

export function recordClientRuntimeFailure(): void {
  safeRecordDiagnostic({
    severity: 'ERROR',
    category: 'CLIENT_RUNTIME',
    operation: 'CLIENT_RUNTIME',
    outcome: 'FAILED',
    code: 'CLIENT_RUNTIME_FAILURE',
    retryable: false,
  })
}
