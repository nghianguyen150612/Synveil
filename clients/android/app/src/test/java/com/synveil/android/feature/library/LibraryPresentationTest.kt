package com.synveil.android.feature.library

import com.synveil.android.data.library.LibraryFailure
import com.synveil.android.data.network.SynveilTransportError
import com.synveil.android.data.node.NodeFailure
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
