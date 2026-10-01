package com.synveil.android.data.cache

import android.content.Context
import androidx.room.Database
import androidx.room.Entity
import androidx.room.Index
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import androidx.room.Transaction
import kotlinx.coroutines.flow.Flow

@Entity(
    tableName = "cached_libraries",
    primaryKeys = ["profileId", "libraryId"],
    indices = [Index(value = ["profileId", "libraryId"])]
)
data class CachedLibraryEntity(
    val profileId: String,
    val libraryId: String,
    val revision: String,
    val name: String,
    val rootNodeId: String,
    val status: String,
    val createdAt: String,
    val updatedAt: String,
    val lastObservedAt: String,
)

@Entity(
    tableName = "cached_nodes",
    primaryKeys = ["profileId", "libraryId", "nodeId"],
    indices = [
        Index(value = ["profileId", "libraryId", "parentNodeId"]),
        Index(value = ["profileId", "libraryId", "nodeId"]),
    ]
)
data class CachedNodeEntity(
    val profileId: String,
    val libraryId: String,
    val nodeId: String,
    val parentNodeId: String?,
    val currentVersionId: String?,
    val revision: String,
    val name: String,
    val kind: String,
    val state: String,
    val createdAt: String?,
    val updatedAt: String?,
    val trashedAt: String?,
    val restoreDeadline: String?,
    val purgeEligible: Boolean,
    val byteLength: String?,
    val sha256: String?,
)

@Entity(
    tableName = "sync_states",
    primaryKeys = ["profileId", "deviceId", "libraryId"],
    indices = [Index(value = ["profileId", "deviceId", "libraryId"])]
)
data class SyncStateEntity(
    val profileId: String,
    val deviceId: String,
    val libraryId: String,
    val journalEpoch: String?,
    val locallyAppliedSequence: String,
    val serverAcknowledgedSequence: String,
    val lastHighWatermark: String?,
    val lastSuccessfulSyncAt: String?,
    val lastAttemptAt: String?,
    val lastErrorCode: String?,
    val state: String,
    val snapshotEpoch: String?,
    val snapshotResumeSequence: String?,
)

@Entity(
    tableName = "pending_acks",
    primaryKeys = ["profileId", "deviceId", "libraryId"],
)
data class PendingAckEntity(
    val profileId: String,
    val deviceId: String,
    val libraryId: String,
    val epoch: String,
    val fromSequence: String,
    val throughSequence: String,
    val highWatermark: String,
    val ackToken: String,
    val createdAt: String,
)

@Entity(
    tableName = "rebaseline_staging",
    primaryKeys = ["profileId", "deviceId", "libraryId"],
)
data class RebaselineStagingEntity(
    val profileId: String,
    val deviceId: String,
    val libraryId: String,
    val bootstrapId: String,
    val generation: String,
    val snapshotEpoch: String,
    val snapshotResumeSequence: String,
    val manifestItemCount: String,
    val nextCursor: String?,
    val completionToken: String?,
    val receivedItemCount: String,
    val state: String,
    val startedAt: String,
    val expiresAt: String?,
)

@Entity(
    tableName = "rebaseline_nodes",
    primaryKeys = ["profileId", "deviceId", "libraryId", "bootstrapId", "nodeId"],
    indices = [Index(value = ["profileId", "deviceId", "libraryId", "bootstrapId"])]
)
data class RebaselineNodeEntity(
    val profileId: String,
    val deviceId: String,
    val libraryId: String,
    val bootstrapId: String,
    val nodeId: String,
    val parentNodeId: String?,
    val name: String,
    val kind: String,
    val state: String,
    val revision: String,
    val currentVersionId: String?,
    val byteLength: String?,
    val sha256: String?,
)

@Entity(
    tableName = "mutation_queue",
    primaryKeys = ["profileId", "deviceId", "libraryId", "mutationId"],
    indices = [
        Index(value = ["profileId", "deviceId", "libraryId", "state", "createdAt"]),
        Index(value = ["profileId", "deviceId", "libraryId", "resourceId", "state"]),
    ],
)
data class MutationQueueEntity(
    val profileId: String,
    val deviceId: String,
    val libraryId: String,
    val mutationId: String,
    val kind: String,
    val resourceId: String,
    val parentDependencyId: String?,
    val baseEpoch: String,
    val baseSequence: String,
    val payloadJson: String,
    val createdAt: Long,
    val state: String,
    val attemptCount: Int,
    val lastAttemptAt: Long?,
    val lastErrorCategory: String?,
    val journalEventId: String?,
    val journalSequence: String?,
    val conflictId: String?,
    val localFingerprint: String,
)

@Entity(
    tableName = "content_operations",
    primaryKeys = ["profileId", "deviceId", "libraryId", "operationId"],
    indices = [Index(value = ["profileId", "deviceId", "libraryId", "state", "createdAt"])],
)
data class ContentOperationEntity(
    val profileId: String,
    val deviceId: String,
    val libraryId: String,
    val operationId: String,
    val nodeId: String,
    val expectedNodeRevision: String,
    val stagingPath: String,
    val byteLength: Long,
    val sha256: String,
    val uploadSessionId: String?,
    val serverOffset: Long,
    val state: String,
    val createdAt: Long,
    val lastAttemptAt: Long?,
    val lastErrorCategory: String?,
)

@Entity(
    tableName = "cached_conflicts",
    primaryKeys = ["profileId", "deviceId", "libraryId", "conflictId"],
    indices = [Index(value = ["profileId", "deviceId", "libraryId", "lifecycle", "createdAt"])],
)
data class CachedConflictEntity(
    val profileId: String,
    val deviceId: String,
    val libraryId: String,
    val conflictId: String,
    val mutationId: String,
    val mutationKind: String,
    val resourceId: String,
    val reason: String,
    val lifecycle: String,
    val createdAt: String,
    val lastObservedAt: Long,
)

@Dao
interface CacheDao {
    @Query("SELECT * FROM cached_libraries WHERE profileId = :profileId ORDER BY name")
    fun observeLibraries(profileId: String): Flow<List<CachedLibraryEntity>>

    @Query("SELECT * FROM cached_libraries WHERE profileId = :profileId ORDER BY name")
    suspend fun libraries(profileId: String): List<CachedLibraryEntity>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertLibraries(values: List<CachedLibraryEntity>)

    @Query("DELETE FROM cached_libraries WHERE profileId = :profileId")
    suspend fun deleteLibraries(profileId: String)

    @Query("SELECT * FROM cached_nodes WHERE profileId = :profileId AND libraryId = :libraryId AND ((:parentNodeId IS NULL AND parentNodeId IS NULL) OR parentNodeId = :parentNodeId) ORDER BY name")
    fun observeChildren(profileId: String, libraryId: String, parentNodeId: String?): Flow<List<CachedNodeEntity>>

    @Query("SELECT * FROM cached_nodes WHERE profileId = :profileId AND libraryId = :libraryId")
    suspend fun nodes(profileId: String, libraryId: String): List<CachedNodeEntity>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertNodes(values: List<CachedNodeEntity>)

    @Query("DELETE FROM cached_nodes WHERE profileId = :profileId AND libraryId = :libraryId")
    suspend fun deleteNodes(profileId: String, libraryId: String)

    @Query("DELETE FROM cached_nodes WHERE profileId = :profileId")
    suspend fun deleteNodesForProfile(profileId: String)

    @Query("DELETE FROM cached_nodes WHERE profileId = :profileId AND libraryId = :libraryId AND nodeId = :nodeId")
    suspend fun deleteNode(profileId: String, libraryId: String, nodeId: String)

    @Transaction
    suspend fun applyFeedPage(
        nodes: List<CachedNodeEntity>,
        deletedNodeIds: List<String>,
        state: SyncStateEntity,
        pendingAck: PendingAckEntity,
    ) {
        nodes.forEach { upsertNodes(listOf(it)) }
        deletedNodeIds.forEach { deleteNode(state.profileId, state.libraryId, it) }
        upsertSyncState(state)
        upsertPendingAck(pendingAck)
    }

    @Query("SELECT * FROM sync_states WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId")
    suspend fun syncState(profileId: String, deviceId: String, libraryId: String): SyncStateEntity?

    @Query("SELECT * FROM sync_states WHERE profileId = :profileId")
    fun observeSyncStates(profileId: String): Flow<List<SyncStateEntity>>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertSyncState(value: SyncStateEntity)

    @Query("SELECT * FROM pending_acks WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId")
    suspend fun pendingAck(profileId: String, deviceId: String, libraryId: String): PendingAckEntity?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertPendingAck(value: PendingAckEntity)

    @Query("DELETE FROM pending_acks WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId")
    suspend fun deletePendingAck(profileId: String, deviceId: String, libraryId: String)

    @Query("SELECT * FROM rebaseline_staging WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId")
    suspend fun staging(profileId: String, deviceId: String, libraryId: String): RebaselineStagingEntity?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertStaging(value: RebaselineStagingEntity)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertStagingNodes(values: List<RebaselineNodeEntity>)

    @Transaction
    suspend fun persistRebaselinePage(staging: RebaselineStagingEntity, nodes: List<RebaselineNodeEntity>) {
        upsertStagingNodes(nodes)
        upsertStaging(staging)
    }

    @Query("SELECT * FROM rebaseline_nodes WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId AND bootstrapId = :bootstrapId")
    suspend fun stagingNodes(profileId: String, deviceId: String, libraryId: String, bootstrapId: String): List<RebaselineNodeEntity>

    @Query("SELECT nodeId FROM rebaseline_nodes WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId AND bootstrapId = :bootstrapId")
    suspend fun stagingNodeIds(profileId: String, deviceId: String, libraryId: String, bootstrapId: String): List<String>

    @Query("DELETE FROM rebaseline_nodes WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId AND bootstrapId = :bootstrapId")
    suspend fun deleteStagingNodes(profileId: String, deviceId: String, libraryId: String, bootstrapId: String)

    @Query("DELETE FROM rebaseline_staging WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId")
    suspend fun deleteStagingScope(profileId: String, deviceId: String, libraryId: String)

    @Transaction
    suspend fun replaceWithStaging(
        profileId: String,
        deviceId: String,
        libraryId: String,
        bootstrapId: String,
        nodes: List<CachedNodeEntity>,
    ) {
        deleteNodes(profileId, libraryId)
        upsertNodes(nodes)
    }

    @Transaction
    suspend fun installStaging(
        profileId: String,
        deviceId: String,
        libraryId: String,
        nodes: List<CachedNodeEntity>,
        staging: RebaselineStagingEntity,
    ) {
        deleteNodes(profileId, libraryId)
        upsertNodes(nodes)
        upsertStaging(staging)
    }

    @Transaction
    suspend fun confirmAck(state: SyncStateEntity, profileId: String, deviceId: String, libraryId: String) {
        upsertSyncState(state)
        deletePendingAck(profileId, deviceId, libraryId)
    }

    @Transaction
    suspend fun clearProfile(profileId: String) {
        deleteLibraries(profileId)
        deleteNodesForProfile(profileId)
        deleteSyncStates(profileId)
        deleteAcks(profileId)
        deleteStagingForProfile(profileId)
        deleteStagingNodesForProfile(profileId)
        clearOutboundForProfile(profileId)
    }

    @Query("SELECT * FROM sync_states WHERE profileId = :profileId")
    suspend fun syncStatesForProfile(profileId: String): List<SyncStateEntity>

    @Query("DELETE FROM sync_states WHERE profileId = :profileId")
    suspend fun deleteSyncStates(profileId: String)

    @Query("DELETE FROM pending_acks WHERE profileId = :profileId")
    suspend fun deleteAcks(profileId: String)

    @Query("DELETE FROM rebaseline_staging WHERE profileId = :profileId")
    suspend fun deleteStagingForProfile(profileId: String)

    @Query("DELETE FROM rebaseline_staging WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId")
    suspend fun deleteStaging(profileId: String, deviceId: String, libraryId: String)

    @Query("DELETE FROM rebaseline_nodes WHERE profileId = :profileId")
    suspend fun deleteStagingNodesForProfile(profileId: String)

    @Query("SELECT * FROM mutation_queue WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId ORDER BY createdAt ASC, mutationId ASC")
    suspend fun mutations(profileId: String, deviceId: String, libraryId: String): List<MutationQueueEntity>

    @Query("SELECT * FROM mutation_queue WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId AND state IN ('PENDING', 'OUTCOME_UNKNOWN') ORDER BY createdAt ASC, mutationId ASC LIMIT :limit")
    suspend fun eligibleMutations(profileId: String, deviceId: String, libraryId: String, limit: Int): List<MutationQueueEntity>

    @Query("UPDATE mutation_queue SET state = 'PENDING', lastErrorCategory = NULL WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId AND state = 'BLOCKED_REBASELINE'")
    suspend fun releaseBlockedMutations(profileId: String, deviceId: String, libraryId: String)

    @Insert(onConflict = OnConflictStrategy.ABORT)
    suspend fun insertMutation(value: MutationQueueEntity)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertMutation(value: MutationQueueEntity)

    @Transaction
    suspend fun applyMutationResult(value: MutationQueueEntity, node: CachedNodeEntity) {
        upsertNodes(listOf(node))
        upsertMutation(value)
    }

    @Query("SELECT * FROM mutation_queue WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId AND mutationId = :mutationId")
    suspend fun mutation(profileId: String, deviceId: String, libraryId: String, mutationId: String): MutationQueueEntity?

    @Query("SELECT EXISTS(SELECT 1 FROM mutation_queue WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId AND state IN ('PENDING', 'SUBMITTING', 'OUTCOME_UNKNOWN', 'BLOCKED_REBASELINE') AND (resourceId = :resourceId OR parentDependencyId = :resourceId OR resourceId = :parentDependencyId OR parentDependencyId = :parentDependencyId))")
    suspend fun hasOutstandingDependency(profileId: String, deviceId: String, libraryId: String, resourceId: String, parentDependencyId: String?): Boolean

    @Query("DELETE FROM mutation_queue WHERE profileId = :profileId")
    suspend fun deleteMutationsForProfile(profileId: String)

    @Query("SELECT * FROM content_operations WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId ORDER BY createdAt ASC")
    suspend fun contentOperations(profileId: String, deviceId: String, libraryId: String): List<ContentOperationEntity>

    @Query("SELECT * FROM content_operations")
    suspend fun allContentOperations(): List<ContentOperationEntity>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertContentOperation(value: ContentOperationEntity)

    @Query("DELETE FROM content_operations WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId AND operationId = :operationId")
    suspend fun deleteContentOperation(profileId: String, deviceId: String, libraryId: String, operationId: String)

    @Query("DELETE FROM content_operations WHERE profileId = :profileId")
    suspend fun deleteContentOperationsForProfile(profileId: String)

    @Query("SELECT * FROM cached_conflicts WHERE profileId = :profileId AND deviceId = :deviceId AND libraryId = :libraryId AND lifecycle = 'OPEN' ORDER BY createdAt DESC")
    fun observeOpenConflicts(profileId: String, deviceId: String, libraryId: String): Flow<List<CachedConflictEntity>>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertConflict(value: CachedConflictEntity)

    @Query("DELETE FROM cached_conflicts WHERE profileId = :profileId")
    suspend fun deleteConflictsForProfile(profileId: String)

    @Transaction
    suspend fun clearOutboundForProfile(profileId: String) {
        deleteMutationsForProfile(profileId)
        deleteContentOperationsForProfile(profileId)
        deleteConflictsForProfile(profileId)
    }
}

@Database(
    entities = [
        CachedLibraryEntity::class,
        CachedNodeEntity::class,
        SyncStateEntity::class,
        PendingAckEntity::class,
        RebaselineStagingEntity::class,
        RebaselineNodeEntity::class,
        MutationQueueEntity::class,
        ContentOperationEntity::class,
        CachedConflictEntity::class,
    ],
    version = 3,
    exportSchema = false,
)
abstract class SynveilCacheDatabase : RoomDatabase() {
    abstract fun cacheDao(): CacheDao

    companion object {
        fun create(context: Context): SynveilCacheDatabase = Room.databaseBuilder(
            context,
            SynveilCacheDatabase::class.java,
            "synveil_cache.db",
        ).addMigrations(MIGRATION_1_2, MIGRATION_2_3).build()

        val MIGRATION_1_2 = object : androidx.room.migration.Migration(1, 2) {
            override fun migrate(db: androidx.sqlite.db.SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE rebaseline_staging ADD COLUMN expiresAt TEXT")
            }
        }

        val MIGRATION_2_3 = object : androidx.room.migration.Migration(2, 3) {
            override fun migrate(db: androidx.sqlite.db.SupportSQLiteDatabase) {
                db.execSQL("CREATE TABLE IF NOT EXISTS mutation_queue (profileId TEXT NOT NULL, deviceId TEXT NOT NULL, libraryId TEXT NOT NULL, mutationId TEXT NOT NULL, kind TEXT NOT NULL, resourceId TEXT NOT NULL, parentDependencyId TEXT, baseEpoch TEXT NOT NULL, baseSequence TEXT NOT NULL, payloadJson TEXT NOT NULL, createdAt INTEGER NOT NULL, state TEXT NOT NULL, attemptCount INTEGER NOT NULL, lastAttemptAt INTEGER, lastErrorCategory TEXT, journalEventId TEXT, journalSequence TEXT, conflictId TEXT, localFingerprint TEXT NOT NULL, PRIMARY KEY(profileId, deviceId, libraryId, mutationId))")
                db.execSQL("CREATE INDEX IF NOT EXISTS index_mutation_queue_profileId_deviceId_libraryId_state_createdAt ON mutation_queue(profileId, deviceId, libraryId, state, createdAt)")
                db.execSQL("CREATE INDEX IF NOT EXISTS index_mutation_queue_profileId_deviceId_libraryId_resourceId_state ON mutation_queue(profileId, deviceId, libraryId, resourceId, state)")
                db.execSQL("CREATE TABLE IF NOT EXISTS content_operations (profileId TEXT NOT NULL, deviceId TEXT NOT NULL, libraryId TEXT NOT NULL, operationId TEXT NOT NULL, nodeId TEXT NOT NULL, expectedNodeRevision TEXT NOT NULL, stagingPath TEXT NOT NULL, byteLength INTEGER NOT NULL, sha256 TEXT NOT NULL, uploadSessionId TEXT, serverOffset INTEGER NOT NULL, state TEXT NOT NULL, createdAt INTEGER NOT NULL, lastAttemptAt INTEGER, lastErrorCategory TEXT, PRIMARY KEY(profileId, deviceId, libraryId, operationId))")
                db.execSQL("CREATE INDEX IF NOT EXISTS index_content_operations_profileId_deviceId_libraryId_state_createdAt ON content_operations(profileId, deviceId, libraryId, state, createdAt)")
                db.execSQL("CREATE TABLE IF NOT EXISTS cached_conflicts (profileId TEXT NOT NULL, deviceId TEXT NOT NULL, libraryId TEXT NOT NULL, conflictId TEXT NOT NULL, mutationId TEXT NOT NULL, mutationKind TEXT NOT NULL, resourceId TEXT NOT NULL, reason TEXT NOT NULL, lifecycle TEXT NOT NULL, createdAt TEXT NOT NULL, lastObservedAt INTEGER NOT NULL, PRIMARY KEY(profileId, deviceId, libraryId, conflictId))")
                db.execSQL("CREATE INDEX IF NOT EXISTS index_cached_conflicts_profileId_deviceId_libraryId_lifecycle_createdAt ON cached_conflicts(profileId, deviceId, libraryId, lifecycle, createdAt)")
            }
        }
    }
}
