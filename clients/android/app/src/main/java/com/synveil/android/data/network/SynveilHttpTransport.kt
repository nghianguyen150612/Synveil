package com.synveil.android.data.network

import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.TransportPolicy
import com.synveil.android.data.enrollment.DeviceCredential
import com.synveil.android.data.enrollment.DeviceCredentialRecord
import com.synveil.android.data.enrollment.EnrollmentExchangeClient
import com.synveil.android.data.enrollment.EnrollmentExchangeResult
import com.synveil.android.data.enrollment.EnrollmentFailureReason
import com.synveil.android.data.enrollment.EnrollmentRecoveryReason
import com.synveil.android.data.enrollment.EnrollmentToken
import com.synveil.android.data.library.LibraryCollectionPage
import com.synveil.android.data.library.LibraryWireParser
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.library.NodeId
import com.synveil.android.data.node.NodePage
import com.synveil.android.data.node.NodeWireParser
import com.synveil.android.data.transfer.DownloadMetadata
import com.synveil.android.data.transfer.DownloadResult
import com.synveil.android.data.transfer.UploadResult
import com.synveil.android.data.transfer.UploadWireParser
import java.io.IOException
import java.net.ConnectException
import java.net.NoRouteToHostException
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import java.nio.charset.StandardCharsets
import java.util.concurrent.TimeUnit
import java.util.concurrent.CancellationException
import javax.net.ssl.SSLException
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import okhttp3.CookieJar
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.Response

private val OCTET_STREAM_MEDIA_TYPE = "application/octet-stream".toMediaType()
private val AUTH_JSON_MEDIA_TYPE = "application/json".toMediaType()

const val MAX_PROBE_RESPONSE_BYTES = 64 * 1024
const val MAX_ENROLLMENT_RESPONSE_BYTES = 16 * 1024
const val MAX_LIBRARY_RESPONSE_BYTES = 1024 * 1024

data class TransportTimeouts(
    val connectMillis: Long = 10_000,
    val readMillis: Long = 10_000,
    val writeMillis: Long = 10_000,
    val callMillis: Long = 15_000,
) {
    init {
        require(connectMillis > 0)
        require(readMillis > 0)
        require(writeMillis > 0)
        require(callMillis > 0)
    }
}

sealed interface SynveilTransportError {
    data object Offline : SynveilTransportError
    data object DnsFailure : SynveilTransportError
    data object Timeout : SynveilTransportError
    data object TlsError : SynveilTransportError
    data class RedirectRejected(val statusCode: Int) : SynveilTransportError
    data class HttpError(
        val statusCode: Int,
        val code: String?,
        val requestId: String?,
    ) : SynveilTransportError
    data object BodyLimitExceeded : SynveilTransportError
    data class UnexpectedContentType(val contentType: String?) : SynveilTransportError
    data object MalformedResponse : SynveilTransportError
    data class ProtocolError(val kind: ProtocolErrorKind) : SynveilTransportError
    data object ConfigurationError : SynveilTransportError
    data object Cancelled : SynveilTransportError
}

enum class ProtocolErrorKind {
    INVALID_JSON,
    INVALID_HEALTH_STATUS,
    INVALID_ERROR_ENVELOPE,
    INVALID_LIBRARY_RESPONSE,
    INVALID_NODE_RESPONSE,
    INVALID_UPLOAD_RESPONSE,
    INVALID_DOWNLOAD_RESPONSE,
}

sealed interface ProbeResult {
    data class Success(val requestId: String?) : ProbeResult
    data class Failure(val error: SynveilTransportError) : ProbeResult
}

sealed interface ConnectionCheckResult {
    data class Ready(
        val livenessRequestId: String?,
        val readinessRequestId: String?,
    ) : ConnectionCheckResult

    data class AliveButNotReady(
        val requestId: String?,
        val code: String?,
    ) : ConnectionCheckResult

    data class Failure(val error: SynveilTransportError) : ConnectionCheckResult
}

class TransportConfigurationException : IllegalArgumentException("invalid transport configuration")

class SynveilHttpTransport(
    private val profile: ServerProfile,
    userAgent: String,
    timeouts: TransportTimeouts = TransportTimeouts(),
    allowLoopbackTestHttp: Boolean = false,
) : EnrollmentExchangeClient {
    private val origin: HttpUrl = validatedOrigin(profile, allowLoopbackTestHttp)
    private val client: OkHttpClient = buildSynveilClient(timeouts)
    private val json = Json {
        ignoreUnknownKeys = false
        explicitNulls = false
    }

    init {
        require(userAgent.isNotBlank())
    }

    private val requestUserAgent = userAgent

    fun checkLiveness(): ProbeResult = executeProbe("/health/live", "live")

    fun checkReadiness(): ProbeResult = executeProbe("/health/ready", "ready")

    override fun exchange(token: EnrollmentToken): EnrollmentExchangeResult {
        val request = Request.Builder()
            .url(origin.resolve("/api/v1/device-enrollment/exchange") ?: return EnrollmentExchangeResult.Failed(EnrollmentFailureReason.INVALID_CONFIGURATION))
            .header("Accept", "application/json")
            .header("Accept-Encoding", "identity")
            .header("Content-Type", "application/json")
            .header("User-Agent", requestUserAgent)
            .post(("{\"enrollment_token\":\"${token.rawValue}\"}").toRequestBody(JSON_MEDIA_TYPE))
            .build()
        return try {
            client.newCall(request).execute().use { response ->
                if (response.isRedirect) return EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.REDIRECT)
                val body = response.body?.byteStream()?.let(::readEnrollmentBounded)
                    ?: return EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.RESPONSE_LOSS)
                if (!isJson(response.header("Content-Type"))) {
                    return EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.WRONG_CONTENT_TYPE)
                }
                val requestId = safeRequestId(response.header("X-Request-Id"))
                if (response.code == 201) return parseEnrollment(body, requestId)
                if (response.code == 503) return EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.HTTP_503)
                parseEnrollmentError(body, response.code, requestId)
            }
        } catch (_: SocketTimeoutException) {
            EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.TIMEOUT)
        } catch (_: EnrollmentBodyLimitException) {
            EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.OVERSIZED_RESPONSE)
        } catch (_: IOException) {
            EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.DISCONNECT)
        } catch (_: CancellationException) {
            EnrollmentExchangeResult.Failed(EnrollmentFailureReason.CANCELED)
        } catch (_: Exception) {
            EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.MALFORMED_RESPONSE)
        }
    }

    fun checkServer(): ConnectionCheckResult {
        val liveness = checkLiveness()
        if (liveness is ProbeResult.Failure) {
            return ConnectionCheckResult.Failure(liveness.error)
        }
        val readiness = checkReadiness()
        return when (readiness) {
            is ProbeResult.Success -> ConnectionCheckResult.Ready(
                livenessRequestId = (liveness as ProbeResult.Success).requestId,
                readinessRequestId = readiness.requestId,
            )
            is ProbeResult.Failure -> when (val error = readiness.error) {
                is SynveilTransportError.HttpError -> if (error.statusCode == 503) {
                    ConnectionCheckResult.AliveButNotReady(error.requestId, error.code)
                } else {
                    ConnectionCheckResult.Failure(error)
                }
                else -> ConnectionCheckResult.Failure(error)
            }
        }
    }

    private fun executeProbe(path: String, expectedStatus: String): ProbeResult {
        val request = Request.Builder()
            .url(origin.newBuilder().encodedPath(path).build())
            .get()
            .header("Accept", "application/json")
            .header("Accept-Encoding", "identity")
            .header("User-Agent", requestUserAgent)
            .build()
        return try {
            client.newCall(request).execute().use { response ->
                val requestId = safeRequestId(response.header("X-Request-Id"))
                if (response.code in 300..399) {
                    return ProbeResult.Failure(SynveilTransportError.RedirectRejected(response.code))
                }
                val body = response.body ?: return ProbeResult.Failure(SynveilTransportError.MalformedResponse)
                val bodyBytes = try {
                    readBounded(body.byteStream())
                } catch (_: BodyLimitException) {
                    return ProbeResult.Failure(SynveilTransportError.BodyLimitExceeded)
                }
                if (!isJson(response.header("Content-Type"))) {
                    return ProbeResult.Failure(
                        SynveilTransportError.UnexpectedContentType(response.header("Content-Type")),
                    )
                }
                if (response.code == 200) {
                    return parseHealth(bodyBytes, expectedStatus, requestId)
                }
                parseError(bodyBytes, response.code, requestId)
            }
        } catch (_: CancellationException) {
            ProbeResult.Failure(SynveilTransportError.Cancelled)
        } catch (error: Exception) {
            ProbeResult.Failure(mapNetworkError(error))
        }
    }

    private fun parseHealth(body: ByteArray, expectedStatus: String, requestId: String?): ProbeResult {
        val parsed = try {
            json.decodeFromString<HealthProbe>(body.toString(StandardCharsets.UTF_8))
        } catch (_: SerializationException) {
            return ProbeResult.Failure(SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_JSON))
        }
        if (parsed.status != expectedStatus) {
            return ProbeResult.Failure(
                SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_HEALTH_STATUS),
            )
        }
        return ProbeResult.Success(requestId)
    }

    private fun parseError(body: ByteArray, statusCode: Int, headerRequestId: String?): ProbeResult {
        val parsed = try {
            json.decodeFromString<ErrorResponse>(body.toString(StandardCharsets.UTF_8))
        } catch (_: SerializationException) {
            return ProbeResult.Failure(SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_ERROR_ENVELOPE))
        }
        val payload = parsed.error
        if (payload.code.isEmpty() || payload.code.length > 64 || !ERROR_CODE.matches(payload.code) ||
            payload.message.isEmpty() || payload.message.length > 512
        ) {
            return ProbeResult.Failure(SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_ERROR_ENVELOPE))
        }
        return ProbeResult.Failure(
            SynveilTransportError.HttpError(
                statusCode = statusCode,
                code = payload.code,
                requestId = headerRequestId ?: safeRequestId(payload.request_id),
            ),
        )
    }

    private fun mapNetworkError(error: Exception): SynveilTransportError = when (error) {
        is SocketTimeoutException -> SynveilTransportError.Timeout
        is UnknownHostException -> SynveilTransportError.DnsFailure
        is SSLException -> SynveilTransportError.TlsError
        is ConnectException, is NoRouteToHostException -> SynveilTransportError.Offline
        is IOException -> SynveilTransportError.Offline
        else -> SynveilTransportError.Offline
    }

    private fun isJson(contentType: String?): Boolean = contentType
        ?.substringBefore(';')
        ?.trim()
        ?.equals("application/json", ignoreCase = true) == true

    private fun safeRequestId(value: String?): String? = value?.takeIf { REQUEST_ID.matches(it) }

    private fun readBounded(input: java.io.InputStream): ByteArray {
        input.use { stream ->
            val output = java.io.ByteArrayOutputStream()
            val buffer = ByteArray(8 * 1024)
            var total = 0
            while (true) {
                val count = stream.read(buffer)
                if (count < 0) break
                total += count
                if (total > MAX_PROBE_RESPONSE_BYTES) throw BodyLimitException()
                output.write(buffer, 0, count)
            }
            return output.toByteArray()
        }
    }

    private fun readEnrollmentBounded(input: java.io.InputStream): ByteArray {
        input.use { stream ->
            val output = java.io.ByteArrayOutputStream()
            val buffer = ByteArray(4 * 1024)
            var total = 0
            while (true) {
                val count = stream.read(buffer)
                if (count < 0) break
                total += count
                if (total > MAX_ENROLLMENT_RESPONSE_BYTES) {
                    throw EnrollmentBodyLimitException()
                }
                output.write(buffer, 0, count)
            }
            return output.toByteArray()
        }
    }

    private fun parseEnrollment(body: ByteArray, requestId: String?): EnrollmentExchangeResult {
        val parsed = try {
            json.decodeFromString<DeviceCredentialResponse>(body.toString(StandardCharsets.UTF_8))
        } catch (_: SerializationException) {
            return EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.MALFORMED_RESPONSE)
        }
        val data = parsed.data
        val credential = DeviceCredential.parse(data.device_credential)
            ?: return EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.MALFORMED_RESPONSE)
        val metaRequestId = safeRequestId(parsed.meta.request_id)
            ?: return EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.MALFORMED_RESPONSE)
        return try {
            EnrollmentExchangeResult.Success(
                DeviceCredentialRecord(
                    ownerUserId = data.owner_user_id,
                    deviceId = data.device_id,
                    credentialId = data.credential_id,
                    credential = credential,
                    createdAt = data.created_at,
                    requestId = requestId ?: metaRequestId,
                ),
            )
        } catch (_: IllegalArgumentException) {
            EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.MALFORMED_RESPONSE)
        }
    }

    private fun parseEnrollmentError(body: ByteArray, statusCode: Int, requestId: String?): EnrollmentExchangeResult {
        val parsed = try {
            json.decodeFromString<ErrorResponse>(body.toString(StandardCharsets.UTF_8))
        } catch (_: SerializationException) {
            return EnrollmentExchangeResult.RecoveryRequired(EnrollmentRecoveryReason.MALFORMED_RESPONSE)
        }
        val error = parsed.error
        return if (error.code == "invalid_enrollment") {
            EnrollmentExchangeResult.Rejected(error.code, requestId ?: safeRequestId(error.request_id))
        } else {
            EnrollmentExchangeResult.Failed(EnrollmentFailureReason.TRANSPORT)
        }
    }

    private fun isNumericLoopback(host: String): Boolean = host == "127.0.0.1" || host == "::1"

    @Serializable
    private data class HealthProbe(val status: String)

    @Serializable
    private data class ErrorResponse(val error: ErrorPayload)

    @Serializable
    private data class ErrorPayload(
        val code: String,
        val message: String,
        val request_id: String,
        val retryable: Boolean,
        val details: JsonElement? = null,
    )

    private class BodyLimitException : IOException()
    private class EnrollmentBodyLimitException : IOException()

    @Serializable
    private data class DeviceCredentialResponse(
        val data: DeviceCredentialPayload,
        val meta: ResponseMeta,
    )

    @Serializable
    private data class DeviceCredentialPayload(
        val owner_user_id: String,
        val device_id: String,
        val credential_id: String,
        val device_credential: String,
        val created_at: String,
    )

    @Serializable
    private data class ResponseMeta(val request_id: String)

    private companion object {
        val REQUEST_ID = Regex("^[A-Za-z0-9._~-]{8,128}$")
        val ERROR_CODE = Regex("^[a-z][a-z0-9_]{1,63}$")
        val JSON_MEDIA_TYPE = "application/json".toMediaType()

    }
}

internal fun validatedOrigin(
    profile: ServerProfile,
    allowLoopbackTestHttp: Boolean,
): HttpUrl {
    val origin = profile.canonicalBaseUrl.value.toHttpUrlOrNull()
        ?: throw TransportConfigurationException()
    val policyMatchesScheme = when (profile.transportPolicy) {
        TransportPolicy.HTTPS -> origin.scheme == "https"
        TransportPolicy.LOOPBACK_TEST_HTTP -> origin.scheme == "http"
    }
    if (!policyMatchesScheme) throw TransportConfigurationException()
    if (profile.transportPolicy == TransportPolicy.LOOPBACK_TEST_HTTP &&
        (!allowLoopbackTestHttp || !isNumericLoopbackHost(origin.host))
    ) {
        throw TransportConfigurationException()
    }
    return origin
}

internal fun buildSynveilClient(timeouts: TransportTimeouts): OkHttpClient = OkHttpClient.Builder()
    .followRedirects(false)
    .followSslRedirects(false)
    .retryOnConnectionFailure(false)
    .cookieJar(CookieJar.NO_COOKIES)
    .connectTimeout(timeouts.connectMillis, TimeUnit.MILLISECONDS)
    .readTimeout(timeouts.readMillis, TimeUnit.MILLISECONDS)
    .writeTimeout(timeouts.writeMillis, TimeUnit.MILLISECONDS)
    .callTimeout(timeouts.callMillis, TimeUnit.MILLISECONDS)
    .build()

private fun isNumericLoopbackHost(host: String): Boolean = host == "127.0.0.1" || host == "::1"

class AuthenticatedSynveilTransport internal constructor(
    profile: ServerProfile,
    credential: DeviceCredential,
    userAgent: String,
    timeouts: TransportTimeouts = TransportTimeouts(),
    allowLoopbackTestHttp: Boolean = false,
) {
    private val origin = validatedOrigin(profile, allowLoopbackTestHttp)
    private val client = buildSynveilClient(timeouts)
    private val requestUserAgent = userAgent.also { require(it.isNotBlank()) }
    private val bearer = credential.rawValue
    private val json = Json {
        ignoreUnknownKeys = false
        explicitNulls = false
    }

    init {
        require(bearer.startsWith("svd1_"))
    }

    fun listLibrariesPage(cursor: String?): LibraryPageResult {
        if (cursor != null && (cursor.isEmpty() || cursor.length > 512)) {
            return LibraryPageResult.Failure(
                SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_LIBRARY_RESPONSE),
            )
        }
        val url = origin.newBuilder()
            .encodedPath("/api/v1/libraries")
            .addQueryParameter("limit", "100")
            .apply { cursor?.let { addQueryParameter("cursor", it) } }
            .build()
        val request = Request.Builder()
            .url(url)
            .get()
            .header("Accept", "application/json")
            .header("Accept-Encoding", "identity")
            .header("User-Agent", requestUserAgent)
            .header("Authorization", "Bearer $bearer")
            .build()
        return try {
            client.newCall(request).execute().use { response ->
                if (response.isRedirect) {
                    return LibraryPageResult.Failure(SynveilTransportError.RedirectRejected(response.code))
                }
                val body = response.body ?: return LibraryPageResult.Failure(SynveilTransportError.MalformedResponse)
                val bodyBytes = try {
                    readBounded(body.byteStream(), MAX_LIBRARY_RESPONSE_BYTES)
                } catch (_: BodyLimitException) {
                    return LibraryPageResult.Failure(SynveilTransportError.BodyLimitExceeded)
                }
                if (!isJsonContentType(response.header("Content-Type"))) {
                    return LibraryPageResult.Failure(
                        SynveilTransportError.UnexpectedContentType(response.header("Content-Type")),
                    )
                }
                val requestId = safeRequestIdHeader(response.header("X-Request-Id"))
                if (response.code == 200) {
                    return LibraryWireParser.parse(json, bodyBytes, requestId)
                }
                LibraryPageResult.Failure(parseErrorResponse(json, bodyBytes, response.code, requestId))
            }
        } catch (_: CancellationException) {
            LibraryPageResult.Failure(SynveilTransportError.Cancelled)
        } catch (error: Exception) {
            LibraryPageResult.Failure(mapNetworkTransportError(error))
        }
    }

    fun listNodesPage(libraryId: LibraryId, parentId: NodeId?, cursor: String?): NodePageResult {
        if (cursor != null && (cursor.isEmpty() || cursor.length > 512)) {
            return NodePageResult.Failure(SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_NODE_RESPONSE))
        }
        val url = origin.newBuilder().encodedPath("/api/v1/libraries/${libraryId.value}/nodes")
            .addQueryParameter("limit", "100")
            .apply {
                parentId?.let { addQueryParameter("parent_id", it.value) }
                cursor?.let { addQueryParameter("cursor", it) }
            }.build()
        return try {
            client.newCall(Request.Builder().url(url).get().authenticatedHeaders(requestUserAgent, bearer).build()).execute().use { response ->
                if (response.isRedirect) return NodePageResult.Failure(SynveilTransportError.RedirectRejected(response.code))
                val body = try { readBounded(response.body?.byteStream() ?: return NodePageResult.Failure(SynveilTransportError.MalformedResponse), MAX_LIBRARY_RESPONSE_BYTES) }
                catch (_: BodyLimitException) { return NodePageResult.Failure(SynveilTransportError.BodyLimitExceeded) }
                if (!isJsonContentType(response.header("Content-Type"))) return NodePageResult.Failure(SynveilTransportError.UnexpectedContentType(response.header("Content-Type")))
                val requestId = safeRequestIdHeader(response.header("X-Request-Id"))
                if (response.code == 200) NodeWireParser.parse(json, body, requestId)
                else NodePageResult.Failure(parseErrorResponse(json, body, response.code, requestId))
            }
        } catch (_: CancellationException) { NodePageResult.Failure(SynveilTransportError.Cancelled) }
        catch (error: Exception) { NodePageResult.Failure(mapNetworkTransportError(error)) }
    }

    fun openCurrentContent(nodeId: NodeId, rangeStart: Long? = null): DownloadResult {
        val url = origin.newBuilder().encodedPath("/api/v1/nodes/${nodeId.value}/content").build()
        val builder = Request.Builder().url(url).get().authenticatedHeaders(requestUserAgent, bearer)
        if (rangeStart != null) builder.header("Range", "bytes=$rangeStart-")
        return try {
            val response = client.newCall(builder.build()).execute()
            if (response.isRedirect) {
                response.close()
                DownloadResult.Failure(SynveilTransportError.RedirectRejected(response.code))
            } else if (response.code in setOf(200, 206, 304)) {
                val metadata = validateDownloadHeaders(response)
                if (metadata == null) {
                    response.close()
                    DownloadResult.Failure(SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_DOWNLOAD_RESPONSE))
                } else {
                    DownloadResult.Success(response, metadata)
                }
            } else {
                response.use { DownloadResult.Failure(parseErrorResponse(json, boundedBody(it), it.code, safeRequestIdHeader(it.header("X-Request-Id")))) }
            }
        } catch (_: CancellationException) {
            DownloadResult.Failure(SynveilTransportError.Cancelled)
        } catch (error: Exception) {
            DownloadResult.Failure(mapNetworkTransportError(error))
        }
    }

    fun createUploadSession(requestJson: String): UploadResult =
        executeUploadJson("/api/v1/upload-sessions", "POST", requestJson.toRequestBody(AUTH_JSON_MEDIA_TYPE))

    fun getUploadSession(sessionId: String): UploadResult =
        executeUploadJson("/api/v1/upload-sessions/$sessionId", "GET", null)

    fun appendUploadChunk(sessionId: String, offset: Long, chunk: ByteArray): UploadResult {
        val request = Request.Builder().url(origin.newBuilder().encodedPath("/api/v1/upload-sessions/$sessionId").build())
            .patch(chunk.toRequestBody(OCTET_STREAM_MEDIA_TYPE))
            .authenticatedHeaders(requestUserAgent, bearer)
            .header("Upload-Offset", offset.toString())
            .build()
        return executeUploadRequest(request)
    }

    fun completeUpload(sessionId: String): UploadResult =
        executeUploadCompletion(sessionId)

    fun abortUpload(sessionId: String): UploadResult =
        executeUploadJson("/api/v1/upload-sessions/$sessionId/abort", "POST", "{}".toRequestBody(AUTH_JSON_MEDIA_TYPE))

    private fun executeUploadJson(path: String, method: String, body: okhttp3.RequestBody?): UploadResult {
        val request = Request.Builder().url(origin.newBuilder().encodedPath(path).build())
            .method(method, body).authenticatedHeaders(requestUserAgent, bearer).build()
        return executeUploadRequest(request)
    }

    private fun executeUploadCompletion(sessionId: String): UploadResult {
        val request = Request.Builder().url(origin.newBuilder().encodedPath("/api/v1/upload-sessions/$sessionId/complete").build())
            .post("{}".toRequestBody(AUTH_JSON_MEDIA_TYPE)).authenticatedHeaders(requestUserAgent, bearer).build()
        return try {
            client.newCall(request).execute().use { response ->
                if (response.isRedirect) return UploadResult.Failure(SynveilTransportError.RedirectRejected(response.code))
                val body = response.body?.byteStream()?.let { readBounded(it, MAX_LIBRARY_RESPONSE_BYTES) } ?: ByteArray(0)
                if (!isJsonContentType(response.header("Content-Type"))) return UploadResult.Failure(SynveilTransportError.UnexpectedContentType(response.header("Content-Type")))
                if (response.code in 200..299) {
                    val completion = try { UploadWireParser.parseCompletion(json, body) }
                    catch (_: Exception) { return UploadResult.Failure(SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_UPLOAD_RESPONSE)) }
                    UploadResult.Completion(completion)
                } else UploadResult.Failure(parseErrorResponse(json, body, response.code, safeRequestIdHeader(response.header("X-Request-Id"))))
            }
        } catch (_: CancellationException) { UploadResult.Failure(SynveilTransportError.Cancelled) }
        catch (error: Exception) { UploadResult.Failure(mapNetworkTransportError(error)) }
    }

    private fun executeUploadRequest(request: Request): UploadResult {
        return try {
            client.newCall(request).execute().use { response ->
                if (response.isRedirect) return UploadResult.Failure(SynveilTransportError.RedirectRejected(response.code))
                val body = response.body?.byteStream()?.let { readBounded(it, MAX_LIBRARY_RESPONSE_BYTES) } ?: ByteArray(0)
                if (response.code == 204) {
                    val offset = response.header("Upload-Offset")
                        ?.takeIf(::isCanonicalUnsignedDecimal)
                        ?.toLongOrNull()
                    return UploadResult.Offset(offset)
                }
                if (!isJsonContentType(response.header("Content-Type"))) return UploadResult.Failure(SynveilTransportError.UnexpectedContentType(response.header("Content-Type")))
                if (response.code in 200..299) {
                    val session = try { UploadWireParser.parse(json, body, response.header("Upload-Offset")) }
                    catch (_: Exception) { return UploadResult.Failure(SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_UPLOAD_RESPONSE)) }
                    UploadResult.Session(session)
                }
                else UploadResult.Failure(parseErrorResponse(json, body, response.code, safeRequestIdHeader(response.header("X-Request-Id"))))
            }
        } catch (_: CancellationException) { UploadResult.Failure(SynveilTransportError.Cancelled) }
        catch (error: Exception) { UploadResult.Failure(mapNetworkTransportError(error)) }
    }

    private fun boundedBody(response: okhttp3.Response): ByteArray =
        response.body?.byteStream()?.let { readBounded(it, MAX_LIBRARY_RESPONSE_BYTES) } ?: ByteArray(0)

    private fun validateDownloadHeaders(response: okhttp3.Response): DownloadMetadata? {
        val length = response.header("Content-Length")?.toLongOrNull()
        if (response.header("Content-Length") != null && (length == null || length < 0)) return null
        val contentType = response.header("Content-Type")?.substringBefore(';')?.trim()?.takeIf { it.isNotEmpty() }
        if (contentType != null && !contentType.contains('/') ) return null
        val contentRange = response.header("Content-Range")
        if (response.code == 206) {
            val match = Regex("bytes ([0-9]+)-([0-9]+)/([0-9]+)").matchEntire(contentRange ?: "") ?: return null
            val start = match.groupValues[1].toLongOrNull() ?: return null
            val end = match.groupValues[2].toLongOrNull() ?: return null
            val total = match.groupValues[3].toLongOrNull() ?: return null
            if (start > end || end >= total || length != end - start + 1) return null
        }
        return DownloadMetadata(length, contentType, response.header("Content-Disposition"), response.header("ETag"), contentRange)
    }
}

private fun Request.Builder.authenticatedHeaders(userAgent: String, bearer: String): Request.Builder =
    header("Accept", "application/json")
        .header("Accept-Encoding", "identity")
        .header("User-Agent", userAgent)
        .header("Authorization", "Bearer $bearer")

sealed interface NodePageResult {
    data class Success(val page: NodePage) : NodePageResult
    data class Failure(val error: SynveilTransportError) : NodePageResult
}

sealed interface LibraryPageResult {
    data class Success(val page: LibraryCollectionPage) : LibraryPageResult
    data class Failure(val error: SynveilTransportError) : LibraryPageResult
}

private fun parseErrorResponse(
    json: Json,
    body: ByteArray,
    statusCode: Int,
    headerRequestId: String?,
): SynveilTransportError {
    val parsed = try {
        json.decodeFromString<ErrorResponseEnvelope>(body.toString(StandardCharsets.UTF_8))
    } catch (_: SerializationException) {
        return SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_ERROR_ENVELOPE)
    }
    val payload = parsed.error
    if (payload.code.isEmpty() || payload.code.length > 64 || !ERROR_CODE_PATTERN.matches(payload.code) ||
        payload.message.isEmpty() || payload.message.length > 512
    ) {
        return SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_ERROR_ENVELOPE)
    }
    return SynveilTransportError.HttpError(
        statusCode = statusCode,
        code = payload.code,
        requestId = headerRequestId ?: safeRequestIdHeader(payload.request_id),
    )
}

private fun mapNetworkTransportError(error: Exception): SynveilTransportError = when (error) {
    is SocketTimeoutException -> SynveilTransportError.Timeout
    is UnknownHostException -> SynveilTransportError.DnsFailure
    is SSLException -> SynveilTransportError.TlsError
    is ConnectException, is NoRouteToHostException -> SynveilTransportError.Offline
    is IOException -> SynveilTransportError.Offline
    else -> SynveilTransportError.Offline
}

private fun isJsonContentType(value: String?): Boolean = value
    ?.substringBefore(';')
    ?.trim()
    ?.equals("application/json", ignoreCase = true) == true

private fun safeRequestIdHeader(value: String?): String? = value?.takeIf { REQUEST_ID_PATTERN.matches(it) }

private fun isCanonicalUnsignedDecimal(value: String): Boolean =
    value.matches(Regex("^(0|[1-9][0-9]*)$"))

private fun readBounded(input: java.io.InputStream, maximumBytes: Int): ByteArray {
    input.use { stream ->
        val output = java.io.ByteArrayOutputStream()
        val buffer = ByteArray(8 * 1024)
        var total = 0
        while (true) {
            val count = stream.read(buffer)
            if (count < 0) break
            total += count
            if (total > maximumBytes) throw BodyLimitException()
            output.write(buffer, 0, count)
        }
        return output.toByteArray()
    }
}

private class BodyLimitException : IOException()

@Serializable
private data class ErrorResponseEnvelope(val error: ErrorPayloadEnvelope)

@Serializable
private data class ErrorPayloadEnvelope(
    val code: String,
    val message: String,
    val request_id: String,
    val retryable: Boolean,
    val details: JsonElement? = null,
)

private val REQUEST_ID_PATTERN = Regex("^[A-Za-z0-9._~-]{8,128}$")
private val ERROR_CODE_PATTERN = Regex("^[a-z][a-z0-9_]{1,63}$")
