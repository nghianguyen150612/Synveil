export interface ApiErrorPayload {
  readonly code: string
  readonly message: string
  readonly request_id: string
  readonly retryable: boolean
  readonly details?: Record<string, unknown>
}

export interface ApiErrorEnvelope {
  readonly error: ApiErrorPayload
}

const MAX_ERROR_TEXT_LENGTH = 64 * 1024
const MAX_SAFE_CORRELATION_ID_LENGTH = 128
const SAFE_CORRELATION_ID_PATTERN = /^[A-Za-z0-9._~-]+$/

const FALLBACK_ERROR: ApiErrorPayload = {
  code: 'http_error',
  message: 'The request could not be completed.',
  request_id: 'unknown',
  retryable: false,
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

/**
 * The backend validates request IDs before returning them. Repeat the small
 * boundary check in the browser before a value reaches user-facing diagnostics.
 * Short IDs are accepted here so test and development servers can still be
 * correlated; production IDs remain bounded by the server contract.
 */
export function safeServerRequestId(value: unknown): string | undefined {
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    value.length > MAX_SAFE_CORRELATION_ID_LENGTH ||
    value === 'unknown' ||
    value === 'pending' ||
    value.includes('svd1_') ||
    value.includes('sve1_') ||
    !SAFE_CORRELATION_ID_PATTERN.test(value)
  ) {
    return undefined
  }
  return value
}

function parseErrorPayload(value: unknown): ApiErrorPayload | undefined {
  if (!isRecord(value) || !isRecord(value.error)) {
    return undefined
  }

  const error = value.error
  if (
    typeof error.code !== 'string' ||
    typeof error.message !== 'string' ||
    typeof error.request_id !== 'string' ||
    typeof error.retryable !== 'boolean'
  ) {
    return undefined
  }

  const details = isRecord(error.details) ? error.details : undefined
  return {
    code: error.code,
    message: error.message,
    request_id: error.request_id,
    retryable: error.retryable,
    ...(details ? { details } : {}),
  }
}

export class ApiRequestError extends Error {
  readonly status: number
  readonly code: string
  readonly requestId: string
  readonly retryable: boolean
  readonly details?: Record<string, unknown>

  constructor(status: number, payload: ApiErrorPayload) {
    super(payload.message)
    this.name = 'ApiRequestError'
    this.status = status
    this.code = payload.code
    this.requestId = payload.request_id
    this.retryable = payload.retryable
    this.details = payload.details
  }

  static async fromResponse(response: Response): Promise<ApiRequestError> {
    const text = await response.text().catch(() => '')
    const boundedText = text.slice(0, MAX_ERROR_TEXT_LENGTH)
    let parsed: unknown

    try {
      parsed = JSON.parse(boundedText) as unknown
    } catch {
      parsed = undefined
    }

    const parsedPayload = parseErrorPayload(parsed)
    const responseRequestId = safeServerRequestId(response.headers.get('X-Request-ID'))
    const payload = parsedPayload
      ? {
          ...parsedPayload,
          request_id:
            safeServerRequestId(parsedPayload.request_id) ?? responseRequestId ?? 'unknown',
        }
      : {
          ...FALLBACK_ERROR,
          request_id: responseRequestId ?? 'unknown',
          retryable: response.status >= 500,
        }
    return new ApiRequestError(response.status, payload)
  }
}

export function isApiRequestError(error: unknown): error is ApiRequestError {
  return error instanceof ApiRequestError
}

export type NormalizedApiErrorKind =
  | 'AuthenticationRequired'
  | 'ForbiddenError'
  | 'ValidationError'
  | 'ConflictError'
  | 'NotFoundError'
  | 'DependencyUnavailable'
  | 'UnexpectedServerError'

export interface NormalizedApiError {
  readonly kind: NormalizedApiErrorKind
  readonly status?: number
  readonly code?: string
  readonly requestId?: string
  readonly retryable: boolean
  readonly details?: Record<string, unknown>
}

/** Keep transport diagnostics typed at the API boundary; views choose their own safe copy. */
export function normalizeApiError(error: unknown): NormalizedApiError {
  if (!isApiRequestError(error)) {
    return { kind: 'DependencyUnavailable', retryable: true }
  }

  const shared = {
    status: error.status,
    code: error.code,
    requestId: error.requestId,
    retryable: error.retryable,
    ...(error.details ? { details: error.details } : {}),
  }

  if (error.status === 401) {
    return { kind: 'AuthenticationRequired', ...shared }
  }
  if (error.status === 404) {
    return { kind: 'NotFoundError', ...shared }
  }
  if (error.status === 403) {
    return { kind: 'ForbiddenError', ...shared }
  }
  if (error.status === 409) {
    return { kind: 'ConflictError', ...shared }
  }
  if (error.status === 503) {
    return { kind: 'DependencyUnavailable', ...shared }
  }
  if (error.status === 400 || error.status === 413 || error.status === 422) {
    return { kind: 'ValidationError', ...shared }
  }
  return { kind: 'UnexpectedServerError', ...shared }
}

/** Convert unknown failures to safe UI copy without exposing diagnostics. */
export function toUserFacingMessage(error: unknown): string {
  switch (normalizeApiError(error).kind) {
    case 'AuthenticationRequired':
      return 'Your session has expired. Sign in again.'
    case 'ForbiddenError':
      return 'This request could not be verified. Refresh and try again.'
    case 'ValidationError':
      return 'Check the information you entered and try again.'
    case 'ConflictError':
      return 'The item changed before this request completed. Refresh and try again.'
    case 'NotFoundError':
      return 'The requested item is not available.'
    case 'DependencyUnavailable':
      return 'The service is temporarily unavailable. Please try again.'
    case 'UnexpectedServerError':
      return 'Something went wrong. Please try again.'
  }
}
