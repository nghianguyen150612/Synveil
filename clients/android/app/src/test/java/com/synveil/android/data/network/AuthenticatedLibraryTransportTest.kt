package com.synveil.android.data.network

import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.core.model.TransportPolicy
import com.synveil.android.data.enrollment.DeviceCredential
import com.synveil.android.data.library.LibraryRepositoryResult
import com.synveil.android.data.library.AuthenticatedLibraryRepository
import java.util.concurrent.TimeUnit
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthenticatedLibraryTransportTest {
    private val servers = mutableListOf<MockWebServer>()

    @After
    fun tearDown() {
        servers.forEach { it.shutdown() }
    }

    @Test
    fun sendsOnlyProfileBoundBearerWithoutCookiesOrCsrf() {
        val server = server()
        server.enqueue(json(200, pageJson()))

        val result = transport(server).listLibrariesPage(null)

        assertTrue(result is LibraryPageResult.Success)
        val request = server.takeRequest()
        assertEquals("GET", request.method)
        assertEquals("/api/v1/libraries?limit=100", request.path)
        assertEquals("Bearer svd1_${"b".repeat(64)}", request.getHeader("Authorization"))
        assertEquals(1, request.headers.values("Authorization").size)
        assertNull(request.getHeader("Cookie"))
        assertNull(request.getHeader("X-CSRF-Token"))
        assertNull(request.getHeader("X-Enrollment-Token"))
    }

    @Test
    fun redirectNeverSendsBearerToAnotherOrigin() {
        val target = server()
        val origin = server()
        origin.enqueue(
            MockResponse()
                .setResponseCode(302)
                .setHeader("Location", target.url("/stolen")),
        )

        val result = transport(origin).listLibrariesPage(null)

        assertEquals(
            LibraryPageResult.Failure(SynveilTransportError.RedirectRejected(302)),
            result,
        )
        assertEquals(1, origin.requestCount)
        assertNull(target.takeRequest(200, TimeUnit.MILLISECONDS))
    }

    @Test
    fun parsesStrictLibraryPageAndPaginationQuery() {
        val server = server()
        server.enqueue(json(200, pageJson(nextCursor = "opaque-1", hasMore = true)))
        server.enqueue(json(200, pageJson(hasMore = false)))

        val result = AuthenticatedLibraryRepository(transport(server)::listLibrariesPage).listLibraries()

        assertTrue(result is LibraryRepositoryResult.Loaded)
        assertEquals(2, (result as LibraryRepositoryResult.Loaded).libraries.size)
        assertEquals("/api/v1/libraries?limit=100", server.takeRequest().path)
        assertEquals("/api/v1/libraries?limit=100&cursor=opaque-1", server.takeRequest().path)
    }

    @Test
    fun mapsAuthenticationAndRevocationErrorsWithoutRetry() {
        val server = server()
        server.enqueue(error(401, "authentication_failed"))
        server.enqueue(error(401, "device_revoked"))
        server.enqueue(error(503, "service_unavailable"))

        assertEquals(401, errorCode(transport(server).listLibrariesPage(null), "authentication_failed"))
        assertEquals(401, errorCode(transport(server).listLibrariesPage(null), "device_revoked"))
        assertEquals(503, errorCode(transport(server).listLibrariesPage(null), "service_unavailable"))
        assertEquals(3, server.requestCount)
    }

    @Test
    fun rejectsUnexpectedSchemaContentTypeAndOversizedBody() {
        val server = server()
        server.enqueue(json(200, pageJson().replace("\"library\"", "\"node\"")))
        server.enqueue(MockResponse().setResponseCode(200).setHeader("Content-Type", "text/html").setBody(pageJson()))
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .setHeader("Content-Type", "application/json")
                .setBody("{" + "\"data\":[] ,\"page\":{\"has_more\":false},\"meta\":{\"request_id\":\"request-01\"},\"padding\":\"${"x".repeat(MAX_LIBRARY_RESPONSE_BYTES)}\"}"),
        )

        assertTrue(transport(server).listLibrariesPage(null) is LibraryPageResult.Failure)
        assertEquals(
            LibraryPageResult.Failure(SynveilTransportError.UnexpectedContentType("text/html")),
            transport(server).listLibrariesPage(null),
        )
        assertEquals(
            LibraryPageResult.Failure(SynveilTransportError.BodyLimitExceeded),
            transport(server).listLibrariesPage(null),
        )
    }

    @Test
    fun rejectsInvalidCursorBeforeNetwork() {
        val server = server()
        val result = transport(server).listLibrariesPage("x".repeat(513))
        assertTrue(result is LibraryPageResult.Failure)
        assertEquals(0, server.requestCount)
    }

    private fun error(status: Int, code: String) = json(
        status,
        "{\"error\":{\"code\":\"$code\",\"message\":\"message\",\"request_id\":\"request-01\",\"retryable\":false}}",
    )

    private fun errorCode(result: LibraryPageResult, expected: String): Int {
        val error = (result as LibraryPageResult.Failure).error as SynveilTransportError.HttpError
        assertEquals(expected, error.code)
        return error.statusCode
    }

    private fun pageJson(nextCursor: String? = null, hasMore: Boolean = false): String =
        """{"data":[{"id":"018bcfe5-687b-7001-8203-040506070810","type":"library","revision":"18446744073709551616","attributes":{"name":"Documents","root_node_id":"018bcfe5-687b-7001-8203-040506070811","status":"ACTIVE","created_at":"2026-09-30T00:00:00Z","updated_at":"2026-09-30T01:00:00+01:00"}}],"page":{"has_more":$hasMore${if (nextCursor == null) "" else ",\"next_cursor\":\"$nextCursor\""}},"meta":{"request_id":"request-01"}}"""

    private fun transport(server: MockWebServer): AuthenticatedSynveilTransport =
        AuthenticatedSynveilTransport(
            profile = profile(server),
            credential = checkNotNull(DeviceCredential.parse("svd1_" + "b".repeat(64))),
            userAgent = "Synveil Android/test",
            allowLoopbackTestHttp = true,
        )

    private fun profile(server: MockWebServer): ServerProfile = ServerProfile.create(
        profileId = ServerProfileId.parse("018bcfe5-687b-7001-8203-040506070809"),
        displayLabel = "Test",
        canonicalBaseUrl = CanonicalServerOrigin.parse(
            server.url("/").toString().replace("localhost", "127.0.0.1"),
            allowLoopbackTestHttp = true,
        ),
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
}
