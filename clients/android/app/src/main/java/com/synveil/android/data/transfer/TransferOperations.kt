package com.synveil.android.data.transfer

import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.library.NodeId
import com.synveil.android.data.network.AuthenticatedSynveilTransport
import com.synveil.android.data.network.SynveilTransportError
import com.synveil.android.data.cache.CacheRepository
import com.synveil.android.data.cache.ContentOperationEntity
import com.synveil.android.data.mutation.newUuidV7
import java.io.File
import java.io.IOException
import java.io.RandomAccessFile
import java.security.MessageDigest
import java.util.Locale
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

sealed interface TransferProgress {
    data object Preparing : TransferProgress
    data class Downloading(val transferred: Long, val total: Long?) : TransferProgress
    data class Uploading(val transferred: Long, val total: Long) : TransferProgress
    data object Verifying : TransferProgress
    data object Completed : TransferProgress
    data class Failed(val message: String) : TransferProgress
    data object Cancelled : TransferProgress
}

sealed interface TransferResult {
    data class Saved(val bytes: Long, val uri: Uri) : TransferResult
    data class Uploaded(val completion: UploadCompletion) : TransferResult
    data class Failed(val error: SynveilTransportError) : TransferResult
    data class Rejected(val reason: TransferRejectionReason) : TransferResult
    data object Cancelled : TransferResult
}

enum class TransferRejectionReason {
    STAGING_QUOTA_EXCEEDED,
    INSUFFICIENT_STORAGE,
    SOURCE_UNAVAILABLE,
}

const val MAX_STAGING_BYTES: Long = 512L * 1024L * 1024L
const val MIN_STAGING_FREE_BYTES: Long = 64L * 1024L * 1024L

internal fun stagingRejectionReason(
    existingBytes: Long,
    incomingBytes: Long,
    availableBytes: Long,
): TransferRejectionReason? = when {
    existingBytes < 0L || incomingBytes < 0L || availableBytes < 0L -> TransferRejectionReason.INSUFFICIENT_STORAGE
    existingBytes + incomingBytes > MAX_STAGING_BYTES -> TransferRejectionReason.STAGING_QUOTA_EXCEEDED
    availableBytes < incomingBytes + MIN_STAGING_FREE_BYTES -> TransferRejectionReason.INSUFFICIENT_STORAGE
    else -> null
}

object TransferOperations {
    suspend fun replaceContent(
        context: Context,
        cache: CacheRepository,
        profileId: String,
        deviceId: String,
        transport: AuthenticatedSynveilTransport,
        resolver: ContentResolver,
        sourceUri: Uri,
        libraryId: LibraryId,
        nodeId: NodeId,
        expectedRevision: String,
        onProgress: (TransferProgress) -> Unit,
    ): TransferResult = withContext(Dispatchers.IO) {
        val stagingDirectory = File(context.filesDir, "transfer-staging").apply { mkdirs() }
        val operationId = newUuidV7()
        val staging = File(stagingDirectory, "$operationId.part")
        val digest = MessageDigest.getInstance("SHA-256")
        var bytes = 0L
        try {
            onProgress(TransferProgress.Preparing)
            resolver.openInputStream(sourceUri)?.use { input ->
                staging.outputStream().use { output ->
                    val buffer = ByteArray(32 * 1024)
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        stagingRejectionReason(
                            existingBytes = stagingDirectory.stagedBytes(staging),
                            incomingBytes = bytes + count,
                            availableBytes = stagingDirectory.usableSpace,
                        )?.let { reason ->
                            onProgress(TransferProgress.Failed(reason.message()))
                            return@withContext TransferResult.Rejected(reason)
                        }
                        output.write(buffer, 0, count)
                        digest.update(buffer, 0, count)
                        bytes += count
                    }
                }
            } ?: return@withContext TransferResult.Failed(SynveilTransportError.Offline)
            val sha256 = digest.digest().joinToString("") { "%02x".format(Locale.ROOT, it) }
            cache.upsertContentOperation(ContentOperationEntity(profileId, deviceId, libraryId.value, operationId, nodeId.value, expectedRevision, staging.absolutePath, bytes, sha256, null, 0, "READY", System.currentTimeMillis(), null, null))
            val request = buildJsonObject {
                put("operation", "REPLACE_CONTENT")
                put("idempotency_key", operationId)
                put("library_id", libraryId.value)
                put("node_id", nodeId.value)
                put("expected_revision", expectedRevision)
                put("expected_bytes", bytes.toString())
                put("expected_sha256", sha256)
            }.toString()
            var sessionResult = transport.createUploadSession(request)
            if (sessionResult is UploadResult.Failure && sessionResult.error.isAmbiguousTransferFailure()) {
                sessionResult = transport.createUploadSession(request)
            }
            var session = when (sessionResult) {
                is UploadResult.Session -> sessionResult.session
                is UploadResult.Failure -> return@withContext TransferResult.Failed(sessionResult.error)
                else -> return@withContext TransferResult.Failed(protocolFailure())
            }
            if (session.operation != "REPLACE_CONTENT" || session.target.nodeId != nodeId.value || session.target.libraryId != libraryId.value || session.expectedBytes != bytes || session.expectedSha256 != sha256) return@withContext TransferResult.Failed(protocolFailure())
            cache.upsertContentOperation(ContentOperationEntity(profileId, deviceId, libraryId.value, operationId, nodeId.value, expectedRevision, staging.absolutePath, bytes, sha256, session.id, session.receivedBytes, "UPLOADING", System.currentTimeMillis(), System.currentTimeMillis(), null))
            var offset = session.receivedBytes
            val chunkSize = 4 * 1024 * 1024
            RandomAccessFile(staging, "r").use { file ->
                while (offset < bytes) {
                    file.seek(offset)
                    val chunk = ByteArray(minOf(chunkSize.toLong(), bytes - offset).toInt())
                    var read = 0
                    while (read < chunk.size) { val count = file.read(chunk, read, chunk.size - read); if (count < 0) return@withContext TransferResult.Failed(protocolFailure()); read += count }
                    when (val append = transport.appendUploadChunk(session.id, offset, chunk)) {
                        is UploadResult.Offset -> { offset = append.value ?: return@withContext TransferResult.Failed(protocolFailure()); if (offset !in 0..bytes) return@withContext TransferResult.Failed(protocolFailure()) }
                        is UploadResult.Failure -> {
                            if (!append.error.isAmbiguousTransferFailure()) return@withContext TransferResult.Failed(append.error)
                            session = when (val recovered = transport.getUploadSession(session.id)) { is UploadResult.Session -> recovered.session; is UploadResult.Failure -> return@withContext TransferResult.Failed(recovered.error); else -> return@withContext TransferResult.Failed(protocolFailure()) }
                            offset = session.receivedBytes
                        }
                        else -> return@withContext TransferResult.Failed(protocolFailure())
                    }
                    cache.upsertContentOperation(ContentOperationEntity(profileId, deviceId, libraryId.value, operationId, nodeId.value, expectedRevision, staging.absolutePath, bytes, sha256, session.id, offset, "UPLOADING", System.currentTimeMillis(), System.currentTimeMillis(), null))
                    onProgress(TransferProgress.Uploading(offset, bytes))
                }
            }
            onProgress(TransferProgress.Verifying)
            when (val completion = transport.completeUpload(session.id)) {
                is UploadResult.Completion -> {
                    if (completion.completion.nodeId != nodeId.value || completion.completion.bytes != bytes || completion.completion.sha256 != sha256) return@withContext TransferResult.Failed(protocolFailure())
                    cache.upsertContentOperation(ContentOperationEntity(profileId, deviceId, libraryId.value, operationId, nodeId.value, expectedRevision, staging.absolutePath, bytes, sha256, session.id, bytes, "COMMITTED", System.currentTimeMillis(), System.currentTimeMillis(), null))
                    cache.recordContentCompletion(profileId, libraryId, nodeId.value, completion.completion.fileVersionId, completion.completion.nodeRevision, bytes.toString(), sha256)
                    staging.delete()
                    onProgress(TransferProgress.Completed)
                    TransferResult.Uploaded(completion.completion)
                }
                is UploadResult.Failure -> TransferResult.Failed(completion.error)
                else -> TransferResult.Failed(protocolFailure())
            }
        } catch (_: CancellationException) { onProgress(TransferProgress.Cancelled); TransferResult.Cancelled }
        catch (_: Exception) { TransferResult.Failed(SynveilTransportError.Offline) }
    }

    suspend fun resumeContentOperation(
        cache: CacheRepository,
        operation: ContentOperationEntity,
        transport: AuthenticatedSynveilTransport,
    ): TransferResult = withContext(Dispatchers.IO) {
        val staging = File(operation.stagingPath)
        if (!staging.isFile || staging.length() != operation.byteLength) {
            cache.upsertContentOperation(operation.copy(state = "FAILED", lastErrorCategory = "staging_missing", lastAttemptAt = System.currentTimeMillis()))
            return@withContext TransferResult.Failed(SynveilTransportError.ProtocolError(com.synveil.android.data.network.ProtocolErrorKind.INVALID_UPLOAD_RESPONSE))
        }
        val request = buildJsonObject {
            put("operation", "REPLACE_CONTENT")
            put("idempotency_key", operation.operationId)
            put("library_id", operation.libraryId)
            put("node_id", operation.nodeId)
            put("expected_revision", operation.expectedNodeRevision)
            put("expected_bytes", operation.byteLength.toString())
            put("expected_sha256", operation.sha256)
        }.toString()
        try {
            var session = if (operation.uploadSessionId == null) {
                when (val created = transport.createUploadSession(request)) {
                    is UploadResult.Session -> created.session
                    is UploadResult.Failure -> return@withContext TransferResult.Failed(created.error)
                    else -> return@withContext TransferResult.Failed(protocolFailure())
                }
            } else {
                when (val recovered = transport.getUploadSession(operation.uploadSessionId)) {
                    is UploadResult.Session -> recovered.session
                    is UploadResult.Failure -> return@withContext TransferResult.Failed(recovered.error)
                    else -> return@withContext TransferResult.Failed(protocolFailure())
                }
            }
            var offset = session.receivedBytes
            val chunkSize = 4 * 1024 * 1024
            RandomAccessFile(staging, "r").use { file ->
                while (offset < operation.byteLength) {
                    file.seek(offset)
                    val chunk = ByteArray(minOf(chunkSize.toLong(), operation.byteLength - offset).toInt())
                    var read = 0
                    while (read < chunk.size) { val count = file.read(chunk, read, chunk.size - read); if (count < 0) return@withContext TransferResult.Failed(protocolFailure()); read += count }
                    when (val appended = transport.appendUploadChunk(session.id, offset, chunk)) {
                        is UploadResult.Offset -> offset = appended.value ?: return@withContext TransferResult.Failed(protocolFailure())
                        is UploadResult.Failure -> {
                            if (!appended.error.isAmbiguousTransferFailure()) return@withContext TransferResult.Failed(appended.error)
                            session = when (val recovered = transport.getUploadSession(session.id)) { is UploadResult.Session -> recovered.session; is UploadResult.Failure -> return@withContext TransferResult.Failed(recovered.error); else -> return@withContext TransferResult.Failed(protocolFailure()) }
                            offset = session.receivedBytes
                        }
                        else -> return@withContext TransferResult.Failed(protocolFailure())
                    }
                    cache.upsertContentOperation(operation.copy(uploadSessionId = session.id, serverOffset = offset, state = "UPLOADING", lastAttemptAt = System.currentTimeMillis(), lastErrorCategory = null))
                }
            }
            when (val completion = transport.completeUpload(session.id)) {
                is UploadResult.Completion -> {
                    if (completion.completion.nodeId != operation.nodeId || completion.completion.bytes != operation.byteLength || completion.completion.sha256 != operation.sha256) return@withContext TransferResult.Failed(protocolFailure())
                    cache.upsertContentOperation(operation.copy(uploadSessionId = session.id, serverOffset = operation.byteLength, state = "COMMITTED", lastAttemptAt = System.currentTimeMillis(), lastErrorCategory = null))
                    cache.recordContentCompletion(operation.profileId, LibraryId.parse(operation.libraryId) ?: return@withContext TransferResult.Failed(protocolFailure()), operation.nodeId, completion.completion.fileVersionId, completion.completion.nodeRevision, operation.byteLength.toString(), operation.sha256)
                    staging.delete()
                    TransferResult.Uploaded(completion.completion)
                }
                is UploadResult.Failure -> TransferResult.Failed(completion.error)
                else -> TransferResult.Failed(protocolFailure())
            }
        } catch (_: Exception) { TransferResult.Failed(SynveilTransportError.Offline) }
    }
    suspend fun downloadTo(
        transport: AuthenticatedSynveilTransport,
        nodeId: NodeId,
        resolver: ContentResolver,
        destination: Uri,
        onProgress: (TransferProgress) -> Unit,
    ): TransferResult = withContext(Dispatchers.IO) {
        onProgress(TransferProgress.Preparing)
        when (val result = transport.openCurrentContent(nodeId)) {
            is DownloadResult.Failure -> TransferResult.Failed(result.error)
            is DownloadResult.Success -> {
                try {
                    val output = resolver.openOutputStream(destination, "w")
                        ?: return@withContext TransferResult.Failed(SynveilTransportError.Offline)
                    output.use { sink ->
                        result.response.body?.byteStream()?.use { source ->
                            val buffer = ByteArray(32 * 1024)
                            var transferred = 0L
                            while (true) {
                                val read = source.read(buffer)
                                if (read < 0) break
                                sink.write(buffer, 0, read)
                                transferred += read
                                onProgress(TransferProgress.Downloading(transferred, result.metadata.contentLength))
                            }
                            if (result.metadata.contentLength != null && transferred != result.metadata.contentLength) {
                                return@withContext TransferResult.Failed(SynveilTransportError.ProtocolError(com.synveil.android.data.network.ProtocolErrorKind.INVALID_DOWNLOAD_RESPONSE))
                            }
                            onProgress(TransferProgress.Completed)
                            TransferResult.Saved(transferred, destination)
                        } ?: TransferResult.Failed(SynveilTransportError.MalformedResponse)
                    }
                } catch (_: CancellationException) {
                    onProgress(TransferProgress.Cancelled)
                    TransferResult.Cancelled
                } catch (_: Exception) {
                    TransferResult.Failed(SynveilTransportError.Offline)
                } finally {
                    result.response.close()
                }
            }
        }
    }

    suspend fun uploadCreateFile(
        context: Context,
        transport: AuthenticatedSynveilTransport,
        resolver: ContentResolver,
        sourceUri: Uri,
        libraryId: LibraryId,
        parentId: NodeId,
        name: String,
        onProgress: (TransferProgress) -> Unit,
    ): TransferResult = withContext(Dispatchers.IO) {
        val stagingDirectory = File(context.filesDir, "transfer-staging").apply { mkdirs() }
        val staging = try {
            File.createTempFile("synveil-upload-", ".part", stagingDirectory)
        } catch (_: IOException) {
            return@withContext TransferResult.Failed(SynveilTransportError.Offline)
        }
        var sessionId: String? = null
        try {
            onProgress(TransferProgress.Preparing)
            val digest = MessageDigest.getInstance("SHA-256")
            var expectedBytes = 0L
            resolver.openInputStream(sourceUri)?.use { input ->
                staging.outputStream().use { output ->
                    val buffer = ByteArray(32 * 1024)
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        stagingRejectionReason(
                            existingBytes = stagingDirectory.stagedBytes(staging),
                            incomingBytes = expectedBytes + read,
                            availableBytes = stagingDirectory.usableSpace,
                        )?.let { reason ->
                            onProgress(TransferProgress.Failed(reason.message()))
                            return@withContext TransferResult.Rejected(reason)
                        }
                        output.write(buffer, 0, read)
                        digest.update(buffer, 0, read)
                        expectedBytes += read
                    }
                }
            } ?: return@withContext TransferResult.Failed(SynveilTransportError.Offline)
            val expectedSha256 = digest.digest().joinToString("") { "%02x".format(Locale.ROOT, it) }
            val request = buildJsonObject {
                put("operation", "CREATE_FILE")
                put("idempotency_key", uuidV7())
                put("library_id", libraryId.value)
                put("parent_id", parentId.value)
                put("name", safeLogicalName(name))
                put("expected_bytes", expectedBytes.toString())
                put("expected_sha256", expectedSha256)
            }.toString()
            var created = transport.createUploadSession(request)
            if (created is UploadResult.Failure && created.error.isAmbiguousTransferFailure()) {
                created = transport.createUploadSession(request)
            }
            var session = when (created) {
                is UploadResult.Session -> created.session
                is UploadResult.Failure -> return@withContext TransferResult.Failed(created.error)
                else -> return@withContext TransferResult.Failed(protocolFailure())
            }
            if (session.operation != "CREATE_FILE" ||
                session.expectedBytes != expectedBytes ||
                session.target.libraryId != libraryId.value ||
                session.target.parentId != parentId.value ||
                session.target.name != safeLogicalName(name)
            ) return@withContext TransferResult.Failed(protocolFailure())
            sessionId = session.id
            var offset = session.receivedBytes
            val chunkSize = 4 * 1024 * 1024
            RandomAccessFile(staging, "r").use { file ->
                while (offset < expectedBytes) {
                    file.seek(offset)
                    val wanted = minOf(chunkSize.toLong(), expectedBytes - offset).toInt()
                    val chunk = ByteArray(wanted)
                    var readTotal = 0
                    while (readTotal < wanted) {
                        val read = file.read(chunk, readTotal, wanted - readTotal)
                        if (read < 0) return@withContext TransferResult.Failed(protocolFailure())
                        readTotal += read
                    }
                    when (val appended = transport.appendUploadChunk(session.id, offset, chunk)) {
                        is UploadResult.Offset -> {
                            val authoritative = appended.value ?: return@withContext TransferResult.Failed(protocolFailure())
                            if (authoritative !in (offset + 1)..expectedBytes) return@withContext TransferResult.Failed(protocolFailure())
                            offset = authoritative
                            onProgress(TransferProgress.Uploading(offset, expectedBytes))
                        }
                        is UploadResult.Failure -> {
                            if (!appended.error.isAmbiguousTransferFailure()) {
                                return@withContext TransferResult.Failed(appended.error)
                            }
                            val recovered = transport.getUploadSession(session.id)
                            session = when (recovered) {
                                is UploadResult.Session -> recovered.session
                                is UploadResult.Failure -> return@withContext TransferResult.Failed(recovered.error)
                                else -> return@withContext TransferResult.Failed(protocolFailure())
                            }
                            if (session.state != UploadState.OPEN ||
                                session.expectedBytes != expectedBytes ||
                                session.target.libraryId != libraryId.value ||
                                session.target.parentId != parentId.value
                            ) {
                                return@withContext TransferResult.Failed(appended.error)
                            }
                            offset = session.receivedBytes
                            onProgress(TransferProgress.Uploading(offset, expectedBytes))
                        }
                        else -> return@withContext TransferResult.Failed(protocolFailure())
                    }
                }
            }
            onProgress(TransferProgress.Verifying)
            when (val completed = transport.completeUpload(session.id)) {
                is UploadResult.Completion -> {
                    if (completed.completion.bytes != expectedBytes ||
                        completed.completion.sha256 != expectedSha256
                    ) {
                        return@withContext TransferResult.Failed(protocolFailure())
                    }
                    onProgress(TransferProgress.Completed)
                    TransferResult.Uploaded(completed.completion)
                }
                is UploadResult.Failure -> TransferResult.Failed(completed.error)
                else -> TransferResult.Failed(protocolFailure())
            }
        } catch (_: CancellationException) {
            onProgress(TransferProgress.Cancelled)
            sessionId?.let { id -> withContext(NonCancellable) { transport.abortUpload(id) } }
            TransferResult.Cancelled
        } catch (_: Exception) {
            TransferResult.Failed(SynveilTransportError.Offline)
        } finally {
            staging.delete()
        }
    }

    fun safeLogicalName(value: String): String {
        val leaf = value.replace('\\', '/').substringAfterLast('/')
        val sanitized = leaf.filter { it >= ' ' && it != '\u007f' && it != '\u0000' }
            .trim('.', ' ')
        return sanitized.take(1024).ifEmpty { "download" }
    }

    fun openIntent(uri: Uri, mimeType: String?): Intent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(uri, mimeType ?: "application/octet-stream")
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }

    fun shareIntent(uri: Uri, mimeType: String?): Intent = Intent(Intent.ACTION_SEND).apply {
        type = mimeType ?: "application/octet-stream"
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }

    fun suggestedName(resolver: ContentResolver, uri: Uri, fallback: String): String {
        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) return safeLogicalName(cursor.getString(0))
        }
        return safeLogicalName(fallback)
    }

    private fun protocolFailure() = SynveilTransportError.ProtocolError(com.synveil.android.data.network.ProtocolErrorKind.INVALID_UPLOAD_RESPONSE)

    private fun TransferRejectionReason.message(): String = when (this) {
        TransferRejectionReason.STAGING_QUOTA_EXCEEDED -> "Transfer exceeds the local staging limit."
        TransferRejectionReason.INSUFFICIENT_STORAGE -> "Not enough free storage to stage this transfer."
        TransferRejectionReason.SOURCE_UNAVAILABLE -> "The selected source is no longer available."
    }

    private fun File.stagedBytes(current: File): Long = listFiles().orEmpty()
        .filter { it != current && it.isFile }
        .sumOf { it.length() }

    private fun SynveilTransportError.isAmbiguousTransferFailure(): Boolean =
        this is SynveilTransportError.Timeout || this is SynveilTransportError.Offline

    private fun uuidV7(): String {
        val timestamp = System.currentTimeMillis()
        val random = UUID.randomUUID()
        val most = (timestamp shl 16) or (0x7L shl 12) or (random.mostSignificantBits and 0xfffL)
        val least = (random.leastSignificantBits and 0x3fff_ffff_ffff_ffffL) or Long.MIN_VALUE
        return UUID(most, least).toString()
    }
}
