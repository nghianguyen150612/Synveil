package com.synveil.android.feature.library

import com.synveil.android.data.library.LibraryFailure
import com.synveil.android.data.network.SynveilTransportError
import com.synveil.android.data.node.NodeFailure
import com.synveil.android.data.settings.SyncSchedulerOutcome
import com.synveil.android.data.settings.SyncSchedulerState
import com.synveil.android.data.transfer.TransferProgress
import org.junit.Assert.assertEquals
import org.junit.Test

class LibraryPresentationTest {
    @Test
    fun syncStatesHaveTruthfulUserMessages() {
        assertEquals("Never synced", librarySyncStateLabel(null))
        assertEquals("Up to date", librarySyncStateLabel("READY"))
        assertEquals("Rebaseline required", librarySyncStateLabel("REBASELINE_REQUIRED"))
        assertEquals("Offline / retry pending", librarySyncStateLabel("ERROR_TRANSIENT"))
        assertEquals("Sync error", librarySyncStateLabel("unexpected"))
    }

    @Test
    fun libraryLayoutUsesOneColumnOnPhonesAndMoreOnLargeWindows() {
        assertEquals(1, libraryColumnCount(599))
        assertEquals(2, libraryColumnCount(600))
        assertEquals(3, libraryColumnCount(840))
    }

    @Test
    fun schedulerStatesExplainRecoveryWithoutProtocolDetails() {
        assertEquals("Background sync is up to date.", librarySchedulerStateLabel(null))
        assertEquals(
            "Background sync is up to date.",
            librarySchedulerStateLabel(SyncSchedulerState("profile", "library", SyncSchedulerOutcome.SUCCESS, 1L, 1L, null)),
        )
        assertEquals(
            "Background sync will retry.",
            librarySchedulerStateLabel(SyncSchedulerState("profile", "library", SyncSchedulerOutcome.RETRY, 1L, null, "timeout")),
        )
        assertEquals(
            "Background sync is paused until authentication is recovered.",
            librarySchedulerStateLabel(SyncSchedulerState("profile", "library", SyncSchedulerOutcome.PAUSED_AUTH, 1L, null, "authentication_failed")),
        )
        assertEquals(
            "This device was revoked; re-enrollment is required.",
            librarySchedulerStateLabel(SyncSchedulerState("profile", "library", SyncSchedulerOutcome.REVOKED, 1L, null, "device_revoked")),
        )
        assertEquals(
            "Last attempt: 1970-01-01T00:00:00.001Z · Last success: 1970-01-01T00:00:00.002Z · Error: transient_error",
            librarySchedulerDetailLabel(SyncSchedulerState("profile", "library", SyncSchedulerOutcome.RETRY, 1L, 2L, "transient_error")),
        )
    }

    @Test
    fun libraryAndNodeFailuresRemainActionableWithoutProtocolDetails() {
        assertEquals("Library request failed.", libraryFailureMessage(LibraryFailure.Transport(SynveilTransportError.Timeout)))
        assertEquals("The server returned an invalid pagination cursor.", libraryFailureMessage(LibraryFailure.RepeatedCursor))
        assertEquals("Authentication is required.", nodeFailureMessage(NodeFailure.Transport(SynveilTransportError.HttpError(401, "authentication_failed", null))))
        assertEquals("The server returned an invalid directory page.", nodeFailureMessage(NodeFailure.ResourceLimit))
    }

    @Test
    fun transferStatesExplainProgressAndRecovery() {
        assertEquals("Preparing transfer…", transferProgressLabel(TransferProgress.Preparing))
        assertEquals("Downloading 4/8 bytes", transferProgressLabel(TransferProgress.Downloading(4, 8)))
        assertEquals("Transfer cancelled", transferProgressLabel(TransferProgress.Cancelled))
        assertEquals("Transfer failed", transferProgressLabel(TransferProgress.Failed("Transfer failed")))
    }
}
