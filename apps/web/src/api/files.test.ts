import { describe, expect, it, vi } from 'vitest'

import { ApiClient } from './client'
import { getLiveNode, listLibraries, listLibraryChildren } from './files'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

describe('live namespace API helpers', () => {
  it('uses bounded library and child-node pagination without physical fields', async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockImplementation(async (input) => {
      if (String(input).includes('/nodes')) {
        return jsonResponse({
          data: [{
            id: 'folder-1',
            type: 'node',
            revision: '7',
            attributes: {
              library_id: 'library-1',
              parent_id: 'root-1',
              name: 'Documents',
              kind: 'DIRECTORY',
              state: 'ACTIVE',
              created_at: '2026-08-31T00:00:00Z',
              updated_at: '2026-08-31T00:00:00Z',
            },
          }],
          page: { has_more: false },
          meta: { request_id: 'nodes' },
        })
      }
      return jsonResponse({
        data: [{
          id: 'library-1',
          type: 'library',
          revision: '3',
          attributes: {
            name: 'Personal files',
            root_node_id: 'root-1',
            status: 'ACTIVE',
            created_at: '2026-08-31T00:00:00Z',
            updated_at: '2026-08-31T00:00:00Z',
          },
        }],
        page: { has_more: false },
        meta: { request_id: 'libraries' },
      })
    })
    const client = new ApiClient({ fetchImpl })

    await listLibraries({ cursor: 'library next', limit: 50 }, client)
    await listLibraryChildren('library-1', { parentId: 'root-1', cursor: 'folder next', limit: 50 }, client)
    await getLiveNode('folder-1', undefined, client)

    expect(fetchImpl.mock.calls[0]?.[0]).toBe('/api/v1/libraries?cursor=library+next&limit=50')
    expect(fetchImpl.mock.calls[1]?.[0]).toBe(
      '/api/v1/libraries/library-1/nodes?parent_id=root-1&cursor=folder+next&limit=50',
    )
    expect(fetchImpl.mock.calls[2]?.[0]).toBe('/api/v1/nodes/folder-1')
    expect(JSON.stringify(fetchImpl.mock.calls[1]?.[0])).not.toMatch(/object_id|storage_key|retention_pin/)
  })
})
