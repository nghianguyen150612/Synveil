package com.synveil.android.data.sync

import com.synveil.android.data.network.ProtocolErrorKind
import com.synveil.android.data.network.SynveilTransportError
import java.math.BigInteger
import java.time.OffsetDateTime
import java.time.format.DateTimeFormatter
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json

const val SYNC_PAGE_SIZE = 200
const val MAX_SYNC_PAGES_PER_RUN = 64
const val MAX_SYNC_EVENTS_PER_RUN = 4096
const val MAX_CANONICAL_NODE_LOOKUPS = 4096
const val MAX_ACK_TOKEN_LENGTH = 256
const val REBASELINE_PAGE_SIZE = 500
const val MAX_REBASELINE_CURSOR_LENGTH = 320
const val MAX_COMPLETION_TOKEN_LENGTH = 336
const val MAX_REBASELINE_NODES = 1_000_000
const val MAX_SYNC_BODY_BYTES = 2 * 1024 * 1024

private val UUID = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
private val DECIMAL = Regex("^(0|[1-9][0-9]*)$")
private val HASH = Regex("^[0-9a-f]{64}$")
private val REQUEST_ID = Regex("^[A-Za-z0-9._~-]{8,128}$")

class U64Decimal private constructor(val value: String) : Comparable<U64Decimal> {
    override fun compareTo(other: U64Decimal): Int = BigInteger(value).compareTo(BigInteger(other.value))
    override fun equals(other: Any?): Boolean = other is U64Decimal && value == other.value
    override fun hashCode(): Int = value.hashCode()
    override fun toString(): String = value

    companion object {
        fun parse(value: String): U64Decimal? = value.takeIf(DECIMAL::matches)?.let(::U64Decimal)
    }
}

enum class SyncStateKind { UNINITIALIZED, READY, SYNCING, ACK_PENDING, REBASELINE_REQUIRED, REBASELINING, PAUSED_AUTH, ERROR_TRANSIENT, ERROR_PROTOCOL }
enum class ChangeKind { NODE_CREATED, NODE_RENAMED, NODE_MOVED, NODE_TRASHED, NODE_RESTORED, FILE_CONTENT_COMMITTED, FILE_VERSION_RESTORED, NODE_PURGED }

data class SyncCheckpoint(
    val deviceId: String,
    val libraryId: String,
    val epoch: U64Decimal,
    val acknowledgedSequence: U64Decimal,
    val createdAt: OffsetDateTime,
    val updatedAt: OffsetDateTime,
    val lastSeenHighWatermark: U64Decimal?,
)

data class SyncChange(
    val eventId: String,
    val sequence: U64Decimal,
    val schemaVersion: Int,
    val resourceKind: String,
    val resourceId: String,
    val changeKind: ChangeKind,
    val resourceRevision: U64Decimal,
    val occurredAt: OffsetDateTime,
    val parentNodeId: String?,
    val nodeKind: String?,
    val nodeState: String?,
    val currentVersionId: String?,
)

data class SyncFeedPage(
    val deviceId: String,
    val libraryId: String,
    val epoch: U64Decimal,
    val fromSequence: U64Decimal,
    val throughSequence: U64Decimal,
    val highWatermark: U64Decimal,
    val hasMore: Boolean,
    val changes: List<SyncChange>,
    val ackToken: String?,
)

data class RebaselineBootstrap(
    val bootstrapId: String,
    val deviceId: String,
    val libraryId: String,
    val state: String,
    val generation: U64Decimal,
    val snapshotEpoch: U64Decimal,
    val snapshotResumeSequence: U64Decimal,
    val manifestItemCount: U64Decimal,
    val createdAt: OffsetDateTime,
    val expiresAt: OffsetDateTime,
    val completedAt: OffsetDateTime?,
)

data class RebaselineNode(
    val nodeId: String,
    val parentNodeId: String?,
    val name: String,
    val kind: String,
    val state: String,
    val revision: U64Decimal,
    val currentVersionId: String?,
    val byteLength: U64Decimal?,
    val sha256: String?,
)

data class RebaselinePage(
    val bootstrap: RebaselineBootstrap,
    val nodes: List<RebaselineNode>,
    val hasMore: Boolean,
    val nextCursor: String?,
    val completionToken: String?,
)

data class RebaselineCompletion(
    val bootstrap: RebaselineBootstrap,
    val journalEpoch: U64Decimal,
    val acknowledgedSequence: U64Decimal,
    val updatedAt: OffsetDateTime,
    val replayed: Boolean,
)

sealed interface SyncResult<out T> {
    data class Success<T>(val value: T) : SyncResult<T>
    data class Failure(val error: SynveilTransportError) : SyncResult<Nothing>
}

internal object SyncWireParser {
    private val json = Json { ignoreUnknownKeys = false; explicitNulls = false }

    fun parseCheckpoint(body: ByteArray, expectedDeviceId: String, expectedLibraryId: String): SyncResult<SyncCheckpoint> = try {
        val wire = json.decodeFromString<CheckpointEnvelope>(body.toString(Charsets.UTF_8))
        requireScope(wire.data.device_id, wire.data.library_id, expectedDeviceId, expectedLibraryId)
        require(REQUEST_ID.matches(wire.meta.request_id))
        val epoch = requireU64(wire.data.epoch)
        val acknowledged = requireU64(wire.data.acknowledged_sequence)
        val high = wire.data.last_seen_high_watermark?.let(::requireU64)
        require(epoch.value != "0" && (high == null || high >= acknowledged))
        val created = parseTime(wire.data.created_at)
        val updated = parseTime(wire.data.updated_at)
        require(!created.isAfter(updated))
        SyncResult.Success(SyncCheckpoint(wire.data.device_id, wire.data.library_id, epoch, acknowledged, created, updated, high))
    } catch (_: Exception) { invalid(ProtocolErrorKind.INVALID_SYNC_RESPONSE) }

    fun parseFeed(body: ByteArray, expectedDeviceId: String, expectedLibraryId: String): SyncResult<SyncFeedPage> = try {
        val wire = json.decodeFromString<FeedEnvelope>(body.toString(Charsets.UTF_8))
        val data = wire.data
        requireScope(data.device_id, data.library_id, expectedDeviceId, expectedLibraryId)
        require(REQUEST_ID.matches(wire.meta.request_id) && data.changes.size <= 500)
        val epoch = requireU64(data.epoch)
        val from = requireU64(data.from_sequence)
        val through = requireU64(data.through_sequence)
        val high = requireU64(data.high_watermark)
        require(high >= through && data.has_more == (through < high)) { "watermark" }
        if (data.changes.isEmpty()) {
            require(data.ack_token == null && from == through && !data.has_more)
        } else {
            require(data.ack_token != null && validEvidence(data.ack_token, MAX_ACK_TOKEN_LENGTH)) { "evidence" }
            require(through >= from && BigInteger(through.value).subtract(BigInteger(from.value)) == BigInteger.valueOf(data.changes.size.toLong())) { "span ${from.value} ${through.value} ${data.changes.size}" }
        }
        val changes = data.changes.map { parseChange(it) }
        require(changes.zipWithNext().all { (a, b) -> b.sequence > a.sequence }) { "order" }
        if (changes.isNotEmpty()) {
            require(changes.first().sequence == next(from)) { "first ${changes.first().sequence} ${from.value}" }
            require(changes.last().sequence == through) { "last ${changes.last().sequence} ${through.value}" }
        }
        SyncResult.Success(SyncFeedPage(data.device_id, data.library_id, epoch, from, through, high, data.has_more, changes, data.ack_token))
    } catch (_: Exception) { invalid(ProtocolErrorKind.INVALID_SYNC_RESPONSE) }

    fun parseBootstrap(body: ByteArray, expectedDeviceId: String, expectedLibraryId: String): SyncResult<RebaselineBootstrap> = try {
        val wire = json.decodeFromString<BootstrapEnvelope>(body.toString(Charsets.UTF_8))
        SyncResult.Success(parseBootstrap(wire.data, wire.meta.request_id, expectedDeviceId, expectedLibraryId))
    } catch (_: Exception) { invalid(ProtocolErrorKind.INVALID_REBASELINE_RESPONSE) }

    fun parsePage(body: ByteArray, expectedDeviceId: String, expectedLibraryId: String, expectedBootstrapId: String): SyncResult<RebaselinePage> = try {
        val wire = json.decodeFromString<PageEnvelope>(body.toString(Charsets.UTF_8))
        require(REQUEST_ID.matches(wire.meta.request_id))
        val bootstrap = parseBootstrap(wire.data.bootstrap, wire.meta.request_id, expectedDeviceId, expectedLibraryId)
        require(bootstrap.bootstrapId == expectedBootstrapId && bootstrap.state == "OPEN")
        require(wire.data.nodes.size <= 1000)
        val cursor = wire.data.next_cursor?.also { require(validEvidence(it, MAX_REBASELINE_CURSOR_LENGTH)) }
        val completion = wire.data.completion_token?.also { require(validEvidence(it, MAX_COMPLETION_TOKEN_LENGTH)) }
        require(wire.data.has_more == (cursor != null))
        require(wire.data.has_more != (completion != null))
        if (wire.data.has_more) require(wire.data.nodes.isNotEmpty())
        val nodes = wire.data.nodes.map(::parseNode)
        require(nodes.zipWithNext().all { it.first.nodeId < it.second.nodeId })
        SyncResult.Success(RebaselinePage(bootstrap, nodes, wire.data.has_more, cursor, completion))
    } catch (_: Exception) { invalid(ProtocolErrorKind.INVALID_REBASELINE_RESPONSE) }

    fun parseCompletion(body: ByteArray, expectedDeviceId: String, expectedLibraryId: String, expectedBootstrapId: String): SyncResult<RebaselineCompletion> = try {
        val wire = json.decodeFromString<CompletionEnvelope>(body.toString(Charsets.UTF_8))
        val bootstrap = parseBootstrap(wire.data.bootstrap, wire.meta.request_id, expectedDeviceId, expectedLibraryId)
        val epoch = requireU64(wire.data.checkpoint.journal_epoch)
        val sequence = requireU64(wire.data.checkpoint.acknowledged_sequence)
        require(bootstrap.bootstrapId == expectedBootstrapId && bootstrap.state == "COMPLETED" && epoch == bootstrap.snapshotEpoch && sequence == bootstrap.snapshotResumeSequence)
        SyncResult.Success(RebaselineCompletion(bootstrap, epoch, sequence, parseTime(wire.data.checkpoint.updated_at), wire.data.replayed))
    } catch (_: Exception) { invalid(ProtocolErrorKind.INVALID_REBASELINE_RESPONSE) }

    private fun parseChange(value: ChangeWire): SyncChange {
        require(isUuidV7(value.event_id) && value.schema_version == 1 && value.resource_kind == "NODE")
        require(isUuidV7(value.resource_id))
        val kind = runCatching { ChangeKind.valueOf(value.change_kind) }.getOrNull() ?: error("kind")
        require(value.parent_node_id == null || isUuidV7(value.parent_node_id))
        require(value.current_version_id == null || isUuidV7(value.current_version_id))
        require(value.node_kind == null || value.node_kind in setOf("FILE", "DIRECTORY"))
        require(value.node_state == null || value.node_state in setOf("ACTIVE", "TRASHED", "PURGING"))
        return SyncChange(value.event_id, requireU64(value.sequence), value.schema_version, value.resource_kind, value.resource_id, kind, requireU64(value.resource_revision), parseTime(value.occurred_at), value.parent_node_id, value.node_kind, value.node_state, value.current_version_id)
    }

    private fun parseNode(value: RebaselineNodeWire): RebaselineNode {
        require(isUuidV7(value.node_id) && value.name.length in 1..1024 && value.name.none(Char::isISOControl))
        require(value.parent_node_id == null || isUuidV7(value.parent_node_id))
        require(value.current_version_id == null || isUuidV7(value.current_version_id))
        require(value.kind in setOf("FILE", "DIRECTORY") && value.state in setOf("ACTIVE", "TRASHED"))
        val content = value.current_content
        require(content == null || HASH.matches(content.sha256))
        return RebaselineNode(value.node_id, value.parent_node_id, value.name, value.kind, value.state, requireU64(value.revision), value.current_version_id, content?.let { requireU64(it.byte_length) }, content?.sha256)
    }

    private fun parseBootstrap(value: BootstrapWire, requestId: String, expectedDeviceId: String, expectedLibraryId: String): RebaselineBootstrap {
        require(REQUEST_ID.matches(requestId))
        requireScope(value.device_id, value.library_id, expectedDeviceId, expectedLibraryId)
        require(isUuidV7(value.bootstrap_id) && value.state in setOf("OPEN", "COMPLETED", "ABORTED", "EXPIRED"))
        val created = parseTime(value.created_at)
        val expires = parseTime(value.expires_at)
        val completed = value.completed_at?.let(::parseTime)
        require(created.isBefore(expires))
        require((value.state == "COMPLETED") == (completed != null))
        require(completed == null || !completed.isBefore(created))
        val generation = requireU64(value.generation)
        val epoch = requireU64(value.snapshot_epoch)
        val count = requireU64(value.manifest_item_count)
        require(generation.value != "0" && epoch.value != "0" && count <= U64Decimal.parse(MAX_REBASELINE_NODES.toString())!!)
        return RebaselineBootstrap(value.bootstrap_id, value.device_id, value.library_id, value.state, generation, epoch, requireU64(value.snapshot_resume_sequence), count, created, expires, completed)
    }

    private fun requireScope(device: String, library: String, expectedDevice: String, expectedLibrary: String) {
        require(device == expectedDevice && library == expectedLibrary && isUuidV7(device) && isUuidV7(library))
    }
    private fun requireU64(value: String) = U64Decimal.parse(value) ?: error("u64")
    private fun next(value: U64Decimal) = U64Decimal.parse(BigInteger(value.value).add(BigInteger.ONE).toString()) ?: error("u64")
    private fun parseTime(value: String) = OffsetDateTime.parse(value, DateTimeFormatter.ISO_OFFSET_DATE_TIME)
    private fun validEvidence(value: String, max: Int) = value.isNotEmpty() && value.length <= max && value.all { it.code in 33..126 }
    private fun isUuidV7(value: String): Boolean =
        UUID.matches(value) && value[14] == '7' && value[19] in "89ab"
    private fun invalid(kind: ProtocolErrorKind): SyncResult<Nothing> = SyncResult.Failure(SynveilTransportError.ProtocolError(kind))
}

@Serializable private data class CheckpointEnvelope(val data: CheckpointWire, val meta: MetaWire)
@Serializable private data class CheckpointWire(val device_id: String, val library_id: String, val epoch: String, val acknowledged_sequence: String, val created_at: String, val updated_at: String, val last_seen_high_watermark: String? = null)
@Serializable private data class FeedEnvelope(val data: FeedWire, val meta: MetaWire)
@Serializable private data class FeedWire(val device_id: String, val library_id: String, val epoch: String, val from_sequence: String, val through_sequence: String, val high_watermark: String, val has_more: Boolean, val changes: List<ChangeWire>, val ack_token: String? = null)
@Serializable private data class ChangeWire(val event_id: String, val schema_version: Int, val sequence: String, val resource_kind: String, val resource_id: String, val change_kind: String, val resource_revision: String, val occurred_at: String, val parent_node_id: String? = null, val node_kind: String? = null, val node_state: String? = null, val current_version_id: String? = null)
@Serializable private data class BootstrapEnvelope(val data: BootstrapWire, val meta: MetaWire)
@Serializable private data class BootstrapWire(val bootstrap_id: String, val device_id: String, val library_id: String, val state: String, val generation: String, val snapshot_epoch: String, val snapshot_resume_sequence: String, val manifest_item_count: String, val created_at: String, val expires_at: String, val completed_at: String? = null)
@Serializable private data class PageEnvelope(val data: PageDataWire, val meta: MetaWire)
@Serializable private data class PageDataWire(val bootstrap: BootstrapWire, val nodes: List<RebaselineNodeWire>, val has_more: Boolean, val next_cursor: String? = null, val completion_token: String? = null)
@Serializable private data class RebaselineNodeWire(val node_id: String, val parent_node_id: String? = null, val name: String, val kind: String, val state: String, val revision: String, val current_version_id: String? = null, val current_content: ContentWire? = null)
@Serializable private data class ContentWire(val byte_length: String, val sha256: String)
@Serializable private data class CompletionEnvelope(val data: CompletionDataWire, val meta: MetaWire)
@Serializable private data class CompletionDataWire(val bootstrap: BootstrapWire, val checkpoint: CompletionCheckpointWire, val replayed: Boolean)
@Serializable private data class CompletionCheckpointWire(val journal_epoch: String, val acknowledged_sequence: String, val updated_at: String)
@Serializable private data class MetaWire(val request_id: String)
