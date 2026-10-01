package com.synveil.android.work

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.work.WorkInfo
import androidx.work.WorkManager
import androidx.work.NetworkType
import androidx.work.testing.WorkManagerTestInitHelper
import com.synveil.android.data.library.LibraryId
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SyncWorkSchedulerInstrumentationTest {
    private lateinit var context: Context
    private lateinit var workManager: WorkManager

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        WorkManagerTestInitHelper.initializeTestWorkManager(context)
        workManager = WorkManager.getInstance(context)
        workManager.cancelAllWork().result.get()
    }

    @Test
    fun oneTimeWorkIsUniqueAndSecretFree() = runBlocking {
        val library = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070811"))
        SyncWorkScheduler.enqueueNow(context, "profile-a", library)
        SyncWorkScheduler.enqueueNow(context, "profile-a", library)
        val infos = workManager.getWorkInfosForUniqueWork("synveil-sync:profile-a:${library.value}").get()
        assertEquals(1, infos.size)
        assertEquals(NetworkType.CONNECTED, infos.single().constraints.requiredNetworkType)
        val input = SyncWorkScheduler.inputData("profile-a", library)
        assertEquals(setOf(SYNC_PROFILE_ID, SYNC_LIBRARY_ID), input.keyValueMap.keys)
        assertEquals("profile-a", input.getString(SYNC_PROFILE_ID))
        assertEquals(library.value, input.getString(SYNC_LIBRARY_ID))
    }

    @Test
    fun periodicWorkUsesPlatformMinimumAndUniqueName() = runBlocking {
        val library = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070811"))
        SyncWorkScheduler.enqueuePeriodic(context, "profile-a", library)
        val infos = workManager.getWorkInfosForUniqueWork("synveil-periodic:profile-a:${library.value}").get()
        assertEquals(1, infos.size)
        assertEquals(WorkInfo.State.ENQUEUED, infos.single().state)
    }

    @Test
    fun schedulerSupportsUnmeteredAndBatteryConstraints() = runBlocking {
        val library = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070811"))
        SyncWorkScheduler.enqueuePeriodic(context, "profile-b", library, 15, com.synveil.android.data.settings.SyncNetworkPolicy.UNMETERED, true)
        val info = workManager.getWorkInfosForUniqueWork("synveil-periodic:profile-b:${library.value}").get().single()
        assertEquals(NetworkType.UNMETERED, info.constraints.requiredNetworkType)
        org.junit.Assert.assertTrue(info.constraints.requiresBatteryNotLow())
    }

    @Test
    fun cancellationAndProfileIsolationUseScopedUniqueWork() = runBlocking {
        val library = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070811"))
        SyncWorkScheduler.enqueueNow(context, "profile-a", library)
        SyncWorkScheduler.enqueueNow(context, "profile-b", library)
        SyncWorkScheduler.cancelProfile(context, "profile-a")
        assertEquals(WorkInfo.State.CANCELLED, workManager.getWorkInfosForUniqueWork("synveil-sync:profile-a:${library.value}").get().single().state)
        assertEquals(WorkInfo.State.ENQUEUED, workManager.getWorkInfosForUniqueWork("synveil-sync:profile-b:${library.value}").get().single().state)
    }

    @Test
    fun testDriverCanSatisfyScheduledConstraints() = runBlocking {
        val library = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070811"))
        SyncWorkScheduler.enqueuePeriodic(context, "profile-c", library, networkPolicy = com.synveil.android.data.settings.SyncNetworkPolicy.UNMETERED, batteryNotLow = true)
        val info = workManager.getWorkInfosForUniqueWork("synveil-periodic:profile-c:${library.value}").get().single()
        val driver = checkNotNull(WorkManagerTestInitHelper.getTestDriver(context))
        driver.setAllConstraintsMet(info.id)
        assertTrue(checkNotNull(workManager.getWorkInfoById(info.id).get()).state != WorkInfo.State.BLOCKED)
    }
}
