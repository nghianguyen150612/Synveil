// @vitest-environment node

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import { backupApi, type BackupApi } from './backups'

const __dirname = dirname(fileURLToPath(import.meta.url))
const OPENAPI_PATH = resolve(__dirname, '../../../../api/openapi.yaml')

/** Narrow any value to a plain non-array object without casting. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function loadOpenapi(): Record<string, unknown> {
  const raw = readFileSync(OPENAPI_PATH, 'utf-8')
  const parsed: unknown = yaml.load(raw)
  return isRecord(parsed) ? parsed : {}
}

const spec = loadOpenapi()

/** Get a typed object member, or undefined if absent/not an object. */
function asRecord(value: unknown): Record<string, unknown> | undefined {
  return isRecord(value) ? value : undefined
}

function getPaths(): Record<string, Record<string, unknown>> {
  const paths = asRecord(spec['paths'])
  const result: Record<string, Record<string, unknown>> = {}
  if (paths) {
    for (const [key, value] of Object.entries(paths)) {
      const pathObj = asRecord(value)
      if (pathObj) {
        result[key] = pathObj
      }
    }
  }
  return result
}

function getMethod(pathObj: Record<string, unknown>, method: string): Record<string, unknown> | undefined {
  const operation = pathObj[method.toLowerCase()]
  return asRecord(operation)
}

function getRequestSchemaRef(operation: Record<string, unknown>): string | undefined {
  const body = asRecord(operation['requestBody'])
  if (!body) return undefined
  const content = asRecord(body['content'])
  if (!content) return undefined
  const json = asRecord(content['application/json'])
  if (!json) return undefined
  const schema = asRecord(json['schema'])
  if (!schema) return undefined
  const ref = schema['$ref']
  return typeof ref === 'string' ? ref : undefined
}

function getProperties(schema: Record<string, unknown>): Record<string, unknown> {
  return asRecord(schema['properties']) ?? {}
}

function resolveSchema(ref: string): Record<string, unknown> | undefined {
  const parts = ref.replace('#/', '').split('/')
  let current: unknown = spec
  for (const part of parts) {
    if (!isRecord(current)) return undefined
    current = current[part]
  }
  return isRecord(current) ? current : undefined
}

function getSchemas(): Record<string, unknown> {
  const components = asRecord(spec['components'])
  if (!components) return {}
  const schemas = asRecord(components['schemas'])
  return schemas ?? {}
}

function operationParameterNames(operation: Record<string, unknown>): string[] {
  const parameters = Array.isArray(operation.parameters) ? operation.parameters : []
  return parameters.flatMap((parameter) => {
    const raw = asRecord(parameter)
    if (!raw) return []
    const ref = raw.$ref
    const resolved = typeof ref === 'string' ? resolveSchema(ref) : raw
    return typeof resolved?.name === 'string' ? [resolved.name] : []
  })
}

function hasBrowserSession(operation: Record<string, unknown>): boolean {
  const security = Array.isArray(operation.security) ? operation.security : []
  return security.some((entry) => {
    const requirement = asRecord(entry)
    return requirement && Array.isArray(requirement.BrowserSession)
  })
}

interface ProductionBackupContract {
  readonly clientMethod: keyof BackupApi
  readonly method: 'GET' | 'POST'
  readonly path: string
  readonly pathParameters: readonly string[]
  readonly success: '200' | '201'
}

const PRODUCTION_BACKUP_CONTRACTS: readonly ProductionBackupContract[] = [
  { clientMethod: 'listBackupSets', method: 'GET', path: '/api/v1/backups/sets', pathParameters: [], success: '200' },
  { clientMethod: 'getBackupSet', method: 'GET', path: '/api/v1/backups/sets/{backup_set_id}', pathParameters: ['backup_set_id'], success: '200' },
  { clientMethod: 'createBackupSet', method: 'POST', path: '/api/v1/backups/sets', pathParameters: [], success: '201' },
  { clientMethod: 'listBackupSnapshots', method: 'GET', path: '/api/v1/backups/sets/{backup_set_id}/snapshots', pathParameters: ['backup_set_id'], success: '200' },
  { clientMethod: 'getBackupSnapshot', method: 'GET', path: '/api/v1/backups/snapshots/{snapshot_id}', pathParameters: ['snapshot_id'], success: '200' },
  { clientMethod: 'listBackupSnapshotNodes', method: 'GET', path: '/api/v1/backups/snapshots/{snapshot_id}/nodes', pathParameters: ['snapshot_id'], success: '200' },
  { clientMethod: 'getBackupRetentionPolicy', method: 'GET', path: '/api/v1/backups/sets/{backup_set_id}/retention-policy', pathParameters: ['backup_set_id'], success: '200' },
  { clientMethod: 'configureBackupRetentionPolicy', method: 'POST', path: '/api/v1/backups/sets/{backup_set_id}/retention-policy', pathParameters: ['backup_set_id'], success: '201' },
  { clientMethod: 'listBackupOperations', method: 'GET', path: '/api/v1/backups/sets/{backup_set_id}/operations', pathParameters: ['backup_set_id'], success: '200' },
  { clientMethod: 'getBackupOperation', method: 'GET', path: '/api/v1/backups/operations/{operation_kind}/{operation_id}', pathParameters: ['operation_kind', 'operation_id'], success: '200' },
  { clientMethod: 'listBackupMaintenanceRuns', method: 'GET', path: '/api/v1/backups/sets/{backup_set_id}/maintenance-runs', pathParameters: ['backup_set_id'], success: '200' },
  { clientMethod: 'getBackupMaintenanceRun', method: 'GET', path: '/api/v1/backups/maintenance-runs/{run_id}', pathParameters: ['run_id'], success: '200' },
  { clientMethod: 'createBackupMaintenanceRun', method: 'POST', path: '/api/v1/backups/sets/{backup_set_id}/maintenance-runs', pathParameters: ['backup_set_id'], success: '201' },
  { clientMethod: 'advanceBackupMaintenanceRun', method: 'POST', path: '/api/v1/backups/maintenance-runs/{run_id}/advance', pathParameters: ['run_id'], success: '200' },
  { clientMethod: 'createBackupRestorePlan', method: 'POST', path: '/api/v1/backups/snapshots/{snapshot_id}/restore-plans', pathParameters: ['snapshot_id'], success: '201' },
  { clientMethod: 'getBackupRestorePlan', method: 'GET', path: '/api/v1/backups/restore-plans/{restore_plan_id}', pathParameters: ['restore_plan_id'], success: '200' },
  { clientMethod: 'executeBackupRestorePlan', method: 'POST', path: '/api/v1/backups/restore-plans/{restore_plan_id}/execute', pathParameters: ['restore_plan_id'], success: '200' },
  { clientMethod: 'getBackupRestoreExecution', method: 'GET', path: '/api/v1/backups/restore-executions/{restore_execution_id}', pathParameters: ['restore_execution_id'], success: '200' },
  { clientMethod: 'createBackupPrunePlan', method: 'POST', path: '/api/v1/backups/snapshots/{snapshot_id}/prune-plans', pathParameters: ['snapshot_id'], success: '201' },
  { clientMethod: 'getBackupPrunePlan', method: 'GET', path: '/api/v1/backups/prune-plans/{prune_plan_id}', pathParameters: ['prune_plan_id'], success: '200' },
  { clientMethod: 'executeBackupPrunePlan', method: 'POST', path: '/api/v1/backups/prune-plans/{prune_plan_id}/execute', pathParameters: ['prune_plan_id'], success: '200' },
  { clientMethod: 'getBackupPruneExecution', method: 'GET', path: '/api/v1/backups/prune-executions/{prune_execution_id}', pathParameters: ['prune_execution_id'], success: '200' },
]

function resolveEnum(prop: Record<string, unknown>): string[] {
  if (Array.isArray(prop['enum'])) {
    return prop['enum'].filter((value): value is string => typeof value === 'string')
  }
  const ref = prop['$ref']
  if (typeof ref === 'string') {
    const resolved = resolveSchema(ref)
    if (resolved && Array.isArray(resolved['enum'])) {
      return resolved['enum'].filter((value): value is string => typeof value === 'string')
    }
  }
  return []
}

describe('OpenAPI contract — structural validation', () => {
  const schemas = getSchemas()

  it('OpenAPI file loads and contains required top-level fields', () => {
    expect(spec).toMatchObject({
      openapi: expect.any(String),
      info: expect.any(Object),
      paths: expect.any(Object),
      components: expect.any(Object),
    })
  })

  it('BackupSnapshot state enum is exactly COMPLETED, BUILDING, FAILED, EXPIRED', () => {
    const schema = schemas.BackupSnapshot as Record<string, unknown>
    expect(schema).toBeDefined()
    const stateProp = getProperties(schema).state as Record<string, unknown>
    const values = resolveEnum(stateProp)
    expect(values).toEqual(expect.arrayContaining(['COMPLETED', 'BUILDING', 'FAILED', 'EXPIRED']))
    expect(values).toHaveLength(4)
  })

  it('BackupRestorePlan state enum contains PLANNED, STALE, EXECUTED (no CREATED)', () => {
    const schema = schemas.BackupRestorePlan as Record<string, unknown>
    expect(schema).toBeDefined()
    const stateProp = getProperties(schema).state as Record<string, unknown>
    const values = resolveEnum(stateProp)
    expect(values).toContain('PLANNED')
    expect(values).toContain('STALE')
    expect(values).toContain('EXECUTED')
    expect(values).not.toContain('CREATED')
  })

  it('BackupPrunePlan state enum contains PLANNED, STALE, EXECUTED (no CREATED)', () => {
    const schema = schemas.BackupPrunePlan as Record<string, unknown>
    expect(schema).toBeDefined()
    const stateProp = getProperties(schema).state as Record<string, unknown>
    const values = resolveEnum(stateProp)
    expect(values).toContain('PLANNED')
    expect(values).toContain('STALE')
    expect(values).toContain('EXECUTED')
    expect(values).not.toContain('CREATED')
  })

  it('BackupOperationSummary operation_kind enum is exactly MAINTENANCE, RESTORE, PRUNE', () => {
    const schema = schemas.BackupOperationSummary as Record<string, unknown>
    expect(schema).toBeDefined()
    const kindProp = getProperties(schema).operation_kind as Record<string, unknown>
    const values = resolveEnum(kindProp)
    expect(values).toEqual(expect.arrayContaining(['MAINTENANCE', 'RESTORE', 'PRUNE']))
    expect(values).toHaveLength(3)
  })
})

describe('OpenAPI contract — route path existence', () => {
  const paths = getPaths()

  it('GET /api/v1/backups/sets exists', () => expect(paths['/api/v1/backups/sets']).toBeDefined())
  it('GET /api/v1/backups/sets/{backup_set_id} exists', () => expect(paths['/api/v1/backups/sets/{backup_set_id}']).toBeDefined())
  it('POST /api/v1/backups/sets exists', () => expect(paths['/api/v1/backups/sets']).toBeDefined())
  it('GET /api/v1/backups/sets/{backup_set_id}/retention-policy exists', () => expect(paths['/api/v1/backups/sets/{backup_set_id}/retention-policy']).toBeDefined())
  it('POST /api/v1/backups/sets/{backup_set_id}/retention-policy exists', () => expect(paths['/api/v1/backups/sets/{backup_set_id}/retention-policy']).toBeDefined())
  it('GET /api/v1/backups/sets/{backup_set_id}/snapshots exists', () => expect(paths['/api/v1/backups/sets/{backup_set_id}/snapshots']).toBeDefined())
  it('/api/v1/backups/snapshots/{snapshot_id}/restore-plans exists', () => expect(paths['/api/v1/backups/snapshots/{snapshot_id}/restore-plans']).toBeDefined())
  it('POST /api/v1/backups/snapshots/{snapshot_id}/restore-plans exists', () => expect(paths['/api/v1/backups/snapshots/{snapshot_id}/restore-plans']).toBeDefined())
  it('POST /api/v1/backups/restore-plans/{restore_plan_id}/execute exists', () => expect(paths['/api/v1/backups/restore-plans/{restore_plan_id}/execute']).toBeDefined())
  it('/api/v1/backups/snapshots/{snapshot_id}/prune-plans exists', () => expect(paths['/api/v1/backups/snapshots/{snapshot_id}/prune-plans']).toBeDefined())
  it('POST /api/v1/backups/prune-plans/{prune_plan_id}/execute exists', () => expect(paths['/api/v1/backups/prune-plans/{prune_plan_id}/execute']).toBeDefined())
})

describe('OpenAPI contract — request body structural checks', () => {
  it('RESTORE create request body schema has target_library_id, target_parent_node_id, destination_name', () => {
    const restorePath = getPaths()['/api/v1/backups/snapshots/{snapshot_id}/restore-plans']
    const postOp = getMethod(restorePath!, 'POST')
    const schemaRef = getRequestSchemaRef(postOp!)
    expect(schemaRef).toBeDefined()
    const schema = resolveSchema(schemaRef!)
    const props = Object.keys(schema?.properties ?? {})
    expect(props).toContain('target_library_id')
    expect(props).toContain('target_parent_node_id')
    expect(props).toContain('destination_name')
  })

  it('prune execute request body schema contains confirm_snapshot_id', () => {
    const pruneExecutePath = getPaths()['/api/v1/backups/prune-plans/{prune_plan_id}/execute']
    const postOp = getMethod(pruneExecutePath!, 'POST')
    const schemaRef = getRequestSchemaRef(postOp!)
    expect(schemaRef).toBeDefined()
    const schema = resolveSchema(schemaRef!)
    const props = Object.keys(schema?.properties ?? {})
    expect(props).toContain('confirm_snapshot_id')
  })

  it('prune execute request body schema does NOT contain force, delete_objects, physical_delete', () => {
    const pruneExecutePath = getPaths()['/api/v1/backups/prune-plans/{prune_plan_id}/execute']
    const postOp = getMethod(pruneExecutePath!, 'POST')
    const schemaRef = getRequestSchemaRef(postOp!)
    const schema = resolveSchema(schemaRef!)
    const props = Object.keys(schema?.properties ?? {})
    expect(props).not.toContain('force')
    expect(props).not.toContain('delete_objects')
    expect(props).not.toContain('physical_delete')
  })
})

describe('OpenAPI contract — security scheme', () => {
  it('GET /api/v1/backups/sets has BrowserSession security', () => {
    const getPath = getPaths()['/api/v1/backups/sets']
    const operation = getMethod(getPath!, 'GET')
    expect(operation).toBeDefined()
    const security = (operation!.security ?? []) as Array<Record<string, unknown>>
    expect(security).toEqual(expect.arrayContaining([expect.objectContaining({ BrowserSession: [] })]))
  })

  it('POST /api/v1/backups/sets has CsrfHeader and BackupIdempotencyKey', () => {
    const postPath = getPaths()['/api/v1/backups/sets']
    const postOp = getMethod(postPath!, 'POST')
    expect(postOp).toBeDefined()
    const params = (postOp!.parameters ?? []) as Array<Record<string, unknown>>
    const refs = params.map(p => p['$ref'] ?? '').join(',')
    expect(refs).toContain('CsrfHeader')
    expect(refs).toContain('BackupIdempotencyKey')
  })
})

describe('OpenAPI contract — execution receipt GET routes', () => {
  it.each([
    {
      label: 'restore execution receipt',
      path: '/api/v1/backups/restore-executions/{restore_execution_id}',
      parameter: 'restore_execution_id',
    },
    {
      label: 'prune execution receipt',
      path: '/api/v1/backups/prune-executions/{prune_execution_id}',
      parameter: 'prune_execution_id',
    },
  ])('covers GET $label with its authoritative path, security, parameter, and success response', ({ path, parameter }) => {
    const pathItem = getPaths()[path]
    expect(pathItem).toBeDefined()
    const operation = getMethod(pathItem!, 'GET')
    expect(operation).toBeDefined()
    expect(operationParameterNames(operation!)).toContain(parameter)
    expect(hasBrowserSession(operation!)).toBe(true)
    expect(asRecord(operation!.responses)?.['200']).toBeDefined()
  })
})

describe('OpenAPI contract — complete production BackupApi coverage', () => {
  it('has one contract checklist row for every exported production backup API method', () => {
    expect(PRODUCTION_BACKUP_CONTRACTS.map((entry) => entry.clientMethod).sort())
      .toEqual(Object.keys(backupApi).sort())
  })

  it.each(PRODUCTION_BACKUP_CONTRACTS)(
    '$clientMethod is covered by $method $path',
    ({ method, path, pathParameters, success }) => {
      const pathItem = getPaths()[path]
      expect(pathItem).toBeDefined()
      const operation = getMethod(pathItem!, method)
      expect(operation).toBeDefined()
      expect(hasBrowserSession(operation!)).toBe(true)
      expect(operationParameterNames(operation!)).toEqual(expect.arrayContaining([...pathParameters]))
      expect(asRecord(operation!.responses)?.[success]).toBeDefined()
      if (method === 'POST') {
        expect(operationParameterNames(operation!)).toEqual(expect.arrayContaining([
          'X-CSRF-Token',
          'Idempotency-Key',
        ]))
      }
    },
  )
})
