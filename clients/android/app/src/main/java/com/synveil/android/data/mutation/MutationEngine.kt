package com.synveil.android.data.mutation

import com.synveil.android.data.cache.CacheRepository
import com.synveil.android.data.cache.CachedNodeEntity
import com.synveil.android.data.cache.CachedConflictEntity
import com.synveil.android.data.cache.MutationQueueEntity
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.network.AuthenticatedSynveilTransport
import com.synveil.android.data.network.SynveilTransportError
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

sealed interface QueueResult {
    data class Enqueued(val mutationId: String) : QueueResult
    data class Rejected(val reason: QueueRejection) : QueueResult
}

enum class QueueRejection { MISSING_SYNC_BASE, INVALID_NAME, RESOURCE_BUSY, ALREADY_TRASHED, INVALID_PARENT }

data class MutationDrainResult(val attempted: Int, val applied: Int, val conflicts: Int, val blocked: Int, val transientFailure: Boolean)

fun interface MutationSubmitter {
    fun submit(deviceId: String, libraryId: String, request: MutationRequest): MutationResult
}

class MutationEngine internal constructor(
    private val cache: CacheRepository,
    private val submitter: MutationSubmitter,
    private val profileId: String,
    private val deviceId: String,
    private val libraryId: LibraryId,
) {
    constructor(
        cache: CacheRepository,
        transport: AuthenticatedSynveilTransport,
        profileId: String,
        deviceId: String,
        libraryId: LibraryId,
    ) : this(cache, MutationSubmitter(transport::submitMutation), profileId, deviceId, libraryId)

    private val mutex = Mutex()

    suspend fun enqueue(intent: MutationIntent): QueueResult {
        val state = cache.state(profileId, deviceId, libraryId)
            ?: return QueueResult.Rejected(QueueRejection.MISSING_SYNC_BASE)
        val epoch = state.journalEpoch ?: return QueueResult.Rejected(QueueRejection.MISSING_SYNC_BASE)
        if (intent is MutationIntent.CreateDirectory && intent.name.isBlank()) return QueueResult.Rejected(QueueRejection.INVALID_NAME)
        if (cache.hasOutstandingDependency(profileId, deviceId, libraryId, intent.resourceId, intent.parentDependencyId)) {
            return QueueResult.Rejected(QueueRejection.RESOURCE_BUSY)
        }
        val id = newUuidV7()
        val payload = intent.payload().toString()
        val entity = MutationQueueEntity(
            profileId = profileId,
            deviceId = deviceId,
            libraryId = libraryId.value,
            mutationId = id,
            kind = intent.kind.name,
            resourceId = intent.resourceId,
            parentDependencyId = intent.parentDependencyId,
            baseEpoch = epoch,
            baseSequence = state.serverAcknowledgedSequence,
            payloadJson = payload,
            createdAt = System.currentTimeMillis(),
            state = MutationState.PENDING.name,
            attemptCount = 0,
            lastAttemptAt = null,
            lastErrorCategory = null,
            journalEventId = null,
            journalSequence = null,
            conflictId = null,
            localFingerprint = intent.fingerprint(epoch, state.serverAcknowledgedSequence),
        )
        cache.enqueueMutation(entity)
        return QueueResult.Enqueued(id)
    }

    suspend fun drain(maxOperations: Int = 16): MutationDrainResult = mutex.withLock {
        val queue = cache.eligibleMutations(profileId, deviceId, libraryId, maxOperations)
        var applied = 0
        var conflicts = 0
        var blocked = 0
        var transient = false
        queue.forEach { current ->
            val submitting = current.copy(
                state = MutationState.SUBMITTING.name,
                attemptCount = current.attemptCount + 1,
                lastAttemptAt = System.currentTimeMillis(),
            )
            cache.updateMutation(submitting)
            when (val result = submitter.submit(deviceId, libraryId.value, current.toRequest())) {
                is MutationResult.Applied -> {
                    if (result.node.libraryId != libraryId.value || result.mutationId != current.mutationId || result.kind.name != current.kind) {
                        cache.updateMutation(submitting.copy(state = MutationState.FAILED_PERMANENT.name, lastErrorCategory = "invalid_mutation_result"))
                    } else {
                        val appliedEntity = submitting.copy(
                            state = MutationState.APPLIED.name,
                            journalEventId = result.journalEventId,
                            journalSequence = result.journalSequence,
                            lastErrorCategory = null,
                        )
                        cache.applyMutationResult(appliedEntity, result.node.toEntity(profileId))
                        applied++
                    }
                }
                is MutationResult.Conflict -> {
                    cache.updateMutation(submitting.copy(state = MutationState.CONFLICT.name, conflictId = result.conflictId, lastErrorCategory = "mutation_conflict"))
                    result.conflictId?.let { conflictId ->
                        cache.upsertConflict(CachedConflictEntity(profileId, deviceId, libraryId.value, conflictId, current.mutationId, current.kind, current.resourceId, "mutation_conflict", "OPEN", "", System.currentTimeMillis()))
                    }
                    conflicts++
                }
                is MutationResult.Failure -> {
                    val error = result.error
                    when {
                        isRebaseline(error) -> {
                            cache.updateMutation(submitting.copy(state = MutationState.BLOCKED_REBASELINE.name, lastErrorCategory = "sync_rebaseline_required"))
                            blocked++
                        }
                        isAmbiguous(error) -> {
                            cache.updateMutation(submitting.copy(state = MutationState.OUTCOME_UNKNOWN.name, lastErrorCategory = error.category()))
                            transient = true
                        }
                        isTransient(error) -> {
                            cache.updateMutation(submitting.copy(state = MutationState.PENDING.name, lastErrorCategory = error.category()))
                            transient = true
                        }
                        else -> cache.updateMutation(submitting.copy(state = MutationState.FAILED_PERMANENT.name, lastErrorCategory = error.category()))
                    }
                }
            }
        }
        MutationDrainResult(queue.size, applied, conflicts, blocked, transient)
    }

    private fun isRebaseline(error: SynveilTransportError) =
        error is SynveilTransportError.HttpError && (error.code == "sync_rebaseline_required" || error.code == "checkpoint_conflict")

    private fun isAmbiguous(error: SynveilTransportError) = error is SynveilTransportError.Timeout || error is SynveilTransportError.Offline || error is SynveilTransportError.DnsFailure
    private fun isTransient(error: SynveilTransportError) = isAmbiguous(error) || (error is SynveilTransportError.HttpError && error.statusCode == 503)
    private fun SynveilTransportError.category() = when (this) {
        SynveilTransportError.Offline -> "offline"
        SynveilTransportError.DnsFailure -> "dns"
        SynveilTransportError.Timeout -> "timeout"
        is SynveilTransportError.HttpError -> code ?: "http_$statusCode"
        else -> "transport"
    }
}

private fun MutationNodeResult.toEntity(profileId: String) = CachedNodeEntity(
    profileId = profileId,
    libraryId = libraryId,
    nodeId = nodeId,
    parentNodeId = parentNodeId,
    currentVersionId = currentVersionId,
    revision = revision,
    name = name,
    kind = kind,
    state = state,
    createdAt = createdAt,
    updatedAt = updatedAt,
    trashedAt = trashedAt,
    restoreDeadline = null,
    purgeEligible = false,
    byteLength = null,
    sha256 = null,
)
