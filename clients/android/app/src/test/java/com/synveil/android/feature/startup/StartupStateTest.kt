package com.synveil.android.feature.startup

import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.data.profile.ProfileConfiguration
import com.synveil.android.data.profile.ProfileConfigurationError
import com.synveil.android.data.profile.ProfileRepositoryState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotSame
import org.junit.Test

class StartupStateTest {
    @Test
    fun foundationStateIdentifiesTheAppWithoutClaimingProductFeatures() {
        val state = StartupUiState.foundation()

        assertEquals("Synveil", state.identity.displayName)
        assertEquals("Development foundation", state.identity.releaseChannel)
        assertEquals("Native Android foundation ready", state.statusMessage)
        assertEquals(
            "Device authentication, file browsing, and foreground transfers are available; sync, backups, replace-content editing, and background transfer remain future work.",
            state.unsupportedFeaturesMessage,
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
        assertEquals("No server configured", noServer.statusMessage)
        assertEquals("Server configuration needs attention", error.statusMessage)
    }
}
