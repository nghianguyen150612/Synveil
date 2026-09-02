import { apiClient, type ApiClient } from './client'

export interface LoginRequest {
  readonly login: string
  readonly login_key: string
  readonly password: string
}

export interface AuthSessionData {
  readonly authenticated: true
  readonly user_id: string
  readonly is_instance_admin: boolean
  readonly session_id: string
  readonly expires_at?: string
}

export interface AuthSessionResponse {
  readonly data: AuthSessionData
  readonly meta: { readonly request_id: string }
}

export interface CsrfResponse {
  readonly data: { readonly csrf_token: string }
  readonly meta: { readonly request_id: string }
}

export interface AuthApi {
  readonly login: (credentials: LoginRequest) => Promise<AuthSessionResponse>
  readonly getCurrentSession: () => Promise<AuthSessionResponse>
  readonly getCsrfToken: () => Promise<CsrfResponse>
  readonly logout: () => Promise<void>
}

/**
 * Browser authentication calls remain behind the one API client boundary.
 * The session credential is never read or stored by this module; the browser
 * owns the Secure/HttpOnly cookie lifecycle.
 */
export function login(
  credentials: LoginRequest,
  client: ApiClient = apiClient,
): Promise<AuthSessionResponse> {
  return client.post<AuthSessionResponse>('/api/v1/auth/login', credentials)
}

export function getCurrentSession(
  client: ApiClient = apiClient,
): Promise<AuthSessionResponse> {
  return client.get<AuthSessionResponse>('/api/v1/auth/session')
}

export function getCsrfToken(client: ApiClient = apiClient): Promise<CsrfResponse> {
  return client.get<CsrfResponse>('/api/v1/auth/csrf')
}

export function logout(client: ApiClient = apiClient): Promise<void> {
  return client.post<void>('/api/v1/auth/logout')
}

/** Default dependency-injection boundary used by AuthProvider and its tests. */
export const authApi: AuthApi = {
  login: (credentials) => login(credentials),
  getCurrentSession: () => getCurrentSession(),
  getCsrfToken: () => getCsrfToken(),
  logout: () => logout(),
}
