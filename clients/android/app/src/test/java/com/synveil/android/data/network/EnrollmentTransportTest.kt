package com.synveil.android.data.network

import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.data.enrollment.EnrollmentExchangeResult
import com.synveil.android.data.enrollment.EnrollmentRecoveryReason
import com.synveil.android.data.enrollment.EnrollmentToken
import java.util.concurrent.TimeUnit
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.SocketPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.After
import org.junit.Test

class EnrollmentTransportTest {
    private val servers = mutableListOf<MockWebServer>()

    @After
    fun tearDown() {
        servers.forEach { it.shutdown() }
    }

    @Test
    fun validExchangeIsStrictAndUsesOnePost() {
        val server = server()
        server.enqueue(
            MockResponse()
                .setResponseCode(201)
                .setHeader("Content-Type", "application/json")
                .setHeader("X-Request-Id", "request-1")
                .setBody("""
                    {"data":{"owner_user_id":"018bcfe5-687b-7001-8203-040506070810","device_id":"018bcfe5-687b-7001-8203-040506070811","credential_id":"018bcfe5-687b-7001-8203-040506070812","device_credential":"svd1_${"b".repeat(64)}","created_at":"2026-09-30T00:00:00Z"},"meta":{"request_id":"request-1"}}
                """.trimIndent()),
        )
        val result = transport(server).exchange(checkNotNull(EnrollmentToken.parse("sve1_" + "a".repeat(64))))
        assertTrue(result is EnrollmentExchangeResult.Success)
        val request = server.takeRequest(1, TimeUnit.SECONDS)!!
        assertEquals("POST", request.method)
        assertEquals("/api/v1/device-enrollment/exchange", request.path)
        assertTrue(request.body.readUtf8().contains("enrollment_token"))
        assertEquals(null, server.takeRequest(100, TimeUnit.MILLISECONDS))
    }

    @Test
    fun timeoutAndDisconnectAreRecoveryRequiredWithoutRetry() {
        val server = server()
        server.enqueue(MockResponse().setSocketPolicy(SocketPolicy.NO_RESPONSE))
        val result = transport(server, TransportTimeouts(50, 50, 50, 100)).exchange(token())
        assertTrue(result is EnrollmentExchangeResult.RecoveryRequired)
        assertEquals(EnrollmentRecoveryReason.TIMEOUT, (result as EnrollmentExchangeResult.RecoveryRequired).reason)
        assertTrue(server.takeRequest(1, TimeUnit.SECONDS) != null)
        assertEquals(null, server.takeRequest(100, TimeUnit.MILLISECONDS))
    }

    @Test
    fun malformedCredentialAndWrongContentTypeCannotBecomeSuccess() {
        val server = server()
        server.enqueue(MockResponse().setResponseCode(201).setHeader("Content-Type", "text/html").setBody("ok"))
        val result = transport(server).exchange(token())
        assertTrue(result is EnrollmentExchangeResult.RecoveryRequired)
        assertEquals(EnrollmentRecoveryReason.WRONG_CONTENT_TYPE, (result as EnrollmentExchangeResult.RecoveryRequired).reason)
    }

    @Test
    fun serviceUnavailableIsRecoveryRequiredWithoutAutomaticRetry() {
        val server = server()
        server.enqueue(
            MockResponse()
                .setResponseCode(503)
                .setHeader("Content-Type", "application/json")
                .setBody("{\"error\":{\"code\":\"unavailable\",\"message\":\"busy\",\"request_id\":\"request-1\",\"retryable\":true}}"),
        )
        val result = transport(server).exchange(token())
        assertEquals(
            EnrollmentRecoveryReason.HTTP_503,
            (result as EnrollmentExchangeResult.RecoveryRequired).reason,
        )
        assertTrue(server.takeRequest(1, TimeUnit.SECONDS) != null)
        assertEquals(null, server.takeRequest(100, TimeUnit.MILLISECONDS))
    }

    private fun token() = checkNotNull(EnrollmentToken.parse("sve1_" + "a".repeat(64)))

    private fun server(): MockWebServer = MockWebServer().also {
        it.start()
        servers += it
    }

    private fun transport(server: MockWebServer, timeouts: TransportTimeouts = TransportTimeouts()) =
        SynveilHttpTransport(
            profile = ServerProfile.create(
                profileId = ServerProfileId.parse("018bcfe5-687b-7001-8203-040506070809"),
                displayLabel = "Test",
                canonicalBaseUrl = CanonicalServerOrigin.parse(
                    server.url("/").newBuilder().host("127.0.0.1").build().toString(),
                    true,
                ),
                createdAt = 1_700_000_000_000L,
            ),
            userAgent = "Synveil Android/test",
            timeouts = timeouts,
            allowLoopbackTestHttp = true,
        )
}
