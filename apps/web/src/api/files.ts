import { apiClient, type ApiClient, type ApiRequestInit } from './client'
import type { DiagnosticOperation } from '../diagnostics/types'

export type LiveLibraryStatus = 'ACTIVE' | 'READ_ONLY' | 'QUARANTINED'
export type LiveNodeKind = 'FILE' | 'DIRECTORY'
export type LiveNodeState = 'ACTIVE' | 'TRASHED' | 'PURGING'

export interface LiveLibraryResource {
  readonly id: string
  readonly type: 'library'
  readonly revision: string
  readonly attributes: {
    readonly name: string
    readonly root_node_id: string
    readonly status: LiveLibraryStatus
    readonly created_at: string
    readonly updated_at: string
  }
}

export interface LiveNodeResource {
  readonly id: string
  readonly type: 'node'
  readonly revision: string
  readonly attributes: {
    readonly library_id: string
    readonly parent_id?: string
    readonly name: string
    readonly kind: LiveNodeKind
    readonly state: LiveNodeState
    readonly created_at: string
    readonly updated_at: string
  }
}

export interface LivePage {
  readonly next_cursor?: string
  readonly has_more: boolean
}

export interface LiveResponseMeta {
  readonly request_id: string
}

export interface LiveLibraryListResponse {
  readonly data: readonly LiveLibraryResource[]
  readonly page: LivePage
  readonly meta: LiveResponseMeta
}

export interface LiveNodeListResponse {
  readonly data: readonly LiveNodeResource[]
  readonly page: LivePage
  readonly meta: LiveResponseMeta
}

export interface LiveNodeResponse {
  readonly data: LiveNodeResource
  readonly meta: LiveResponseMeta
}

export interface FileMetadataRequestOptions {
  readonly signal?: AbortSignal
}

export interface LiveLibraryListOptions extends FileMetadataRequestOptions {
  readonly cursor?: string
  readonly limit?: number
}

export interface LiveNodeListOptions extends FileMetadataRequestOptions {
  readonly cursor?: string
  readonly limit?: number
  readonly parentId?: string
}

function pathWithQuery(
  path: string,
  values: Readonly<Record<string, string | number | undefined>>,
): string {
  const query = new URLSearchParams()
  for (const [key, value] of Object.entries(values)) {
    if (value !== undefined) {
      query.set(key, String(value))
    }
  }
  const encoded = query.toString()
  return encoded ? `${path}?${encoded}` : path
}

function requestOptions(
  options: FileMetadataRequestOptions | undefined,
  diagnosticOperation: DiagnosticOperation,
): ApiRequestInit {
  return {
    diagnosticOperation,
    ...(options?.signal ? { signal: options.signal } : {}),
  }
}

export function listLibraries(
  options: LiveLibraryListOptions = {},
  client: ApiClient = apiClient,
): Promise<LiveLibraryListResponse> {
  return client.get<LiveLibraryListResponse>(
    pathWithQuery('/api/v1/libraries', {
      cursor: options.cursor,
      limit: options.limit,
    }),
    requestOptions(options, 'LIBRARY_LIST'),
  )
}

export function listLibraryChildren(
  libraryId: string,
  options: LiveNodeListOptions = {},
  client: ApiClient = apiClient,
): Promise<LiveNodeListResponse> {
  return client.get<LiveNodeListResponse>(
    pathWithQuery(`/api/v1/libraries/${encodeURIComponent(libraryId)}/nodes`, {
      parent_id: options.parentId,
      cursor: options.cursor,
      limit: options.limit,
    }),
    requestOptions(options, 'LIBRARY_CHILDREN'),
  )
}

export function getLiveNode(
  nodeId: string,
  options?: FileMetadataRequestOptions,
  client: ApiClient = apiClient,
): Promise<LiveNodeResponse> {
  return client.get<LiveNodeResponse>(
    `/api/v1/nodes/${encodeURIComponent(nodeId)}`,
    requestOptions(options, 'NODE_GET'),
  )
}

export interface FileMetadataApi {
  readonly listLibraries: (
    options?: LiveLibraryListOptions,
  ) => Promise<LiveLibraryListResponse>
  readonly listLibraryChildren: (
    libraryId: string,
    options?: LiveNodeListOptions,
  ) => Promise<LiveNodeListResponse>
  readonly getLiveNode: (
    nodeId: string,
    options?: FileMetadataRequestOptions,
  ) => Promise<LiveNodeResponse>
}

export const fileMetadataApi: FileMetadataApi = {
  listLibraries: (options) => listLibraries(options),
  listLibraryChildren: (libraryId, options) => listLibraryChildren(libraryId, options),
  getLiveNode: (nodeId, options) => getLiveNode(nodeId, options),
}
