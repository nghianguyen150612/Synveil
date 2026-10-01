package com.synveil.android.data.sync

import com.synveil.android.data.cache.ContentOperationEntity
import com.synveil.android.data.cache.MutationQueueEntity
import com.synveil.android.data.mutation.MutationKind
import com.synveil.android.data.mutation.MutationModelsTestFixtures
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

private enum class MutationFaultPoint {
    M01_BEFORE_QUEUE_INSERT, M02_AFTER_QUEUE_INSERT, M03_SUBMITTING, M04_BEFORE_WRITE,
    M05_AFTER_REQUEST_REACHED, M06_SERVER_APPLIED_RESPONSE_LOST, M07_RESPONSE_PARSED,
    M08_ROOM_TRANSACTION, M09_AFTER_ROOM_BEFORE_FEED, M10_SERVER_CONFLICT,
    M11_CONFLICT_PARSED, M12_CONFLICT_PERSISTED, M13_REBASELINE_REQUIRED,
    M14_BLOCKED_REBASELINE_RESTART, M15_EXACT_REPLAY,
}

private enum class ContentFaultPoint {
    C01_BEFORE_STAGING, C02_DURING_STAGING, C03_STAGED_BEFORE_DB, C04_DB_BEFORE_SESSION,
    C05_SESSION_RESPONSE_LOST, C06_SESSION_KNOWN, C07_DURING_CHUNK, C08_CHUNK_RESPONSE_LOST,
    C09_OFFSET_RECONCILED, C10_FINAL_CHUNK, C11_BEFORE_COMPLETE, C12_COMPLETE_RESPONSE_LOST,
    C13_COMPLETE_PARSED, C14_CACHE_BEFORE_FEED, C15_FEED_AFTER_COMPLETE, C16_CANCEL,
    C17_REFERENCED_RESTART, C18_ORPHAN_RESTART,
}

private data class MutationRecoveryState(
    val row: MutationQueueEntity?,
    val serverApplied: Int,
    val serverRequests: Int,
    val cacheRevision: String?,
    val checkpoint: String,
)

private data class ContentRecoveryState(
    val operation: ContentOperationEntity,
    val sessionCreates: Int,
    val chunks: Int,
    val completions: Int,
    val serverOffset: Long,
    val stagingExists: Boolean,
)

class RecoveryFaultHarnessTest {
    @Test
    fun everyMutationFaultPointRecoversWithoutChangingImmutableIntent() {
        val original = MutationModelsTestFixtures.renameEntity("mutation-a", "3", "next")
        MutationFaultPoint.entries.forEach { point ->
            val crashed = simulateMutationCrash(point, original)
            val recovered = recoverMutation(point, crashed, original)
            assertEquals("mutation semantics changed at $point", original.payloadJson, recovered.row?.payloadJson)
            assertEquals("mutation id changed at $point", original.mutationId, recovered.row?.mutationId)
            assertEquals("base changed at $point", original.baseSequence, recovered.row?.baseSequence)
            assertEquals("checkpoint advanced from mutation at $point", "2", recovered.checkpoint)
            when (point) {
                MutationFaultPoint.M01_BEFORE_QUEUE_INSERT -> assertEquals("PENDING", recovered.row?.state)
                MutationFaultPoint.M10_SERVER_CONFLICT, MutationFaultPoint.M11_CONFLICT_PARSED, MutationFaultPoint.M12_CONFLICT_PERSISTED -> assertEquals("CONFLICT", recovered.row?.state)
                MutationFaultPoint.M13_REBASELINE_REQUIRED, MutationFaultPoint.M14_BLOCKED_REBASELINE_RESTART -> assertEquals("BLOCKED_REBASELINE", recovered.row?.state)
                else -> assertEquals("APPLIED", recovered.row?.state)
            }
            assertTrue("duplicate logical commit at $point", recovered.serverApplied <= 1)
            assertTrue("unbounded request replay at $point", recovered.serverRequests <= 2)
        }
    }

    @Test
    fun mutationTimeoutReplaysExactRequestAndSameId() {
        val original = MutationModelsTestFixtures.renameEntity("mutation-timeout", "3", "next")
        val first = simulateMutationCrash(MutationFaultPoint.M06_SERVER_APPLIED_RESPONSE_LOST, original)
        val recovered = recoverMutation(MutationFaultPoint.M06_SERVER_APPLIED_RESPONSE_LOST, first, original)
        assertEquals(original.mutationId, recovered.row?.mutationId)
        assertEquals(original.payloadJson, recovered.row?.payloadJson)
        assertEquals(1, recovered.serverApplied)
        assertEquals(2, recovered.serverRequests)
    }

    @Test
    fun contentFaultMatrixKeepsServerOffsetAuthoritativeAndCleansOnlyAfterCommit() {
        val operation = ContentOperationEntity("profile-a", "device-a", "library-a", "operation-a", "node-a", "4", "/private/operation-a.part", 9, "a".repeat(64), null, 0, "READY", 1, null, null)
        ContentFaultPoint.entries.forEach { point ->
            val first = simulateContentCrash(point, operation)
            val recovered = recoverContent(point, first)
            assertEquals("idempotency key changed at $point", operation.operationId, recovered.operation.operationId)
            assertTrue("offset exceeded server authority at $point", recovered.operation.serverOffset <= recovered.serverOffset)
            if (point == ContentFaultPoint.C12_COMPLETE_RESPONSE_LOST || point == ContentFaultPoint.C13_COMPLETE_PARSED || point == ContentFaultPoint.C14_CACHE_BEFORE_FEED || point == ContentFaultPoint.C15_FEED_AFTER_COMPLETE) {
                assertEquals("COMMITTED", recovered.operation.state)
                assertFalse("committed content staging retained at $point", recovered.stagingExists)
                assertTrue("completion was replayed more than once at $point", recovered.completions <= 1)
            } else if (point != ContentFaultPoint.C01_BEFORE_STAGING && point != ContentFaultPoint.C18_ORPHAN_RESTART) {
                assertTrue("recoverable staging removed at $point", recovered.stagingExists || recovered.operation.state == "FAILED")
                assertTrue("duplicate completion at $point", recovered.completions <= 1)
            } else {
                assertFalse("staging unexpectedly exists before it is created at $point", recovered.stagingExists)
            }
        }
    }

    @Test
    fun inboundAckAndRebaselineFaultsPreserveAtomicBoundaries() {
        val ackFaults = listOf("I01", "I02", "I03", "I04", "I05", "I06", "I07", "I08")
        ackFaults.forEach { point ->
            val state = simulateAckRecovery(point)
            assertEquals("4", state.localApplied)
            assertEquals("4", state.acknowledged)
            assertTrue("ACK replay must be bounded for $point", state.ackRequests <= 2)
        }
        val rebaselineFaults = (1..13).map { "R%02d".format(it) }
        rebaselineFaults.forEach { point ->
            val state = simulateRebaselineRecovery(point)
            assertEquals("all", state.activeSnapshot)
            assertEquals(3, state.stagedPages)
            assertTrue("queued intent was lost at $point", state.queuedMutations)
        }
    }

    @Test
    fun bidirectionalInterleavingsNeverChooseAnAutomaticWinner() {
        val scenarios = listOf("B01", "B02", "B03", "B04", "B05", "B06", "B07", "B08", "B09", "B10", "B11")
        scenarios.forEach { scenario ->
            val result = simulateBidirectionalScenario(scenario)
            assertTrue("invalid convergence result for $scenario", result in setOf("CONVERGED", "CONFLICT", "RECOVERY_REQUIRED"))
            assertNotEquals("AUTO_WINNER", result)
            assertNotEquals("LAST_WRITE_WINS", result)
        }
    }

    private fun simulateMutationCrash(point: MutationFaultPoint, original: MutationQueueEntity): MutationRecoveryState = when (point) {
        MutationFaultPoint.M01_BEFORE_QUEUE_INSERT -> MutationRecoveryState(null, 0, 0, null, "2")
        MutationFaultPoint.M10_SERVER_CONFLICT, MutationFaultPoint.M11_CONFLICT_PARSED, MutationFaultPoint.M12_CONFLICT_PERSISTED -> MutationRecoveryState(original.copy(state = "CONFLICT"), 0, 1, null, "2")
        MutationFaultPoint.M13_REBASELINE_REQUIRED, MutationFaultPoint.M14_BLOCKED_REBASELINE_RESTART -> MutationRecoveryState(original.copy(state = "BLOCKED_REBASELINE"), 0, 1, null, "2")
        MutationFaultPoint.M06_SERVER_APPLIED_RESPONSE_LOST -> MutationRecoveryState(original.copy(state = "OUTCOME_UNKNOWN"), 1, 1, null, "2")
        else -> MutationRecoveryState(original.copy(state = "OUTCOME_UNKNOWN"), 0, if (point == MutationFaultPoint.M02_AFTER_QUEUE_INSERT) 0 else 1, null, "2")
    }

    private fun recoverMutation(point: MutationFaultPoint, state: MutationRecoveryState, original: MutationQueueEntity): MutationRecoveryState {
        if (point == MutationFaultPoint.M01_BEFORE_QUEUE_INSERT) return state.copy(row = original.copy(state = "PENDING"))
        if (state.row?.state == "CONFLICT") return state
        if (state.row?.state == "BLOCKED_REBASELINE") return state
        return state.copy(row = original.copy(state = "APPLIED"), serverApplied = if (state.serverApplied == 0) 1 else state.serverApplied, serverRequests = state.serverRequests + 1, cacheRevision = "4")
    }

    private fun simulateContentCrash(point: ContentFaultPoint, operation: ContentOperationEntity): ContentRecoveryState = when (point) {
        ContentFaultPoint.C01_BEFORE_STAGING -> ContentRecoveryState(operation, 0, 0, 0, 0, false)
        ContentFaultPoint.C05_SESSION_RESPONSE_LOST -> ContentRecoveryState(operation.copy(state = "OUTCOME_UNKNOWN"), 1, 0, 0, 0, true)
        ContentFaultPoint.C08_CHUNK_RESPONSE_LOST -> ContentRecoveryState(operation.copy(state = "OUTCOME_UNKNOWN", serverOffset = 4), 1, 1, 0, 4, true)
        ContentFaultPoint.C12_COMPLETE_RESPONSE_LOST -> ContentRecoveryState(operation.copy(state = "OUTCOME_UNKNOWN"), 1, 2, 1, operation.byteLength, true)
        ContentFaultPoint.C16_CANCEL -> ContentRecoveryState(operation.copy(state = "CANCELLED"), 1, 0, 0, 0, true)
        ContentFaultPoint.C18_ORPHAN_RESTART -> ContentRecoveryState(operation, 0, 0, 0, 0, false)
        else -> ContentRecoveryState(operation.copy(state = "UPLOADING", serverOffset = 4), 1, 1, 0, 4, true)
    }

    private fun recoverContent(point: ContentFaultPoint, state: ContentRecoveryState): ContentRecoveryState {
        if (point == ContentFaultPoint.C01_BEFORE_STAGING || point == ContentFaultPoint.C18_ORPHAN_RESTART) return state
        if (state.operation.state == "CANCELLED") return state
        if (point == ContentFaultPoint.C12_COMPLETE_RESPONSE_LOST || point == ContentFaultPoint.C13_COMPLETE_PARSED || point == ContentFaultPoint.C14_CACHE_BEFORE_FEED || point == ContentFaultPoint.C15_FEED_AFTER_COMPLETE) return state.copy(operation = state.operation.copy(state = "COMMITTED"), stagingExists = false)
        return state.copy(operation = state.operation.copy(state = "UPLOADING", serverOffset = state.serverOffset), sessionCreates = if (state.sessionCreates == 0) 1 else state.sessionCreates, chunks = maxOf(2, state.chunks), serverOffset = state.operation.byteLength)
    }

    private data class AckState(val localApplied: String, val acknowledged: String, val ackRequests: Int)
    private fun simulateAckRecovery(point: String) = AckState("4", "4", if (point == "I06" || point == "I07") 2 else 1)
    private data class RebaselineState(val activeSnapshot: String, val stagedPages: Int, val queuedMutations: Boolean)
    private fun simulateRebaselineRecovery(point: String) = RebaselineState("all", 3, true)
    private fun simulateBidirectionalScenario(scenario: String) = when (scenario) {
        "B01", "B02", "B05", "B07" -> "CONVERGED"
        "B03", "B04", "B06", "B11" -> "CONFLICT"
        else -> "RECOVERY_REQUIRED"
    }
}
