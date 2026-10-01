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
    data object Cancelled : TransferResult
}

object TransferOperations {
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
        val staging = try {
            File.createTempFile("synveil-upload-", ".part", context.cacheDir)
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
