import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { FileMetadataApi, LiveLibraryResource, LiveNodeResource } from '../../api/files'
import { RestoreDestinationPicker } from './RestoreDestinationPicker'

const libraryA: LiveLibraryResource = {
  id: 'library-a',
  type: 'library',
  revision: '1',
  attributes: {
    name: 'Library A',
    root_node_id: 'root-a',
    status: 'ACTIVE',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
}

const libraryB: LiveLibraryResource = {
  ...libraryA,
  id: 'library-b',
  attributes: { ...libraryA.attributes, name: 'Library B', root_node_id: 'root-b' },
}

function node(
  id: string,
  libraryId: string,
  name: string,
  kind: 'FILE' | 'DIRECTORY' = 'DIRECTORY',
): LiveNodeResource {
  return {
    id,
    type: 'node',
    revision: '1',
    attributes: {
      library_id: libraryId,
      name,
      kind,
      state: 'ACTIVE',
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    },
  }
}

function page<T>(data: readonly T[], hasMore = false, nextCursor?: string) {
  return {
    data,
    page: { has_more: hasMore, ...(nextCursor ? { next_cursor: nextCursor } : {}) },
    meta: { request_id: 'picker' },
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve
  })
  return { promise, resolve }
}

function renderPicker(fileApi: FileMetadataApi) {
  return render(
    <RestoreDestinationPicker fileApi={fileApi} onDestinationChange={vi.fn()} />,
  )
}

describe('restore destination picker', () => {
  it('shows files for context but exposes selection and navigation only for active directories', async () => {
    const directory = node('folder-1', libraryA.id, 'Documents')
    const file = node('file-1', libraryA.id, 'readme.txt', 'FILE')
    const fileApi: FileMetadataApi = {
      listLibraries: vi.fn().mockResolvedValue(page([libraryA])),
      listLibraryChildren: vi.fn().mockResolvedValue(page([directory, file])),
      getLiveNode: vi.fn(),
    }
    renderPicker(fileApi)

    expect(await screen.findByRole('button', { name: 'Open folder Documents' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open folder Documents' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Select as destination Documents' })).toBeInTheDocument()
    expect(screen.getByText('Not a destination folder')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Open folder readme.txt' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Select as destination readme.txt' })).not.toBeInTheDocument()
  })

  it('keeps library and folder pagination bounded', async () => {
    const firstLibrary = page([libraryA], true, 'library-next')
    const secondLibrary = page([libraryB])
    const firstChildren = page([node('folder-1', libraryA.id, 'Documents')], true, 'folder-next')
    const secondChildren = page([node('folder-2', libraryA.id, 'More')])
    const listLibraries = vi.fn()
      .mockResolvedValueOnce(firstLibrary)
      .mockResolvedValueOnce(secondLibrary)
    const listChildren = vi.fn()
      .mockResolvedValueOnce(firstChildren)
      .mockResolvedValueOnce(secondChildren)
    const fileApi: FileMetadataApi = {
      listLibraries,
      listLibraryChildren: listChildren,
      getLiveNode: vi.fn(),
    }
    renderPicker(fileApi)

    await screen.findByRole('button', { name: 'Load more libraries' })
    fireEvent.click(screen.getByRole('button', { name: 'Load more libraries' }))
    await waitFor(() => expect(listLibraries).toHaveBeenCalledTimes(2))
    expect(listLibraries.mock.calls[1]?.[0]).toMatchObject({ cursor: 'library-next', limit: 50 })

    await screen.findByRole('button', { name: 'Load more folders' })
    fireEvent.click(screen.getByRole('button', { name: 'Load more folders' }))
    await waitFor(() => expect(listChildren).toHaveBeenCalledTimes(2))
    expect(listChildren.mock.calls[1]?.[1]).toMatchObject({ cursor: 'folder-next', limit: 50 })
  })

  it('ignores a late library response after switching libraries', async () => {
    const oldLibraryRoot = deferred<ReturnType<typeof page<LiveNodeResource>>>()
    const newLibraryRoot = deferred<ReturnType<typeof page<LiveNodeResource>>>()
    const listChildren = vi.fn().mockImplementation((libraryId: string) =>
      libraryId === libraryA.id ? oldLibraryRoot.promise : newLibraryRoot.promise,
    )
    const fileApi: FileMetadataApi = {
      listLibraries: vi.fn().mockResolvedValue(page([libraryA, libraryB])),
      listLibraryChildren: listChildren,
      getLiveNode: vi.fn(),
    }
    renderPicker(fileApi)

    await screen.findByRole('option', { name: 'Library A' })
    fireEvent.change(screen.getByLabelText('Destination library'), { target: { value: libraryB.id } })
    newLibraryRoot.resolve(page([node('b-folder', libraryB.id, 'B folder')]))
    expect(await screen.findByRole('button', { name: 'Open folder B folder' })).toBeInTheDocument()

    oldLibraryRoot.resolve(page([node('a-folder', libraryA.id, 'A folder')]))
    await waitFor(() => expect(screen.queryByText('A folder')).not.toBeInTheDocument())
  })

  it('ignores late folder responses during rapid navigation', async () => {
    const rootChildren = [
      node('folder-a', libraryA.id, 'A'),
      node('folder-b', libraryA.id, 'B'),
      node('folder-c', libraryA.id, 'C'),
    ]
    const folderA = deferred<ReturnType<typeof page<LiveNodeResource>>>()
    const folderB = deferred<ReturnType<typeof page<LiveNodeResource>>>()
    const folderC = deferred<ReturnType<typeof page<LiveNodeResource>>>()
    const listChildren = vi.fn().mockImplementation((_libraryId: string, options?: { readonly parentId?: string }) => {
      if (!options?.parentId) return Promise.resolve(page(rootChildren))
      if (options?.parentId === 'folder-a') return folderA.promise
      if (options?.parentId === 'folder-b') return folderB.promise
      return folderC.promise
    })
    const fileApi: FileMetadataApi = {
      listLibraries: vi.fn().mockResolvedValue(page([libraryA])),
      listLibraryChildren: listChildren,
      getLiveNode: vi.fn(),
    }
    renderPicker(fileApi)

    await screen.findByRole('button', { name: 'Open folder A' })
    fireEvent.click(screen.getByRole('button', { name: 'Open folder A' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Library root' }))
    await screen.findByRole('button', { name: 'Open folder B' })
    fireEvent.click(screen.getByRole('button', { name: 'Open folder B' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Library root' }))
    await screen.findByRole('button', { name: 'Open folder C' })
    fireEvent.click(screen.getByRole('button', { name: 'Open folder C' }))

    folderA.resolve(page([node('a-child', libraryA.id, 'A child')]))
    folderB.resolve(page([node('b-child-late', libraryA.id, 'B child late')]))
    folderC.resolve(page([node('c-child', libraryA.id, 'C child')]))

    expect(await screen.findByRole('button', { name: 'Open folder C child' })).toBeInTheDocument()
    expect(screen.queryByText('A child')).not.toBeInTheDocument()
    expect(screen.queryByText('B child late')).not.toBeInTheDocument()
  })
})
