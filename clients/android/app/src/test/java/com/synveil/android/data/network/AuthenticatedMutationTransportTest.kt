package com.synveil.android.data.network

import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.data.enrollment.DeviceCredential
import com.synveil.android.data.mutation.MutationKind
import com.synveil.android.data.mutation.MutationRequest
import com.synveil.android.data.mutation.MutationResult
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.TimeUnit

class AuthenticatedMutationTransportTest {
    private val servers = mutableListOf<MockWebServer>()

    @After fun tearDown() = servers.forEach(MockWebServer::shutdown)

    @Test
    fun sendsImmutableTypedMutationWithoutCookieOrCsrf() {
        val server = MockWebServer().also { it.start(); servers += it }
        val device = "018bcfe5-687b-7001-8203-040506070811"
        val library = "018bcfe5-687b-7001-8203-040506070810"
        val mutation = "018bcfe5-687b-7001-8203-040506070812"
        server.enqueue(MockResponse().setResponseCode(200).setHeader("Content-Type", "application/json").setBody(
            """{"data":{"outcome":"APPLIED","mutation_id":"$mutation","kind":"RENAME_NODE","replayed":false,"node":{"id":"018bcfe5-687b-7001-8203-040506070813","library_id":"$library","kind":"FILE","state":"ACTIVE","name":"next","revision":"4","created_at":"2026-09-30T00:00:00Z","updated_at":"2026-09-30T00:00:00Z"},"journal_event_id":"event-1","journal_sequence":"4"},"meta":{"request_id":"request-01"}}"""
        ))
        val request = MutationRequest(mutation, "2", "3", MutationKind.RENAME_NODE, buildJsonObject {
            put("node_id", "018bcfe5-687b-7001-8203-040506070813")
            put("expected_revision", "3")
            put("new_name", "next")
        })
        val result = transport(server).submitMutation(device, library, request)
        assertTrue(result is MutationResult.Applied)
        val sent = server.takeRequest()
        assertEquals("POST", sent.method)
        assertEquals("/api/v1/devices/$device/libraries/$library/mutations", sent.path)
        assertEquals("Bearer svd1_${"b".repeat(64)}", sent.getHeader("Authorization"))
        assertNull(sent.getHeader("Cookie"))
        assertNull(sent.getHeader("X-CSRF-Token"))
        assertTrue(sent.body.readUtf8().contains("RENAME_NODE"))
    }

    @Test
    fun redirectDoesNotLeakBearer() {
        val target = MockWebServer().also { it.start(); servers += it }
        val origin = MockWebServer().also { it.start(); servers += it }
        origin.enqueue(MockResponse().setResponseCode(302).setHeader("Location", target.url("/stolen")))
        val request = MutationRequest("018bcfe5-687b-7001-8203-040506070812", "2", "3", MutationKind.TRASH_NODE, buildJsonObject {
            put("node_id", "018bcfe5-687b-7001-8203-040506070813")
            put("expected_revision", "3")
        })
        val result = transport(origin).submitMutation("018bcfe5-687b-7001-8203-040506070811", "018bcfe5-687b-7001-8203-040506070810", request)
        assertEquals(MutationResult.Failure(SynveilTransportError.RedirectRejected(302)), result)
        assertNull(target.takeRequest(200, TimeUnit.MILLISECONDS))
    }

    private fun transport(server: MockWebServer) = AuthenticatedSynveilTransport(
        profile = ServerProfile.create(ServerProfileId.parse("018bcfe5-687b-7001-8203-040506070809"), "Test", CanonicalServerOrigin.parse(server.url("/").toString().replace("localhost", "127.0.0.1"), true), 1_700_000_000_000L),
        credential = checkNotNull(DeviceCredential.parse("svd1_" + "b".repeat(64))),
        userAgent = "Synveil Android/test",
        allowLoopbackTestHttp = true,
    )
}
