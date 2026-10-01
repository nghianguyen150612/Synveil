package com.synveil.android.data.cache

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import androidx.datastore.preferences.core.stringPreferencesKey
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

enum class CacheRecoveryState {
    READY,
    CORRUPT,
    MIGRATION_FAILED,
    STORAGE_LOCKED,
    LOW_DISK_SPACE,
    MAINTENANCE_FAILED,
}

object CacheRecoveryStateClassifier {
    fun classify(error: Throwable, availableBytes: Long?): CacheRecoveryState {
        if (availableBytes != null && availableBytes < CacheMaintenancePolicy.MINIMUM_FREE_BYTES) return CacheRecoveryState.LOW_DISK_SPACE
        val message = generateSequence(error) { it.cause }
            .mapNotNull { it.message }
            .joinToString(" ")
            .lowercase()
        return when {
            "locked" in message || "busy" in message -> CacheRecoveryState.STORAGE_LOCKED
            "migration" in message -> CacheRecoveryState.MIGRATION_FAILED
            "not a database" in message || "malformed" in message || "corrupt" in message -> CacheRecoveryState.CORRUPT
            else -> CacheRecoveryState.MAINTENANCE_FAILED
        }
    }
}

object CacheMaintenancePolicy {
    const val DEFAULT_BATCH_LIMIT = 500
    const val MINIMUM_FREE_BYTES = 8L * 1024L * 1024L

    fun batchLimit(requested: Int): Int = if (requested <= 0) DEFAULT_BATCH_LIMIT else requested.coerceAtMost(DEFAULT_BATCH_LIMIT)
}

data class CacheMaintenanceReport(
    val expiredStagingScopes: Int,
    val orphanRebaselineNodes: Int,
    val staleLibraries: Int,
)

private val Context.cacheRecoveryDataStore by preferencesDataStore("cache_recovery.preferences_pb")

class CacheRecoveryStateStore(private val context: Context) {
    val state: Flow<CacheRecoveryState> = context.cacheRecoveryDataStore.data.map { values ->
        values[STATE]?.let { runCatching { CacheRecoveryState.valueOf(it) }.getOrNull() } ?: CacheRecoveryState.READY
    }

    val updatedAt: Flow<Long?> = context.cacheRecoveryDataStore.data.map { it[UPDATED_AT] }

    suspend fun record(state: CacheRecoveryState, now: Long = System.currentTimeMillis()) {
        context.cacheRecoveryDataStore.edit { values ->
            values[STATE] = state.name
            values[UPDATED_AT] = now
        }
    }

    private companion object {
        val STATE = stringPreferencesKey("state")
        val UPDATED_AT = longPreferencesKey("updated_at")
    }
}
