package com.synveil.android.feature.startup

import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.data.profile.ProfileConfiguration
import com.synveil.android.data.profile.ProfileConfigurationError
import com.synveil.android.data.profile.ProfileRepositoryState
import com.synveil.android.data.session.DeviceSessionState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertTrue
import org.junit.Test

class StartupStateTest {
    @Test
    fun foundationStateIdentifiesTheAppAndOffersProfileSetup() {
        val state = StartupUiState.foundation()

        assertEquals("Synveil", state.identity.displayName)
        assertEquals("Android client", state.identity.releaseChannel)
        assertEquals("No server profile configured", state.statusMessage)
        assertEquals(StartupPrimaryAction.CONFIGURE_PROFILE, state.primaryAction)
        assertEquals("Set up server profile", state.primaryActionLabel)
        assertEquals(
            "Authenticated library access uses the enrolled DeviceBearer. Conflict inspection and resolution remain owner/web review.",
            state.productBoundaryMessage,
        )
    }

    @Test
    fun eachFoundationStateIsDeterministicAndIndependent() {
        val first = StartupUiState.foundation()
        val second = StartupUiState.foundation()

        assertEquals(first, second)
        assertNotSame(first, second)
    }

    @Test
    fun startupPresentationDistinguishesConfiguredFromConnected() {
        val profile = ServerProfile.create(
            profileId = ServerProfileId.new(
                timestampMillis = { 1_700_000_000_000L },
                randomBytes = object : com.synveil.android.core.model.RandomBytes {
                    override fun nextBytes(size: Int): ByteArray = ByteArray(size)
                },
            ),
            displayLabel = "Primary",
            canonicalBaseUrl = CanonicalServerOrigin.parse("https://example.com", false),
            createdAt = 1_700_000_000_000L,
        )

        val configured = startupUiStateFor(
            ProfileRepositoryState.Configured(
                ProfileConfiguration(listOf(profile), profile.profileId),
            ),
        )
        val noServer = startupUiStateFor(ProfileRepositoryState.NoServerConfigured)
        val error = startupUiStateFor(
            ProfileRepositoryState.ConfigurationError(ProfileConfigurationError.INVALID_PERSISTED_CONFIGURATION),
        )

        assertEquals("Server configured: Primary", configured.statusMessage)
        assertEquals(StartupConfigurationState.ServerConfigured("Primary"), configured.configurationState)
        assertEquals("No server profile configured", noServer.statusMessage)
        assertEquals("Server configuration needs attention", error.statusMessage)
    }

    @Test
    fun sessionStatesMapToSafePrimaryActions() {
        val profile = profile()
        val states = listOf(
            DeviceSessionState.NotEnrolled(profile.profileId.toString(), "Primary") to StartupPrimaryAction.ENROLL_DEVICE,
            DeviceSessionState.RecoveryRequired(profile.profileId.toString(), "Primary") to StartupPrimaryAction.RECOVER_ENROLLMENT,
            DeviceSessionState.AuthenticationRequired(profile.profileId.toString(), "Primary") to StartupPrimaryAction.REENROLL_DEVICE,
            DeviceSessionState.DeviceRevoked(profile.profileId.toString(), "Primary") to StartupPrimaryAction.REENROLL_DEVICE,
            DeviceSessionState.ServerUnavailable(profile.profileId.toString(), "Primary") to StartupPrimaryAction.MANAGE_PROFILE,
            DeviceSessionState.TlsError(profile.profileId.toString(), "Primary") to StartupPrimaryAction.MANAGE_PROFILE,
            DeviceSessionState.SecureStoreUnavailable(profile.profileId.toString(), "Primary") to StartupPrimaryAction.MANAGE_PROFILE,
            DeviceSessionState.ProtocolError(profile.profileId.toString(), "Primary") to StartupPrimaryAction.MANAGE_PROFILE,
            DeviceSessionState.Ready(profile.profileId.toString(), "Primary") to StartupPrimaryAction.OPEN_LIBRARIES,
        )

        states.forEach { (session, action) ->
            val state = startupUiStateFor(
                ProfileRepositoryState.Configured(ProfileConfiguration(listOf(profile), profile.profileId)),
                session,
            )
            assertEquals(action, state.primaryAction)
            assertEquals("Primary", state.sessionState.displayLabel)
            assertTrue(!state.statusMessage.contains("svd1_"))
        }
    }

    @Test
    fun sessionErrorMessagesNeverIncludeCredentialMaterial() {
        val profile = profile()
        val state = startupUiStateFor(
            ProfileRepositoryState.Configured(ProfileConfiguration(listOf(profile), profile.profileId)),
            DeviceSessionState.AuthenticationRequired(profile.profileId.toString(), "Primary"),
        )

        assertEquals("Device authentication required", state.statusMessage)
        assertEquals(StartupPrimaryAction.REENROLL_DEVICE, state.primaryAction)
        assertTrue(!state.statusMessage.contains("svd1_"))
    }

    @Test
    fun staleSessionStateFromPreviousProfileCannotAuthorizeTheActiveProfile() {
        val active = profile("018bcfe5-687b-7001-8203-040506070809", "Active")
        val previous = profile("018bcfe5-687b-7001-8203-040506070810", "Previous")

        val state = startupUiStateFor(
            ProfileRepositoryState.Configured(ProfileConfiguration(listOf(active, previous), active.profileId)),
            DeviceSessionState.Ready(previous.profileId.toString(), previous.displayLabel),
        )

        assertEquals(StartupSessionState.ProfileConfigured(active.profileId.toString(), "Active"), state.sessionState)
        assertEquals(StartupPrimaryAction.MANAGE_PROFILE, state.primaryAction)
        assertEquals("Server configured: Active", state.statusMessage)
    }

    private fun profile(id: String = "018bcfe5-687b-7001-8203-040506070809", label: String = "Primary"): ServerProfile = ServerProfile.create(
        profileId = ServerProfileId.parse(id),
        displayLabel = label,
        canonicalBaseUrl = CanonicalServerOrigin.parse("https://example.com", false),
        createdAt = 1_700_000_000_000L,
    )
}
