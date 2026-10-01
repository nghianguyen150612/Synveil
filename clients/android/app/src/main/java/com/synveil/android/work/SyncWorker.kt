package com.synveil.android.work

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.Data
import androidx.work.WorkerParameters
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import com.synveil.android.SynveilApplication
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.sync.SyncOutcomeKind
import java.util.concurrent.TimeUnit

const val SYNC_PROFILE_ID = "profile_id"
const val SYNC_LIBRARY_ID = "library_id"

class SynveilSyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val profileId = inputData.getString(SYNC_PROFILE_ID) ?: return Result.failure()
        val libraryId = inputData.getString(SYNC_LIBRARY_ID)?.let(LibraryId::parse) ?: return Result.failure()
        val outcome = (applicationContext as SynveilApplication).syncCoordinator.synchronize(profileId, libraryId)
        return when (outcome.kind) {
            SyncOutcomeKind.SUCCESS -> Result.success()
            SyncOutcomeKind.MORE_WORK -> Result.retry()
            SyncOutcomeKind.TRANSIENT_ERROR -> Result.retry()
            SyncOutcomeKind.REBASELINE_REQUIRED -> Result.retry()
            SyncOutcomeKind.AUTHENTICATION_REQUIRED, SyncOutcomeKind.DEVICE_REVOKED, SyncOutcomeKind.PROTOCOL_ERROR -> Result.failure()
        }
    }
}

object SyncWorkScheduler {
    private fun constraints() = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
    fun enqueueNow(context: Context, profileId: String, libraryId: LibraryId) {
        val request = OneTimeWorkRequestBuilder<SynveilSyncWorker>()
            .setInputData(Data.Builder().putString(SYNC_PROFILE_ID, profileId).putString(SYNC_LIBRARY_ID, libraryId.value).build())
            .addTag("synveil-profile:$profileId")
            .setConstraints(constraints())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork("synveil-sync:$profileId:${libraryId.value}", ExistingWorkPolicy.KEEP, request)
    }

    fun enqueuePeriodic(context: Context, profileId: String, libraryId: LibraryId) {
        val request = PeriodicWorkRequestBuilder<SynveilSyncWorker>(15, TimeUnit.MINUTES)
            .setInputData(Data.Builder().putString(SYNC_PROFILE_ID, profileId).putString(SYNC_LIBRARY_ID, libraryId.value).build())
            .addTag("synveil-profile:$profileId")
            .setConstraints(constraints())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork("synveil-periodic:$profileId:${libraryId.value}", ExistingPeriodicWorkPolicy.KEEP, request)
    }

    fun cancelProfile(context: Context, profileId: String) {
        WorkManager.getInstance(context).cancelAllWorkByTag("synveil-profile:$profileId")
    }
}
