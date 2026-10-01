package com.synveil.android.work

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.Data
import androidx.work.WorkerParameters
import androidx.work.BackoffPolicy
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.Constraints
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import com.synveil.android.SynveilApplication
import com.synveil.android.data.settings.SyncNetworkPolicy
import com.synveil.android.data.settings.SyncSchedulerOutcome
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.sync.SyncOutcomeKind
import java.util.concurrent.TimeUnit

const val SYNC_PROFILE_ID = "profile_id"
const val SYNC_LIBRARY_ID = "library_id"

enum class SyncWorkerAction { SUCCESS, RETRY, FAILURE }

data class SyncWorkerDecision(
    val outcome: SyncSchedulerOutcome,
    val action: SyncWorkerAction,
)

class SynveilSyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val profileId = inputData.getString(SYNC_PROFILE_ID) ?: return Result.failure()
        val libraryId = inputData.getString(SYNC_LIBRARY_ID)?.let(LibraryId::parse) ?: return Result.failure()
        val application = applicationContext as SynveilApplication
        application.syncSchedulerStateStore.record(profileId, libraryId.value, SyncSchedulerOutcome.RETRY, "attempting")
        val outcome = application.syncCoordinator.synchronize(profileId, libraryId)
        val decision = SyncWorkScheduler.decision(outcome.kind, runAttemptCount)
        application.syncSchedulerStateStore.record(
            profileId,
            libraryId.value,
            decision.outcome,
            outcome.kind.takeIf { it != SyncOutcomeKind.SUCCESS }?.name?.lowercase(),
        )
        return when (decision.action) {
            SyncWorkerAction.SUCCESS -> Result.success()
            SyncWorkerAction.RETRY -> Result.retry()
            SyncWorkerAction.FAILURE -> Result.failure()
        }
    }
}

object SyncWorkScheduler {
    const val MAX_AUTOMATIC_RETRIES = 5

    fun inputData(profileId: String, libraryId: LibraryId): Data =
        Data.Builder()
            .putString(SYNC_PROFILE_ID, profileId)
            .putString(SYNC_LIBRARY_ID, libraryId.value)
            .build()

    fun decision(kind: SyncOutcomeKind, runAttemptCount: Int): SyncWorkerDecision {
        val outcome = when (kind) {
            SyncOutcomeKind.SUCCESS -> SyncSchedulerOutcome.SUCCESS
            SyncOutcomeKind.MORE_WORK, SyncOutcomeKind.TRANSIENT_ERROR -> SyncSchedulerOutcome.RETRY
            SyncOutcomeKind.REBASELINE_REQUIRED -> SyncSchedulerOutcome.REBASELINE_REQUIRED
            SyncOutcomeKind.AUTHENTICATION_REQUIRED -> SyncSchedulerOutcome.PAUSED_AUTH
            SyncOutcomeKind.DEVICE_REVOKED -> SyncSchedulerOutcome.REVOKED
            SyncOutcomeKind.PROTOCOL_ERROR -> SyncSchedulerOutcome.PROTOCOL_ERROR
        }
        val action = when {
            outcome == SyncSchedulerOutcome.SUCCESS -> SyncWorkerAction.SUCCESS
            outcome == SyncSchedulerOutcome.RETRY || outcome == SyncSchedulerOutcome.REBASELINE_REQUIRED ->
                if (runAttemptCount < MAX_AUTOMATIC_RETRIES) SyncWorkerAction.RETRY else SyncWorkerAction.FAILURE
            else -> SyncWorkerAction.FAILURE
        }
        return SyncWorkerDecision(outcome, action)
    }

    fun constraints(networkPolicy: SyncNetworkPolicy = SyncNetworkPolicy.CONNECTED, batteryNotLow: Boolean = false): Constraints = Constraints.Builder()
        .setRequiredNetworkType(if (networkPolicy == com.synveil.android.data.settings.SyncNetworkPolicy.UNMETERED) NetworkType.UNMETERED else NetworkType.CONNECTED)
        .setRequiresBatteryNotLow(batteryNotLow)
        .build()
    fun enqueueNow(context: Context, profileId: String, libraryId: LibraryId) {
        val request = OneTimeWorkRequestBuilder<SynveilSyncWorker>()
            .setInputData(inputData(profileId, libraryId))
            .addTag("synveil-profile:$profileId")
            .setConstraints(constraints())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork("synveil-sync:$profileId:${libraryId.value}", ExistingWorkPolicy.KEEP, request)
    }

    fun enqueuePeriodic(context: Context, profileId: String, libraryId: LibraryId, intervalMinutes: Int = 15, networkPolicy: com.synveil.android.data.settings.SyncNetworkPolicy = com.synveil.android.data.settings.SyncNetworkPolicy.CONNECTED, batteryNotLow: Boolean = false) {
        val request = PeriodicWorkRequestBuilder<SynveilSyncWorker>(intervalMinutes.coerceAtLeast(15).toLong(), TimeUnit.MINUTES)
            .setInputData(inputData(profileId, libraryId))
            .addTag("synveil-profile:$profileId")
            .setConstraints(constraints(networkPolicy, batteryNotLow))
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork("synveil-periodic:$profileId:${libraryId.value}", ExistingPeriodicWorkPolicy.KEEP, request)
    }

    fun cancelPeriodic(context: Context, profileId: String, libraryId: LibraryId) {
        WorkManager.getInstance(context).cancelUniqueWork("synveil-periodic:$profileId:${libraryId.value}")
    }

    fun cancelProfile(context: Context, profileId: String) {
        WorkManager.getInstance(context).cancelAllWorkByTag("synveil-profile:$profileId")
    }
}
