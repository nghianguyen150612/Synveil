package com.synveil.android.data.library

import com.synveil.android.data.network.LibraryPageResult
import com.synveil.android.data.network.SynveilTransportError
import java.time.OffsetDateTime
import java.time.format.DateTimeFormatter
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json

const val LIBRARY_PAGE_SIZE = 100
const val MAX_LIBRARY_LIST_PAGES = 64
const val MAX_LIBRARY_LIST_ITEMS = 4096
const val MAX_LIBRARY_CURSOR_LENGTH = 512

private val CANONICAL_UUID = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
private val CANONICAL_UNSIGNED_DECIMAL = Regex("^(0|[1-9][0-9]*)$")
private val REQUEST_ID = Regex("^[A-Za-z0-9._~-]{8,128}$")

class LibraryId private constructor(val value: String) {
    override fun equals(other: Any?): Boolean = other is LibraryId && value == other.value
    override fun hashCode(): Int = value.hashCode()
    override fun toString(): String = value
    companion object {
        fun parse(value: String): LibraryId? = value.takeIf(CANONICAL_UUID::matches)?.let(::LibraryId)
    }
}

class NodeId private constructor(val value: String) {
    override fun equals(other: Any?): Boolean = other is NodeId && value == other.value
    override fun hashCode(): Int = value.hashCode()
    override fun toString(): String = value
    companion object {
        fun parse(value: String): NodeId? = value.takeIf(CANONICAL_UUID::matches)?.let(::NodeId)
    }
}

class LibraryRevision private constructor(val value: String) {
    override fun equals(other: Any?): Boolean = other is LibraryRevision && value == other.value
    override fun hashCode(): Int = value.hashCode()
    override fun toString(): String = value
    companion object {
        fun parse(value: String): LibraryRevision? =
            value.takeIf(CANONICAL_UNSIGNED_DECIMAL::matches)?.let(::LibraryRevision)
    }
}

enum class LibraryStatus { ACTIVE, READ_ONLY, QUARANTINED }

data class Library(
    val id: LibraryId,
    val revision: LibraryRevision,
    val name: String,
    val rootNodeId: NodeId,
    val status: LibraryStatus,
    val createdAt: OffsetDateTime,
    val updatedAt: OffsetDateTime,
)

data class LibraryCollectionPage(
    val libraries: List<Library>,
    val nextCursor: String?,
    val hasMore: Boolean,
    val requestId: String?,
)

sealed interface LibraryRepositoryResult {
    data class Loaded(val libraries: List<Library>) : LibraryRepositoryResult
    data class Failed(val failure: LibraryFailure) : LibraryRepositoryResult
}

sealed interface LibraryFailure {
    data class Transport(val error: SynveilTransportError) : LibraryFailure
    data object NoActiveProfile : LibraryFailure
    data object ResourceLimit : LibraryFailure
    data object RepeatedCursor : LibraryFailure
}

interface LibraryRepository {
    fun listLibraries(): LibraryRepositoryResult
}

class AuthenticatedLibraryRepository(
    private val transport: (String?) -> LibraryPageResult,
) : LibraryRepository {
    override fun listLibraries(): LibraryRepositoryResult {
        var cursor: String? = null
        val seenCursors = mutableSetOf<String>()
        val libraries = mutableListOf<Library>()
        repeat(MAX_LIBRARY_LIST_PAGES) {
            when (val result = transport(cursor)) {
                is LibraryPageResult.Failure -> return LibraryRepositoryResult.Failed(LibraryFailure.Transport(result.error))
                is LibraryPageResult.Success -> {
                    val page = result.page
                    libraries += page.libraries
                    if (libraries.size > MAX_LIBRARY_LIST_ITEMS) {
                        return LibraryRepositoryResult.Failed(LibraryFailure.ResourceLimit)
                    }
                    if (!page.hasMore) return LibraryRepositoryResult.Loaded(libraries.toList())
                    val nextCursor = page.nextCursor
                        ?: return LibraryRepositoryResult.Failed(LibraryFailure.ResourceLimit)
                    if (nextCursor.isEmpty() || nextCursor.length > MAX_LIBRARY_CURSOR_LENGTH ||
                        !seenCursors.add(nextCursor)
                    ) {
                        return LibraryRepositoryResult.Failed(LibraryFailure.RepeatedCursor)
                    }
                    cursor = nextCursor
                }
            }
        }
        return LibraryRepositoryResult.Failed(LibraryFailure.ResourceLimit)
    }
}

internal object LibraryWireParser {
    fun parse(json: Json, body: ByteArray, requestId: String?): LibraryPageResult {
        val wire = try {
            json.decodeFromString<LibraryCollectionWire>(body.toString(Charsets.UTF_8))
        } catch (_: SerializationException) {
            return invalidPage()
        }
        if (wire.data.size > LIBRARY_PAGE_SIZE || !REQUEST_ID.matches(wire.meta.request_id)) return invalidPage()
        val cursor = wire.page.next_cursor
        if (cursor != null && (cursor.isEmpty() || cursor.length > MAX_LIBRARY_CURSOR_LENGTH)) return invalidPage()
        if (wire.page.has_more && cursor == null) return invalidPage()
        if (!wire.page.has_more && cursor != null) return invalidPage()
        val libraries = wire.data.map { resource ->
            val id = LibraryId.parse(resource.id) ?: return invalidPage()
            if (resource.type != "library") return invalidPage()
            val revision = LibraryRevision.parse(resource.revision) ?: return invalidPage()
            val rootNodeId = NodeId.parse(resource.attributes.root_node_id) ?: return invalidPage()
            if (resource.attributes.name.length !in 1..1024) return invalidPage()
            val status = runCatching { LibraryStatus.valueOf(resource.attributes.status) }.getOrNull()
                ?: return invalidPage()
            val createdAt = parseTimestamp(resource.attributes.created_at) ?: return invalidPage()
            val updatedAt = parseTimestamp(resource.attributes.updated_at) ?: return invalidPage()
            Library(id, revision, resource.attributes.name, rootNodeId, status, createdAt, updatedAt)
        }
        return LibraryPageResult.Success(LibraryCollectionPage(libraries, cursor, wire.page.has_more, requestId))
    }

    private fun parseTimestamp(value: String): OffsetDateTime? =
        runCatching { OffsetDateTime.parse(value, DateTimeFormatter.ISO_OFFSET_DATE_TIME) }.getOrNull()

    private fun invalidPage(): LibraryPageResult = LibraryPageResult.Failure(
        SynveilTransportError.ProtocolError(com.synveil.android.data.network.ProtocolErrorKind.INVALID_LIBRARY_RESPONSE),
    )
}

@Serializable
private data class LibraryCollectionWire(val data: List<LibraryResourceWire>, val page: PageWire, val meta: ResponseMetaWire)

@Serializable
private data class LibraryResourceWire(
    val id: String,
    val type: String,
    val revision: String,
    val attributes: LibraryAttributesWire,
)

@Serializable
private data class LibraryAttributesWire(
    val name: String,
    val root_node_id: String,
    val status: String,
    val created_at: String,
    val updated_at: String,
)

@Serializable
private data class PageWire(val next_cursor: String? = null, val has_more: Boolean)

@Serializable
private data class ResponseMetaWire(val request_id: String)
