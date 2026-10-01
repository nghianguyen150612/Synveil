package com.synveil.android.data.mutation

import com.synveil.android.data.cache.MutationQueueEntity
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MutationModelsTest {
    @Test
    fun uuidV7HasCanonicalVersionAndVariant() {
        val id = newUuidV7()
        assertEquals('7', id[14])
        assertTrue(id[19] in "89ab")
    }

    @Test
    fun typedPayloadIsClosedAndReplayRequestIsStable() {
        val entity = MutationQueueEntity(
            profileId = "p", deviceId = "d", libraryId = "l", mutationId = "00000000-0000-7000-8000-000000000000",
            kind = MutationKind.RENAME_NODE.name, resourceId = "node", parentDependencyId = null,
            baseEpoch = "2", baseSequence = "9", payloadJson = "{\"node_id\":\"node\",\"expected_revision\":\"3\",\"new_name\":\"next\"}",
            createdAt = 1, state = MutationState.OUTCOME_UNKNOWN.name, attemptCount = 1, lastAttemptAt = 2,
            lastErrorCategory = "timeout", journalEventId = null, journalSequence = null, conflictId = null,
            localFingerprint = "fingerprint",
        )
        val first = entity.toRequest().json()
        val second = entity.toRequest().json()
        assertEquals(first, second)
        assertFalse(first.contains("OUTCOME_UNKNOWN"))
        assertTrue(first.contains("mutation_id"))
        assertTrue(first.contains("expected_revision"))
    }
}
