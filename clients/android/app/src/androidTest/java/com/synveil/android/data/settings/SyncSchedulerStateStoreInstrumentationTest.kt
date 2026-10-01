package com.synveil.android.data.settings

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SyncSchedulerStateStoreInstrumentationTest {
    private lateinit var store: SyncSchedulerStateStore

    @Before
    fun setUp() = runBlocking {
        store = SyncSchedulerStateStore(ApplicationProvider.getApplicationContext<Context>())
        store.clearProfile("scheduler-profile-a")
        store.clearProfile("scheduler-profile-b")
    }

    @Test
    fun stateIsDurableAndProfileScopedWithoutSecrets() = runBlocking {
        store.record("scheduler-profile-a", "library-a", SyncSchedulerOutcome.SUCCESS, now = 100L)
        store.record("scheduler-profile-a", "library-b", SyncSchedulerOutcome.PAUSED_AUTH, "authentication_required", now = 200L)
        store.record("scheduler-profile-b", "library-a", SyncSchedulerOutcome.REVOKED, "device_revoked", now = 300L)

        val profileA = store.observeForProfile("scheduler-profile-a").first()
        assertEquals(listOf("library-a", "library-b"), profileA.map { it.libraryId })
        assertEquals(SyncSchedulerOutcome.SUCCESS, profileA[0].outcome)
        assertEquals(100L, profileA[0].lastSuccessAt)
        assertEquals(SyncSchedulerOutcome.PAUSED_AUTH, profileA[1].outcome)
        assertEquals("authentication_required", profileA[1].lastErrorCode)
        assertEquals(1, store.observeForProfile("scheduler-profile-b").first().size)
    }

    @Test
    fun profileCleanupRemovesOnlyItsSchedulerState() = runBlocking {
        store.record("scheduler-profile-a", "library-a", SyncSchedulerOutcome.RETRY, "transient_error", now = 100L)
        store.record("scheduler-profile-b", "library-a", SyncSchedulerOutcome.SUCCESS, now = 200L)
        store.clearProfile("scheduler-profile-a")

        assertNull(store.observe("scheduler-profile-a", "library-a").first())
        assertEquals(1, store.observeForProfile("scheduler-profile-b").first().size)
    }
}
