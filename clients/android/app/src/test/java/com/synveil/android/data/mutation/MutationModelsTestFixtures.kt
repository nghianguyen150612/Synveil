package com.synveil.android.data.mutation

import com.synveil.android.data.cache.MutationQueueEntity

object MutationModelsTestFixtures {
    fun renameEntity(id: String, revision: String, newName: String) = MutationQueueEntity(
        profileId = "profile-a", deviceId = "device-a", libraryId = "library-a", mutationId = id,
        kind = MutationKind.RENAME_NODE.name, resourceId = "node-a", parentDependencyId = null,
        baseEpoch = "1", baseSequence = "2", payloadJson = "{\"node_id\":\"node-a\",\"expected_revision\":\"$revision\",\"new_name\":\"$newName\"}",
        createdAt = 1, state = "PENDING", attemptCount = 0, lastAttemptAt = null, lastErrorCategory = null,
        journalEventId = null, journalSequence = null, conflictId = null, localFingerprint = "fingerprint-$id",
    )
}
