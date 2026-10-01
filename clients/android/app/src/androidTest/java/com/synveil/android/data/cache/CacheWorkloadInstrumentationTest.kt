package com.synveil.android.data.cache

import android.content.Context
import androidx.room.Room
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class CacheWorkloadInstrumentationTest {
    @Test
    fun representativeMetadataWorkloadUsesStableBoundedQueries(): Unit = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val database = Room.inMemoryDatabaseBuilder(context, SynveilCacheDatabase::class.java).build()
        val dao = database.cacheDao()
        dao.upsertLibraries(listOf(CachedLibraryEntity("profile-a", "library-a", "1", "Workload", "root", "ACTIVE", "created", "updated", "2026-10-01T00:00:00Z")))
        dao.upsertNodes((0 until 5_000).map { index ->
            CachedNodeEntity("profile-a", "library-a", "node-%05d".format(index), null, null, "1", "node-$index", "FILE", "ACTIVE", null, null, null, null, false, null, null)
        })
        dao.insertMutations((0 until 1_000).map { index ->
            MutationQueueEntity("profile-a", "device-a", "library-a", "mutation-%04d".format(index), "RENAME_NODE", "node-$index", null, "1", index.toString(), "{}", index.toLong(), "PENDING", 0, null, null, null, null, null, "fingerprint-$index")
        })
        dao.upsertConflicts((0 until 100).map { index ->
            CachedConflictEntity("profile-a", "device-a", "library-a", "conflict-%03d".format(index), "mutation-%04d".format(index), "RENAME_NODE", "node-$index", "concurrent", "OPEN", "2026-10-01T00:00:00Z", index.toLong())
        })

        assertEquals(5_000, dao.nodes("profile-a", "library-a").size)
        assertEquals(16, dao.eligibleMutations("profile-a", "device-a", "library-a", 16).size)
        assertEquals(100, dao.observeOpenConflicts("profile-a", "device-a", "library-a").first().size)
        assertTrue(dao.mutations("profile-a", "device-a", "library-a").zipWithNext().all { (first, second) -> first.createdAt <= second.createdAt })
        database.close()
    }
}
