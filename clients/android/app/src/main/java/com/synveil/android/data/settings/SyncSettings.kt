package com.synveil.android.data.settings

import android.content.Context
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

private val Context.syncSettingsDataStore by preferencesDataStore("sync_settings.preferences_pb")

enum class SyncNetworkPolicy { CONNECTED, UNMETERED }

data class SyncSettings(
    val backgroundEnabled: Boolean = true,
    val networkPolicy: SyncNetworkPolicy = SyncNetworkPolicy.CONNECTED,
    val periodicMinutes: Int = 15,
    val batteryNotLow: Boolean = false,
)

class SyncSettingsStore(private val context: Context) {
    val settings: Flow<SyncSettings> = context.syncSettingsDataStore.data.map { values ->
        SyncSettings(
            backgroundEnabled = values[BACKGROUND] ?: true,
            networkPolicy = runCatching { SyncNetworkPolicy.valueOf(values[NETWORK] ?: SyncNetworkPolicy.CONNECTED.name) }.getOrDefault(SyncNetworkPolicy.CONNECTED),
            periodicMinutes = (values[PERIODIC] ?: 15).coerceAtLeast(15),
            batteryNotLow = values[BATTERY] ?: false,
        )
    }

    suspend fun update(transform: (SyncSettings) -> SyncSettings) {
        context.syncSettingsDataStore.edit { values ->
            val current = SyncSettings(values[BACKGROUND] ?: true, runCatching { SyncNetworkPolicy.valueOf(values[NETWORK] ?: SyncNetworkPolicy.CONNECTED.name) }.getOrDefault(SyncNetworkPolicy.CONNECTED), (values[PERIODIC] ?: 15).coerceAtLeast(15), values[BATTERY] ?: false)
            val next = transform(current)
            values[BACKGROUND] = next.backgroundEnabled
            values[NETWORK] = next.networkPolicy.name
            values[PERIODIC] = next.periodicMinutes.coerceAtLeast(15)
            values[BATTERY] = next.batteryNotLow
        }
    }

    private companion object {
        val BACKGROUND = booleanPreferencesKey("background_enabled")
        val NETWORK = stringPreferencesKey("network_policy")
        val PERIODIC = intPreferencesKey("periodic_minutes")
        val BATTERY = booleanPreferencesKey("battery_not_low")
    }
}
