import { apiClient, type ApiClient } from './client'

export type CreateUploadSessionRequest =
  | {
      readonly operation: 'CREATE_FILE'
      readonly library_id: string
      readonly parent_id: string
      readonly name: string
      readonly expected_bytes: string
      readonly expected_sha256?: string
    }
  | {
      readonly operation: 'REPLACE_CONTENT'
      readonly library_id: string
      readonly node_id: string
      readonly expected_revision: string
      readonly expected_bytes: string
      readonly expected_sha256?: string
    }

export type UploadSessionState =
  | 'OPEN'
  | 'VERIFYING'
  | 'COMMITTING'
  | 'COMMITTED'
  | 'FAILED'
  | 'EXPIRED'
  | 'ABORTED'

export type UploadTarget =
  | {
      readonly operation: 'CREATE_FILE'
      readonly library_id: string
      readonly parent_id: string
      readonly node_id: string
      readonly name: string
    }
  | {
      readonly operation: 'REPLACE_CONTENT'
      readonly library_id: string
      readonly node_id: string
      readonly expected_revision: string
    }

export interface UploadCompletionResource {
  readonly id: string
  readonly type: 'upload_completion'
  readonly attributes: {
    readonly node_id: string
    readonly file_version_id: string
    readonly object_id: string
    readonly node_revision: string
    readonly bytes: string
    readonly sha256: string
    readonly committed_at: string
  }
}

export interface UploadSessionResource {
  readonly id: string
  readonly type: 'upload_session'
  readonly attributes: {
    readonly operation: CreateUploadSessionRequest['operation']
    readonly state: UploadSessionState
    readonly received_bytes: string
    readonly expected_bytes: string
    readonly expected_sha256?: string
    readonly created_at: string
    readonly updated_at: string
    readonly expires_at: string
    readonly target: UploadTarget
    readonly last_error_code?: string
    readonly terminal_failure_code?: string
    readonly completion?: UploadCompletionResource
  }
}

export interface UploadSessionResponse {
  readonly data: UploadSessionResource
  readonly meta: { readonly request_id: string }
}

export interface UploadCompletionResponse {
  readonly data: UploadCompletionResource
  readonly meta: { readonly request_id: string }
}

export interface AppendUploadResult {
  /** The server-returned authoritative offset; never infer it locally. */
  readonly upload_offset: string
}

const MAX_U64 = 18_446_744_073_709_551_615n
const CANONICAL_UNSIGNED_DECIMAL = /^(0|[1-9][0-9]*)$/

function uploadPath(uploadSessionId: string): string {
  return `/api/v1/upload-sessions/${encodeURIComponent(uploadSessionId)}`
}

function requireCanonicalOffset(value: string | null): string {
  if (
    value === null ||
    value.length > 20 ||
    !CANONICAL_UNSIGNED_DECIMAL.test(value) ||
    BigInt(value) > MAX_U64
  ) {
    throw new Error('The upload response did not include a valid authoritative offset.')
  }
  return value
}

export function createUploadSession(
  request: CreateUploadSessionRequest,
  client: ApiClient = apiClient,
): Promise<UploadSessionResponse> {
  return client.post<UploadSessionResponse>('/api/v1/upload-sessions', request, {
    diagnosticOperation: 'UPLOAD_SESSION_CREATE',
  })
}

export function getUploadSession(
  uploadSessionId: string,
  client: ApiClient = apiClient,
): Promise<UploadSessionResponse> {
  return client.get<UploadSessionResponse>(uploadPath(uploadSessionId), {
    diagnosticOperation: 'UPLOAD_SESSION_GET',
  })
}

/**
 * Send file bytes directly. A failed/ambiguous request must be recovered by
 * calling getUploadSession and resuming from its received_bytes value.
 */
export async function appendUploadChunk(
  uploadSessionId: string,
  uploadOffset: string,
  bytes: Blob | ArrayBuffer,
  client: ApiClient = apiClient,
): Promise<AppendUploadResult> {
  requireCanonicalOffset(uploadOffset)
  const response = await client.requestResponse(uploadPath(uploadSessionId), {
    method: 'PATCH',
    headers: {
      'Content-Type': 'application/octet-stream',
      'Upload-Offset': uploadOffset,
    },
    body: bytes,
    diagnosticOperation: 'UPLOAD_CHUNK_APPEND',
  })
  return {
    upload_offset: requireCanonicalOffset(response.headers.get('Upload-Offset')),
  }
}

export function completeUpload(
  uploadSessionId: string,
  client: ApiClient = apiClient,
): Promise<UploadCompletionResponse> {
  return client.post<UploadCompletionResponse>(`${uploadPath(uploadSessionId)}/complete`, undefined, {
    diagnosticOperation: 'UPLOAD_COMPLETE',
  })
}

export function abortUpload(
  uploadSessionId: string,
  client: ApiClient = apiClient,
): Promise<UploadSessionResponse> {
  return client.post<UploadSessionResponse>(`${uploadPath(uploadSessionId)}/abort`, undefined, {
    diagnosticOperation: 'UPLOAD_ABORT',
  })
}
