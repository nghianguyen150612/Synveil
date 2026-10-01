package com.synveil.android.data.cache

import com.synveil.android.data.library.Library
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.library.LibraryRevision
import com.synveil.android.data.library.LibraryStatus
import com.synveil.android.data.library.NodeId
import com.synveil.android.data.node.Node
import com.synveil.android.data.cache.MutationQueueEntity
import com.synveil.android.data.cache.ContentOperationEntity
import com.synveil.android.data.cache.CachedConflictEntity
import com.synveil.android.data.node.NodeKind
import com.synveil.android.data.node.NodeRevision
import com.synveil.android.data.node.NodeState
import java.time.OffsetDateTime
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

class CacheRepository(private val dao: CacheDao) {
    fun observeLibraries(profileId: String): Flow<List<Library>> = dao.observeLibraries(profileId).map { values ->
        values.mapNotNull { value ->
            val id = LibraryId.parse(value.libraryId) ?: return@mapNotNull null
            val root = NodeId.parse(value.rootNodeId) ?: return@mapNotNull null
            val revision = LibraryRevision.parse(value.revision) ?: return@mapNotNull null
            val status = runCatching { LibraryStatus.valueOf(value.status) }.getOrNull() ?: return@mapNotNull null
            val created = runCatching { OffsetDateTime.parse(value.createdAt) }.getOrNull() ?: return@mapNotNull null
            val updated = runCatching { OffsetDateTime.parse(value.updatedAt) }.getOrNull() ?: return@mapNotNull null
            Library(id, revision, value.name, root, status, created, updated)
        }
    }

    fun observeChildren(profileId: String, libraryId: LibraryId, parentNodeId: NodeId?): Flow<List<Node>> =
        dao.observeChildren(profileId, libraryId.value, parentNodeId?.value).map { values ->
            values.mapNotNull { value -> value.toDomain() }
        }

    suspend fun upsertLibraries(profileId: String, values: List<Library>, observedAt: String) {
        dao.upsertLibraries(values.map { value ->
            CachedLibraryEntity(profileId, value.id.value, value.revision.value, value.name, value.rootNodeId.value, value.status.name, value.createdAt.toString(), value.updatedAt.toString(), observedAt)
        })
    }

    suspend fun upsertNodes(profileId: String, values: List<Node>) {
        dao.upsertNodes(values.map { it.toEntity(profileId) })
    }

    suspend fun deleteNode(profileId: String, libraryId: LibraryId, nodeId: NodeId) = dao.deleteNode(profileId, libraryId.value, nodeId.value)

    suspend fun state(profileId: String, deviceId: String, libraryId: LibraryId): SyncStateEntity? = dao.syncState(profileId, deviceId, libraryId.value)
    fun observeSyncStates(profileId: String): Flow<List<SyncStateEntity>> = dao.observeSyncStates(profileId)

    suspend fun pendingAck(profileId: String, deviceId: String, libraryId: LibraryId): PendingAckEntity? = dao.pendingAck(profileId, deviceId, libraryId.value)

    suspend fun upsertState(value: SyncStateEntity) = dao.upsertSyncState(value)
    suspend fun upsertAck(value: PendingAckEntity) = dao.upsertPendingAck(value)
    suspend fun deleteAck(profileId: String, deviceId: String, libraryId: LibraryId) = dao.deletePendingAck(profileId, deviceId, libraryId.value)
    suspend fun staging(profileId: String, deviceId: String, libraryId: LibraryId) = dao.staging(profileId, deviceId, libraryId.value)
    suspend fun upsertStaging(value: RebaselineStagingEntity) = dao.upsertStaging(value)
    suspend fun upsertStagingNodes(values: List<RebaselineNodeEntity>) = dao.upsertStagingNodes(values)
    suspend fun persistRebaselinePage(staging: RebaselineStagingEntity, values: List<RebaselineNodeEntity>) = dao.persistRebaselinePage(staging, values)
    suspend fun stagingNodes(profileId: String, deviceId: String, libraryId: LibraryId, bootstrapId: String) = dao.stagingNodes(profileId, deviceId, libraryId.value, bootstrapId)
    suspend fun stagingNodeIds(profileId: String, deviceId: String, libraryId: LibraryId, bootstrapId: String) = dao.stagingNodeIds(profileId, deviceId, libraryId.value, bootstrapId)
    suspend fun replaceWithStaging(profileId: String, deviceId: String, libraryId: LibraryId, bootstrapId: String, nodes: List<CachedNodeEntity>) = dao.replaceWithStaging(profileId, deviceId, libraryId.value, bootstrapId, nodes)
    suspend fun installStaging(profileId: String, deviceId: String, libraryId: LibraryId, nodes: List<CachedNodeEntity>, staging: RebaselineStagingEntity) = dao.installStaging(profileId, deviceId, libraryId.value, nodes, staging)
    suspend fun deleteStaging(profileId: String, deviceId: String, libraryId: LibraryId) {
        dao.staging(profileId, deviceId, libraryId.value)?.let {
            dao.deleteStagingNodes(profileId, deviceId, libraryId.value, it.bootstrapId)
        }
        dao.deleteStagingScope(profileId, deviceId, libraryId.value)
    }
    suspend fun clearProfile(profileId: String) = dao.clearProfile(profileId)
    suspend fun discardPendingAck(profileId: String, deviceId: String, libraryId: LibraryId) =
        dao.deletePendingAck(profileId, deviceId, libraryId.value)
    suspend fun applyFeedPage(nodes: List<Node>, deletedNodeIds: List<String>, state: SyncStateEntity, pendingAck: PendingAckEntity) =
        dao.applyFeedPage(nodes.map { it.toEntity(state.profileId) }, deletedNodeIds, state, pendingAck)
    suspend fun confirmAck(state: SyncStateEntity, profileId: String, deviceId: String, libraryId: LibraryId) =
        dao.confirmAck(state, profileId, deviceId, libraryId.value)
    suspend fun activeNodes(profileId: String, libraryId: LibraryId) = dao.nodes(profileId, libraryId.value)

    suspend fun enqueueMutation(value: MutationQueueEntity) = dao.insertMutation(value)
    suspend fun updateMutation(value: MutationQueueEntity) = dao.upsertMutation(value)
    suspend fun applyMutationResult(value: MutationQueueEntity, node: CachedNodeEntity) = dao.applyMutationResult(value, node)
    suspend fun mutation(profileId: String, deviceId: String, libraryId: LibraryId, mutationId: String) =
        dao.mutation(profileId, deviceId, libraryId.value, mutationId)
    suspend fun eligibleMutations(profileId: String, deviceId: String, libraryId: LibraryId, limit: Int) =
        dao.eligibleMutations(profileId, deviceId, libraryId.value, limit)
    suspend fun releaseBlockedMutations(profileId: String, deviceId: String, libraryId: LibraryId) =
        dao.releaseBlockedMutations(profileId, deviceId, libraryId.value)
    suspend fun mutations(profileId: String, deviceId: String, libraryId: LibraryId) =
        dao.mutations(profileId, deviceId, libraryId.value)
    suspend fun hasOutstandingDependency(profileId: String, deviceId: String, libraryId: LibraryId, resourceId: String, parentDependencyId: String?) =
        dao.hasOutstandingDependency(profileId, deviceId, libraryId.value, resourceId, parentDependencyId)
    suspend fun upsertContentOperation(value: ContentOperationEntity) = dao.upsertContentOperation(value)
    suspend fun recordContentCompletion(profileId: String, libraryId: LibraryId, nodeId: String, versionId: String, revision: String, byteLength: String, sha256: String) =
        dao.recordContentCompletion(profileId, libraryId.value, nodeId, versionId, revision, byteLength, sha256)
    suspend fun deleteContentOperation(profileId: String, deviceId: String, libraryId: LibraryId, operationId: String) = dao.deleteContentOperation(profileId, deviceId, libraryId.value, operationId)
    suspend fun contentOperations(profileId: String, deviceId: String, libraryId: LibraryId) =
        dao.contentOperations(profileId, deviceId, libraryId.value)
    suspend fun allContentOperations() = dao.allContentOperations()
    fun observeOpenConflicts(profileId: String, deviceId: String, libraryId: LibraryId) =
        dao.observeOpenConflicts(profileId, deviceId, libraryId.value)
    suspend fun upsertConflict(value: CachedConflictEntity) = dao.upsertConflict(value)
    suspend fun pruneTerminalOutboundState(before: Long, limit: Int) = dao.pruneTerminalOutboundState(before, limit)

    private fun CachedNodeEntity.toDomain(): Node? {
        val node = NodeId.parse(nodeId) ?: return null
        val library = LibraryId.parse(libraryId) ?: return null
        val parent = parentNodeId?.let { NodeId.parse(it) ?: return null }
        val version = currentVersionId?.takeIf { it.matches(UUID_PATTERN) }
        val revision = NodeRevision.parse(revision) ?: return null
        val kind = runCatching { NodeKind.valueOf(kind) }.getOrNull() ?: return null
        val state = runCatching { NodeState.valueOf(state) }.getOrNull() ?: return null
        val created = createdAt?.let { runCatching { OffsetDateTime.parse(it) }.getOrNull() }
        val updated = updatedAt?.let { runCatching { OffsetDateTime.parse(it) }.getOrNull() }
        val trashed = trashedAt?.let { runCatching { OffsetDateTime.parse(it) }.getOrNull() }
        val deadline = restoreDeadline?.let { runCatching { OffsetDateTime.parse(it) }.getOrNull() }
        if ((createdAt != null && created == null) || (updatedAt != null && updated == null) ||
            (trashedAt != null && trashed == null) || (restoreDeadline != null && deadline == null)
        ) return null
        return Node(node, library, parent, version, revision, name, kind, state, created, updated, trashed, deadline, purgeEligible)
    }

    private fun Node.toEntity(profileId: String) = CachedNodeEntity(profileId, libraryId.value, nodeId.value, parentId?.value, currentVersionId, revision.value, name, kind.name, state.name, createdAt.toString(), updatedAt.toString(), trashedAt?.toString(), restoreDeadline?.toString(), purgeEligible, null, null)

    companion object {
        private val UUID_PATTERN = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
    }
}
