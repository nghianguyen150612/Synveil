package com.synveil.android.work

import com.synveil.android.data.settings.SyncSchedulerOutcome
import com.synveil.android.data.sync.SyncOutcomeKind
import org.junit.Assert.assertEquals
import org.junit.Test

class SyncWorkerTest {
    @Test
    fun mapsOutcomesToTruthfulSchedulerActions() {
        assertEquals(SyncSchedulerOutcome.SUCCESS, SyncWorkScheduler.decision(SyncOutcomeKind.SUCCESS, 0).outcome)
        assertEquals(SyncWorkerAction.SUCCESS, SyncWorkScheduler.decision(SyncOutcomeKind.SUCCESS, 0).action)
        assertEquals(SyncSchedulerOutcome.RETRY, SyncWorkScheduler.decision(SyncOutcomeKind.MORE_WORK, 0).outcome)
        assertEquals(SyncWorkerAction.RETRY, SyncWorkScheduler.decision(SyncOutcomeKind.TRANSIENT_ERROR, 0).action)
        assertEquals(SyncSchedulerOutcome.REBASELINE_REQUIRED, SyncWorkScheduler.decision(SyncOutcomeKind.REBASELINE_REQUIRED, 0).outcome)
        assertEquals(SyncSchedulerOutcome.PAUSED_AUTH, SyncWorkScheduler.decision(SyncOutcomeKind.AUTHENTICATION_REQUIRED, 0).outcome)
        assertEquals(SyncWorkerAction.FAILURE, SyncWorkScheduler.decision(SyncOutcomeKind.AUTHENTICATION_REQUIRED, 0).action)
        assertEquals(SyncSchedulerOutcome.REVOKED, SyncWorkScheduler.decision(SyncOutcomeKind.DEVICE_REVOKED, 0).outcome)
        assertEquals(SyncSchedulerOutcome.PROTOCOL_ERROR, SyncWorkScheduler.decision(SyncOutcomeKind.PROTOCOL_ERROR, 0).outcome)
    }

    @Test
    fun automaticRetriesAreBounded() {
        val decision = SyncWorkScheduler.decision(
            SyncOutcomeKind.TRANSIENT_ERROR,
            SyncWorkScheduler.MAX_AUTOMATIC_RETRIES,
        )
        assertEquals(SyncSchedulerOutcome.RETRY, decision.outcome)
        assertEquals(SyncWorkerAction.FAILURE, decision.action)
    }
}
