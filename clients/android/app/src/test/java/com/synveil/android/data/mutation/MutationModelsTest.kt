package com.synveil.android.data.mutation

import com.synveil.android.data.cache.MutationQueueEntity
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

private const val TEST_NODE = "018bcfe5-687b-7001-8203-040506070801"
private const val TEST_PARENT = "018bcfe5-687b-7001-8203-040506070802"

class MutationModelsTest {
    @Test
    fun everyMutationKindSerializesOnlyItsTypedPayload() {
        val intents = listOf(
            MutationIntent.CreateDirectory(checkNotNull(com.synveil.android.data.library.NodeId.parse(TEST_PARENT)), "4", "new-folder"),
            MutationIntent.RenameNode(checkNotNull(com.synveil.android.data.library.NodeId.parse(TEST_NODE)), "5", "renamed"),
            MutationIntent.MoveNode(checkNotNull(com.synveil.android.data.library.NodeId.parse(TEST_NODE)), "5", checkNotNull(com.synveil.android.data.library.NodeId.parse(TEST_PARENT)), "6"),
            MutationIntent.TrashNode(checkNotNull(com.synveil.android.data.library.NodeId.parse(TEST_NODE)), "5"),
            MutationIntent.RestoreNode(checkNotNull(com.synveil.android.data.library.NodeId.parse(TEST_NODE)), "5", checkNotNull(com.synveil.android.data.library.NodeId.parse(TEST_PARENT)), "6"),
        )

        assertEquals(
            setOf("parent_node_id", "expected_parent_revision", "name"),
            intents[0].payload().keys,
        )
        assertEquals(setOf("node_id", "expected_revision", "new_name"), intents[1].payload().keys)
        assertEquals(setOf("node_id", "expected_revision", "new_parent_node_id", "expected_new_parent_revision"), intents[2].payload().keys)
        assertEquals(setOf("node_id", "expected_revision"), intents[3].payload().keys)
        assertEquals(setOf("node_id", "expected_revision", "expected_parent_node_id", "expected_parent_revision"), intents[4].payload().keys)
    }

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
