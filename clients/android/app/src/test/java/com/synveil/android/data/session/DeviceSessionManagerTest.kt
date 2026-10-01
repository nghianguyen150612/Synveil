package com.synveil.android.data.session

import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.data.enrollment.CredentialCleanupResult
import com.synveil.android.data.enrollment.CredentialScope
import com.synveil.android.data.enrollment.CredentialVaultException
import com.synveil.android.data.enrollment.CredentialVaultFailure
import com.synveil.android.data.enrollment.DeviceCredential
import com.synveil.android.data.enrollment.DeviceCredentialRecord
import com.synveil.android.data.enrollment.EnrollmentMetadata
import com.synveil.android.data.enrollment.EnrollmentMetadataStore
import com.synveil.android.data.enrollment.SecureCredentialVault
import com.synveil.android.data.library.LibraryRepositoryResult
import com.synveil.android.data.profile.ProfileConfiguration
import com.synveil.android.data.profile.ProfileMutationResult
import com.synveil.android.data.profile.ProfileRepositoryState
import com.synveil.android.data.profile.ServerProfileRepository
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.runBlocking
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class DeviceSessionManagerTest {
    private val servers = mutableListOf<MockWebServer>()

    @After
    fun tearDown() {
        servers.forEach { it.shutdown() }
    }

    @Test
    fun loadsCredentialOnlyThroughVaultAndSwitchesProfileBinding() = runBlocking {
        val serverA = server()
        val serverB = server()
        serverA.enqueue(jsonPage())
        serverB.enqueue(jsonPage())
        val profileA = profile("018bcfe5-687b-7001-8203-040506070809", serverA)
        val profileB = profile("018bcfe5-687b-7001-8203-04050607080a", serverB)
        val metadata = RecordingMetadataStore()
        val vault = com.synveil.android.data.enrollment.InMemoryCredentialVault()
        val recordA = record("a")
        val recordB = record("c")
        vault.store(CredentialScope.forProfile(profileA, recordA), recordA)
        vault.store(CredentialScope.forProfile(profileB, recordB), recordB)
        metadata.active[profileA.profileId.toString()] = metadata(profileA, recordA)
        metadata.active[profileB.profileId.toString()] = metadata(profileB, recordB)
        val profiles = MutableStateFlow<ProfileRepositoryState>(configured(profileA, profileB, profileA))
        val manager = manager(profiles, metadata, vault)

        assertTrue(manager.listLibraries() is LibraryRepositoryResult.Loaded)
        profiles.value = configured(profileA, profileB, profileB)
        assertTrue(manager.listLibraries() is LibraryRepositoryResult.Loaded)

        assertEquals("Bearer svd1_${"a".repeat(64)}", serverA.takeRequest().getHeader("Authorization"))
        assertEquals("Bearer svd1_${"c".repeat(64)}", serverB.takeRequest().getHeader("Authorization"))
        assertNull(serverA.takeRequest(200, TimeUnit.MILLISECONDS))
        assertNull(serverB.takeRequest(200, TimeUnit.MILLISECONDS))
        assertTrue(manager.state.value.toString().contains(profileB.profileId.toString()))
        assertTrue(!manager.state.value.toString().contains("svd1_"))
    }

    @Test
    fun missingCredentialAndVaultFailureAreNotAnonymousFallbacks() = runBlocking {
        val server = server()
        val profile = profile("018bcfe5-687b-7001-8203-040506070809", server)
        val profiles = MutableStateFlow<ProfileRepositoryState>(configured(profile, profile, profile))
        val metadata = RecordingMetadataStore()
        val record = record("a")
        metadata.active[profile.profileId.toString()] = metadata(profile, record)

        val missing = manager(profiles, metadata, com.synveil.android.data.enrollment.InMemoryCredentialVault())
        assertTrue(missing.listLibraries() is LibraryRepositoryResult.Failed)
        assertTrue(missing.state.value is DeviceSessionState.RecoveryRequired)

        val unavailable = manager(profiles, metadata, FailingVault())
        assertTrue(unavailable.listLibraries() is LibraryRepositoryResult.Failed)
        assertTrue(unavailable.state.value is DeviceSessionState.SecureStoreUnavailable)
        assertEquals(0, server.requestCount)
    }

    private fun manager(
        profiles: Flow<ProfileRepositoryState>,
        metadata: RecordingMetadataStore,
        vault: SecureCredentialVault,
    ) = DeviceSessionManager(
        profileRepository = FakeProfileRepository(profiles),
        metadataStore = metadata,
        vault = vault,
        userAgent = "Synveil Android/test",
        allowLoopbackTestHttp = true,
        scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined),
    )

    private fun configured(a: ServerProfile, b: ServerProfile, active: ServerProfile) =
        ProfileRepositoryState.Configured(
            ProfileConfiguration(listOf(a, b).distinctBy { it.profileId }, active.profileId),
        )

    private fun profile(id: String, server: MockWebServer) = ServerProfile.create(
        profileId = ServerProfileId.parse(id),
        displayLabel = id.takeLast(4),
        canonicalBaseUrl = CanonicalServerOrigin.parse(
            server.url("/").toString().replace("localhost", "127.0.0.1"),
            allowLoopbackTestHttp = true,
        ),
        createdAt = 1_700_000_000_000L,
    )

    private fun record(seed: String) = DeviceCredentialRecord(
        ownerUserId = "018bcfe5-687b-7001-8203-040506070810",
        deviceId = "018bcfe5-687b-7001-8203-040506070811",
        credentialId = "018bcfe5-687b-7001-8203-040506070812",
        credential = checkNotNull(DeviceCredential.parse("svd1_" + seed.repeat(64))),
        createdAt = "2026-09-30T00:00:00Z",
        requestId = "request-01",
    )

    private fun metadata(profile: ServerProfile, record: DeviceCredentialRecord) = EnrollmentMetadata(
        profileId = profile.profileId.toString(),
        canonicalBaseUrl = profile.canonicalBaseUrl.value,
        transportPolicy = profile.transportPolicy.name,
        ownerUserId = record.ownerUserId,
        deviceId = record.deviceId,
        credentialId = record.credentialId,
        createdAt = record.createdAt,
    )

    private fun jsonPage() = MockResponse()
        .setResponseCode(200)
        .setHeader("Content-Type", "application/json")
        .setBody("{\"data\":[],\"page\":{\"has_more\":false},\"meta\":{\"request_id\":\"request-01\"}}")

    private fun server() = MockWebServer().also {
        it.start()
        servers += it
    }

    private class RecordingMetadataStore : EnrollmentMetadataStore {
        val active = mutableMapOf<String, EnrollmentMetadata>()
        override suspend fun pending(profileId: String): EnrollmentMetadata? = null
        override suspend fun active(profileId: String): EnrollmentMetadata? = active[profileId]
        override suspend fun writePending(metadata: EnrollmentMetadata) = Unit
        override suspend fun commitActive(metadata: EnrollmentMetadata) { active[metadata.profileId] = metadata }
        override suspend fun clear(profileId: String) { active.remove(profileId) }
    }

    private class FailingVault : SecureCredentialVault {
        override suspend fun preflight(profileId: String) = Unit
        override suspend fun store(scope: CredentialScope, record: DeviceCredentialRecord) = Unit
        override suspend fun load(profileId: String, expectedScope: CredentialScope): DeviceCredentialRecord? {
            throw CredentialVaultFailure(CredentialVaultException.Unavailable)
        }
        override suspend fun delete(profileId: String) = Unit
    }

    private class FakeProfileRepository(
        override val state: Flow<ProfileRepositoryState>,
    ) : ServerProfileRepository {
        override suspend fun addProfile(displayLabel: String, rawBaseUrl: String): ProfileMutationResult = error("unused")
        override suspend fun updateProfile(profileId: ServerProfileId, displayLabel: String, rawBaseUrl: String): ProfileMutationResult = error("unused")
        override suspend fun selectActiveProfile(profileId: ServerProfileId) = error("unused")
        override suspend fun removeProfile(profileId: ServerProfileId) = error("unused")
        override suspend fun recordSuccessfulConnection(profileId: ServerProfileId, connectedAt: Long) = error("unused")
    }
}
