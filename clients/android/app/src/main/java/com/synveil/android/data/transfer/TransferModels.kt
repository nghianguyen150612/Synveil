package com.synveil.android.data.transfer

import com.synveil.android.data.network.SynveilTransportError
import java.time.OffsetDateTime
import java.time.format.DateTimeFormatter
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json

private val CANONICAL_UUID = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
private val CANONICAL_UNSIGNED_DECIMAL = Regex("^(0|[1-9][0-9]*)$")
private val CANONICAL_SHA256 = Regex("^[0-9a-f]{64}$")
private val REQUEST_ID = Regex("^[A-Za-z0-9._~-]{8,128}$")

data class DownloadMetadata(
    val contentLength: Long?,
    val contentType: String?,
    val contentDisposition: String?,
    val etag: String?,
    val contentRange: String?,
)

sealed interface DownloadResult {
    data class Success(val response: okhttp3.Response, val metadata: DownloadMetadata) : DownloadResult
    data class Failure(val error: SynveilTransportError) : DownloadResult
}

data class UploadSession(
    val id: String,
    val operation: String,
    val state: UploadState,
    val receivedBytes: Long,
    val expectedBytes: Long,
    val expectedSha256: String?,
    val createdAt: OffsetDateTime,
    val updatedAt: OffsetDateTime,
    val expiresAt: OffsetDateTime,
    val target: UploadTarget,
    val completion: UploadCompletion?,
)

enum class UploadState { OPEN, VERIFYING, COMMITTING, COMMITTED, FAILED, EXPIRED, ABORTED }

data class UploadTarget(
    val operation: String,
    val libraryId: String,
    val parentId: String?,
    val nodeId: String?,
    val name: String?,
    val expectedRevision: String?,
)

data class UploadCompletion(
    val nodeId: String,
    val fileVersionId: String,
    val nodeRevision: String,
    val bytes: Long,
    val sha256: String,
    val committedAt: OffsetDateTime,
)

sealed interface UploadResult {
    data class Session(val session: UploadSession) : UploadResult
    data class Completion(val completion: UploadCompletion) : UploadResult
    data class Offset(val value: Long?) : UploadResult
    data class Failure(val error: SynveilTransportError) : UploadResult
}

internal object UploadWireParser {
    fun parse(json: Json, body: ByteArray, offsetHeader: String?): UploadSession {
        val wire = json.decodeFromString<UploadSessionResponseWire>(body.toString(Charsets.UTF_8))
        require(CANONICAL_UUID.matches(wire.data.id))
        require(REQUEST_ID.matches(wire.meta.request_id))
        require(wire.data.type == "upload_session")
        val attr = wire.data.attributes
        val received = attr.received_bytes.toLongStrict()
        val expected = attr.expected_bytes.toLongStrict()
        require(received in 0..expected)
        require(attr.operation == attr.target.operation)
        val target = when (attr.target.operation) {
            "CREATE_FILE" -> UploadTarget(
                operation = "CREATE_FILE",
                libraryId = attr.target.library_id?.also { require(CANONICAL_UUID.matches(it)) }
                    ?: error("missing library_id"),
                parentId = attr.target.parent_id?.also { require(CANONICAL_UUID.matches(it)) }
                    ?: error("missing parent_id"),
                nodeId = attr.target.node_id?.also { require(CANONICAL_UUID.matches(it)) }
                    ?: error("missing node_id"),
                name = attr.target.name?.also { require(it.length in 1..1024) }
                    ?: error("missing name"),
                expectedRevision = null,
            )
            "REPLACE_CONTENT" -> UploadTarget(
                operation = "REPLACE_CONTENT",
                libraryId = attr.target.library_id?.also { require(CANONICAL_UUID.matches(it)) }
                    ?: error("missing library_id"),
                parentId = null,
                nodeId = attr.target.node_id?.also { require(CANONICAL_UUID.matches(it)) }
                    ?: error("missing node_id"),
                name = null,
                expectedRevision = attr.target.expected_revision?.also {
                    require(CANONICAL_UNSIGNED_DECIMAL.matches(it))
                } ?: error("missing expected_revision"),
            )
            else -> error("unknown upload target")
        }
        val state = runCatching { UploadState.valueOf(attr.state) }.getOrElse { error("unknown upload state") }
        val completion = attr.completion?.let { parseCompletion(it) }
        val session = UploadSession(
            id = wire.data.id,
            operation = attr.operation,
            state = state,
            receivedBytes = received,
            expectedBytes = expected,
            expectedSha256 = attr.expected_sha256,
            createdAt = parseTime(attr.created_at),
            updatedAt = parseTime(attr.updated_at),
            expiresAt = parseTime(attr.expires_at),
            target = target,
            completion = completion,
        )
        if (offsetHeader != null && offsetHeader.toLongStrict() != received) error("offset mismatch")
        return session
    }

    fun parseCompletion(json: Json, body: ByteArray): UploadCompletion {
        val wire = json.decodeFromString<UploadCompletionResponseWire>(body.toString(Charsets.UTF_8))
        require(REQUEST_ID.matches(wire.meta.request_id))
        require(wire.data.type == "upload_completion")
        return parseCompletion(wire.data.attributes)
    }

    private fun parseCompletion(value: UploadCompletionResourceWire): UploadCompletion {
        require(CANONICAL_UUID.matches(value.id))
        require(value.type == "upload_completion")
        return parseCompletion(value.attributes)
    }

    private fun parseCompletion(value: UploadCompletionAttributesWire): UploadCompletion = UploadCompletion(
        nodeId = value.node_id.also { require(CANONICAL_UUID.matches(it)) },
        fileVersionId = value.file_version_id.also { require(CANONICAL_UUID.matches(it)) },
        nodeRevision = value.node_revision.also { require(CANONICAL_UNSIGNED_DECIMAL.matches(it)) },
        bytes = value.bytes.toLongStrict(),
        sha256 = value.sha256.also { require(CANONICAL_SHA256.matches(it)) },
        committedAt = parseTime(value.committed_at),
    )

    private fun parseTime(value: String): OffsetDateTime =
        OffsetDateTime.parse(value, DateTimeFormatter.ISO_OFFSET_DATE_TIME)

    private fun String.toLongStrict(): Long =
        require(matches(Regex("^(0|[1-9][0-9]*)$"))).let { toLong() }
}

@Serializable
private data class UploadSessionResponseWire(val data: UploadSessionResourceWire, val meta: UploadMetaWire)

@Serializable
private data class UploadSessionResourceWire(
    val id: String,
    val type: String,
    val attributes: UploadSessionAttributesWire,
)

@Serializable
private data class UploadSessionAttributesWire(
    val operation: String,
    val state: String,
    val received_bytes: String,
    val expected_bytes: String,
    val expected_sha256: String? = null,
    val created_at: String,
    val updated_at: String,
    val expires_at: String,
    val target: UploadTargetWire,
    val last_error_code: String? = null,
    val terminal_failure_code: String? = null,
    val completion: UploadCompletionResourceWire? = null,
)

@Serializable
private data class UploadTargetWire(
    val operation: String,
    val library_id: String? = null,
    val parent_id: String? = null,
    val node_id: String? = null,
    val name: String? = null,
    val expected_revision: String? = null,
)

@Serializable
private data class UploadCompletionResourceWire(
    val id: String,
    val type: String,
    val attributes: UploadCompletionAttributesWire,
)

@Serializable
private data class UploadCompletionAttributesWire(
    val node_id: String,
    val file_version_id: String,
    val node_revision: String,
    val bytes: String,
    val sha256: String,
    val committed_at: String,
)

@Serializable
private data class UploadCompletionResponseWire(val data: UploadCompletionResourceWire, val meta: UploadMetaWire)

@Serializable
private data class UploadMetaWire(val request_id: String)
