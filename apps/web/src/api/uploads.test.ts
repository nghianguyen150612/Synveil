import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiClient } from './client'
import { ApiRequestError } from './errors'
import {
  abortUpload,
  appendUploadChunk,
  completeUpload,
  createUploadSession,
  getUploadSession,
  type UploadCompletionResponse,
  type UploadSessionResponse,
} from './uploads'

const SESSION_ID = '0198da23-7b6d-7a11-8000-000000000001'

const sessionResponse: UploadSessionResponse = {
  data: {
    id: SESSION_ID,
    type: 'upload_session',
    attributes: {
      operation: 'CREATE_FILE',
      state: 'OPEN',
      received_bytes: '0',
      expected_bytes: '8',
      created_at: '2026-08-24T00:00:00Z',
      updated_at: '2026-08-24T00:00:00Z',
      expires_at: '2026-08-25T00:00:00Z',
      target: {
        operation: 'CREATE_FILE',
        library_id: '0198da23-7b6d-7a11-8000-000000000002',
        parent_id: '0198da23-7b6d-7a11-8000-000000000003',
        node_id: '0198da23-7b6d-7a11-8000-000000000004',
        name: 'report.bin',
      },
    },
  },
  meta: { request_id: 'upload-request-1234' },
}

const completionResponse: UploadCompletionResponse = {
  data: {
    id: SESSION_ID,
    type: 'upload_completion',
    attributes: {
      node_id: '0198da23-7b6d-7a11-8000-000000000004',
      file_version_id: '0198da23-7b6d-7a11-8000-000000000005',
      object_id: '0198da23-7b6d-7a11-8000-000000000006',
      node_revision: '1',
      bytes: '8',
      sha256: `sha256:${'0'.repeat(64)}`,
      committed_at: '2026-08-24T00:01:00Z',
    },
  },
  meta: { request_id: 'upload-complete-1234' },
}

function jsonResponse(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

describe('upload API helpers', () => {
  beforeEach(() => {
    document.cookie = 'synveil_csrf=upload-csrf-proof; path=/'
  })

  it('uses canonical routes, same-origin credentials, and CSRF for mutations', async () => {
    const fetchImpl = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(sessionResponse, 201))
      .mockResolvedValueOnce(jsonResponse(sessionResponse))
      .mockResolvedValueOnce(jsonResponse(completionResponse))
      .mockResolvedValueOnce(jsonResponse(sessionResponse))
    const client = new ApiClient({ fetchImpl })

    await expect(
      createUploadSession(
        {
          operation: 'CREATE_FILE',
          library_id: sessionResponse.data.attributes.target.library_id,
          parent_id:
            sessionResponse.data.attributes.target.operation === 'CREATE_FILE'
              ? sessionResponse.data.attributes.target.parent_id
              : '',
          name: 'report.bin',
          expected_bytes: '8',
        },
        client,
      ),
    ).resolves.toEqual(sessionResponse)
    await expect(getUploadSession(SESSION_ID, client)).resolves.toEqual(sessionResponse)
    await expect(completeUpload(SESSION_ID, client)).resolves.toEqual(completionResponse)
    await expect(abortUpload(SESSION_ID, client)).resolves.toEqual(sessionResponse)

    expect(fetchImpl.mock.calls.map(([url]) => url)).toEqual([
      '/api/v1/upload-sessions',
      `/api/v1/upload-sessions/${SESSION_ID}`,
      `/api/v1/upload-sessions/${SESSION_ID}/complete`,
      `/api/v1/upload-sessions/${SESSION_ID}/abort`,
    ])
    for (const [, init] of fetchImpl.mock.calls) {
      expect(init?.credentials).toBe('same-origin')
    }
    const createHeaders = new Headers(fetchImpl.mock.calls[0]?.[1]?.headers)
    expect(createHeaders.get('Content-Type')).toBe('application/json')
    expect(createHeaders.get('X-CSRF-Token')).toBe('upload-csrf-proof')
    const statusHeaders = new Headers(fetchImpl.mock.calls[1]?.[1]?.headers)
    expect(statusHeaders.get('X-CSRF-Token')).toBeNull()
    for (const callIndex of [2, 3]) {
      const headers = new Headers(fetchImpl.mock.calls[callIndex]?.[1]?.headers)
      expect(headers.get('X-CSRF-Token')).toBe('upload-csrf-proof')
    }
  })

  it('sends Blob and ArrayBuffer bodies raw and trusts only the returned offset', async () => {
    const fetchImpl = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(
        new Response(null, { status: 204, headers: { 'Upload-Offset': '3' } }),
      )
      .mockResolvedValueOnce(
        new Response(null, { status: 204, headers: { 'Upload-Offset': '8' } }),
      )
    const client = new ApiClient({ fetchImpl })
    const blob = new Blob([new Uint8Array([0, 1, 255])])
    const buffer = new Uint8Array([2, 3, 4, 5, 6]).buffer

    await expect(appendUploadChunk(SESSION_ID, '0', blob, client)).resolves.toEqual({
      upload_offset: '3',
    })
    await expect(appendUploadChunk(SESSION_ID, '3', buffer, client)).resolves.toEqual({
      upload_offset: '8',
    })

    const firstInit = fetchImpl.mock.calls[0]?.[1]
    const firstHeaders = new Headers(firstInit?.headers)
    expect(firstInit).toEqual(
      expect.objectContaining({ method: 'PATCH', credentials: 'same-origin', body: blob }),
    )
    expect(firstHeaders.get('Content-Type')).toBe('application/octet-stream')
    expect(firstHeaders.get('Upload-Offset')).toBe('0')
    expect(firstHeaders.get('X-CSRF-Token')).toBe('upload-csrf-proof')
    expect(typeof firstInit?.body).not.toBe('string')

    const secondInit = fetchImpl.mock.calls[1]?.[1]
    expect(secondInit?.body).toBe(buffer)
    expect(typeof secondInit?.body).not.toBe('string')
  })

  it('rejects missing, malformed, or non-u64 authoritative offsets safely', async () => {
    const fetchImpl = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(new Response(null, { status: 204 }))
      .mockResolvedValueOnce(
        new Response(null, { status: 204, headers: { 'Upload-Offset': '01' } }),
      )
      .mockResolvedValueOnce(
        new Response(null, {
          status: 204,
          headers: { 'Upload-Offset': '18446744073709551616' },
        }),
      )
    const client = new ApiClient({ fetchImpl })
    const bytes = new ArrayBuffer(1)

    for (const offset of ['0', '0', '0']) {
      await expect(appendUploadChunk(SESSION_ID, offset, bytes, client)).rejects.toThrow(
        'valid authoritative offset',
      )
    }
    expect(fetchImpl).toHaveBeenCalledTimes(3)

    await expect(appendUploadChunk(SESSION_ID, '01', bytes, client)).rejects.toThrow(
      'valid authoritative offset',
    )
    expect(fetchImpl).toHaveBeenCalledTimes(3)
  })

  it('preserves the safe API error envelope for an offset conflict', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(
        {
          error: {
            code: 'invalid_offset',
            message: 'The supplied upload offset does not match the authoritative offset.',
            request_id: 'upload-conflict-1234',
            retryable: false,
            details: { current_offset: '6' },
          },
        },
        409,
      ),
    )
    const client = new ApiClient({ fetchImpl })

    const failure = await appendUploadChunk(
      SESSION_ID,
      '0',
      new ArrayBuffer(1),
      client,
    ).catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(ApiRequestError)
    expect(failure).toMatchObject({
      status: 409,
      code: 'invalid_offset',
      requestId: 'upload-conflict-1234',
      details: { current_offset: '6' },
    })
  })
})
