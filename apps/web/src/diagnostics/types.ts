export const DIAGNOSTIC_SCHEMA_VERSION = 1 as const

export type DiagnosticSeverity = 'INFO' | 'WARNING' | 'ERROR'

export type DiagnosticCategory =
  | 'NETWORK'
  | 'API'
  | 'AUTH'
  | 'MUTATION_RECOVERY'
  | 'CLIENT_RENDER'
  | 'CLIENT_RUNTIME'

export type DiagnosticOutcome = 'FAILED' | 'RECOVERED'

export type DiagnosticCode =
  | 'NETWORK_UNAVAILABLE'
  | 'REQUEST_ABORTED'
  | 'API_AUTH_REQUIRED'
  | 'API_FORBIDDEN'
  | 'API_NOT_FOUND'
  | 'API_CONFLICT'
  | 'API_VALIDATION'
  | 'API_DEPENDENCY_UNAVAILABLE'
  | 'API_INTERNAL_ERROR'
  | 'AUTH_RECOVERY_FAILED'
  | 'AUTH_RECOVERED'
  | 'MUTATION_OUTCOME_UNCERTAIN'
  | 'MUTATION_RECONCILED'
  | 'CLIENT_RENDER_FAILURE'
  | 'CLIENT_RUNTIME_FAILURE'

/** Logical names are deliberately independent of resource URLs and IDs. */
export type DiagnosticOperation =
  | 'BACKUP_SET_LIST'
  | 'BACKUP_SET_GET'
  | 'BACKUP_SET_CREATE'
  | 'SNAPSHOT_LIST'
  | 'SNAPSHOT_GET'
  | 'SNAPSHOT_TREE'
  | 'RETENTION_GET'
  | 'RETENTION_UPDATE'
  | 'MAINTENANCE_LIST'
  | 'MAINTENANCE_GET'
  | 'MAINTENANCE_CREATE'
  | 'MAINTENANCE_ADVANCE'
  | 'OPERATION_LIST'
  | 'OPERATION_GET'
  | 'RESTORE_PLAN_CREATE'
  | 'RESTORE_PLAN_GET'
  | 'RESTORE_EXECUTE'
  | 'RESTORE_EXECUTION_GET'
  | 'PRUNE_PLAN_CREATE'
  | 'PRUNE_PLAN_GET'
  | 'PRUNE_EXECUTE'
  | 'PRUNE_EXECUTION_GET'
  | 'LIBRARY_LIST'
  | 'LIBRARY_CHILDREN'
  | 'NODE_GET'
  | 'AUTH_BOOTSTRAP'
  | 'AUTH_BOOTSTRAP_CREATE'
  | 'AUTH_LOGIN'
  | 'AUTH_SESSION_CHECK'
  | 'AUTH_CSRF'
  | 'AUTH_RECOVERY'
  | 'AUTH_LOGOUT'
  | 'UPLOAD_SESSION_CREATE'
  | 'UPLOAD_SESSION_GET'
  | 'UPLOAD_CHUNK_APPEND'
  | 'UPLOAD_COMPLETE'
  | 'UPLOAD_ABORT'
  | 'CLIENT_RENDER'
  | 'CLIENT_RUNTIME'

export type DiagnosticHttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

export type DiagnosticRouteCategory =
  | 'HOME'
  | 'BACKUP_OVERVIEW'
  | 'BACKUP_DETAIL'
  | 'BACKUP_RESTORE'
  | 'BACKUP_PRUNE'
  | 'DEVELOPMENT_HEALTH'
  | 'AUTH'
  | 'NOT_FOUND'
  | 'UNKNOWN'

export interface DiagnosticEvent {
  readonly schema_version: typeof DIAGNOSTIC_SCHEMA_VERSION
  readonly event_id: string
  readonly occurred_at: string
  readonly severity: DiagnosticSeverity
  readonly category: DiagnosticCategory
  readonly operation: DiagnosticOperation
  readonly outcome: DiagnosticOutcome
  readonly code: DiagnosticCode
  readonly http_method?: DiagnosticHttpMethod
  readonly http_status?: number
  readonly api_error_code?: string
  readonly server_request_id?: string
  readonly retryable: boolean
  readonly route_category?: DiagnosticRouteCategory
  readonly occurrences?: number
}

export interface DiagnosticEventInput {
  readonly severity: DiagnosticSeverity
  readonly category: DiagnosticCategory
  readonly operation: DiagnosticOperation
  readonly outcome: DiagnosticOutcome
  readonly code: DiagnosticCode
  readonly http_method?: DiagnosticHttpMethod
  readonly http_status?: number
  readonly api_error_code?: string
  readonly server_request_id?: string
  readonly retryable: boolean
  readonly route_category?: DiagnosticRouteCategory
}

export interface DiagnosticCopyBundle {
  readonly schema_version: typeof DIAGNOSTIC_SCHEMA_VERSION
  readonly generated_at: string
  readonly browser_online: boolean | 'unknown'
  readonly route_category: DiagnosticRouteCategory
  readonly events: readonly DiagnosticEvent[]
}
