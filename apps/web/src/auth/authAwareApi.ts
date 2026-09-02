import { useMemo } from 'react'

import {
  backupApi as defaultBackupApi,
  type BackupApi,
} from '../api/backups'
import {
  fileMetadataApi as defaultFileApi,
  type FileMetadataApi,
} from '../api/files'
import { isApiRequestError } from '../api/errors'
import type { AuthRecoveryResult } from './auth-context'
import { useAuth } from './useAuth'

export type AuthBoundaryFailure =
  | 'session_recovered'
  | 'unauthenticated'
  | 'recovery_failed'
  | 'principal_changed'
  | 'stale_generation'

/**
 * An auth-bound request deliberately failed closed. It is distinct from the
 * original 401/403 so page-level legacy handlers cannot start a second auth
 * recovery attempt after the centralized coordinator already handled it.
 */
export class AuthBoundaryError extends Error {
  readonly reason: AuthBoundaryFailure

  constructor(reason: AuthBoundaryFailure) {
    super('The protected request was stopped at an authentication boundary.')
    this.name = 'AuthBoundaryError'
    this.reason = reason
  }
}

export function isAuthBoundaryError(error: unknown): error is AuthBoundaryError {
  return error instanceof AuthBoundaryError
}

interface RequestCoordinator {
  readonly generation: number
  readonly principalId: string | null
  readonly recoverSession: () => Promise<AuthRecoveryResult>
  readonly markSessionExpired: () => void
  readonly currentGeneration: () => number
  readonly currentPrincipalId: () => string | null
  readonly isRecoveryRetryGeneration: (generation: number) => boolean
  readonly completeRecoveryGeneration: (generation: number) => void
}

function useRequestCoordinator(): RequestCoordinator {
  const {
    completeRecoveryGeneration,
    currentGeneration,
    currentPrincipalId,
    generation,
    isRecoveryRetryGeneration,
    markSessionExpired,
    recoverSession,
    session,
  } = useAuth()
  const principalId = session?.user_id ?? null
  return useMemo(
    () => ({
      generation,
      principalId,
      recoverSession,
      markSessionExpired,
      currentGeneration,
      currentPrincipalId,
      isRecoveryRetryGeneration,
      completeRecoveryGeneration,
    }),
    [
      completeRecoveryGeneration,
      currentGeneration,
      currentPrincipalId,
      generation,
      isRecoveryRetryGeneration,
      markSessionExpired,
      principalId,
      recoverSession,
    ],
  )
}

function isUnauthorized(error: unknown): boolean {
  return isApiRequestError(error) && error.status === 401
}

function isRecoverableCsrfFailure(error: unknown): boolean {
  return (
    isApiRequestError(error) &&
    error.status === 403 &&
    error.code === 'permission_denied'
  )
}

function assertCurrent(
  auth: RequestCoordinator,
  generation: number,
  principalId: string | null,
): void {
  if (auth.currentGeneration() !== generation) {
    throw new AuthBoundaryError('stale_generation')
  }
  if (auth.currentPrincipalId() !== principalId) {
    throw new AuthBoundaryError('principal_changed')
  }
}

function recoveryFailure(result: AuthRecoveryResult): AuthBoundaryError {
  return new AuthBoundaryError(
    result.status === 'unauthenticated' ? 'unauthenticated' : 'recovery_failed',
  )
}

async function protectedGet<T>(
  request: () => Promise<T>,
  auth: RequestCoordinator,
): Promise<T> {
  const originalGeneration = auth.generation
  const originalPrincipal = auth.principalId
  const isRecoveryRetry = auth.isRecoveryRetryGeneration(originalGeneration)
  try {
    const response = await request()
    assertCurrent(auth, originalGeneration, originalPrincipal)
    auth.completeRecoveryGeneration(originalGeneration)
    return response
  } catch (error) {
    if (!isUnauthorized(error)) {
      throw error
    }
    if (isRecoveryRetry) {
      auth.markSessionExpired()
      throw new AuthBoundaryError('unauthenticated')
    }
  }

  const recovery = await auth.recoverSession()
  if (recovery.status !== 'recovered') {
    throw recoveryFailure(recovery)
  }
  if (recovery.principalId !== originalPrincipal) {
    throw new AuthBoundaryError('principal_changed')
  }

  if (recovery.generation !== originalGeneration) {
    // ProtectedRoute keys its outlet by this generation. The canonical route
    // reconstruction is the single safe GET retry, while the old component's
    // request is stopped here.
    throw new AuthBoundaryError('stale_generation')
  }

  const retryGeneration = auth.currentGeneration()
  try {
    const response = await request()
    assertCurrent(auth, retryGeneration, originalPrincipal)
    return response
  } catch (error) {
    if (isUnauthorized(error)) {
      auth.markSessionExpired()
      throw new AuthBoundaryError('unauthenticated')
    }
    throw error
  }
}

async function protectedMutation<T>(
  request: () => Promise<T>,
  auth: RequestCoordinator,
): Promise<T> {
  const originalGeneration = auth.generation
  const originalPrincipal = auth.principalId
  try {
    const response = await request()
    assertCurrent(auth, originalGeneration, originalPrincipal)
    return response
  } catch (error) {
    if (!isUnauthorized(error) && !isRecoverableCsrfFailure(error)) {
      throw error
    }
  }

  const recovery = await auth.recoverSession()
  if (recovery.status !== 'recovered') {
    throw recoveryFailure(recovery)
  }
  if (recovery.principalId !== originalPrincipal) {
    throw new AuthBoundaryError('principal_changed')
  }

  // Destructive and create-style requests are never replayed here. Prompt 57
  // owns the later explicit same-key retry after a canonical GET/review.
  throw new AuthBoundaryError('session_recovered')
}

export function useAuthAwareBackupApi(api: BackupApi = defaultBackupApi): BackupApi {
  const boundary = useRequestCoordinator()
  return useMemo(() => {
    return {
      listBackupSets: (...args) => protectedGet(() => api.listBackupSets(...args), boundary),
      getBackupSet: (...args) => protectedGet(() => api.getBackupSet(...args), boundary),
      createBackupSet: (...args) => protectedMutation(() => api.createBackupSet(...args), boundary),
      listBackupSnapshots: (...args) => protectedGet(() => api.listBackupSnapshots(...args), boundary),
      getBackupSnapshot: (...args) => protectedGet(() => api.getBackupSnapshot(...args), boundary),
      listBackupSnapshotNodes: (...args) => protectedGet(() => api.listBackupSnapshotNodes(...args), boundary),
      getBackupRetentionPolicy: (...args) => protectedGet(() => api.getBackupRetentionPolicy(...args), boundary),
      configureBackupRetentionPolicy: (...args) => protectedMutation(() => api.configureBackupRetentionPolicy(...args), boundary),
      listBackupOperations: (...args) => protectedGet(() => api.listBackupOperations(...args), boundary),
      getBackupOperation: (...args) => protectedGet(() => api.getBackupOperation(...args), boundary),
      listBackupMaintenanceRuns: (...args) => protectedGet(() => api.listBackupMaintenanceRuns(...args), boundary),
      getBackupMaintenanceRun: (...args) => protectedGet(() => api.getBackupMaintenanceRun(...args), boundary),
      createBackupMaintenanceRun: (...args) => protectedMutation(() => api.createBackupMaintenanceRun(...args), boundary),
      advanceBackupMaintenanceRun: (...args) => protectedMutation(() => api.advanceBackupMaintenanceRun(...args), boundary),
      createBackupRestorePlan: (...args) => protectedMutation(() => api.createBackupRestorePlan(...args), boundary),
      getBackupRestorePlan: (...args) => protectedGet(() => api.getBackupRestorePlan(...args), boundary),
      executeBackupRestorePlan: (...args) => protectedMutation(() => api.executeBackupRestorePlan(...args), boundary),
      getBackupRestoreExecution: (...args) => protectedGet(() => api.getBackupRestoreExecution(...args), boundary),
      createBackupPrunePlan: (...args) => protectedMutation(() => api.createBackupPrunePlan(...args), boundary),
      getBackupPrunePlan: (...args) => protectedGet(() => api.getBackupPrunePlan(...args), boundary),
      executeBackupPrunePlan: (...args) => protectedMutation(() => api.executeBackupPrunePlan(...args), boundary),
      getBackupPruneExecution: (...args) => protectedGet(() => api.getBackupPruneExecution(...args), boundary),
    }
  }, [api, boundary])
}

export function useAuthAwareFileApi(api: FileMetadataApi = defaultFileApi): FileMetadataApi {
  const boundary = useRequestCoordinator()
  return useMemo(() => {
    return {
      listLibraries: (...args) => protectedGet(() => api.listLibraries(...args), boundary),
      listLibraryChildren: (...args) => protectedGet(() => api.listLibraryChildren(...args), boundary),
      getLiveNode: (...args) => protectedGet(() => api.getLiveNode(...args), boundary),
    }
  }, [api, boundary])
}
