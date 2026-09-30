package com.synveil.android.data.enrollment

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import kotlinx.coroutines.flow.first
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

const val ENROLLMENT_METADATA_SCHEMA_VERSION = 1

@Serializable
data class EnrollmentMetadata(
    val schemaVersion: Int = ENROLLMENT_METADATA_SCHEMA_VERSION,
    val profileId: String,
    val canonicalBaseUrl: String,
    val transportPolicy: String,
    val ownerUserId: String,
    val deviceId: String,
    val credentialId: String,
    val createdAt: String,
)

interface EnrollmentMetadataStore {
    suspend fun pending(profileId: String): EnrollmentMetadata?
    suspend fun active(profileId: String): EnrollmentMetadata?
    suspend fun writePending(metadata: EnrollmentMetadata)
    suspend fun commitActive(metadata: EnrollmentMetadata)
    suspend fun clear(profileId: String)
}

class DataStoreEnrollmentMetadataStore(
    private val dataStore: DataStore<Preferences>,
) : EnrollmentMetadataStore {
    override suspend fun pending(profileId: String): EnrollmentMetadata? =
        read().pending.firstOrNull { it.profileId == profileId }

    override suspend fun active(profileId: String): EnrollmentMetadata? =
        read().active.firstOrNull { it.profileId == profileId }

    override suspend fun writePending(metadata: EnrollmentMetadata) {
        update { current ->
            current.copy(pending = (current.pending.filterNot { it.profileId == metadata.profileId } + metadata))
        }
    }

    override suspend fun commitActive(metadata: EnrollmentMetadata) {
        update { current ->
            current.copy(
                active = current.active.filterNot { it.profileId == metadata.profileId } + metadata,
                pending = current.pending.filterNot { it.profileId == metadata.profileId },
            )
        }
    }

    override suspend fun clear(profileId: String) {
        update { current ->
            current.copy(
                active = current.active.filterNot { it.profileId == profileId },
                pending = current.pending.filterNot { it.profileId == profileId },
            )
        }
    }

    private suspend fun read(): MetadataState = dataStore.data.first()[metadataKey]
        ?.let { encoded ->
            runCatching { json.decodeFromString<MetadataState>(encoded) }
                .getOrElse { throw IllegalStateException("invalid enrollment metadata") }
        }
        ?: MetadataState()

    private suspend fun update(transform: (MetadataState) -> MetadataState) {
        dataStore.edit { preferences ->
            val current = preferences[metadataKey]?.let { encoded ->
                runCatching { json.decodeFromString<MetadataState>(encoded) }
                    .getOrElse { throw IllegalStateException("invalid enrollment metadata") }
            } ?: MetadataState()
            preferences[metadataKey] = json.encodeToString(transform(current))
        }
    }

    @Serializable
    private data class MetadataState(
        val schemaVersion: Int = ENROLLMENT_METADATA_SCHEMA_VERSION,
        val pending: List<EnrollmentMetadata> = emptyList(),
        val active: List<EnrollmentMetadata> = emptyList(),
    )

    private companion object {
        val metadataKey = stringPreferencesKey("enrollment_metadata_v1")
        val json = Json {
            encodeDefaults = true
            explicitNulls = true
            ignoreUnknownKeys = false
        }
    }
}
