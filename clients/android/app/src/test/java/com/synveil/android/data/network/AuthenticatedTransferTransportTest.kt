package com.synveil.android.data.network

import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.data.enrollment.DeviceCredential
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.library.NodeId
import com.synveil.android.data.transfer.DownloadResult
import com.synveil.android.data.transfer.UploadResult
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthenticatedTransferTransportTest {
    private val servers = mutableListOf<MockWebServer>()

    @After
    fun tearDown() = servers.forEach(MockWebServer::shutdown)

    @Test
    fun nodeDownloadAndUploadRequestsUseOnlyProfileBearer() {
        val server = server()
        val libraryId = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070810"))
        val nodeId = checkNotNull(NodeId.parse("018bcfe5-687b-7001-8203-040506070811"))
        server.enqueue(json(200, nodePageJson(libraryId.value, nodeId.value)))
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .setHeader("Content-Type", "application/octet-stream")
                .setHeader("Content-Length", "5")
                .setHeader("ETag", "\"hash\"")
                .setBody("hello"),
        )
        server.enqueue(json(201, uploadSessionJson(libraryId.value, nodeId.value)))
        server.enqueue(MockResponse().setResponseCode(204).setHeader("Upload-Offset", "5"))
        val transport = transport(server)

        assertTrue(transport.listNodesPage(libraryId, null, null) is NodePageResult.Success)
        val download = transport.openCurrentContent(nodeId)
        assertTrue(download is DownloadResult.Success)
        (download as DownloadResult.Success).response.close()
        assertTrue(transport.createUploadSession("{\"operation\":\"CREATE_FILE\"}") is UploadResult.Session)
        assertTrue(transport.appendUploadChunk("018bcfe5-687b-7001-8203-040506070814", 0, byteArrayOf(1, 2, 3, 4, 5)) is UploadResult.Offset)

        val nodeRequest = server.takeRequest()
        assertEquals("GET", nodeRequest.method)
        assertEquals("/api/v1/libraries/$libraryId/nodes?limit=100", nodeRequest.path)
        assertCommonAuth(nodeRequest)
        val downloadRequest = server.takeRequest()
        assertEquals("/api/v1/nodes/$nodeId/content", downloadRequest.path)
        assertCommonAuth(downloadRequest)
        val createRequest = server.takeRequest()
        assertEquals("POST", createRequest.method)
        assertCommonAuth(createRequest)
        val patchRequest = server.takeRequest()
        assertEquals("PATCH", patchRequest.method)
        assertEquals("application/octet-stream", patchRequest.getHeader("Content-Type"))
        assertEquals("0", patchRequest.getHeader("Upload-Offset"))
        assertCommonAuth(patchRequest)
    }

    private fun assertCommonAuth(request: okhttp3.mockwebserver.RecordedRequest) {
        assertEquals("Bearer svd1_${"b".repeat(64)}", request.getHeader("Authorization"))
        assertNull(request.getHeader("Cookie"))
        assertNull(request.getHeader("X-CSRF-Token"))
    }

    private fun nodePageJson(libraryId: String, nodeId: String): String =
        """{"data":[{"id":"$nodeId","type":"node","revision":"0","attributes":{"library_id":"$libraryId","parent_id":"018bcfe5-687b-7001-8203-040506070812","name":"file.txt","kind":"FILE","state":"ACTIVE","created_at":"2026-09-30T00:00:00Z","updated_at":"2026-09-30T00:00:00Z","purge_eligible":false}}],"page":{"has_more":false},"meta":{"request_id":"request-01"}}"""

    private fun uploadSessionJson(libraryId: String, nodeId: String): String =
        """{"data":{"id":"018bcfe5-687b-7001-8203-040506070814","type":"upload_session","attributes":{"operation":"CREATE_FILE","state":"OPEN","received_bytes":"0","expected_bytes":"5","created_at":"2026-09-30T00:00:00Z","updated_at":"2026-09-30T00:00:00Z","expires_at":"2026-09-30T01:00:00Z","target":{"operation":"CREATE_FILE","library_id":"$libraryId","parent_id":"$nodeId","node_id":"018bcfe5-687b-7001-8203-040506070815","name":"file.txt"}}},"meta":{"request_id":"request-01"}}"""

    private fun transport(server: MockWebServer): AuthenticatedSynveilTransport = AuthenticatedSynveilTransport(
        profile = ServerProfile.create(
            profileId = ServerProfileId.parse("018bcfe5-687b-7001-8203-040506070809"),
            displayLabel = "Test",
            canonicalBaseUrl = CanonicalServerOrigin.parse(
                server.url("/").toString().replace("localhost", "127.0.0.1"),
                allowLoopbackTestHttp = true,
            ),
            createdAt = 1_700_000_000_000L,
        ),
        credential = checkNotNull(DeviceCredential.parse("svd1_" + "b".repeat(64))),
        userAgent = "Synveil Android/test",
        allowLoopbackTestHttp = true,
    )

    private fun server() = MockWebServer().also { it.start(); servers += it }

    private fun json(status: Int, body: String) = MockResponse()
        .setResponseCode(status)
        .setHeader("Content-Type", "application/json")
        .setBody(body)
}
