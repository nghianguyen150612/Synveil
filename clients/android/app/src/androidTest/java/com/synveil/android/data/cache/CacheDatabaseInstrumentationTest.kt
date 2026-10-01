package com.synveil.android.data.cache

import android.content.Context
import androidx.room.Room
import androidx.sqlite.db.SupportSQLiteOpenHelper
import androidx.sqlite.db.framework.FrameworkSQLiteOpenHelperFactory
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.flow.first

@RunWith(AndroidJUnit4::class)
class CacheDatabaseInstrumentationTest {
    @Test
    fun migration2To3PreservesCanonicalRowsAndInitializesOutboundTables() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val name = "migration-${System.nanoTime()}.db"
        val helper = FrameworkSQLiteOpenHelperFactory().create(
            SupportSQLiteOpenHelper.Configuration.builder(context)
                .name(name)
                .callback(object : SupportSQLiteOpenHelper.Callback(2) {
                    override fun onCreate(db: androidx.sqlite.db.SupportSQLiteDatabase) {
                        db.execSQL("CREATE TABLE cached_libraries (profileId TEXT NOT NULL, libraryId TEXT NOT NULL, revision TEXT NOT NULL, name TEXT NOT NULL, rootNodeId TEXT NOT NULL, status TEXT NOT NULL, createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL, lastObservedAt TEXT NOT NULL, PRIMARY KEY(profileId, libraryId))")
                        db.execSQL("CREATE TABLE cached_nodes (profileId TEXT NOT NULL, libraryId TEXT NOT NULL, nodeId TEXT NOT NULL, parentNodeId TEXT, currentVersionId TEXT, revision TEXT NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL, state TEXT NOT NULL, createdAt TEXT, updatedAt TEXT, trashedAt TEXT, restoreDeadline TEXT, purgeEligible INTEGER NOT NULL, byteLength TEXT, sha256 TEXT, PRIMARY KEY(profileId, libraryId, nodeId))")
                        db.execSQL("CREATE TABLE sync_states (profileId TEXT NOT NULL, deviceId TEXT NOT NULL, libraryId TEXT NOT NULL, journalEpoch TEXT, locallyAppliedSequence TEXT NOT NULL, serverAcknowledgedSequence TEXT NOT NULL, lastHighWatermark TEXT, lastSuccessfulSyncAt TEXT, lastAttemptAt TEXT, lastErrorCode TEXT, state TEXT NOT NULL, snapshotEpoch TEXT, snapshotResumeSequence TEXT, PRIMARY KEY(profileId, deviceId, libraryId))")
                        db.execSQL("CREATE TABLE pending_acks (profileId TEXT NOT NULL, deviceId TEXT NOT NULL, libraryId TEXT NOT NULL, epoch TEXT NOT NULL, fromSequence TEXT NOT NULL, throughSequence TEXT NOT NULL, highWatermark TEXT NOT NULL, ackToken TEXT NOT NULL, createdAt TEXT NOT NULL, PRIMARY KEY(profileId, deviceId, libraryId))")
                        db.execSQL("CREATE TABLE rebaseline_staging (profileId TEXT NOT NULL, deviceId TEXT NOT NULL, libraryId TEXT NOT NULL, bootstrapId TEXT NOT NULL, generation TEXT NOT NULL, snapshotEpoch TEXT NOT NULL, snapshotResumeSequence TEXT NOT NULL, manifestItemCount TEXT NOT NULL, nextCursor TEXT, completionToken TEXT, receivedItemCount TEXT NOT NULL, state TEXT NOT NULL, startedAt TEXT NOT NULL, expiresAt TEXT, PRIMARY KEY(profileId, deviceId, libraryId))")
                        db.execSQL("INSERT INTO cached_libraries VALUES ('profile-a', 'library-a', '3', 'Library', 'root', 'ACTIVE', 'created', 'updated', 'observed')")
                        db.execSQL("INSERT INTO cached_nodes VALUES ('profile-a', 'library-a', 'node-a', NULL, NULL, '4', 'file.txt', 'FILE', 'ACTIVE', NULL, NULL, NULL, NULL, 0, NULL, NULL)")
                        db.execSQL("INSERT INTO sync_states VALUES ('profile-a', 'device-a', 'library-a', 'epoch', '4', '3', '5', NULL, NULL, NULL, 'ACK_PENDING', NULL, NULL)")
                        db.execSQL("INSERT INTO pending_acks VALUES ('profile-a', 'device-a', 'library-a', 'epoch', '4', '4', '5', 'ack-token', 'created')")
                        db.execSQL("INSERT INTO rebaseline_staging VALUES ('profile-a', 'device-a', 'library-a', 'bootstrap', 'generation', 'epoch', '4', '1', NULL, NULL, '0', 'REBASELINING', 'started', NULL)")
                    }

                    override fun onUpgrade(db: androidx.sqlite.db.SupportSQLiteDatabase, oldVersion: Int, newVersion: Int) = Unit
                })
                .build(),
        )
        val database = helper.writableDatabase

        SynveilCacheDatabase.MIGRATION_2_3.migrate(database)

        assertEquals(1, database.query("SELECT COUNT(*) FROM cached_libraries WHERE profileId = 'profile-a'").use { it.moveToFirst(); it.getInt(0) })
        assertEquals(1, database.query("SELECT COUNT(*) FROM cached_nodes WHERE nodeId = 'node-a'").use { it.moveToFirst(); it.getInt(0) })
        assertEquals("ack-token", database.query("SELECT ackToken FROM pending_acks WHERE profileId = 'profile-a'").use { it.moveToFirst(); it.getString(0) })
        assertEquals(1, database.query("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'mutation_queue'").use { it.moveToFirst(); it.getInt(0) })
        assertEquals(1, database.query("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'content_operations'").use { it.moveToFirst(); it.getInt(0) })
        assertEquals(1, database.query("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'cached_conflicts'").use { it.moveToFirst(); it.getInt(0) })

        helper.close()
        context.deleteDatabase(name)
    }

    @Test
    fun cacheReopensAndPreservesProfileScopedState(): Unit = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val name = "test-cache-${System.nanoTime()}.db"
        val first = Room.databaseBuilder(context, SynveilCacheDatabase::class.java, name).build()
        first.cacheDao().upsertSyncState(
            SyncStateEntity("profile-a", "device-a", "library-a", "1", "2", "2", "3", null, null, null, "READY", null, null),
        )
        first.close()
        val second = Room.databaseBuilder(context, SynveilCacheDatabase::class.java, name).build()
        assertEquals("2", second.cacheDao().syncState("profile-a", "device-a", "library-a")?.locallyAppliedSequence)
        second.close()
        context.deleteDatabase(name)
    }

    @Test
    fun profileRowsAndPendingRecoveryStateRemainIsolated(): Unit = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val name = "test-cache-${System.nanoTime()}.db"
        val database = Room.databaseBuilder(context, SynveilCacheDatabase::class.java, name).build()
        val dao = database.cacheDao()
        dao.upsertSyncState(SyncStateEntity("profile-a", "device-a", "library-a", "1", "4", "3", "8", null, null, null, "ACK_PENDING", null, null))
        dao.upsertSyncState(SyncStateEntity("profile-b", "device-b", "library-a", "1", "1", "1", "1", null, null, null, "READY", null, null))
        dao.upsertPendingAck(PendingAckEntity("profile-a", "device-a", "library-a", "1", "3", "4", "8", "ack-evidence", "2026-10-01T00:00:00Z"))
        assertEquals("ack-evidence", dao.pendingAck("profile-a", "device-a", "library-a")?.ackToken)
        assertEquals(null, dao.pendingAck("profile-b", "device-b", "library-a"))
        database.close()
        context.deleteDatabase(name)
        assertTrue(true)
    }

    @Test
    fun outboundMutationAndContentRowsSurviveReopen(): Unit = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val name = "test-cache-${System.nanoTime()}.db"
        val first = Room.databaseBuilder(context, SynveilCacheDatabase::class.java, name).build()
        val dao = first.cacheDao()
        dao.insertMutation(MutationQueueEntity("profile-a", "device-a", "library-a", "mutation-a", "RENAME_NODE", "node-a", null, "1", "2", "{}", 1, "OUTCOME_UNKNOWN", 1, null, "timeout", null, null, null, "fingerprint"))
        dao.upsertContentOperation(ContentOperationEntity("profile-a", "device-a", "library-a", "operation-a", "node-a", "3", "/private/staging.part", 4, "a".repeat(64), null, 0, "READY", 1, null, null))
        first.close()
        val second = Room.databaseBuilder(context, SynveilCacheDatabase::class.java, name).build()
        assertEquals(1, second.cacheDao().mutations("profile-a", "device-a", "library-a").size)
        assertEquals(1, second.cacheDao().contentOperations("profile-a", "device-a", "library-a").size)
        second.close()
        context.deleteDatabase(name)
    }

    @Test
    fun cleanupPrunesOnlyOldTerminalRowsAndRetainsRecoverableState(): Unit = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val name = "cleanup-${System.nanoTime()}.db"
        val database = Room.databaseBuilder(context, SynveilCacheDatabase::class.java, name).build()
        val dao = database.cacheDao()
        val old = 1L
        val recent = System.currentTimeMillis()
        dao.upsertMutation(MutationQueueEntity("profile-a", "device-a", "library-a", "old-applied", "RENAME_NODE", "node-a", null, "1", "2", "{}", old, "APPLIED", 1, null, null, null, null, null, "fingerprint-old"))
        dao.upsertMutation(MutationQueueEntity("profile-a", "device-a", "library-a", "open-pending", "RENAME_NODE", "node-b", null, "1", "2", "{}", old, "OUTCOME_UNKNOWN", 1, null, "timeout", null, null, null, "fingerprint-open"))
        dao.upsertContentOperation(ContentOperationEntity("profile-a", "device-a", "library-a", "old-committed", "node-a", "4", "/private/old.part", 1, "a".repeat(64), null, 1, "COMMITTED", old, null, null))
        dao.upsertConflict(CachedConflictEntity("profile-a", "device-a", "library-a", "old-open", "open-pending", "RENAME_NODE", "node-b", "stale", "OPEN", "created", old))
        dao.upsertConflict(CachedConflictEntity("profile-a", "device-a", "library-a", "old-resolved", "old-applied", "RENAME_NODE", "node-a", "stale", "RESOLVED", "created", old))

        dao.pruneTerminalOutboundState(before = recent, limit = 500)

        assertEquals(null, dao.mutation("profile-a", "device-a", "library-a", "old-applied"))
        assertEquals("OUTCOME_UNKNOWN", dao.mutation("profile-a", "device-a", "library-a", "open-pending")?.state)
        assertEquals(0, dao.contentOperations("profile-a", "device-a", "library-a").size)
        assertEquals(1, dao.observeOpenConflicts("profile-a", "device-a", "library-a").first().size)
        database.close()
        context.deleteDatabase(name)
    }
}
