package com.synveil.android.feature.startup

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
            "Authentication, networking, sync, uploads, backups, and file browsing are not implemented yet.",
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
}
