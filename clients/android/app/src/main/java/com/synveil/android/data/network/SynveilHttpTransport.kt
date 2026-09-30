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

const val MAX_PROBE_RESPONSE_BYTES = 64 * 1024
const val MAX_ENROLLMENT_RESPONSE_BYTES = 16 * 1024

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
    private val origin: HttpUrl
    private val client: OkHttpClient = buildClient(timeouts)
    private val json = Json {
        ignoreUnknownKeys = false
        explicitNulls = false
    }

    init {
        origin = profile.canonicalBaseUrl.value.toHttpUrlOrNull()
            ?: throw TransportConfigurationException()
        val policyMatchesScheme = when (profile.transportPolicy) {
            TransportPolicy.HTTPS -> origin.scheme == "https"
            TransportPolicy.LOOPBACK_TEST_HTTP -> origin.scheme == "http"
        }
        if (!policyMatchesScheme) {
            throw TransportConfigurationException()
        }
        if (profile.transportPolicy == TransportPolicy.LOOPBACK_TEST_HTTP &&
            (!allowLoopbackTestHttp || !isNumericLoopback(origin.host))
        ) {
            throw TransportConfigurationException()
        }
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

        fun buildClient(timeouts: TransportTimeouts): OkHttpClient = OkHttpClient.Builder()
            .followRedirects(false)
            .followSslRedirects(false)
            .retryOnConnectionFailure(false)
            .cookieJar(CookieJar.NO_COOKIES)
            .connectTimeout(timeouts.connectMillis, TimeUnit.MILLISECONDS)
            .readTimeout(timeouts.readMillis, TimeUnit.MILLISECONDS)
            .writeTimeout(timeouts.writeMillis, TimeUnit.MILLISECONDS)
            .callTimeout(timeouts.callMillis, TimeUnit.MILLISECONDS)
            .build()
    }
}
