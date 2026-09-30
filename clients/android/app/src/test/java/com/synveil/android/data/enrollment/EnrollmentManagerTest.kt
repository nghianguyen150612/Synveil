package com.synveil.android.data.enrollment

import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EnrollmentManagerTest {
    @Test
    fun tokensAreStrictAndSecretStringsAreRedacted() {
        val valid = "sve1_" + "a".repeat(64)
        val invalid = listOf(
            "sve1_" + "A".repeat(64),
            "sve1_" + "a".repeat(63),
            "sve1_" + "a".repeat(64) + "x",
            "sve1_" + "a".repeat(64) + "\n",
        )
        assertTrue(EnrollmentToken.isValid(valid))
        invalid.forEach { assertTrue(!EnrollmentToken.isValid(it)) }
        assertEquals("[REDACTED_ENROLLMENT_TOKEN]", EnrollmentToken.parse(valid).toString())
        assertEquals(
            "[REDACTED_DEVICE_CREDENTIAL]",
            DeviceCredential.parse("svd1_" + "b".repeat(64)).toString(),
        )
    }

    @Test
    fun preflightFailureSendsNoExchangeRequest() = runBlocking {
        var exchanges = 0
        val manager = EnrollmentManager(
            RecordingMetadataStore(),
            InMemoryCredentialVault(failPreflight = true),
        )
        val state = manager.enroll(
            profile(),
            "sve1_" + "a".repeat(64),
            object : EnrollmentExchangeClient {
                override fun exchange(token: EnrollmentToken): EnrollmentExchangeResult {
                    exchanges += 1
                    error("exchange must not run")
                }
            },
        )
        assertEquals(EnrollmentStateKind.SECURE_STORE_UNAVAILABLE, state.kind)
        assertEquals(0, exchanges)
    }

    @Test
    fun successfulFinalizationRequiresWriteReadBackAndMetadataCommit() = runBlocking {
        val metadata = RecordingMetadataStore()
        val vault = InMemoryCredentialVault()
        val manager = EnrollmentManager(metadata, vault)
        val state = manager.enroll(
            profile(),
            "sve1_" + "a".repeat(64),
            object : EnrollmentExchangeClient {
                override fun exchange(token: EnrollmentToken) = EnrollmentExchangeResult.Success(record())
            },
        )
        assertEquals(EnrollmentStateKind.ENROLLED, state.kind)
        assertTrue(metadata.operations == listOf("pending", "active"))
        assertEquals(null, metadata.pending(profile().profileId.toString()))
        assertTrue(metadata.active(profile().profileId.toString()) != null)
    }

    @Test
    fun pendingMetadataWithMissingSecretRequiresRecovery() = runBlocking {
        val metadata = RecordingMetadataStore()
        val profile = profile()
        metadata.writePending(metadataFor(profile))
        val state = EnrollmentManager(metadata, InMemoryCredentialVault()).recover(profile)
        assertEquals(EnrollmentStateKind.RECOVERY_REQUIRED, state.kind)
    }

    @Test
    fun pendingMetadataWithValidSecretRecoversToActive() = runBlocking {
        val metadata = RecordingMetadataStore()
        val vault = InMemoryCredentialVault()
        val profile = profile()
        val record = record()
        val scope = CredentialScope.forProfile(profile, record)
        metadata.writePending(metadataFor(profile))
        vault.store(scope, record)

        val state = EnrollmentManager(metadata, vault).recover(profile)

        assertEquals(EnrollmentStateKind.ENROLLED, state.kind)
        assertEquals(null, metadata.pending(profile.profileId.toString()))
        assertTrue(metadata.active(profile.profileId.toString()) != null)
    }

    @Test
    fun writeAndReadBackFailuresRemainRecoveryRequired() = runBlocking {
        val profile = profile()
        val token = "sve1_" + "a".repeat(64)
        val writeFailure = EnrollmentManager(RecordingMetadataStore(), InMemoryCredentialVault(failWrite = true))
            .enroll(profile, token, successClient())
        val readFailure = EnrollmentManager(RecordingMetadataStore(), InMemoryCredentialVault(failRead = true))
            .enroll(profile, token, successClient())

        assertEquals(EnrollmentStateKind.RECOVERY_REQUIRED, writeFailure.kind)
        assertEquals(EnrollmentStateKind.RECOVERY_REQUIRED, readFailure.kind)
    }

    @Test
    fun localForgetDoesNotClaimServerRevocation() = runBlocking {
        val metadata = RecordingMetadataStore()
        val manager = EnrollmentManager(metadata, InMemoryCredentialVault())
        val state = manager.enroll(
            profile(),
            "sve1_" + "a".repeat(64),
            object : EnrollmentExchangeClient {
                override fun exchange(token: EnrollmentToken) = EnrollmentExchangeResult.Success(record())
            },
        )
        assertEquals(EnrollmentStateKind.ENROLLED, state.kind)
        assertTrue(manager.forget(profile()) is CredentialCleanupResult.Success)
        assertEquals(null, metadata.active(profile().profileId.toString()))
    }

    private fun profile() = ServerProfile.create(
        profileId = ServerProfileId.parse("018bcfe5-687b-7001-8203-040506070809"),
        displayLabel = "Test",
        canonicalBaseUrl = CanonicalServerOrigin.parse("https://example.com", false),
        createdAt = 1_700_000_000_000L,
    )

    private fun record() = DeviceCredentialRecord(
        ownerUserId = "018bcfe5-687b-7001-8203-040506070810",
        deviceId = "018bcfe5-687b-7001-8203-040506070811",
        credentialId = "018bcfe5-687b-7001-8203-040506070812",
        credential = checkNotNull(DeviceCredential.parse("svd1_" + "b".repeat(64))),
        createdAt = "2026-09-30T00:00:00Z",
        requestId = "request-1",
    )

    private fun metadataFor(profile: ServerProfile) = EnrollmentMetadata(
        profileId = profile.profileId.toString(),
        canonicalBaseUrl = profile.canonicalBaseUrl.value,
        transportPolicy = profile.transportPolicy.name,
        ownerUserId = "018bcfe5-687b-7001-8203-040506070810",
        deviceId = "018bcfe5-687b-7001-8203-040506070811",
        credentialId = "018bcfe5-687b-7001-8203-040506070812",
        createdAt = "2026-09-30T00:00:00Z",
    )

    private fun successClient() = object : EnrollmentExchangeClient {
        override fun exchange(token: EnrollmentToken) = EnrollmentExchangeResult.Success(record())
    }

    private class RecordingMetadataStore : EnrollmentMetadataStore {
        private var pendingRecord: EnrollmentMetadata? = null
        private var activeRecord: EnrollmentMetadata? = null
        val operations = mutableListOf<String>()

        override suspend fun pending(profileId: String) = pendingRecord?.takeIf { it.profileId == profileId }
        override suspend fun active(profileId: String) = activeRecord?.takeIf { it.profileId == profileId }
        override suspend fun writePending(metadata: EnrollmentMetadata) {
            operations += "pending"
            pendingRecord = metadata
        }
        override suspend fun commitActive(metadata: EnrollmentMetadata) {
            operations += "active"
            activeRecord = metadata
            pendingRecord = null
        }
        override suspend fun clear(profileId: String) {
            pendingRecord = null
            activeRecord = null
        }
    }
}
