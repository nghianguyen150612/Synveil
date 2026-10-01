package com.synveil.android.data.sync

import com.synveil.android.data.cache.CacheRepository
import com.synveil.android.data.cache.CachedNodeEntity
import com.synveil.android.data.cache.PendingAckEntity
import com.synveil.android.data.cache.RebaselineNodeEntity
import com.synveil.android.data.cache.RebaselineStagingEntity
import com.synveil.android.data.cache.SyncStateEntity
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.library.NodeId
import com.synveil.android.data.network.AuthenticatedSynveilTransport
import com.synveil.android.data.network.NodePageResult
import com.synveil.android.data.network.ProtocolErrorKind
import com.synveil.android.data.network.SynveilTransportError
import com.synveil.android.data.node.Node
import java.math.BigInteger
import java.time.OffsetDateTime
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

enum class SyncOutcomeKind {
    SUCCESS,
    MORE_WORK,
    REBASELINE_REQUIRED,
    AUTHENTICATION_REQUIRED,
    DEVICE_REVOKED,
    TRANSIENT_ERROR,
    PROTOCOL_ERROR,
}

data class SyncOutcome(
    val kind: SyncOutcomeKind,
    val error: SynveilTransportError? = null,
)

class SyncEngine(
    private val transport: AuthenticatedSynveilTransport,
    private val cache: CacheRepository,
    private val profileId: String,
    private val deviceId: String,
    private val libraryId: LibraryId,
) {
    private val mutex = scopeMutex(profileId, deviceId, libraryId.value)

    suspend fun synchronize(): SyncOutcome = mutex.withLock {
        val initial = cache.state(profileId, deviceId, libraryId)
        val pending = cache.pendingAck(profileId, deviceId, libraryId)
        if (pending != null) {
            val replay = acknowledgePending(initial, pending)
            if (replay != null) return@withLock replay
        }

        val staging = cache.staging(profileId, deviceId, libraryId)
        if (staging != null) return@withLock rebaselineLocked(staging)

        val existing = cache.state(profileId, deviceId, libraryId) ?: initialState()
        val checkpoint = when (val result = transport.getSyncCheckpoint(deviceId, libraryId.value)) {
            is SyncResult.Success -> result.value
            is SyncResult.Failure -> return@withLock mapFailure(result.error, existing)
        }
        val localSequence = parseU64(existing.locallyAppliedSequence) ?: return@withLock protocolFailure(existing)
        val acknowledgedSequence = parseU64(existing.serverAcknowledgedSequence) ?: return@withLock protocolFailure(existing)
        if (existing.journalEpoch != null && existing.journalEpoch != checkpoint.epoch.value) {
            return@withLock markRebaseline(existing, "epoch_mismatch")
        }
        if (localSequence != acknowledgedSequence || acknowledgedSequence != checkpoint.acknowledgedSequence) {
            return@withLock markRebaseline(existing, "checkpoint_conflict")
        }

        var appliedSequence = localSequence
        var pageCount = 0
        var eventCount = 0
        var currentState = existing.copy(
            journalEpoch = checkpoint.epoch.value,
            state = SyncStateKind.SYNCING.name,
            lastAttemptAt = now(),
            lastErrorCode = null,
        )
        cache.upsertState(currentState)

        while (pageCount < MAX_SYNC_PAGES_PER_RUN && eventCount <= MAX_SYNC_EVENTS_PER_RUN) {
            pageCount += 1
            when (val result = transport.fetchSyncChanges(deviceId, libraryId.value, SYNC_PAGE_SIZE)) {
                is SyncResult.Failure -> return@withLock mapFailure(result.error, currentState)
                is SyncResult.Success -> {
                    val page = result.value
                    if (page.epoch != checkpoint.epoch || page.fromSequence != appliedSequence) {
                        return@withLock markRebaseline(currentState, "invalid_feed_boundary")
                    }
                    if (page.changes.isEmpty()) {
                        if (page.hasMore || page.ackToken != null || page.throughSequence != appliedSequence) {
                            return@withLock protocolFailure(currentState)
                        }
                        cache.upsertState(currentState.copy(
                            locallyAppliedSequence = appliedSequence.value,
                            serverAcknowledgedSequence = appliedSequence.value,
                            lastHighWatermark = page.highWatermark.value,
                            lastSuccessfulSyncAt = now(),
                            lastAttemptAt = now(),
                            state = SyncStateKind.READY.name,
                            lastErrorCode = null,
                        ))
                        return@withLock SyncOutcome(SyncOutcomeKind.SUCCESS)
                    }

                    eventCount += page.changes.size
                    if (eventCount > MAX_SYNC_EVENTS_PER_RUN) return@withLock budgetReached(currentState)
                    val expectedThrough = add(appliedSequence, page.changes.size)
                    if (page.throughSequence != expectedThrough || page.ackToken == null) {
                        return@withLock protocolFailure(currentState)
                    }

                    val sequenceSet = HashSet<String>(page.changes.size)
                    val eventSet = HashSet<String>(page.changes.size)
                    val materialized = ArrayList<Node>()
                    val deleted = ArrayList<String>()
                    var lookupCount = 0
                    for (change in page.changes) {
                        if (!sequenceSet.add(change.sequence.value) || !eventSet.add(change.eventId)) {
                            return@withLock protocolFailure(currentState)
                        }
                        if (change.changeKind == ChangeKind.NODE_PURGED) {
                            deleted += change.resourceId
                            continue
                        }
                        lookupCount += 1
                        if (lookupCount > MAX_CANONICAL_NODE_LOOKUPS) return@withLock budgetReached(currentState)
                        val nodeId = NodeId.parse(change.resourceId) ?: return@withLock protocolFailure(currentState)
                        val node = when (val nodeResult = transport.getNode(nodeId)) {
                            is NodePageResult.Failure -> return@withLock mapFailure(nodeResult.error, currentState)
                            is NodePageResult.Success -> nodeResult.page.nodes.singleOrNull()
                                ?: return@withLock protocolFailure(currentState)
                        }
                        if (node.libraryId != libraryId) return@withLock protocolFailure(currentState)
                        val nodeRevision = parseU64(node.revision.value) ?: return@withLock protocolFailure(currentState)
                        if (nodeRevision < change.resourceRevision) return@withLock markRebaseline(currentState, "revision_regression")
                        val old = cache.activeNodes(profileId, libraryId).firstOrNull { it.nodeId == node.nodeId.value }
                        if (old != null) {
                            val oldRevision = parseU64(old.revision) ?: return@withLock protocolFailure(currentState)
                            if (nodeRevision < oldRevision) return@withLock markRebaseline(currentState, "revision_regression")
                        }
                        materialized += node
                    }

                    val nextState = currentState.copy(
                        locallyAppliedSequence = page.throughSequence.value,
                        lastHighWatermark = page.highWatermark.value,
                        state = SyncStateKind.ACK_PENDING.name,
                        lastAttemptAt = now(),
                    )
                    val pendingAck = PendingAckEntity(
                        profileId = profileId,
                        deviceId = deviceId,
                        libraryId = libraryId.value,
                        epoch = page.epoch.value,
                        fromSequence = page.fromSequence.value,
                        throughSequence = page.throughSequence.value,
                        highWatermark = page.highWatermark.value,
                        ackToken = page.ackToken,
                        createdAt = now(),
                    )
                    cache.applyFeedPage(materialized, deleted, nextState, pendingAck)

                    when (val ack = transport.acknowledgeSyncChanges(deviceId, libraryId.value, page.ackToken)) {
                        is SyncResult.Failure -> return@withLock mapFailure(ack.error, nextState)
                        is SyncResult.Success -> {
                            if (ack.value.epoch != page.epoch || ack.value.acknowledgedSequence != page.throughSequence) {
                                return@withLock markRebaseline(nextState, "checkpoint_conflict")
                            }
                            currentState = nextState.copy(
                                serverAcknowledgedSequence = page.throughSequence.value,
                                state = if (page.hasMore) SyncStateKind.SYNCING.name else SyncStateKind.READY.name,
                                lastSuccessfulSyncAt = if (page.hasMore) nextState.lastSuccessfulSyncAt else now(),
                                lastErrorCode = null,
                            )
                            cache.confirmAck(currentState, profileId, deviceId, libraryId)
                            appliedSequence = page.throughSequence
                            if (!page.hasMore) return@withLock SyncOutcome(SyncOutcomeKind.SUCCESS)
                        }
                    }
                }
            }
        }
        budgetReached(currentState)
    }

    suspend fun rebaseline(): SyncOutcome = mutex.withLock {
        rebaselineLocked(cache.staging(profileId, deviceId, libraryId))
    }

    private suspend fun acknowledgePending(
        state: SyncStateEntity?,
        pending: PendingAckEntity,
    ): SyncOutcome? {
        return when (val result = transport.acknowledgeSyncChanges(deviceId, libraryId.value, pending.ackToken)) {
            is SyncResult.Failure -> mapFailure(result.error, state ?: initialState())
            is SyncResult.Success -> {
                if (result.value.epoch.value != pending.epoch || result.value.acknowledgedSequence.value != pending.throughSequence) {
                    markRebaseline(state, "checkpoint_conflict")
                } else {
                    val updated = (state ?: initialState()).copy(
                        journalEpoch = pending.epoch,
                        locallyAppliedSequence = pending.throughSequence,
                        serverAcknowledgedSequence = pending.throughSequence,
                        lastHighWatermark = pending.highWatermark,
                        lastSuccessfulSyncAt = now(),
                        lastAttemptAt = now(),
                        lastErrorCode = null,
                        state = SyncStateKind.READY.name,
                    )
                    cache.confirmAck(updated, profileId, deviceId, libraryId)
                    null
                }
            }
        }
    }

    private suspend fun rebaselineLocked(existing: RebaselineStagingEntity?): SyncOutcome {
        var staging = existing
        val bootstrap: RebaselineBootstrap
        if (staging == null) {
            bootstrap = when (val result = transport.startRebaseline(deviceId, libraryId.value)) {
                is SyncResult.Failure -> return mapFailure(result.error, cache.state(profileId, deviceId, libraryId) ?: initialState())
                is SyncResult.Success -> result.value
            }
            if (bootstrap.state != "OPEN") return protocolFailure(cache.state(profileId, deviceId, libraryId))
            staging = RebaselineStagingEntity(
                profileId = profileId,
                deviceId = deviceId,
                libraryId = libraryId.value,
                bootstrapId = bootstrap.bootstrapId,
                generation = bootstrap.generation.value,
                snapshotEpoch = bootstrap.snapshotEpoch.value,
                snapshotResumeSequence = bootstrap.snapshotResumeSequence.value,
                manifestItemCount = bootstrap.manifestItemCount.value,
                nextCursor = null,
                completionToken = null,
                receivedItemCount = "0",
                state = SyncStateKind.REBASELINING.name,
                startedAt = bootstrap.createdAt.toString(),
                expiresAt = bootstrap.expiresAt.toString(),
            )
            cache.upsertStaging(staging)
            cache.upsertState((cache.state(profileId, deviceId, libraryId) ?: initialState()).copy(
                journalEpoch = bootstrap.snapshotEpoch.value,
                state = SyncStateKind.REBASELINING.name,
                lastAttemptAt = now(),
                lastErrorCode = null,
            ))
        } else {
            val recovered = staging.toBootstrap() ?: run {
                cache.deleteStaging(profileId, deviceId, libraryId)
                return rebaselineLocked(null)
            }
            if (staging.state == STAGED_PENDING_COMPLETE) {
                val token = staging.completionToken ?: return protocolFailure(cache.state(profileId, deviceId, libraryId))
                return finishRebaseline(recovered, token)
            }
            return fetchRebaseline(recovered, staging)
        }
        return fetchRebaseline(bootstrap, staging)
    }

    private suspend fun fetchRebaseline(
        bootstrap: RebaselineBootstrap,
        initialStaging: RebaselineStagingEntity,
    ): SyncOutcome {
        var staging = initialStaging
        var cursor = staging.nextCursor
        var pages = 0
        while (pages++ < MAX_REBASELINE_PAGES) {
            when (val result = transport.fetchRebaselinePage(deviceId, libraryId.value, bootstrap.bootstrapId, cursor, REBASELINE_PAGE_SIZE)) {
                is SyncResult.Failure -> {
                    if (isExpired(result.error)) cache.deleteStaging(profileId, deviceId, libraryId)
                    return mapFailure(result.error, cache.state(profileId, deviceId, libraryId) ?: initialState())
                }
                is SyncResult.Success -> {
                    val page = result.value
                    if (!sameBootstrap(page.bootstrap, bootstrap)) return protocolFailure(null)
                    if (page.nextCursor != null && page.nextCursor == cursor) return protocolFailure(null)
                    val existingIds = cache.stagingNodeIds(profileId, deviceId, libraryId, bootstrap.bootstrapId).toHashSet()
                    val pageIds = HashSet<String>(page.nodes.size)
                    if (page.nodes.any { !pageIds.add(it.nodeId) || existingIds.contains(it.nodeId) }) return protocolFailure(null)
                    if (existingIds.maxOrNull()?.let { previous -> page.nodes.firstOrNull()?.nodeId?.let { previous >= it } == true } == true) {
                        return protocolFailure(null)
                    }
                    val received = parseU64(staging.receivedItemCount) ?: return protocolFailure(null)
                    val nextReceived = add(received, page.nodes.size)
                    val declared = bootstrap.manifestItemCount
                    if (nextReceived > declared || nextReceived > U64Decimal.parse(MAX_REBASELINE_NODES.toString())!!) return protocolFailure(null)
                    val rows = page.nodes.map { it.toEntity(bootstrap.bootstrapId) }
                    val next = staging.copy(
                        nextCursor = page.nextCursor,
                        completionToken = page.completionToken,
                        receivedItemCount = nextReceived.value,
                        state = SyncStateKind.REBASELINING.name,
                    )
                    cache.persistRebaselinePage(next, rows)
                    staging = next
                    cursor = page.nextCursor
                    if (!page.hasMore) {
                        if (page.completionToken == null || nextReceived != declared) return protocolFailure(null)
                        val staged = cache.stagingNodes(profileId, deviceId, libraryId, bootstrap.bootstrapId)
                        val active = staged.map { it.toCachedNode() }
                        val swapped = next.copy(state = STAGED_PENDING_COMPLETE)
                        cache.installStaging(profileId, deviceId, libraryId, active, swapped)
                        return finishRebaseline(bootstrap, page.completionToken)
                    }
                }
            }
        }
        return budgetReached(cache.state(profileId, deviceId, libraryId) ?: initialState(), rebaseline = true)
    }

    private suspend fun finishRebaseline(bootstrap: RebaselineBootstrap, token: String): SyncOutcome {
        return when (val result = transport.completeRebaseline(deviceId, libraryId.value, bootstrap.bootstrapId, token)) {
            is SyncResult.Failure -> mapFailure(result.error, cache.state(profileId, deviceId, libraryId) ?: initialState())
            is SyncResult.Success -> {
                cache.upsertState((cache.state(profileId, deviceId, libraryId) ?: initialState()).copy(
                    journalEpoch = result.value.journalEpoch.value,
                    locallyAppliedSequence = result.value.acknowledgedSequence.value,
                    serverAcknowledgedSequence = result.value.acknowledgedSequence.value,
                    snapshotEpoch = result.value.journalEpoch.value,
                    snapshotResumeSequence = result.value.acknowledgedSequence.value,
                    state = SyncStateKind.READY.name,
                    lastSuccessfulSyncAt = now(),
                    lastAttemptAt = now(),
                    lastErrorCode = null,
                ))
                cache.deleteStaging(profileId, deviceId, libraryId)
                SyncOutcome(SyncOutcomeKind.SUCCESS)
            }
        }
    }

    private fun RebaselineStagingEntity.toBootstrap(): RebaselineBootstrap? = runCatching {
        val expires = expiresAt?.let(OffsetDateTime::parse) ?: return null
        RebaselineBootstrap(
            bootstrapId = bootstrapId,
            deviceId = deviceId,
            libraryId = libraryId,
            state = "OPEN",
            generation = parseU64(generation) ?: return null,
            snapshotEpoch = parseU64(snapshotEpoch) ?: return null,
            snapshotResumeSequence = parseU64(snapshotResumeSequence) ?: return null,
            manifestItemCount = parseU64(manifestItemCount) ?: return null,
            createdAt = OffsetDateTime.parse(startedAt),
            expiresAt = expires,
            completedAt = null,
        )
    }.getOrNull()

    private fun RebaselineNodeEntity.toCachedNode() = CachedNodeEntity(
        profileId = profileId,
        libraryId = libraryId,
        nodeId = nodeId,
        parentNodeId = parentNodeId,
        currentVersionId = currentVersionId,
        revision = revision,
        name = name,
        kind = kind,
        state = state,
        createdAt = null,
        updatedAt = null,
        trashedAt = null,
        restoreDeadline = null,
        purgeEligible = false,
        byteLength = byteLength,
        sha256 = sha256,
    )

    private fun sameBootstrap(actual: RebaselineBootstrap, expected: RebaselineBootstrap): Boolean =
        actual.bootstrapId == expected.bootstrapId &&
            actual.deviceId == expected.deviceId &&
            actual.libraryId == expected.libraryId &&
            actual.generation == expected.generation &&
            actual.snapshotEpoch == expected.snapshotEpoch &&
            actual.snapshotResumeSequence == expected.snapshotResumeSequence &&
            actual.manifestItemCount == expected.manifestItemCount

    private fun RebaselineNode.toEntity(bootstrapId: String) = RebaselineNodeEntity(
        profileId = profileId,
        deviceId = deviceId,
        libraryId = libraryId.value,
        bootstrapId = bootstrapId,
        nodeId = nodeId,
        parentNodeId = parentNodeId,
        name = name,
        kind = kind,
        state = state,
        revision = revision.value,
        currentVersionId = currentVersionId,
        byteLength = byteLength?.value,
        sha256 = sha256,
    )

    private fun initialState() = SyncStateEntity(
        profileId = profileId,
        deviceId = deviceId,
        libraryId = libraryId.value,
        journalEpoch = null,
        locallyAppliedSequence = "0",
        serverAcknowledgedSequence = "0",
        lastHighWatermark = null,
        lastSuccessfulSyncAt = null,
        lastAttemptAt = null,
        lastErrorCode = null,
        state = SyncStateKind.UNINITIALIZED.name,
        snapshotEpoch = null,
        snapshotResumeSequence = null,
    )

    private fun parseU64(value: String): U64Decimal? = U64Decimal.parse(value)

    private fun add(value: U64Decimal, amount: Int): U64Decimal =
        U64Decimal.parse(BigInteger(value.value).add(BigInteger.valueOf(amount.toLong())).toString())
            ?: error("u64 overflow")

    private fun now(): String = OffsetDateTime.now().toString()

    private suspend fun markRebaseline(state: SyncStateEntity?, code: String): SyncOutcome {
        cache.discardPendingAck(profileId, deviceId, libraryId)
        cache.upsertState((state ?: initialState()).copy(
            state = SyncStateKind.REBASELINE_REQUIRED.name,
            lastErrorCode = code,
            lastAttemptAt = now(),
        ))
        return SyncOutcome(SyncOutcomeKind.REBASELINE_REQUIRED)
    }

    private suspend fun protocolFailure(state: SyncStateEntity?): SyncOutcome {
        cache.upsertState((state ?: initialState()).copy(
            state = SyncStateKind.ERROR_PROTOCOL.name,
            lastErrorCode = "protocol_error",
            lastAttemptAt = now(),
        ))
        return SyncOutcome(
            SyncOutcomeKind.PROTOCOL_ERROR,
            SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_SYNC_RESPONSE),
        )
    }

    private suspend fun budgetReached(state: SyncStateEntity, rebaseline: Boolean = false): SyncOutcome {
        cache.upsertState(state.copy(
            state = if (rebaseline) SyncStateKind.REBASELINING.name else SyncStateKind.SYNCING.name,
            lastErrorCode = "resource_budget",
            lastAttemptAt = now(),
        ))
        return SyncOutcome(SyncOutcomeKind.MORE_WORK)
    }

    private suspend fun mapFailure(error: SynveilTransportError, state: SyncStateEntity): SyncOutcome {
        val code = (error as? SynveilTransportError.HttpError)?.code
        val kind = when {
            code == "device_revoked" -> SyncOutcomeKind.DEVICE_REVOKED
            code == "authentication_failed" -> SyncOutcomeKind.AUTHENTICATION_REQUIRED
            code == "sync_rebaseline_required" || code == "checkpoint_conflict" || code == "bootstrap_expired" ||
                (error as? SynveilTransportError.HttpError)?.statusCode == 410 -> SyncOutcomeKind.REBASELINE_REQUIRED
            error is SynveilTransportError.Timeout || error is SynveilTransportError.Offline ||
                error is SynveilTransportError.DnsFailure || error is SynveilTransportError.Cancelled ||
                (error as? SynveilTransportError.HttpError)?.statusCode == 503 -> SyncOutcomeKind.TRANSIENT_ERROR
            else -> SyncOutcomeKind.PROTOCOL_ERROR
        }
        val stateKind = when (kind) {
            SyncOutcomeKind.REBASELINE_REQUIRED -> SyncStateKind.REBASELINE_REQUIRED
            SyncOutcomeKind.AUTHENTICATION_REQUIRED, SyncOutcomeKind.DEVICE_REVOKED -> SyncStateKind.PAUSED_AUTH
            SyncOutcomeKind.TRANSIENT_ERROR -> SyncStateKind.ERROR_TRANSIENT
            else -> SyncStateKind.ERROR_PROTOCOL
        }
        cache.upsertState(state.copy(state = stateKind.name, lastErrorCode = code ?: kind.name.lowercase(), lastAttemptAt = now()))
        return SyncOutcome(kind, error)
    }

    private fun isExpired(error: SynveilTransportError): Boolean =
        (error as? SynveilTransportError.HttpError)?.statusCode == 410 ||
            (error as? SynveilTransportError.HttpError)?.code == "rebaseline_expired"

    companion object {
        private const val MAX_REBASELINE_PAGES = 2048
        private const val STAGED_PENDING_COMPLETE = "SWAPPED_PENDING_COMPLETE"
        private val scopeLocks = ConcurrentHashMap<String, Mutex>()

        private fun scopeMutex(profileId: String, deviceId: String, libraryId: String): Mutex =
            scopeLocks.computeIfAbsent("$profileId:$deviceId:$libraryId") { Mutex() }
    }
}
