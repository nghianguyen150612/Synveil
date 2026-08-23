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

const FALLBACK_ERROR: ApiErrorPayload = {
  code: 'http_error',
  message: 'The request could not be completed.',
  request_id: 'unknown',
  retryable: false,
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
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

    const payload = parseErrorPayload(parsed) ?? {
      ...FALLBACK_ERROR,
      retryable: response.status >= 500,
    }
    return new ApiRequestError(response.status, payload)
  }
}

export function isApiRequestError(error: unknown): error is ApiRequestError {
  return error instanceof ApiRequestError
}

/** Convert unknown failures to safe UI copy without exposing diagnostics. */
export function toUserFacingMessage(error: unknown): string {
  if (isApiRequestError(error)) {
    return error.message
  }
  return 'Something went wrong. Please try again.'
}
