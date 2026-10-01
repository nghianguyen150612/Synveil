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
}

@Database(
    entities = [
        CachedLibraryEntity::class,
        CachedNodeEntity::class,
        SyncStateEntity::class,
        PendingAckEntity::class,
        RebaselineStagingEntity::class,
        RebaselineNodeEntity::class,
    ],
    version = 2,
    exportSchema = false,
)
abstract class SynveilCacheDatabase : RoomDatabase() {
    abstract fun cacheDao(): CacheDao

    companion object {
        fun create(context: Context): SynveilCacheDatabase = Room.databaseBuilder(
            context,
            SynveilCacheDatabase::class.java,
            "synveil_cache.db",
        ).addMigrations(MIGRATION_1_2).build()

        val MIGRATION_1_2 = object : androidx.room.migration.Migration(1, 2) {
            override fun migrate(db: androidx.sqlite.db.SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE rebaseline_staging ADD COLUMN expiresAt TEXT")
            }
        }
    }
}
