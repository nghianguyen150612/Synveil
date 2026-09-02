import { apiClient, type ApiClient, type ApiRequestInit } from './client'
import type { DiagnosticOperation } from '../diagnostics/types'

export { createUuidV7 } from './uuid'

export type BackupSetState = 'CREATED' | 'ACTIVE' | 'DISABLED'
export type BackupSnapshotState = 'BUILDING' | 'COMPLETED' | 'FAILED' | 'EXPIRED'
export type BackupSnapshotNodeKind = 'FILE' | 'DIRECTORY'
export type BackupSnapshotNodeState = 'ACTIVE' | 'TRASHED'
export type BackupMaintenanceRunState =
  | 'CREATED'
  | 'SNAPSHOT_CAPTURED'
  | 'EXPIRY_PLANNED'
  | 'COMPLETED'
  | 'STALE'
export type BackupOperationKind = 'MAINTENANCE' | 'RESTORE' | 'PRUNE'
export type BackupOperationState =
  | BackupMaintenanceRunState
  | 'PLANNED'
  | 'EXECUTED'
export type BackupOperationPhase =
  | 'AWAITING_ADVANCE'
  | 'SNAPSHOT_CAPTURED'
  | 'EXPIRY_PLANNED'
  | 'AWAITING_EXECUTION'
  | 'AWAITING_CONFIRMATION'
  | 'COMPLETED'
  | 'STALE'
export type BackupOperationNextAction =
  | 'ADVANCE'
  | 'EXECUTE'
  | 'CREATE_NEW_RUN'
  | 'CREATE_NEW_PLAN'
  | 'NONE'
export type BackupRestorePlanState = 'PLANNED' | 'STALE' | 'EXECUTED'
export type BackupPrunePlanState = 'PLANNED' | 'STALE' | 'EXECUTED'
export type BackupPruneImpact =
  | 'RETAINED_BY_OTHER_REFERENCE'
  | 'WOULD_BECOME_UNREFERENCED'

export interface BackupPage {
  readonly next_cursor?: string
  readonly has_more: boolean
}

export interface BackupResponseMeta {
  readonly request_id: string
}

export interface BackupSet {
  readonly backup_set_id: string
  readonly name: string
  readonly library_id: string
  readonly state: BackupSetState
  readonly created_at: string
  readonly updated_at: string
}

export interface BackupSnapshot {
  readonly snapshot_id: string
  readonly backup_set_id: string
  readonly library_id: string
  readonly state: BackupSnapshotState
  readonly created_at: string
  readonly committed_at?: string
  readonly expired_at?: string
  readonly logical_node_count: string
  readonly content_reference_count: string
}

export interface BackupSnapshotNode {
  readonly snapshot_node_id: string
  readonly parent_snapshot_node_id?: string
  readonly name: string
  readonly kind: BackupSnapshotNodeKind
  readonly state: BackupSnapshotNodeState
  readonly revision: string
  readonly file_version_id?: string
  readonly byte_length?: string
  readonly sha256?: string
  readonly created_at: string
  readonly updated_at: string
}

export interface BackupRetentionPolicy {
  readonly data: {
    readonly policy_revision_id: string
    readonly backup_set_id: string
    readonly revision_number: string
    readonly keep_latest_completed: string
    readonly expire_after_seconds: string
    readonly created_at: string
  }
  readonly meta: BackupResponseMeta
}

export interface BackupMaintenanceRun {
  readonly maintenance_run_id: string
  readonly backup_set_id: string
  readonly state: BackupMaintenanceRunState
  readonly policy_revision_id: string
  readonly policy_revision_number: string
  readonly captured_snapshot_id?: string
  readonly expiry_plan_id?: string
  readonly expiry_execution_id?: string
  readonly created_at: string
  readonly snapshot_captured_at?: string
  readonly expiry_planned_at?: string
  readonly completed_at?: string
  readonly stale_at?: string
}

export interface BackupOperationProgress {
  readonly completed_steps: number
  readonly total_steps: number
}

export interface BackupOperationSummary {
  readonly operation_kind: BackupOperationKind
  readonly operation_id: string
  readonly backup_set_id: string
  readonly snapshot_id?: string
  readonly state: BackupOperationState
  readonly phase: BackupOperationPhase
  readonly progress: BackupOperationProgress
  readonly terminal: boolean
  readonly next_action: BackupOperationNextAction
  readonly created_at: string
  readonly last_transition_at: string
  readonly completed_at?: string
}

interface BackupOperationDetailBase extends BackupOperationSummary {
  readonly operation_kind: BackupOperationKind
}

export interface BackupMaintenanceOperation extends BackupOperationDetailBase {
  readonly operation_kind: 'MAINTENANCE'
  readonly maintenance_run_id: string
  readonly policy_revision_id: string
  readonly policy_revision_number: string
  readonly captured_snapshot_id?: string
  readonly expiry_plan_id?: string
  readonly expiry_execution_id?: string
  readonly snapshot_captured_at?: string
  readonly expiry_planned_at?: string
  readonly stale_at?: string
}

export interface BackupRestoreOperation extends BackupOperationDetailBase {
  readonly operation_kind: 'RESTORE'
  readonly restore_plan_id: string
  readonly snapshot_id: string
  readonly source_snapshot_id: string
  readonly target_library_id: string
  readonly target_parent_node_id: string
  readonly destination_name: string
  readonly planned_entry_count: string
  readonly planned_directory_count: string
  readonly planned_file_count: string
  readonly restore_execution_id?: string
  readonly executed_at?: string
  readonly stale_at?: string
}

export interface BackupPruneOperation extends BackupOperationDetailBase {
  readonly operation_kind: 'PRUNE'
  readonly prune_plan_id: string
  readonly snapshot_id: string
  readonly planned_pin_release_count: string
  readonly distinct_content_count: string
  readonly retained_by_other_reference_count: string
  readonly would_become_unreferenced_count: string
  readonly prune_execution_id?: string
  readonly released_content_reference_count?: string
  readonly gc_handoff_count?: string
  readonly executed_at?: string
  readonly stale_at?: string
}

export type BackupOperationDetail =
  | BackupMaintenanceOperation
  | BackupRestoreOperation
  | BackupPruneOperation

export interface BackupRequestOptions {
  readonly signal?: AbortSignal
}

export interface BackupSetListOptions extends BackupRequestOptions {
  readonly cursor?: string
  readonly limit?: number
}

export interface BackupSnapshotListOptions extends BackupRequestOptions {
  readonly cursor?: string
  readonly limit?: number
  readonly state?: BackupSnapshotState
}

export interface BackupSnapshotNodeListOptions extends BackupRequestOptions {
  readonly cursor?: string
  readonly limit?: number
  readonly parentId?: string
}

export interface BackupMaintenanceRunListOptions extends BackupRequestOptions {
  readonly cursor?: string
  readonly limit?: number
}

export interface BackupOperationListOptions extends BackupRequestOptions {
  readonly cursor?: string
  readonly limit?: number
  readonly kind?: BackupOperationKind
}

export interface BackupSetListResponse {
  readonly data: readonly BackupSet[]
  readonly page: BackupPage
  readonly meta: BackupResponseMeta
}

export interface BackupSetResponse {
  readonly data: BackupSet
  readonly meta: BackupResponseMeta
}

export interface CreateBackupSetRequest {
  readonly library_id: string
  readonly name: string
}

export interface ConfigureBackupRetentionPolicyRequest {
  readonly keep_latest_completed: number
  readonly expire_after_seconds: number
}

export interface BackupSnapshotListResponse {
  readonly data: readonly BackupSnapshot[]
  readonly page: BackupPage
  readonly meta: BackupResponseMeta
}

export interface BackupSnapshotResponse {
  readonly data: BackupSnapshot
  readonly meta: BackupResponseMeta
}

export interface BackupSnapshotNodeListResponse {
  readonly data: readonly BackupSnapshotNode[]
  readonly page: BackupPage
  readonly meta: BackupResponseMeta
}

export interface BackupMaintenanceRunListResponse {
  readonly data: readonly BackupMaintenanceRun[]
  readonly page: BackupPage
  readonly meta: BackupResponseMeta
}

export interface BackupMaintenanceRunResponse {
  readonly data: BackupMaintenanceRun
  readonly meta: BackupResponseMeta
}

export interface BackupOperationListResponse {
  readonly data: readonly BackupOperationSummary[]
  readonly page: BackupPage
  readonly meta: BackupResponseMeta
}

export interface BackupOperationResponse {
  readonly data: BackupOperationDetail
  readonly meta: BackupResponseMeta
}

export interface CreateBackupRestorePlanRequest {
  readonly target_library_id: string
  readonly target_parent_node_id: string
  readonly destination_name: string
}

export interface BackupRestorePlan {
  readonly restore_plan_id: string
  readonly source_snapshot_id: string
  readonly source_backup_set_id: string
  readonly target_library_id: string
  readonly target_parent_node_id: string
  readonly destination_name: string
  readonly state: BackupRestorePlanState
  readonly planned_entry_count: string
  readonly planned_directory_count: string
  readonly planned_file_count: string
  readonly created_at: string
  readonly stale_at?: string | null
}

export interface BackupRestoreExecution {
  readonly restore_execution_id: string
  readonly restore_plan_id: string
  readonly source_snapshot_id: string
  readonly target_library_id: string
  readonly created_directory_count: string
  readonly created_file_count: string
  readonly journal_start_sequence: string
  readonly journal_end_sequence: string
  readonly executed_at: string
}

export interface BackupRestorePlanResponse {
  readonly data: BackupRestorePlan
  readonly meta: BackupResponseMeta
}

export interface BackupRestoreExecutionResponse {
  readonly data: BackupRestoreExecution
  readonly meta: BackupResponseMeta
}

export interface BackupPruneImpactSummary {
  readonly impact: BackupPruneImpact
  readonly content_count: string
}

export interface BackupPrunePlan {
  readonly prune_plan_id: string
  readonly snapshot_id: string
  readonly backup_set_id: string
  readonly state: BackupPrunePlanState
  readonly entry_count: string
  readonly distinct_content_count: string
  readonly retained_by_other_reference_count: string
  readonly would_become_unreferenced_count: string
  readonly impact_summary: readonly BackupPruneImpactSummary[]
  readonly created_at: string
  readonly stale_at?: string
}

export interface BackupPruneExecution {
  readonly prune_execution_id: string
  readonly prune_plan_id: string
  readonly snapshot_id: string
  readonly backup_set_id: string
  readonly released_content_reference_count: string
  readonly distinct_content_count: string
  readonly retained_elsewhere_count: string
  readonly gc_handoff_count: string
  readonly executed_at: string
}

export interface BackupPrunePlanResponse {
  readonly data: BackupPrunePlan
  readonly meta: BackupResponseMeta
}

export interface BackupPruneExecutionResponse {
  readonly data: BackupPruneExecution
  readonly meta: BackupResponseMeta
}

function pathWithQuery(path: string, values: Readonly<Record<string, string | number | undefined>>): string {
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
  options: BackupRequestOptions | undefined,
  diagnosticOperation: DiagnosticOperation,
): ApiRequestInit {
  return {
    diagnosticOperation,
    ...(options?.signal ? { signal: options.signal } : {}),
  }
}

function backupSetPath(backupSetId: string): string {
  return `/api/v1/backups/sets/${encodeURIComponent(backupSetId)}`
}

function snapshotPath(snapshotId: string): string {
  return `/api/v1/backups/snapshots/${encodeURIComponent(snapshotId)}`
}

function restorePlanPath(restorePlanId: string): string {
  return `/api/v1/backups/restore-plans/${encodeURIComponent(restorePlanId)}`
}

function prunePlanPath(prunePlanId: string): string {
  return `/api/v1/backups/prune-plans/${encodeURIComponent(prunePlanId)}`
}

export function listBackupSets(
  options: BackupSetListOptions = {},
  client: ApiClient = apiClient,
): Promise<BackupSetListResponse> {
  return client.get<BackupSetListResponse>(
    pathWithQuery('/api/v1/backups/sets', {
      cursor: options.cursor,
      limit: options.limit,
    }),
    requestOptions(options, 'BACKUP_SET_LIST'),
  )
}

export function getBackupSet(
  backupSetId: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupSetResponse> {
  return client.get<BackupSetResponse>(
    backupSetPath(backupSetId),
    requestOptions(options, 'BACKUP_SET_GET'),
  )
}

export function createBackupSet(
  request: CreateBackupSetRequest,
  idempotencyKey: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupSetResponse> {
  return client.post<BackupSetResponse>('/api/v1/backups/sets', request, {
    ...requestOptions(options, 'BACKUP_SET_CREATE'),
    headers: { 'Idempotency-Key': idempotencyKey },
  })
}

export function listBackupSnapshots(
  backupSetId: string,
  options: BackupSnapshotListOptions = {},
  client: ApiClient = apiClient,
): Promise<BackupSnapshotListResponse> {
  return client.get<BackupSnapshotListResponse>(
    pathWithQuery(`${backupSetPath(backupSetId)}/snapshots`, {
      state: options.state,
      cursor: options.cursor,
      limit: options.limit,
    }),
    requestOptions(options, 'SNAPSHOT_LIST'),
  )
}

export function getBackupSnapshot(
  snapshotId: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupSnapshotResponse> {
  return client.get<BackupSnapshotResponse>(
    snapshotPath(snapshotId),
    requestOptions(options, 'SNAPSHOT_GET'),
  )
}

export function listBackupSnapshotNodes(
  snapshotId: string,
  options: BackupSnapshotNodeListOptions = {},
  client: ApiClient = apiClient,
): Promise<BackupSnapshotNodeListResponse> {
  return client.get<BackupSnapshotNodeListResponse>(
    pathWithQuery(`${snapshotPath(snapshotId)}/nodes`, {
      parent_id: options.parentId,
      cursor: options.cursor,
      limit: options.limit,
    }),
    requestOptions(options, 'SNAPSHOT_TREE'),
  )
}

export function getBackupRetentionPolicy(
  backupSetId: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupRetentionPolicy> {
  return client.get<BackupRetentionPolicy>(
    `${backupSetPath(backupSetId)}/retention-policy`,
    requestOptions(options, 'RETENTION_GET'),
  )
}

export function configureBackupRetentionPolicy(
  backupSetId: string,
  request: ConfigureBackupRetentionPolicyRequest,
  idempotencyKey: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupRetentionPolicy> {
  return client.post<BackupRetentionPolicy>(
    `${backupSetPath(backupSetId)}/retention-policy`,
    request,
    {
      ...requestOptions(options, 'RETENTION_UPDATE'),
      headers: { 'Idempotency-Key': idempotencyKey },
    },
  )
}

export function listBackupOperations(
  backupSetId: string,
  options: BackupOperationListOptions = {},
  client: ApiClient = apiClient,
): Promise<BackupOperationListResponse> {
  return client.get<BackupOperationListResponse>(
    pathWithQuery(`${backupSetPath(backupSetId)}/operations`, {
      kind: options.kind,
      cursor: options.cursor,
      limit: options.limit,
    }),
    requestOptions(options, 'OPERATION_LIST'),
  )
}

export function getBackupOperation(
  operationKind: BackupOperationKind,
  operationId: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupOperationResponse> {
  return client.get<BackupOperationResponse>(
    `/api/v1/backups/operations/${encodeURIComponent(operationKind)}/${encodeURIComponent(operationId)}`,
    requestOptions(options, 'OPERATION_GET'),
  )
}

export function listBackupMaintenanceRuns(
  backupSetId: string,
  options: BackupMaintenanceRunListOptions = {},
  client: ApiClient = apiClient,
): Promise<BackupMaintenanceRunListResponse> {
  return client.get<BackupMaintenanceRunListResponse>(
    pathWithQuery(`${backupSetPath(backupSetId)}/maintenance-runs`, {
      cursor: options.cursor,
      limit: options.limit,
    }),
    requestOptions(options, 'MAINTENANCE_LIST'),
  )
}

export function getBackupMaintenanceRun(
  runId: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupMaintenanceRunResponse> {
  return client.get<BackupMaintenanceRunResponse>(
    `/api/v1/backups/maintenance-runs/${encodeURIComponent(runId)}`,
    requestOptions(options, 'MAINTENANCE_GET'),
  )
}

export function createBackupMaintenanceRun(
  backupSetId: string,
  idempotencyKey: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupMaintenanceRunResponse> {
  return client.post<BackupMaintenanceRunResponse>(
    `${backupSetPath(backupSetId)}/maintenance-runs`,
    undefined,
    {
      ...requestOptions(options, 'MAINTENANCE_CREATE'),
      headers: { 'Idempotency-Key': idempotencyKey },
    },
  )
}

export function advanceBackupMaintenanceRun(
  runId: string,
  idempotencyKey: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupMaintenanceRunResponse> {
  return client.post<BackupMaintenanceRunResponse>(
    `/api/v1/backups/maintenance-runs/${encodeURIComponent(runId)}/advance`,
    undefined,
    {
      ...requestOptions(options, 'MAINTENANCE_ADVANCE'),
      headers: { 'Idempotency-Key': idempotencyKey },
    },
  )
}

export function createBackupRestorePlan(
  snapshotId: string,
  request: CreateBackupRestorePlanRequest,
  idempotencyKey: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupRestorePlanResponse> {
  return client.post<BackupRestorePlanResponse>(
    `${snapshotPath(snapshotId)}/restore-plans`,
    request,
    {
      ...requestOptions(options, 'RESTORE_PLAN_CREATE'),
      headers: { 'Idempotency-Key': idempotencyKey },
    },
  )
}

export function getBackupRestorePlan(
  restorePlanId: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupRestorePlanResponse> {
  return client.get<BackupRestorePlanResponse>(
    restorePlanPath(restorePlanId),
    requestOptions(options, 'RESTORE_PLAN_GET'),
  )
}

export function executeBackupRestorePlan(
  restorePlanId: string,
  idempotencyKey: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupRestoreExecutionResponse> {
  return client.post<BackupRestoreExecutionResponse>(
    `${restorePlanPath(restorePlanId)}/execute`,
    undefined,
    {
      ...requestOptions(options, 'RESTORE_EXECUTE'),
      headers: { 'Idempotency-Key': idempotencyKey },
    },
  )
}

export function getBackupRestoreExecution(
  executionId: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupRestoreExecutionResponse> {
  return client.get<BackupRestoreExecutionResponse>(
    `/api/v1/backups/restore-executions/${encodeURIComponent(executionId)}`,
    requestOptions(options, 'RESTORE_EXECUTION_GET'),
  )
}

export function createBackupPrunePlan(
  snapshotId: string,
  idempotencyKey: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupPrunePlanResponse> {
  return client.post<BackupPrunePlanResponse>(
    `${snapshotPath(snapshotId)}/prune-plans`,
    undefined,
    {
      ...requestOptions(options, 'PRUNE_PLAN_CREATE'),
      headers: { 'Idempotency-Key': idempotencyKey },
    },
  )
}

export function getBackupPrunePlan(
  prunePlanId: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupPrunePlanResponse> {
  return client.get<BackupPrunePlanResponse>(
    prunePlanPath(prunePlanId),
    requestOptions(options, 'PRUNE_PLAN_GET'),
  )
}

export function executeBackupPrunePlan(
  prunePlanId: string,
  confirmSnapshotId: string,
  idempotencyKey: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupPruneExecutionResponse> {
  return client.post<BackupPruneExecutionResponse>(
    `${prunePlanPath(prunePlanId)}/execute`,
    { confirm_snapshot_id: confirmSnapshotId },
    {
      ...requestOptions(options, 'PRUNE_EXECUTE'),
      headers: { 'Idempotency-Key': idempotencyKey },
    },
  )
}

export function getBackupPruneExecution(
  pruneExecutionId: string,
  options?: BackupRequestOptions,
  client: ApiClient = apiClient,
): Promise<BackupPruneExecutionResponse> {
  return client.get<BackupPruneExecutionResponse>(
    `/api/v1/backups/prune-executions/${encodeURIComponent(pruneExecutionId)}`,
    requestOptions(options, 'PRUNE_EXECUTION_GET'),
  )
}

export interface BackupApi {
  readonly listBackupSets: (
    options?: BackupSetListOptions,
  ) => Promise<BackupSetListResponse>
  readonly getBackupSet: (
    backupSetId: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupSetResponse>
  readonly createBackupSet: (
    request: CreateBackupSetRequest,
    idempotencyKey: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupSetResponse>
  readonly listBackupSnapshots: (
    backupSetId: string,
    options?: BackupSnapshotListOptions,
  ) => Promise<BackupSnapshotListResponse>
  readonly getBackupSnapshot: (
    snapshotId: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupSnapshotResponse>
  readonly listBackupSnapshotNodes: (
    snapshotId: string,
    options?: BackupSnapshotNodeListOptions,
  ) => Promise<BackupSnapshotNodeListResponse>
  readonly getBackupRetentionPolicy: (
    backupSetId: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupRetentionPolicy>
  readonly configureBackupRetentionPolicy: (
    backupSetId: string,
    request: ConfigureBackupRetentionPolicyRequest,
    idempotencyKey: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupRetentionPolicy>
  readonly listBackupOperations: (
    backupSetId: string,
    options?: BackupOperationListOptions,
  ) => Promise<BackupOperationListResponse>
  readonly getBackupOperation: (
    operationKind: BackupOperationKind,
    operationId: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupOperationResponse>
  readonly listBackupMaintenanceRuns: (
    backupSetId: string,
    options?: BackupMaintenanceRunListOptions,
  ) => Promise<BackupMaintenanceRunListResponse>
  readonly getBackupMaintenanceRun: (
    runId: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupMaintenanceRunResponse>
  readonly createBackupMaintenanceRun: (
    backupSetId: string,
    idempotencyKey: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupMaintenanceRunResponse>
  readonly advanceBackupMaintenanceRun: (
    runId: string,
    idempotencyKey: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupMaintenanceRunResponse>
  readonly createBackupRestorePlan: (
    snapshotId: string,
    request: CreateBackupRestorePlanRequest,
    idempotencyKey: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupRestorePlanResponse>
  readonly getBackupRestorePlan: (
    restorePlanId: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupRestorePlanResponse>
  readonly executeBackupRestorePlan: (
    restorePlanId: string,
    idempotencyKey: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupRestoreExecutionResponse>
  readonly getBackupRestoreExecution: (
    executionId: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupRestoreExecutionResponse>
  readonly createBackupPrunePlan: (
    snapshotId: string,
    idempotencyKey: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupPrunePlanResponse>
  readonly getBackupPrunePlan: (
    prunePlanId: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupPrunePlanResponse>
  readonly executeBackupPrunePlan: (
    prunePlanId: string,
    confirmSnapshotId: string,
    idempotencyKey: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupPruneExecutionResponse>
  readonly getBackupPruneExecution: (
    pruneExecutionId: string,
    options?: BackupRequestOptions,
  ) => Promise<BackupPruneExecutionResponse>
}

export const backupApi: BackupApi = {
  listBackupSets: (options) => listBackupSets(options),
  getBackupSet: (backupSetId, options) => getBackupSet(backupSetId, options),
  createBackupSet: (request, idempotencyKey, options) =>
    createBackupSet(request, idempotencyKey, options),
  listBackupSnapshots: (backupSetId, options) => listBackupSnapshots(backupSetId, options),
  getBackupSnapshot: (snapshotId, options) => getBackupSnapshot(snapshotId, options),
  listBackupSnapshotNodes: (snapshotId, options) => listBackupSnapshotNodes(snapshotId, options),
  getBackupRetentionPolicy: (backupSetId, options) => getBackupRetentionPolicy(backupSetId, options),
  configureBackupRetentionPolicy: (backupSetId, request, idempotencyKey, options) =>
    configureBackupRetentionPolicy(backupSetId, request, idempotencyKey, options),
  listBackupOperations: (backupSetId, options) => listBackupOperations(backupSetId, options),
  getBackupOperation: (operationKind, operationId, options) =>
    getBackupOperation(operationKind, operationId, options),
  listBackupMaintenanceRuns: (backupSetId, options) =>
    listBackupMaintenanceRuns(backupSetId, options),
  getBackupMaintenanceRun: (runId, options) => getBackupMaintenanceRun(runId, options),
  createBackupMaintenanceRun: (backupSetId, idempotencyKey, options) =>
    createBackupMaintenanceRun(backupSetId, idempotencyKey, options),
  advanceBackupMaintenanceRun: (runId, idempotencyKey, options) =>
    advanceBackupMaintenanceRun(runId, idempotencyKey, options),
  createBackupRestorePlan: (snapshotId, request, idempotencyKey, options) =>
    createBackupRestorePlan(snapshotId, request, idempotencyKey, options),
  getBackupRestorePlan: (restorePlanId, options) => getBackupRestorePlan(restorePlanId, options),
  executeBackupRestorePlan: (restorePlanId, idempotencyKey, options) =>
    executeBackupRestorePlan(restorePlanId, idempotencyKey, options),
  getBackupRestoreExecution: (executionId, options) => getBackupRestoreExecution(executionId, options),
  createBackupPrunePlan: (snapshotId, idempotencyKey, options) =>
    createBackupPrunePlan(snapshotId, idempotencyKey, options),
  getBackupPrunePlan: (prunePlanId, options) => getBackupPrunePlan(prunePlanId, options),
  executeBackupPrunePlan: (prunePlanId, confirmSnapshotId, idempotencyKey, options) =>
    executeBackupPrunePlan(prunePlanId, confirmSnapshotId, idempotencyKey, options),
  getBackupPruneExecution: (pruneExecutionId, options) =>
    getBackupPruneExecution(pruneExecutionId, options),
}
