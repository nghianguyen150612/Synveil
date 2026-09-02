import { ApiRequestError } from './errors'

export interface ApiClientOptions {
  readonly baseUrl?: string
  readonly credentials?: RequestCredentials
  readonly fetchImpl?: typeof fetch
}

function trimTrailingSlashes(value: string): string {
  return value.replace(/\/+$/, '')
}

function publicApiBaseUrl(): string {
  const configured = import.meta.env.VITE_API_BASE_URL
  return typeof configured === 'string' ? trimTrailingSlashes(configured.trim()) : ''
}

export class ApiClient {
  private readonly baseUrl: string
  private readonly credentials: RequestCredentials
  private readonly fetchImpl: typeof fetch

  constructor(options: ApiClientOptions = {}) {
    const fetchImpl = options.fetchImpl ?? globalThis.fetch?.bind(globalThis)
    if (!fetchImpl) {
      throw new Error('The browser fetch API is unavailable.')
    }

    this.baseUrl = trimTrailingSlashes(options.baseUrl ?? publicApiBaseUrl())
    this.credentials = options.credentials ?? 'same-origin'
    this.fetchImpl = fetchImpl
  }

  async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const normalizedPath = path.startsWith('/') ? path : `/${path}`
    const headers = new Headers(init.headers)
    if (!headers.has('Accept')) {
      headers.set('Accept', 'application/json')
    }

    const response = await this.fetchImpl(`${this.baseUrl}${normalizedPath}`, {
      ...init,
      credentials: init.credentials ?? this.credentials,
      headers,
    })

    if (!response.ok) {
      throw await ApiRequestError.fromResponse(response)
    }

    if (response.status === 204) {
      return undefined as T
    }

    return (await response.json()) as T
  }

  get<T>(path: string, init: Omit<RequestInit, 'method'> = {}): Promise<T> {
    return this.request<T>(path, { ...init, method: 'GET' })
  }
}

/** One API boundary instance; it never accepts or stores bearer secrets. */
export const apiClient = new ApiClient()
