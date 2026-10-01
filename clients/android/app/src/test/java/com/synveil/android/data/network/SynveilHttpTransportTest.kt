package com.synveil.android.data.network

import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.core.model.TransportPolicy
import java.util.concurrent.TimeUnit
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.SocketPolicy
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SynveilHttpTransportTest {
    private val servers = mutableListOf<MockWebServer>()

    @After
    fun tearDown() {
        servers.forEach { it.shutdown() }
    }

    @Test
    fun livenessAndReadinessUseCanonicalPathsAndStrictResponses() {
        val server = server()
        server.enqueue(json(200, "{\"status\":\"live\"}").setHeader("X-Request-Id", "live-req"))
        server.enqueue(json(200, "{\"status\":\"ready\"}").setHeader("X-Request-Id", "ready-req"))

        assertEquals(
            ConnectionCheckResult.Ready("live-req", "ready-req"),
            transport(server).checkServer(),
        )
        assertEquals("/health/live", server.takeRequest().path)
        assertEquals("/health/ready", server.takeRequest().path)
    }

    @Test
    fun readiness503MapsToAliveButNotReady() {
        val server = server()
        server.enqueue(json(200, "{\"status\":\"live\"}"))
        server.enqueue(
            json(
                503,
                "{\"error\":{\"code\":\"internal_dependency_unavailable\",\"message\":\"not ready\",\"request_id\":\"ready-req\",\"retryable\":false}}",
            ),
        )

        assertEquals(
            ConnectionCheckResult.AliveButNotReady("ready-req", "internal_dependency_unavailable"),
            transport(server).checkServer(),
        )
    }

    @Test
    fun redirectsAreRejectedWithoutCrossOriginLeakage() {
        val target = server()
        val origin = server()
        origin.enqueue(
            MockResponse()
                .setResponseCode(302)
                .setHeader("Location", target.url("/stolen")),
        )

        assertEquals(
            ProbeResult.Failure(SynveilTransportError.RedirectRejected(302)),
            transport(origin).checkLiveness(),
        )
        assertNull(target.takeRequest(200, TimeUnit.MILLISECONDS))
    }

    @Test
    fun strictJsonAndContentTypeValidationRejectsInvalidResponses() {
        val server = server()
        server.enqueue(json(200, "{}"))
        server.enqueue(json(200, "{\"status\":\"wrong\"}"))
        server.enqueue(json(200, "{\"status\":\"live\",\"unexpected\":true}"))
        server.enqueue(MockResponse().setResponseCode(200).setHeader("Content-Type", "text/html").setBody("{\"status\":\"live\"}"))

        repeat(4) {
            val result = transport(server).checkLiveness()
            assertTrue(result is ProbeResult.Failure)
            assertTrue(
                (result as ProbeResult.Failure).error is SynveilTransportError.ProtocolError ||
                    result.error is SynveilTransportError.UnexpectedContentType,
            )
        }
    }

    @Test
    fun oversizedBodyFailsBeforeDecode() {
        val server = server()
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .setHeader("Content-Type", "application/json")
                .setBody("{\"status\":\"live\",\"padding\":\"" + "x".repeat(MAX_PROBE_RESPONSE_BYTES) + "\"}"),
        )

        assertEquals(
            ProbeResult.Failure(SynveilTransportError.BodyLimitExceeded),
            transport(server).checkLiveness(),
        )
    }

    @Test
    fun safeHeadersAreSentWithoutAuthenticationOrCookies() {
        val server = server()
        server.enqueue(json(200, "{\"status\":\"live\"}"))

        assertEquals(ProbeResult.Success(null), transport(server).checkLiveness())
        val request = server.takeRequest()
        assertEquals("application/json", request.getHeader("Accept"))
        assertEquals("identity", request.getHeader("Accept-Encoding"))
        assertEquals("Synveil Android/test", request.getHeader("User-Agent"))
        assertNull(request.getHeader("Authorization"))
        assertNull(request.getHeader("Cookie"))
        assertNull(request.getHeader("X-CSRF-Token"))
    }

    @Test
    fun invalidRequestIdsAreIgnoredAndValidIdsAreBounded() {
        val server = server()
        server.enqueue(json(200, "{\"status\":\"live\"}").setHeader("X-Request-Id", "short"))
        server.enqueue(json(200, "{\"status\":\"live\"}").setHeader("X-Request-Id", "valid.req-1"))

        assertEquals(ProbeResult.Success(null), transport(server).checkLiveness())
        assertEquals(ProbeResult.Success("valid.req-1"), transport(server).checkLiveness())
    }

    @Test
    fun loopbackHttpRequiresExplicitDebugPolicyAndNonLoopbackIsRejected() {
        val loopback = profile("http://127.0.0.1:8080/", TransportPolicy.LOOPBACK_TEST_HTTP)

        SynveilHttpTransport(loopback, "test", allowLoopbackTestHttp = true)
        assertThrowsConfiguration { SynveilHttpTransport(loopback, "test", allowLoopbackTestHttp = false) }
    }

    @Test
    fun timeoutMapsToTypedTimeout() {
        val server = server()
        server.enqueue(json(200, "{\"status\":\"live\"}").setBodyDelay(250, TimeUnit.MILLISECONDS))

        val result = transport(
            server,
            TransportTimeouts(connectMillis = 1_000, readMillis = 25, writeMillis = 1_000, callMillis = 100),
        ).checkLiveness()

        assertEquals(ProbeResult.Failure(SynveilTransportError.Timeout), result)
    }

    @Test
    fun disconnectedServerMapsToOffline() {
        val server = server()
        server.enqueue(MockResponse().setSocketPolicy(SocketPolicy.DISCONNECT_AT_START))
        server.enqueue(json(200, "{\"status\":\"live\"}"))

        assertEquals(
            ProbeResult.Failure(SynveilTransportError.Offline),
            transport(server).checkLiveness(),
        )
        assertEquals(1, server.requestCount)
    }

    @Test
    fun failedLivenessDoesNotProbeReadiness() {
        val server = server()
        server.enqueue(MockResponse().setSocketPolicy(SocketPolicy.DISCONNECT_AT_START))

        assertEquals(
            ConnectionCheckResult.Failure(SynveilTransportError.Offline),
            transport(server).checkServer(),
        )
        assertEquals(1, server.requestCount)
    }

    private fun transport(server: MockWebServer, timeouts: TransportTimeouts = TransportTimeouts()): SynveilHttpTransport =
        SynveilHttpTransport(
            profile = profile(server.url("/").toString().replace("localhost", "127.0.0.1"), TransportPolicy.LOOPBACK_TEST_HTTP),
            userAgent = "Synveil Android/test",
            timeouts = timeouts,
            allowLoopbackTestHttp = true,
        )

    private fun profile(url: String, policy: TransportPolicy): ServerProfile = ServerProfile.create(
        profileId = ServerProfileId.new(timestampMillis = { 1_700_000_000_000L }),
        displayLabel = "Test",
        canonicalBaseUrl = CanonicalServerOrigin.parse(url, allowLoopbackTestHttp = policy == TransportPolicy.LOOPBACK_TEST_HTTP),
        createdAt = 1_700_000_000_000L,
    )

    private fun server(): MockWebServer = MockWebServer().also {
        it.start()
        servers += it
    }

    private fun json(code: Int, body: String): MockResponse = MockResponse()
        .setResponseCode(code)
        .setHeader("Content-Type", "application/json")
        .setBody(body)

    private fun assertThrowsConfiguration(block: () -> Unit) {
        try {
            block()
            throw AssertionError("expected configuration failure")
        } catch (_: TransportConfigurationException) {
        }
    }
}
