package com.synveil.android.data.profile

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.MutablePreferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.map
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import com.synveil.android.core.model.CanonicalOriginException
import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.DisplayLabelException
import com.synveil.android.core.model.MAX_PROFILES
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.core.model.TransportPolicy
import com.synveil.android.core.model.validateDisplayLabel

const val PROFILE_SCHEMA_VERSION = 1

data class ProfileConfiguration(
    val profiles: List<ServerProfile>,
    val activeProfileId: ServerProfileId?,
) {
    val activeProfile: ServerProfile?
        get() = profiles.firstOrNull { it.profileId == activeProfileId }
}

sealed interface ProfileRepositoryState {
    data object NoServerConfigured : ProfileRepositoryState

    data class Configured(
        val configuration: ProfileConfiguration,
    ) : ProfileRepositoryState

    data class ConfigurationError(
        val reason: ProfileConfigurationError,
    ) : ProfileRepositoryState
}

enum class ProfileConfigurationError {
    INVALID_PERSISTED_CONFIGURATION,
    UNSUPPORTED_SCHEMA_VERSION,
    STORAGE_UNAVAILABLE,
}

enum class ProfileOperationError {
    DUPLICATE_ORIGIN,
    PROFILE_NOT_FOUND,
    ACTIVE_PROFILE_REQUIRED,
    PROFILE_LIMIT_REACHED,
    INVALID_PERSISTED_CONFIGURATION,
    STORAGE_UNAVAILABLE,
}

class ProfileOperationException(
    val reason: ProfileOperationError,
) : IllegalStateException(reason.name)

sealed interface ProfileMutationResult {
    val profile: ServerProfile

    data class Created(override val profile: ServerProfile) : ProfileMutationResult
    data class UpdatedLabelOnly(override val profile: ServerProfile) : ProfileMutationResult
    data class UpdatedOrigin(override val profile: ServerProfile) : ProfileMutationResult
}

interface ServerProfileRepository {
    val state: Flow<ProfileRepositoryState>

    suspend fun addProfile(
        displayLabel: String,
        rawBaseUrl: String,
    ): ProfileMutationResult

    suspend fun updateProfile(
        profileId: ServerProfileId,
        displayLabel: String,
        rawBaseUrl: String,
    ): ProfileMutationResult

    suspend fun selectActiveProfile(profileId: ServerProfileId)

    suspend fun removeProfile(profileId: ServerProfileId)
}

class DataStoreServerProfileRepository(
    private val dataStore: DataStore<Preferences>,
    private val allowLoopbackTestHttp: Boolean,
    private val idGenerator: () -> ServerProfileId = { ServerProfileId.new() },
    private val nowMillis: () -> Long = { System.currentTimeMillis() },
) : ServerProfileRepository {
    override val state: Flow<ProfileRepositoryState> = dataStore.data
        .map { preferences ->
            runCatching { decode(preferences) }
                .fold(
                    onSuccess = { configuration ->
                        if (configuration.profiles.isEmpty()) {
                            ProfileRepositoryState.NoServerConfigured
                        } else {
                            ProfileRepositoryState.Configured(configuration)
                        }
                    },
                    onFailure = { error ->
                        ProfileRepositoryState.ConfigurationError(error.configurationReason())
                    },
                )
        }
        .catch {
            emit(ProfileRepositoryState.ConfigurationError(ProfileConfigurationError.STORAGE_UNAVAILABLE))
        }

    override suspend fun addProfile(
        displayLabel: String,
        rawBaseUrl: String,
    ): ProfileMutationResult {
        var result: ProfileMutationResult? = null
        updateConfiguration { current ->
            val profile = newProfile(displayLabel, rawBaseUrl)
            ensureCanAdd(current, profile)
            result = ProfileMutationResult.Created(profile)
            ProfileConfiguration(
                profiles = current.profiles + profile,
                activeProfileId = current.activeProfileId ?: profile.profileId,
            )
        }
        return checkNotNull(result)
    }

    override suspend fun updateProfile(
        profileId: ServerProfileId,
        displayLabel: String,
        rawBaseUrl: String,
    ): ProfileMutationResult {
        var result: ProfileMutationResult? = null
        updateConfiguration { current ->
            val existing = current.profiles.firstOrNull { it.profileId == profileId }
                ?: throw ProfileOperationException(ProfileOperationError.PROFILE_NOT_FOUND)
            val origin = parseOrigin(rawBaseUrl)
            val edited = existing.edit(displayLabel, origin)
            if (current.profiles.any { it.profileId != profileId && it.canonicalBaseUrl.value == edited.canonicalBaseUrl.value }) {
                throw ProfileOperationException(ProfileOperationError.DUPLICATE_ORIGIN)
            }
            result = if (existing.canonicalBaseUrl == edited.canonicalBaseUrl) {
                ProfileMutationResult.UpdatedLabelOnly(edited)
            } else {
                ProfileMutationResult.UpdatedOrigin(edited)
            }
            ProfileConfiguration(
                profiles = current.profiles.map { if (it.profileId == profileId) edited else it },
                activeProfileId = current.activeProfileId,
            )
        }
        return checkNotNull(result)
    }

    override suspend fun selectActiveProfile(profileId: ServerProfileId) {
        updateConfiguration { current ->
            if (current.profiles.none { it.profileId == profileId }) {
                throw ProfileOperationException(ProfileOperationError.PROFILE_NOT_FOUND)
            }
            current.copy(activeProfileId = profileId)
        }
    }

    override suspend fun removeProfile(profileId: ServerProfileId) {
        updateConfiguration { current ->
            if (current.profiles.none { it.profileId == profileId }) {
                throw ProfileOperationException(ProfileOperationError.PROFILE_NOT_FOUND)
            }
            val remaining = current.profiles.filterNot { it.profileId == profileId }
            val nextActive = when {
                remaining.isEmpty() -> null
                current.activeProfileId != profileId -> current.activeProfileId
                else -> remaining.first().profileId
            }
            current.copy(profiles = remaining, activeProfileId = nextActive)
        }
    }

    private suspend fun updateConfiguration(
        transform: (ProfileConfiguration) -> ProfileConfiguration,
    ) {
        try {
            dataStore.edit { preferences ->
                val updated = transform(decode(preferences))
                validateConfiguration(updated)
                encode(preferences, updated)
            }
        } catch (error: ProfileOperationException) {
            throw error
        } catch (error: CanonicalOriginException) {
            throw error
        } catch (error: DisplayLabelException) {
            throw error
        } catch (_: ProfileConfigurationException) {
            throw ProfileOperationException(ProfileOperationError.INVALID_PERSISTED_CONFIGURATION)
        } catch (_: Exception) {
            throw ProfileOperationException(ProfileOperationError.STORAGE_UNAVAILABLE)
        }
    }

    private fun newProfile(displayLabel: String, rawBaseUrl: String): ServerProfile {
        val origin = parseOrigin(rawBaseUrl)
        return try {
            ServerProfile.create(
                profileId = idGenerator(),
                displayLabel = displayLabel,
                canonicalBaseUrl = origin,
                createdAt = nowMillis(),
            )
        } catch (error: DisplayLabelException) {
            throw error
        }
    }

    private fun parseOrigin(rawBaseUrl: String): CanonicalServerOrigin = try {
        CanonicalServerOrigin.parse(rawBaseUrl, allowLoopbackTestHttp)
    } catch (error: CanonicalOriginException) {
        throw error
    }

    private fun ensureCanAdd(current: ProfileConfiguration, profile: ServerProfile) {
        if (current.profiles.size >= MAX_PROFILES) {
            throw ProfileOperationException(ProfileOperationError.PROFILE_LIMIT_REACHED)
        }
        if (current.profiles.any { it.canonicalBaseUrl.value == profile.canonicalBaseUrl.value }) {
            throw ProfileOperationException(ProfileOperationError.DUPLICATE_ORIGIN)
        }
    }

    private fun decode(preferences: Preferences): ProfileConfiguration {
        val schemaVersion = preferences[schemaVersionKey] ?: PROFILE_SCHEMA_VERSION
        if (schemaVersion != PROFILE_SCHEMA_VERSION) {
            throw ProfileConfigurationException(ProfileConfigurationError.UNSUPPORTED_SCHEMA_VERSION)
        }
        val jsonValue = preferences[configurationKey] ?: return ProfileConfiguration(emptyList(), null)
        val persisted = try {
            json.decodeFromString<PersistedConfiguration>(jsonValue)
        } catch (_: SerializationException) {
            throw ProfileConfigurationException(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION)
        }
        if (persisted.schemaVersion != PROFILE_SCHEMA_VERSION) {
            throw ProfileConfigurationException(ProfileConfigurationError.UNSUPPORTED_SCHEMA_VERSION)
        }
        val profiles = persisted.profiles.map { record ->
            val profileId = try {
                ServerProfileId.parse(record.profileId)
            } catch (_: IllegalArgumentException) {
                throw ProfileConfigurationException(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION)
            }
            val policy = try {
                TransportPolicy.valueOf(record.transportPolicy)
            } catch (_: IllegalArgumentException) {
                throw ProfileConfigurationException(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION)
            }
            val origin = try {
                CanonicalServerOrigin.fromPersisted(record.canonicalBaseUrl, policy, allowLoopbackTestHttp)
            } catch (_: CanonicalOriginException) {
                throw ProfileConfigurationException(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION)
            }
            try {
                ServerProfile.fromPersisted(
                    profileId = profileId,
                    displayLabel = record.displayLabel,
                    canonicalBaseUrl = origin,
                    createdAt = record.createdAt,
                    lastConnectedAt = record.lastConnectedAt,
                )
            } catch (_: Exception) {
                throw ProfileConfigurationException(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION)
            }
        }
        val activeProfileId = persisted.activeProfileId?.let {
            try {
                ServerProfileId.parse(it)
            } catch (_: IllegalArgumentException) {
                throw ProfileConfigurationException(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION)
            }
        }
        val configuration = ProfileConfiguration(profiles, activeProfileId)
        try {
            validateConfiguration(configuration)
        } catch (_: Exception) {
            throw ProfileConfigurationException(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION)
        }
        return configuration
    }

    private fun encode(preferences: MutablePreferences, configuration: ProfileConfiguration) {
        preferences[schemaVersionKey] = PROFILE_SCHEMA_VERSION
        preferences[configurationKey] = json.encodeToString(
            PersistedConfiguration(
                schemaVersion = PROFILE_SCHEMA_VERSION,
                profiles = configuration.profiles.map { profile ->
                    PersistedProfile(
                        profileId = profile.profileId.toString(),
                        canonicalBaseUrl = profile.canonicalBaseUrl.value,
                        transportPolicy = profile.transportPolicy.name,
                        displayLabel = profile.displayLabel,
                        createdAt = profile.createdAt,
                        lastConnectedAt = profile.lastConnectedAt,
                    )
                },
                activeProfileId = configuration.activeProfileId?.toString(),
            ),
        )
    }

    private fun validateConfiguration(configuration: ProfileConfiguration) {
        if (configuration.profiles.size > MAX_PROFILES) {
            throw ProfileOperationException(ProfileOperationError.PROFILE_LIMIT_REACHED)
        }
        if (configuration.profiles.map { it.profileId }.toSet().size != configuration.profiles.size) {
            throw ProfileOperationException(ProfileOperationError.INVALID_PERSISTED_CONFIGURATION)
        }
        if (configuration.profiles.map { it.canonicalBaseUrl.value }.toSet().size != configuration.profiles.size) {
            throw ProfileOperationException(ProfileOperationError.DUPLICATE_ORIGIN)
        }
        if (configuration.activeProfileId != null && configuration.profiles.none { it.profileId == configuration.activeProfileId }) {
            throw ProfileOperationException(ProfileOperationError.ACTIVE_PROFILE_REQUIRED)
        }
    }

    private fun Throwable.configurationReason(): ProfileConfigurationError = when (this) {
        is ProfileConfigurationException -> reason
        else -> ProfileConfigurationError.STORAGE_UNAVAILABLE
    }

    private class ProfileConfigurationException(
        val reason: ProfileConfigurationError,
    ) : IllegalStateException(reason.name)

    @Serializable
    private data class PersistedConfiguration(
        val schemaVersion: Int,
        val profiles: List<PersistedProfile>,
        val activeProfileId: String?,
    )

    @Serializable
    private data class PersistedProfile(
        val profileId: String,
        val canonicalBaseUrl: String,
        val transportPolicy: String,
        val displayLabel: String,
        val createdAt: Long,
        val lastConnectedAt: Long?,
    )

    private companion object {
        val schemaVersionKey = intPreferencesKey("schema_version")
        val configurationKey = stringPreferencesKey("profile_configuration_json")
        val json = Json {
            encodeDefaults = true
            explicitNulls = true
            ignoreUnknownKeys = false
        }
    }
}
