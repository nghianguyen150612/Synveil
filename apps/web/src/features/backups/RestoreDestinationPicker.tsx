import { useCallback, useEffect, useRef, useState } from 'react'

import { isApiRequestError } from '../../api/errors'
import type {
  FileMetadataApi,
  LiveLibraryResource,
  LiveNodeResource,
} from '../../api/files'

const DIRECTORY_PAGE_LIMIT = 50
const LIBRARY_PAGE_LIMIT = 50

type LoadStatus = 'idle' | 'loading' | 'success' | 'error'

interface CollectionState<T> {
  readonly status: LoadStatus
  readonly items: readonly T[]
  readonly nextCursor?: string
  readonly hasMore: boolean
  readonly error?: unknown
}

interface BrowserFolder {
  readonly nodeId: string
  readonly name: string
  readonly path: string
}

interface FolderBreadcrumb {
  readonly nodeId: string
  readonly name: string
}

interface FolderState {
  readonly status: LoadStatus
  readonly libraryId?: string
  readonly parentNodeId?: string
  readonly breadcrumbs: readonly FolderBreadcrumb[]
  readonly currentFolder?: BrowserFolder
  readonly items: readonly LiveNodeResource[]
  readonly nextCursor?: string
  readonly hasMore: boolean
  readonly error?: unknown
}

export interface DestinationSelection {
  readonly targetLibraryId: string
  readonly targetParentNodeId: string
  readonly libraryName: string
  readonly parentName: string
  readonly parentPath: string
}

export interface RestoreDestinationPickerProps {
  readonly fileApi: FileMetadataApi
  readonly disabled?: boolean
  readonly onDestinationChange: (selection: DestinationSelection | undefined) => void
  readonly onUnauthorized?: () => void
}

function initialFolderState(): FolderState {
  return {
    status: 'idle',
    breadcrumbs: [],
    items: [],
    hasMore: false,
  }
}

function isAbortError(error: unknown): boolean {
  return typeof DOMException !== 'undefined' && error instanceof DOMException && error.name === 'AbortError'
}

function namespaceErrorMessage(error: unknown, subject: string): string {
  if (isApiRequestError(error)) {
    if (error.status === 401) {
      return 'Your session has expired. Sign in again before choosing a destination.'
    }
    if (error.status === 404) {
      return 'This destination is no longer available.'
    }
    if (error.status === 503 || error.status >= 500) {
      return 'The live namespace is temporarily unavailable. Please try again.'
    }
  }
  return `We could not load ${subject}. Please try again.`
}

function rootFolder(library: LiveLibraryResource): BrowserFolder {
  return {
    nodeId: library.attributes.root_node_id,
    name: 'Library root',
    path: 'Library root',
  }
}

function selectionForFolder(
  library: LiveLibraryResource,
  folder: BrowserFolder,
): DestinationSelection {
  return {
    targetLibraryId: library.id,
    targetParentNodeId: folder.nodeId,
    libraryName: library.attributes.name,
    parentName: folder.name,
    parentPath: folder.path,
  }
}

export function RestoreDestinationPicker({
  fileApi,
  disabled = false,
  onDestinationChange,
  onUnauthorized,
}: RestoreDestinationPickerProps) {
  const [libraries, setLibraries] = useState<CollectionState<LiveLibraryResource>>({
    status: 'loading',
    items: [],
    hasMore: false,
  })
  const [selectedLibraryId, setSelectedLibraryId] = useState<string>()
  const [folder, setFolder] = useState<FolderState>(initialFolderState)
  const [selectedDestination, setSelectedDestination] = useState<DestinationSelection>()
  const [librariesLoadingMore, setLibrariesLoadingMore] = useState(false)
  const [foldersLoadingMore, setFoldersLoadingMore] = useState(false)
  const [libraryRefreshVersion, setLibraryRefreshVersion] = useState(0)

  const mounted = useRef(true)
  const libraryRequestId = useRef(0)
  const folderRequestId = useRef(0)
  const folderController = useRef<AbortController | undefined>(undefined)
  const selectedLibraryIdRef = useRef<string | undefined>(undefined)

  const selectedLibrary = libraries.items.find((library) => library.id === selectedLibraryId)

  useEffect(() => {
    return () => {
      mounted.current = false
      folderController.current?.abort()
    }
  }, [])

  useEffect(() => {
    const controller = new AbortController()
    const requestId = libraryRequestId.current + 1
    libraryRequestId.current = requestId
    setLibraries({ status: 'loading', items: [], hasMore: false })
    selectedLibraryIdRef.current = undefined
    setSelectedLibraryId(undefined)
    setSelectedDestination(undefined)
    onDestinationChange(undefined)

    void fileApi
      .listLibraries({ limit: LIBRARY_PAGE_LIMIT, signal: controller.signal })
      .then((response) => {
        if (
          !mounted.current ||
          controller.signal.aborted ||
          libraryRequestId.current !== requestId
        ) {
          return
        }
        const items = [...response.data]
        setLibraries({
          status: 'success',
          items,
          nextCursor: response.page.next_cursor,
          hasMore: response.page.has_more,
        })
        const firstLibrary = items[0]
        if (firstLibrary) {
          selectedLibraryIdRef.current = firstLibrary.id
          setSelectedLibraryId(firstLibrary.id)
        }
      })
      .catch((error: unknown) => {
        if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
          return
        }
        setLibraries({ status: 'error', items: [], hasMore: false, error })
        if (isApiRequestError(error) && error.status === 401) {
          onUnauthorized?.()
        }
      })

    return () => controller.abort()
  }, [fileApi, libraryRefreshVersion, onDestinationChange, onUnauthorized])

  const loadFolder = useCallback(
    async ({
      libraryId,
      parentNodeId,
      breadcrumbs,
      currentFolder,
      cursor,
      append,
    }: {
      readonly libraryId: string
      readonly parentNodeId?: string
      readonly breadcrumbs: readonly FolderBreadcrumb[]
      readonly currentFolder: BrowserFolder
      readonly cursor?: string
      readonly append: boolean
    }) => {
      folderController.current?.abort()
      const controller = new AbortController()
      folderController.current = controller
      const requestId = folderRequestId.current + 1
      folderRequestId.current = requestId
      setFolder((current) => ({
        status: 'loading',
        libraryId,
        parentNodeId,
        breadcrumbs,
        currentFolder,
        items: append ? current.items : [],
        hasMore: append ? current.hasMore : false,
      }))

      try {
        const response = await fileApi.listLibraryChildren(libraryId, {
          parentId: parentNodeId,
          cursor,
          limit: DIRECTORY_PAGE_LIMIT,
          signal: controller.signal,
        })
        if (
          !mounted.current ||
          controller.signal.aborted ||
          folderRequestId.current !== requestId ||
          selectedLibraryIdRef.current !== libraryId
        ) {
          return
        }
        setFolder((current) => ({
          status: 'success',
          libraryId,
          parentNodeId,
          breadcrumbs,
          currentFolder,
          items: append ? [...current.items, ...response.data] : [...response.data],
          nextCursor: response.page.next_cursor,
          hasMore: response.page.has_more,
        }))
      } catch (error) {
        if (
          !mounted.current ||
          controller.signal.aborted ||
          isAbortError(error) ||
          folderRequestId.current !== requestId ||
          selectedLibraryIdRef.current !== libraryId
        ) {
          return
        }
        setFolder({
          status: 'error',
          libraryId,
          parentNodeId,
          breadcrumbs,
          currentFolder,
          items: [],
          hasMore: false,
          error,
        })
        if (isApiRequestError(error) && error.status === 401) {
          onUnauthorized?.()
        }
      }
    },
    [fileApi, onUnauthorized],
  )

  useEffect(() => {
    if (!selectedLibrary) {
      folderController.current?.abort()
      folderRequestId.current += 1
      setFolder(initialFolderState())
      setSelectedDestination(undefined)
      onDestinationChange(undefined)
      return
    }

    const root = rootFolder(selectedLibrary)
    const rootSelection = selectionForFolder(selectedLibrary, root)
    setSelectedDestination(rootSelection)
    onDestinationChange(rootSelection)
    void loadFolder({
      libraryId: selectedLibrary.id,
      breadcrumbs: [{ nodeId: root.nodeId, name: root.name }],
      currentFolder: root,
      append: false,
    })
  }, [loadFolder, onDestinationChange, selectedLibrary])

  function selectLibrary(nextLibraryId: string) {
    if (nextLibraryId === selectedLibraryId) {
      return
    }
    selectedLibraryIdRef.current = nextLibraryId
    folderController.current?.abort()
    folderRequestId.current += 1
    setFolder(initialFolderState())
    setSelectedDestination(undefined)
    onDestinationChange(undefined)
    setSelectedLibraryId(nextLibraryId)
  }

  function chooseFolder(nextFolder: BrowserFolder) {
    if (!selectedLibrary || disabled) {
      return
    }
    const nextSelection = selectionForFolder(selectedLibrary, nextFolder)
    setSelectedDestination(nextSelection)
    onDestinationChange(nextSelection)
  }

  function openFolder(node: LiveNodeResource) {
    if (
      !selectedLibrary ||
      disabled ||
      node.attributes.kind !== 'DIRECTORY' ||
      node.attributes.state !== 'ACTIVE'
    ) {
      return
    }
    const nextFolder: BrowserFolder = {
      nodeId: node.id,
      name: node.attributes.name,
      path: [...folder.breadcrumbs.map((breadcrumb) => breadcrumb.name), node.attributes.name].join(' / '),
    }
    void loadFolder({
      libraryId: selectedLibrary.id,
      parentNodeId: node.id,
      breadcrumbs: [...folder.breadcrumbs, { nodeId: node.id, name: node.attributes.name }],
      currentFolder: nextFolder,
      append: false,
    })
  }

  function goToBreadcrumb(index: number) {
    if (!selectedLibrary || disabled) {
      return
    }
    const breadcrumb = folder.breadcrumbs[index]
    if (!breadcrumb) {
      return
    }
    const nextFolder: BrowserFolder = {
      nodeId: breadcrumb.nodeId,
      name: breadcrumb.name,
      path: folder.breadcrumbs
        .slice(0, index + 1)
        .map((item) => item.name)
        .join(' / '),
    }
    void loadFolder({
      libraryId: selectedLibrary.id,
      parentNodeId: index === 0 ? undefined : breadcrumb.nodeId,
      breadcrumbs: folder.breadcrumbs.slice(0, index + 1),
      currentFolder: nextFolder,
      append: false,
    })
  }

  async function loadMoreLibraries() {
    if (!libraries.nextCursor || !libraries.hasMore || librariesLoadingMore || disabled) {
      return
    }
    const requestId = libraryRequestId.current + 1
    libraryRequestId.current = requestId
    const controller = new AbortController()
    setLibrariesLoadingMore(true)
    try {
      const response = await fileApi.listLibraries({
        cursor: libraries.nextCursor,
        limit: LIBRARY_PAGE_LIMIT,
        signal: controller.signal,
      })
      if (!mounted.current || controller.signal.aborted || libraryRequestId.current !== requestId) {
        return
      }
      setLibraries((current) => ({
        status: 'success',
        items: [...current.items, ...response.data],
        nextCursor: response.page.next_cursor,
        hasMore: response.page.has_more,
      }))
    } catch (error) {
      if (!mounted.current || controller.signal.aborted || isAbortError(error)) {
        return
      }
      setLibraries((current) => ({ ...current, error }))
      if (isApiRequestError(error) && error.status === 401) {
        onUnauthorized?.()
      }
    } finally {
      if (mounted.current) {
        setLibrariesLoadingMore(false)
      }
    }
  }

  async function loadMoreFolders() {
    if (
      !selectedLibrary ||
      !folder.nextCursor ||
      !folder.hasMore ||
      foldersLoadingMore ||
      disabled ||
      !folder.currentFolder
    ) {
      return
    }
    setFoldersLoadingMore(true)
    try {
      await loadFolder({
        libraryId: selectedLibrary.id,
        parentNodeId: folder.parentNodeId,
        breadcrumbs: folder.breadcrumbs,
        currentFolder: folder.currentFolder,
        cursor: folder.nextCursor,
        append: true,
      })
    } finally {
      if (mounted.current) {
        setFoldersLoadingMore(false)
      }
    }
  }

  const currentFolder = folder.currentFolder

  return (
    <section className="restore-destination-picker" aria-labelledby="restore-destination-title">
      <div className="restore-section-heading">
        <div>
          <p className="eyebrow">Step 1</p>
          <h2 id="restore-destination-title">Choose a destination</h2>
          <p>
            Select a library and a folder visible to your account. The server checks the destination
            again when the plan is created.
          </p>
        </div>
      </div>

      <div className="restore-destination-controls">
        <div className="field">
          <label htmlFor="restore-library-select">Destination library</label>
          <select
            id="restore-library-select"
            value={selectedLibraryId ?? ''}
            onChange={(event) => selectLibrary(event.currentTarget.value)}
            disabled={disabled || libraries.status === 'loading' || libraries.items.length === 0}
          >
            <option value="" disabled>
              Choose a library
            </option>
            {libraries.items.map((library) => (
              <option value={library.id} key={library.id}>
                {library.attributes.name}
              </option>
            ))}
          </select>
        </div>

        {libraries.status === 'loading' && (
          <p className="restore-loading" role="status" aria-busy="true">
            Loading destination libraries…
          </p>
        )}
        {libraries.status === 'error' && (
          <div className="restore-inline-error" role="alert">
            <p>{namespaceErrorMessage(libraries.error, 'destination libraries')}</p>
            <button
              type="button"
              className="button--secondary"
              onClick={() => setLibraryRefreshVersion((current) => current + 1)}
            >
              Reload destination choices
            </button>
          </div>
        )}
        {libraries.status === 'success' && libraries.items.length === 0 && (
          <p className="restore-empty-copy">No destination libraries are available.</p>
        )}
        {libraries.status === 'success' && libraries.hasMore && (
          <button
            type="button"
            className="button--secondary restore-load-more"
            onClick={() => void loadMoreLibraries()}
            disabled={librariesLoadingMore || disabled}
          >
            {librariesLoadingMore ? 'Loading more libraries…' : 'Load more libraries'}
          </button>
        )}
      </div>

      {selectedLibrary && (
        <div className="restore-folder-browser" aria-labelledby="restore-folder-title">
          <div className="restore-folder-heading">
            <div>
              <p className="eyebrow">Live namespace</p>
              <h3 id="restore-folder-title">Choose a destination folder</h3>
            </div>
            <span className="restore-library-label">{selectedLibrary.attributes.name}</span>
          </div>

          <nav className="restore-breadcrumbs" aria-label="Destination folder breadcrumb">
            <ol>
              {folder.breadcrumbs.map((breadcrumb, index) => {
                const current = index === folder.breadcrumbs.length - 1
                return (
                  <li key={breadcrumb.nodeId}>
                    {current ? (
                      <span aria-current="page">{breadcrumb.name}</span>
                    ) : (
                      <button
                        type="button"
                        className="restore-breadcrumb-button"
                        onClick={() => goToBreadcrumb(index)}
                        disabled={disabled}
                      >
                        {breadcrumb.name}
                      </button>
                    )}
                  </li>
                )
              })}
            </ol>
          </nav>

          {currentFolder && (
            <div className="restore-current-folder">
              <div>
                <strong>{currentFolder.name}</strong>
                <span>{currentFolder.path}</span>
              </div>
              <button
                type="button"
                className="button--secondary"
                onClick={() => chooseFolder(currentFolder)}
                disabled={disabled}
              >
                Select this folder
              </button>
            </div>
          )}

          {folder.status === 'loading' && (
            <p className="restore-loading" role="status" aria-busy="true">
              Loading folders…
            </p>
          )}
          {folder.status === 'error' && (
            <div className="restore-inline-error" role="alert">
              <p>{namespaceErrorMessage(folder.error, 'folders')}</p>
              <button
                type="button"
                className="button--secondary"
                onClick={() => {
                  if (folder.currentFolder) {
                    void loadFolder({
                      libraryId: selectedLibrary.id,
                      parentNodeId: folder.parentNodeId,
                      breadcrumbs: folder.breadcrumbs,
                      currentFolder: folder.currentFolder,
                      append: false,
                    })
                  }
                }}
                disabled={disabled}
              >
                Try again
              </button>
            </div>
          )}
          {folder.status === 'success' && folder.items.length === 0 && (
            <p className="restore-empty-copy">This folder is empty.</p>
          )}
          {folder.status === 'success' && folder.items.length > 0 && (
            <>
              <p className="restore-browser-note">
                Files are shown for context. Only active directories can be selected as a destination
                folder.
              </p>
              <ul className="restore-folder-list">
                {folder.items.map((node) => {
                  const isDirectory =
                    node.attributes.kind === 'DIRECTORY' && node.attributes.state === 'ACTIVE'
                  return (
                    <li key={node.id}>
                      <div className="restore-folder-row">
                        <div className="restore-folder-name">
                          <span className="restore-folder-icon" aria-hidden="true">
                            {node.attributes.kind === 'DIRECTORY' ? '▸' : '□'}
                          </span>
                          <span>
                            <strong>{node.attributes.name}</strong>
                            <small>
                              {node.attributes.kind === 'DIRECTORY' ? 'Directory' : 'File'}
                              {node.attributes.state !== 'ACTIVE' ? ' · Not active' : ''}
                            </small>
                          </span>
                        </div>
                        {isDirectory ? (
                          <div className="restore-folder-actions">
                            <button
                              type="button"
                              className="button--secondary"
                              onClick={() => openFolder(node)}
                              disabled={disabled}
                            >
                              Open folder
                              <span className="visually-hidden"> {node.attributes.name}</span>
                            </button>
                            <button
                              type="button"
                              className="button--secondary"
                              onClick={() =>
                                chooseFolder({
                                  nodeId: node.id,
                                  name: node.attributes.name,
                                  path: [
                                    ...folder.breadcrumbs.map((breadcrumb) => breadcrumb.name),
                                    node.attributes.name,
                                  ].join(' / '),
                                })
                              }
                              disabled={disabled}
                            >
                              Select as destination
                              <span className="visually-hidden"> {node.attributes.name}</span>
                            </button>
                          </div>
                        ) : (
                          <span className="restore-folder-unselectable">Not a destination folder</span>
                        )}
                      </div>
                    </li>
                  )
                })}
              </ul>
            </>
          )}
          {folder.status === 'success' && folder.hasMore && (
            <button
              type="button"
              className="button--secondary restore-load-more"
              onClick={() => void loadMoreFolders()}
              disabled={foldersLoadingMore || disabled}
            >
              {foldersLoadingMore ? 'Loading more folders…' : 'Load more folders'}
            </button>
          )}
        </div>
      )}

      {selectedDestination && (
        <p className="restore-selection-status" role="status" aria-live="polite">
          Selected destination: <strong>{selectedDestination.libraryName}</strong> /{' '}
          <strong>{selectedDestination.parentPath}</strong>
        </p>
      )}
    </section>
  )
}
