package com.synveil.android.data.settings

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import androidx.datastore.preferences.core.stringPreferencesKey
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

enum class SyncSchedulerOutcome {
    SUCCESS,
    RETRY,
    PAUSED_AUTH,
    REVOKED,
    PROTOCOL_ERROR,
    REBASELINE_REQUIRED,
}

data class SyncSchedulerState(
    val profileId: String,
    val libraryId: String,
    val outcome: SyncSchedulerOutcome,
    val lastAttemptAt: Long,
    val lastSuccessAt: Long?,
    val lastErrorCode: String?,
)

private val Context.syncSchedulerStateDataStore by preferencesDataStore("sync_scheduler_state.preferences_pb")

class SyncSchedulerStateStore(private val context: Context) {
    fun observe(profileId: String, libraryId: String): Flow<SyncSchedulerState?> =
        context.syncSchedulerStateDataStore.data.map { values -> decode(values, scope(profileId, libraryId)) }

    fun observeForProfile(profileId: String): Flow<List<SyncSchedulerState>> =
        context.syncSchedulerStateDataStore.data.map { values ->
            val prefix = "$PREFIX$profileId:"
            values.asMap().keys
                .filter { it.name.startsWith(prefix) && it.name.endsWith(OUTCOME_SUFFIX) }
                .map { key -> key.name.removePrefix(PREFIX).removeSuffix(OUTCOME_SUFFIX) }
                .mapNotNull { decode(values, it) }
                .sortedBy { it.libraryId }
        }

    suspend fun record(
        profileId: String,
        libraryId: String,
        outcome: SyncSchedulerOutcome,
        errorCode: String? = null,
        now: Long = System.currentTimeMillis(),
    ) {
        val scope = scope(profileId, libraryId)
        context.syncSchedulerStateDataStore.edit { values ->
            val previous = decode(values, scope)
            values[outcomeKey(scope)] = outcome.name
            values[lastAttemptKey(scope)] = now
            if (outcome == SyncSchedulerOutcome.SUCCESS) {
                values[lastSuccessKey(scope)] = now
                values.remove(errorKey(scope))
            } else {
                previous?.lastSuccessAt?.let { values[lastSuccessKey(scope)] = it }
                if (errorCode == null) values.remove(errorKey(scope)) else values[errorKey(scope)] = errorCode.take(MAX_ERROR_LENGTH)
            }
        }
    }

    suspend fun clearProfile(profileId: String) {
        val prefix = "$PREFIX$profileId:"
        context.syncSchedulerStateDataStore.edit { values ->
            values.asMap().keys
                .filter { it.name.startsWith(prefix) }
                .map { it.name.removePrefix(PREFIX).substringBeforeLast(':') }
                .distinct()
                .forEach { scope ->
                    values.remove(outcomeKey(scope))
                    values.remove(lastAttemptKey(scope))
                    values.remove(lastSuccessKey(scope))
                    values.remove(errorKey(scope))
                }
        }
    }

    private fun scope(profileId: String, libraryId: String): String = "$profileId:$libraryId"

    private fun decode(values: androidx.datastore.preferences.core.Preferences, scope: String): SyncSchedulerState? {
        val outcome = values[outcomeKey(scope)]?.let { runCatching { SyncSchedulerOutcome.valueOf(it) }.getOrNull() } ?: return null
        val lastAttempt = values[lastAttemptKey(scope)] ?: return null
        val separator = scope.indexOf(':')
        if (separator <= 0 || separator == scope.lastIndex) return null
        return SyncSchedulerState(
            profileId = scope.substring(0, separator),
            libraryId = scope.substring(separator + 1),
            outcome = outcome,
            lastAttemptAt = lastAttempt,
            lastSuccessAt = values[lastSuccessKey(scope)],
            lastErrorCode = values[errorKey(scope)],
        )
    }

    private companion object {
        const val PREFIX = "scope:"
        const val OUTCOME_SUFFIX = ":outcome"
        const val MAX_ERROR_LENGTH = 64

        fun outcomeKey(scope: String) = stringPreferencesKey("$PREFIX$scope:outcome")
        fun lastAttemptKey(scope: String) = longPreferencesKey("$PREFIX$scope:last_attempt")
        fun lastSuccessKey(scope: String) = longPreferencesKey("$PREFIX$scope:last_success")
        fun errorKey(scope: String) = stringPreferencesKey("$PREFIX$scope:error")
    }
}
