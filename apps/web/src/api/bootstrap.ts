import { apiClient, type ApiClient } from './client'

export interface BootstrapAdminRequest {
  readonly login: string
  readonly login_key: string
  readonly password: string
}

export interface BootstrapStatusResponse {
  readonly data: { readonly setup_required: boolean }
  readonly meta: { readonly request_id: string }
}

export interface BootstrapApi {
  readonly getStatus: () => Promise<BootstrapStatusResponse>
  readonly createFirstAdmin: (
    request: BootstrapAdminRequest,
  ) => Promise<BootstrapStatusResponse>
}

/** Read the safe one-time setup state; it never returns user or credential data. */
export function getBootstrapStatus(
  client: ApiClient = apiClient,
): Promise<BootstrapStatusResponse> {
  return client.get<BootstrapStatusResponse>('/api/v1/system/bootstrap-status', {
    diagnosticOperation: 'AUTH_BOOTSTRAP',
  })
}

/** Create the first administrator through the server-side race-safe boundary. */
export function createFirstAdmin(
  request: BootstrapAdminRequest,
  client: ApiClient = apiClient,
): Promise<BootstrapStatusResponse> {
  return client.post<BootstrapStatusResponse>('/api/v1/bootstrap/admin', request, {
    diagnosticOperation: 'AUTH_BOOTSTRAP_CREATE',
  })
}

/** Default dependency-injection boundary used by AuthProvider and its tests. */
export const bootstrapApi: BootstrapApi = {
  getStatus: () => getBootstrapStatus(),
  createFirstAdmin: (request) => createFirstAdmin(request),
}
