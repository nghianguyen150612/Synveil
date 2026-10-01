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
    }

    @Test
    fun periodicWorkUsesPlatformMinimumAndUniqueName() = runBlocking {
        val library = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070811"))
        SyncWorkScheduler.enqueuePeriodic(context, "profile-a", library)
        val infos = workManager.getWorkInfosForUniqueWork("synveil-periodic:profile-a:${library.value}").get()
        assertEquals(1, infos.size)
        assertEquals(WorkInfo.State.ENQUEUED, infos.single().state)
    }
}
