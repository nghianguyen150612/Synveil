package com.synveil.android.data.mutation

import com.synveil.android.data.cache.MutationQueueEntity
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.library.NodeId
import com.synveil.android.data.network.SynveilTransportError
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import java.security.MessageDigest
import java.util.UUID

enum class MutationKind { CREATE_DIRECTORY, RENAME_NODE, MOVE_NODE, TRASH_NODE, RESTORE_NODE }

enum class MutationState {
    PENDING, SUBMITTING, OUTCOME_UNKNOWN, APPLIED, CONFLICT, BLOCKED_REBASELINE, FAILED_PERMANENT,
}

sealed interface MutationIntent {
    val kind: MutationKind
    val resourceId: String
    val parentDependencyId: String?
    fun payload(): JsonObject

    data class CreateDirectory(val parentNodeId: NodeId, val expectedParentRevision: String, val name: String) : MutationIntent {
        override val kind = MutationKind.CREATE_DIRECTORY
        override val resourceId = parentNodeId.value
        override val parentDependencyId = parentNodeId.value
        override fun payload() = buildJsonObject {
            put("parent_node_id", parentNodeId.value)
            put("expected_parent_revision", expectedParentRevision)
            put("name", name)
        }
    }
    data class RenameNode(val nodeId: NodeId, val expectedRevision: String, val newName: String) : MutationIntent {
        override val kind = MutationKind.RENAME_NODE
        override val resourceId = nodeId.value
        override val parentDependencyId: String? = null
        override fun payload() = buildJsonObject {
            put("node_id", nodeId.value)
            put("expected_revision", expectedRevision)
            put("new_name", newName)
        }
    }
    data class MoveNode(val nodeId: NodeId, val expectedRevision: String, val newParentNodeId: NodeId, val expectedNewParentRevision: String) : MutationIntent {
        override val kind = MutationKind.MOVE_NODE
        override val resourceId = nodeId.value
        override val parentDependencyId = newParentNodeId.value
        override fun payload() = buildJsonObject {
            put("node_id", nodeId.value)
            put("expected_revision", expectedRevision)
            put("new_parent_node_id", newParentNodeId.value)
            put("expected_new_parent_revision", expectedNewParentRevision)
        }
    }
    data class TrashNode(val nodeId: NodeId, val expectedRevision: String) : MutationIntent {
        override val kind = MutationKind.TRASH_NODE
        override val resourceId = nodeId.value
        override val parentDependencyId: String? = null
        override fun payload() = buildJsonObject {
            put("node_id", nodeId.value)
            put("expected_revision", expectedRevision)
        }
    }
    data class RestoreNode(val nodeId: NodeId, val expectedRevision: String, val expectedParentNodeId: NodeId, val expectedParentRevision: String) : MutationIntent {
        override val kind = MutationKind.RESTORE_NODE
        override val resourceId = nodeId.value
        override val parentDependencyId = expectedParentNodeId.value
        override fun payload() = buildJsonObject {
            put("node_id", nodeId.value)
            put("expected_revision", expectedRevision)
            put("expected_parent_node_id", expectedParentNodeId.value)
            put("expected_parent_revision", expectedParentRevision)
        }
    }
}

data class MutationRequest(
    val mutationId: String,
    val baseEpoch: String,
    val baseSequence: String,
    val kind: MutationKind,
    val payload: JsonObject,
) {
    fun json(): String = buildJsonObject {
        put("mutation_id", mutationId)
        put("base_epoch", baseEpoch)
        put("base_sequence", baseSequence)
        put("kind", kind.name)
        put("payload", payload)
    }.toString()
}

data class MutationNodeResult(
    val nodeId: String,
    val libraryId: String,
    val parentNodeId: String?,
    val kind: String,
    val state: String,
    val name: String,
    val revision: String,
    val currentVersionId: String?,
    val trashedAt: String?,
    val createdAt: String,
    val updatedAt: String,
)

sealed interface MutationResult {
    data class Applied(
        val mutationId: String,
        val kind: MutationKind,
        val replayed: Boolean,
        val node: MutationNodeResult,
        val journalEventId: String,
        val journalSequence: String,
    ) : MutationResult
    data class Conflict(val error: SynveilTransportError, val conflictId: String?) : MutationResult
    data class Failure(val error: SynveilTransportError) : MutationResult
}

internal object MutationWireParser {
    fun parse(json: Json, body: ByteArray, requestId: String?, request: MutationRequest): MutationResult {
        return try {
            val wire = json.decodeFromString<MutationEnvelope>(body.toString(Charsets.UTF_8))
            require(wire.meta.request_id.matches(Regex("^[A-Za-z0-9._~-]{8,128}$")))
            require(requestId == null || requestId == wire.meta.request_id)
            val data = wire.data
            require(data.outcome == "APPLIED" && data.mutation_id == request.mutationId && data.kind == request.kind.name)
            require(data.journal_event_id.isNotEmpty() && data.journal_sequence.matches(Regex("^(0|[1-9][0-9]*)$")))
            val node = data.node
            require(node.id.matches(UUID_PATTERN) && node.library_id.matches(UUID_PATTERN))
            require(node.name.length in 1..1024 && node.revision.matches(Regex("^(0|[1-9][0-9]*)$")))
            MutationResult.Applied(
                data.mutation_id,
                request.kind,
                data.replayed,
                MutationNodeResult(node.id, node.library_id, node.parent_node_id, node.kind, node.state, node.name, node.revision, node.current_version_id, node.trashed_at, node.created_at, node.updated_at),
                data.journal_event_id,
                data.journal_sequence,
            )
        } catch (_: Exception) {
            MutationResult.Failure(com.synveil.android.data.network.SynveilTransportError.ProtocolError(com.synveil.android.data.network.ProtocolErrorKind.INVALID_MUTATION_RESPONSE))
        }
    }
}

@Serializable
private data class MutationEnvelope(val data: MutationData, val meta: MutationMeta)
@Serializable
private data class MutationMeta(val request_id: String)
@Serializable
private data class MutationData(
    val outcome: String,
    val mutation_id: String,
    val kind: String,
    val replayed: Boolean,
    val node: MutationNode,
    val journal_event_id: String,
    val journal_sequence: String,
)
@Serializable
private data class MutationNode(
    val id: String,
    val library_id: String,
    val parent_node_id: String? = null,
    val kind: String,
    val state: String,
    val name: String,
    val revision: String,
    val current_version_id: String? = null,
    val trashed_at: String? = null,
    val created_at: String,
    val updated_at: String,
)

private val UUID_PATTERN = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")

fun MutationQueueEntity.toRequest(): MutationRequest = MutationRequest(
    mutationId = mutationId,
    baseEpoch = baseEpoch,
    baseSequence = baseSequence,
    kind = MutationKind.valueOf(kind),
    payload = Json.parseToJsonElement(payloadJson).jsonObject,
)

fun MutationIntent.fingerprint(baseEpoch: String, baseSequence: String): String {
    val bytes = "$baseEpoch\u0000$baseSequence\u0000${kind.name}\u0000${payload()}".toByteArray()
    return MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
}

fun newUuidV7(): String {
    val timestamp = System.currentTimeMillis()
    val random = UUID.randomUUID()
    val most = (timestamp shl 16) or (0x7L shl 12) or (random.mostSignificantBits and 0xfffL)
    val least = (random.leastSignificantBits and 0x3fff_ffff_ffff_ffffL) or Long.MIN_VALUE
    return UUID(most, least).toString()
}
