package com.synveil.android.data.node

import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.library.NodeId
import com.synveil.android.data.network.NodePageResult
import com.synveil.android.data.network.ProtocolErrorKind
import com.synveil.android.data.network.SynveilTransportError
import java.time.OffsetDateTime
import java.time.format.DateTimeFormatter
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json

const val NODE_PAGE_SIZE = 100
const val MAX_NODE_PAGES = 64
const val MAX_NODES_PER_DIRECTORY = 4096
const val MAX_NODE_CURSOR_LENGTH = 512

private val CANONICAL_UUID = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
private val CANONICAL_UNSIGNED_DECIMAL = Regex("^(0|[1-9][0-9]*)$")
private val REQUEST_ID = Regex("^[A-Za-z0-9._~-]{8,128}$")

enum class NodeKind { FILE, DIRECTORY }
enum class NodeState { ACTIVE, TRASHED, PURGING }

class NodeRevision private constructor(val value: String) {
    override fun equals(other: Any?): Boolean = other is NodeRevision && value == other.value
    override fun hashCode(): Int = value.hashCode()
    override fun toString(): String = value

    companion object {
        fun parse(value: String): NodeRevision? =
            value.takeIf(CANONICAL_UNSIGNED_DECIMAL::matches)?.let(::NodeRevision)
    }
}

data class Node(
    val nodeId: NodeId,
    val libraryId: LibraryId,
    val parentId: NodeId?,
    val currentVersionId: String?,
    val revision: NodeRevision,
    val name: String,
    val kind: NodeKind,
    val state: NodeState,
    val createdAt: OffsetDateTime,
    val updatedAt: OffsetDateTime,
    val trashedAt: OffsetDateTime?,
    val restoreDeadline: OffsetDateTime?,
    val purgeEligible: Boolean,
)

data class NodePage(
    val nodes: List<Node>,
    val nextCursor: String?,
    val hasMore: Boolean,
    val requestId: String?,
)

sealed interface NodeRepositoryResult {
    data class Loaded(val nodes: List<Node>) : NodeRepositoryResult
    data class Failed(val failure: NodeFailure) : NodeRepositoryResult
}

sealed interface NodeFailure {
    data class Transport(val error: SynveilTransportError) : NodeFailure
    data object ResourceLimit : NodeFailure
    data object RepeatedCursor : NodeFailure
    data object InvalidScope : NodeFailure
}

interface NodeRepository {
    fun listChildren(libraryId: LibraryId, parentId: NodeId?): NodeRepositoryResult
}

class AuthenticatedNodeRepository(
    private val transport: (LibraryId, NodeId?, String?) -> NodePageResult,
) : NodeRepository {
    override fun listChildren(libraryId: LibraryId, parentId: NodeId?): NodeRepositoryResult {
        var cursor: String? = null
        val seen = mutableSetOf<String>()
        val nodes = mutableListOf<Node>()
        repeat(MAX_NODE_PAGES) {
            when (val result = transport(libraryId, parentId, cursor)) {
                is NodePageResult.Failure -> return NodeRepositoryResult.Failed(NodeFailure.Transport(result.error))
                is NodePageResult.Success -> {
                    val page = result.page
                    if (page.nodes.any { it.libraryId != libraryId }) {
                        return NodeRepositoryResult.Failed(NodeFailure.InvalidScope)
                    }
                    nodes += page.nodes
                    if (nodes.size > MAX_NODES_PER_DIRECTORY) {
                        return NodeRepositoryResult.Failed(NodeFailure.ResourceLimit)
                    }
                    if (!page.hasMore) return NodeRepositoryResult.Loaded(nodes.toList())
                    val next = page.nextCursor
                        ?: return NodeRepositoryResult.Failed(NodeFailure.ResourceLimit)
                    if (next.isEmpty() || next.length > MAX_NODE_CURSOR_LENGTH || !seen.add(next)) {
                        return NodeRepositoryResult.Failed(NodeFailure.RepeatedCursor)
                    }
                    cursor = next
                }
            }
        }
        return NodeRepositoryResult.Failed(NodeFailure.ResourceLimit)
    }
}

internal object NodeWireParser {
    fun parse(json: Json, body: ByteArray, requestId: String?): NodePageResult {
        val wire = try {
            json.decodeFromString<NodeCollectionWire>(body.toString(Charsets.UTF_8))
        } catch (_: SerializationException) {
            return invalid()
        }
        if (wire.data.size > NODE_PAGE_SIZE || !REQUEST_ID.matches(wire.meta.request_id)) return invalid()
        val cursor = wire.page.next_cursor
        if (cursor != null && (cursor.isEmpty() || cursor.length > MAX_NODE_CURSOR_LENGTH)) return invalid()
        if (wire.page.has_more != (cursor != null)) return invalid()
        val nodes = wire.data.map { resource ->
            if (resource.type != "node" || resource.attributes.name.length !in 1..1024) return invalid()
            val nodeId = NodeId.parse(resource.id) ?: return invalid()
            val libraryId = LibraryId.parse(resource.attributes.library_id) ?: return invalid()
            val parentId = resource.attributes.parent_id?.let { NodeId.parse(it) ?: return invalid() }
            val currentVersionId = resource.attributes.current_version_id?.also {
                if (!CANONICAL_UUID.matches(it)) return invalid()
            }
            val revision = NodeRevision.parse(resource.revision) ?: return invalid()
            val kind = runCatching { NodeKind.valueOf(resource.attributes.kind) }.getOrNull() ?: return invalid()
            val state = runCatching { NodeState.valueOf(resource.attributes.state) }.getOrNull() ?: return invalid()
            val createdAt = parseTime(resource.attributes.created_at) ?: return invalid()
            val updatedAt = parseTime(resource.attributes.updated_at) ?: return invalid()
            val trashedAt = resource.attributes.trashed_at?.let { parseTime(it) ?: return invalid() }
            val restoreDeadline = resource.attributes.restore_deadline?.let { parseTime(it) ?: return invalid() }
            Node(nodeId, libraryId, parentId, currentVersionId, revision, resource.attributes.name, kind, state, createdAt, updatedAt, trashedAt, restoreDeadline, resource.attributes.purge_eligible)
        }
        return NodePageResult.Success(NodePage(nodes, cursor, wire.page.has_more, requestId))
    }

    private fun parseTime(value: String): OffsetDateTime? =
        runCatching { OffsetDateTime.parse(value, DateTimeFormatter.ISO_OFFSET_DATE_TIME) }.getOrNull()

    private fun invalid(): NodePageResult = NodePageResult.Failure(
        SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_NODE_RESPONSE),
    )
}

@Serializable
private data class NodeCollectionWire(val data: List<NodeResourceWire>, val page: NodePageWire, val meta: NodeMetaWire)

@Serializable
private data class NodeResourceWire(
    val id: String,
    val type: String,
    val revision: String,
    val attributes: NodeAttributesWire,
)

@Serializable
private data class NodeAttributesWire(
    val library_id: String,
    val parent_id: String? = null,
    val current_version_id: String? = null,
    val name: String,
    val kind: String,
    val state: String,
    val created_at: String,
    val updated_at: String,
    val trashed_at: String? = null,
    val restore_deadline: String? = null,
    val purge_eligible: Boolean,
)

@Serializable
private data class NodePageWire(val next_cursor: String? = null, val has_more: Boolean)

@Serializable
private data class NodeMetaWire(val request_id: String)
