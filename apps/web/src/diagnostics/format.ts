import type {
  DiagnosticCopyBundle,
  DiagnosticEvent,
  DiagnosticRouteCategory,
} from './types'

function browserOnlineState(): boolean | 'unknown' {
  return typeof navigator !== 'undefined' && typeof navigator.onLine === 'boolean'
    ? navigator.onLine
    : 'unknown'
}

function copySafeEvent(event: DiagnosticEvent): DiagnosticEvent {
  return {
    schema_version: event.schema_version,
    event_id: event.event_id,
    occurred_at: event.occurred_at,
    severity: event.severity,
    category: event.category,
    operation: event.operation,
    outcome: event.outcome,
    code: event.code,
    ...(event.http_method ? { http_method: event.http_method } : {}),
    ...(event.http_status !== undefined ? { http_status: event.http_status } : {}),
    ...(event.api_error_code ? { api_error_code: event.api_error_code } : {}),
    ...(event.server_request_id ? { server_request_id: event.server_request_id } : {}),
    retryable: event.retryable,
    ...(event.route_category ? { route_category: event.route_category } : {}),
    ...(event.occurrences !== undefined ? { occurrences: event.occurrences } : {}),
  }
}

export function buildDiagnosticsBundle(
  events: readonly DiagnosticEvent[],
  routeCategory: DiagnosticRouteCategory,
  generatedAt = new Date().toISOString(),
): DiagnosticCopyBundle {
  return {
    schema_version: 1,
    generated_at: generatedAt,
    browser_online: browserOnlineState(),
    route_category: routeCategory,
    events: events.map(copySafeEvent),
  }
}

export function serializeDiagnosticsBundle(
  events: readonly DiagnosticEvent[],
  routeCategory: DiagnosticRouteCategory,
  generatedAt?: string,
): string {
  return JSON.stringify(buildDiagnosticsBundle(events, routeCategory, generatedAt), null, 2)
}
