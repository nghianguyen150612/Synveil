package com.synveil.android.data.profile

import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.assertThrows
import org.junit.Test
import java.nio.file.Files
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.core.model.MAX_PROFILES
import com.synveil.android.core.model.RandomBytes

class ServerProfileRepositoryTest {
    @Test
    fun profilesPersistAcrossRepositoryRecreationAndFirstProfileBecomesActive() = runBlocking {
        val fixture = Fixture()
        val first = fixture.repository.addProfile("Primary", "https://example.com")
        val firstProfile = (first as ProfileMutationResult.Created).profile
        fixture.repository.addProfile("Backup", "https://backup.example.com")

        val recreated = fixture.repository()
        val state = recreated.state.first()

        assertTrue(state is ProfileRepositoryState.Configured)
        val configuration = (state as ProfileRepositoryState.Configured).configuration
        assertEquals(firstProfile.profileId, configuration.activeProfileId)
        assertEquals(2, configuration.profiles.size)
        assertEquals("https://example.com/", configuration.profiles.first().canonicalBaseUrl.value)
        fixture.close()
    }

    @Test
    fun duplicateOriginsAreRejectedAndLabelsDoNotDeduplicate() = runBlocking {
        val fixture = Fixture()
        fixture.repository.addProfile("Primary", "https://example.com")

        assertThrows(ProfileOperationException::class.java) {
            runBlocking { fixture.repository.addProfile("Different label", "https://example.com/") }
        }
        fixture.close()
    }

    @Test
    fun editsPreserveIdentityAndClassifyOriginChanges() = runBlocking {
        val fixture = Fixture()
        val created = fixture.repository.addProfile("Primary", "https://example.com") as ProfileMutationResult.Created
        val labelEdit = fixture.repository.updateProfile(created.profile.profileId, "Renamed", "https://example.com/")
        assertTrue(labelEdit is ProfileMutationResult.UpdatedLabelOnly)
        assertEquals(created.profile.profileId, labelEdit.profile.profileId)

        val originEdit = fixture.repository.updateProfile(created.profile.profileId, "Renamed", "https://changed.example.com")
        assertTrue(originEdit is ProfileMutationResult.UpdatedOrigin)
        assertEquals(created.profile.profileId, originEdit.profile.profileId)
        assertNull(originEdit.profile.lastConnectedAt)
        fixture.close()
    }

    @Test
    fun activeSelectionAndRemovalMaintainInvariant() = runBlocking {
        val fixture = Fixture()
        val first = (fixture.repository.addProfile("One", "https://one.example.com") as ProfileMutationResult.Created).profile
        val second = (fixture.repository.addProfile("Two", "https://two.example.com") as ProfileMutationResult.Created).profile
        fixture.repository.selectActiveProfile(second.profileId)
        fixture.repository.removeProfile(second.profileId)

        var configuration = (fixture.repository.state.first() as ProfileRepositoryState.Configured).configuration
        assertEquals(first.profileId, configuration.activeProfileId)
        fixture.repository.removeProfile(first.profileId)
        configuration = when (val state = fixture.repository.state.first()) {
            ProfileRepositoryState.NoServerConfigured -> ProfileConfiguration(emptyList(), null)
            is ProfileRepositoryState.Configured -> state.configuration
            is ProfileRepositoryState.ConfigurationError -> error("unexpected configuration error")
        }
        assertTrue(configuration.profiles.isEmpty())
        assertNull(configuration.activeProfileId)
        fixture.close()
    }

    @Test
    fun invalidPersistedDataFailsClosedWithoutInventingProfiles() = runBlocking {
        val fixture = Fixture()
        fixture.store.edit { preferences ->
            preferences[intPreferencesKey("schema_version")] = PROFILE_SCHEMA_VERSION
            preferences[stringPreferencesKey("profile_configuration_json")] = "not-json"
        }

        val state = fixture.repository.state.first()
        assertEquals(
            ProfileRepositoryState.ConfigurationError(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION),
            state,
        )
        fixture.close()
    }

    @Test
    fun persistedSchemaContainsOnlyNonSecretProfileFields() = runBlocking {
        val fixture = Fixture()
        fixture.repository.addProfile("Primary", "https://example.com")
        val serialized = fixture.store.data.first()[stringPreferencesKey("profile_configuration_json")]!!

        assertTrue(serialized.contains("profileId"))
        assertTrue(serialized.contains("canonicalBaseUrl"))
        assertFalse(serialized.contains("password"))
        assertFalse(serialized.contains("session"))
        assertFalse(serialized.contains("bearer"))
        assertFalse(serialized.contains("token"))
        fixture.close()
    }

    @Test
    fun loopbackPolicyIsAcceptedOnlyWhenRepositoryIsExplicitlyDevelopmentConfigured() = runBlocking {
        val fixture = Fixture(allowLoopbackTestHttp = true)
        val created = fixture.repository.addProfile("Local", "http://127.0.0.1:8080")
        assertEquals("http://127.0.0.1:8080/", created.profile.canonicalBaseUrl.value)
        val releaseRepository = DataStoreServerProfileRepository(fixture.store, allowLoopbackTestHttp = false)
        assertEquals(
            ProfileRepositoryState.ConfigurationError(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION),
            releaseRepository.state.first(),
        )
        fixture.close()
    }

    @Test
    fun persistedConfigurationRejectsAnInvalidActiveProfileReference() = runBlocking {
        val fixture = Fixture()
        fixture.store.edit { preferences ->
            preferences[intPreferencesKey("schema_version")] = PROFILE_SCHEMA_VERSION
            preferences[stringPreferencesKey("profile_configuration_json")] =
                "{\"schemaVersion\":1,\"profiles\":[],\"activeProfileId\":\"018bcfe5-687b-7001-8203-040506070809\"}"
        }

        assertEquals(
            ProfileRepositoryState.ConfigurationError(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION),
            fixture.repository.state.first(),
        )
        fixture.close()
    }

    @Test
    fun persistedProfileCountAboveBoundFailsClosed() = runBlocking {
        val fixture = Fixture()
        val records = (0..MAX_PROFILES).joinToString(",") { index ->
            val id = ServerProfileId.new(
                timestampMillis = { 1_700_000_000_000L + index },
                randomBytes = object : RandomBytes {
                    override fun nextBytes(size: Int): ByteArray = ByteArray(size) { (it + index).toByte() }
                },
            )
            "{\"profileId\":\"$id\",\"canonicalBaseUrl\":\"https://$index.example.com/\",\"transportPolicy\":\"HTTPS\",\"displayLabel\":\"Profile $index\",\"createdAt\":1700000000000,\"lastConnectedAt\":null}"
        }
        fixture.store.edit { preferences ->
            preferences[intPreferencesKey("schema_version")] = PROFILE_SCHEMA_VERSION
            preferences[stringPreferencesKey("profile_configuration_json")] =
                "{\"schemaVersion\":1,\"profiles\":[$records],\"activeProfileId\":null}"
        }

        assertEquals(
            ProfileRepositoryState.ConfigurationError(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION),
            fixture.repository.state.first(),
        )
        fixture.close()
    }

    private class Fixture(
        private val allowLoopbackTestHttp: Boolean = false,
    ) {
        private val directory = Files.createTempDirectory("synveil-profile-test")
        val store = PreferenceDataStoreFactory.create {
            directory.resolve("profiles.preferences_pb").toFile()
        }
        private var nextId = 0

        fun repository(): DataStoreServerProfileRepository = DataStoreServerProfileRepository(
            dataStore = store,
            allowLoopbackTestHttp = allowLoopbackTestHttp,
            idGenerator = {
                ServerProfileId.new(
                    timestampMillis = { 1_700_000_000_000L + nextId++ },
                    randomBytes = object : com.synveil.android.core.model.RandomBytes {
                        override fun nextBytes(size: Int): ByteArray = ByteArray(size) { (it + nextId).toByte() }
                    },
                )
            },
            nowMillis = { 1_700_000_000_000L },
        )

        val repository: DataStoreServerProfileRepository
            get() = repository()

        fun close() {
            directory.toFile().deleteRecursively()
        }
    }
}
