import { ApiRequestError } from './errors'

export interface ApiClientOptions {
  readonly baseUrl?: string
  readonly credentials?: RequestCredentials
  readonly fetchImpl?: typeof fetch
}

const MUTATING_METHODS = new Set(['POST', 'PUT', 'PATCH', 'DELETE'])
const CSRF_COOKIE_NAME = 'synveil_csrf'
const CSRF_HEADER_NAME = 'X-CSRF-Token'

function trimTrailingSlashes(value: string): string {
  return value.replace(/\/+$/, '')
}

function publicApiBaseUrl(): string {
  const configured = import.meta.env.VITE_API_BASE_URL
  return typeof configured === 'string' ? trimTrailingSlashes(configured.trim()) : ''
}

function browserCookie(name: string): string | undefined {
  if (typeof document === 'undefined') {
    return undefined
  }

  for (const pair of document.cookie.split(';')) {
    const [candidateName, ...candidateValue] = pair.trim().split('=')
    if (candidateName === name) {
      return candidateValue.join('=') || undefined
    }
  }
  return undefined
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

  async requestResponse(path: string, init: RequestInit = {}): Promise<Response> {
    const normalizedPath = path.startsWith('/') ? path : `/${path}`
    const headers = new Headers(init.headers)
    const method = (init.method ?? 'GET').toUpperCase()
    if (!headers.has('Accept')) {
      headers.set('Accept', 'application/json')
    }
    if (MUTATING_METHODS.has(method) && !headers.has(CSRF_HEADER_NAME)) {
      const csrfToken = browserCookie(CSRF_COOKIE_NAME)
      if (csrfToken) {
        headers.set(CSRF_HEADER_NAME, csrfToken)
      }
    }

    const response = await this.fetchImpl(`${this.baseUrl}${normalizedPath}`, {
      ...init,
      credentials: init.credentials ?? this.credentials,
      headers,
    })

    if (!response.ok) {
      throw await ApiRequestError.fromResponse(response)
    }

    return response
  }

  async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const response = await this.requestResponse(path, init)

    if (response.status === 204) {
      return undefined as T
    }

    return (await response.json()) as T
  }

  get<T>(path: string, init: Omit<RequestInit, 'method'> = {}): Promise<T> {
    return this.request<T>(path, { ...init, method: 'GET' })
  }

  post<T>(
    path: string,
    body?: unknown,
    init: Omit<RequestInit, 'method' | 'body'> = {},
  ): Promise<T> {
    const headers = new Headers(init.headers)
    if (body !== undefined && !headers.has('Content-Type')) {
      headers.set('Content-Type', 'application/json')
    }
    return this.request<T>(path, {
      ...init,
      method: 'POST',
      headers,
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })
  }
}

/** One API boundary instance; it never accepts or stores bearer secrets. */
export const apiClient = new ApiClient()
