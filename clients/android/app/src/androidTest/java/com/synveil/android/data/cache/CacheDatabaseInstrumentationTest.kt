package com.synveil.android.data.cache

import android.content.Context
import androidx.room.Room
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import kotlinx.coroutines.runBlocking

@RunWith(AndroidJUnit4::class)
class CacheDatabaseInstrumentationTest {
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
    fun profileRowsAndPendingRecoveryStateRemainIsolated() = runBlocking {
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
}
