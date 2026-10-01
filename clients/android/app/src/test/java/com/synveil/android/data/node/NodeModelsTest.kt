package com.synveil.android.data.node

import com.synveil.android.data.library.AuthenticatedLibraryRepository
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.library.NodeId
import com.synveil.android.data.network.NodePageResult
import com.synveil.android.data.network.ProtocolErrorKind
import com.synveil.android.data.network.SynveilTransportError
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class NodeModelsTest {
    private val json = Json { ignoreUnknownKeys = false; explicitNulls = false }

    @Test
    fun parsesStrictNodePage() {
        val result = NodeWireParser.parse(json, validPage().toByteArray(), "request-01")
        val page = (result as NodePageResult.Success).page
        assertEquals(NodeKind.FILE, page.nodes.single().kind)
        assertEquals(NodeState.ACTIVE, page.nodes.single().state)
        assertEquals("18446744073709551616", page.nodes.single().revision.value)
        assertEquals("request-01", page.requestId)
    }

    @Test
    fun rejectsUnknownValuesAndUnexpectedFields() {
        listOf(
            validPage().replace("\"node\"", "\"library\""),
            validPage().replace("\"FILE\"", "\"LINK\""),
            validPage().replace("\"ACTIVE\"", "\"UNKNOWN\""),
            validPage().replace("18446744073709551616", "01"),
            validPage().replace("018bcfe5-687b-7001-8203-040506070811", "not-a-uuid"),
            validPage().replace("\"name\":\"report.txt\"", "\"unexpected\":true,\"name\":\"report.txt\""),
        ).forEach { body ->
            assertEquals(
                NodePageResult.Failure(SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_NODE_RESPONSE)),
                NodeWireParser.parse(json, body.toByteArray(), "request-01"),
            )
        }
    }

    @Test
    fun repositoryBoundsRepeatedCursorsAndScope() {
        val libraryId = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070810"))
        val otherLibrary = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070812"))
        val parent = checkNotNull(NodeId.parse("018bcfe5-687b-7001-8203-040506070811"))
        val repeated = AuthenticatedNodeRepository { _, _, _ ->
            NodePageResult.Success(NodePage(emptyList(), "same", true, "request-01"))
        }.listChildren(libraryId, null)
        assertEquals(NodeRepositoryResult.Failed(NodeFailure.RepeatedCursor), repeated)

        val foreign = AuthenticatedNodeRepository { _, _, _ ->
            NodePageResult.Success(NodePage(listOf(node(otherLibrary, parent)), null, false, "request-01"))
        }.listChildren(libraryId, null)
        assertEquals(NodeRepositoryResult.Failed(NodeFailure.InvalidScope), foreign)
        assertTrue(parent.value.isNotEmpty())
    }

    private fun validPage(): String =
        """{"data":[{"id":"018bcfe5-687b-7001-8203-040506070811","type":"node","revision":"18446744073709551616","attributes":{"library_id":"018bcfe5-687b-7001-8203-040506070810","parent_id":"018bcfe5-687b-7001-8203-040506070812","current_version_id":"018bcfe5-687b-7001-8203-040506070813","name":"report.txt","kind":"FILE","state":"ACTIVE","created_at":"2026-09-30T00:00:00Z","updated_at":"2026-09-30T01:00:00Z","purge_eligible":false}}],"page":{"has_more":false},"meta":{"request_id":"request-01"}}"""

    private fun node(libraryId: LibraryId, parentId: NodeId) = Node(
        nodeId = checkNotNull(NodeId.parse("018bcfe5-687b-7001-8203-040506070811")),
        libraryId = libraryId,
        parentId = parentId,
        currentVersionId = null,
        revision = checkNotNull(NodeRevision.parse("0")),
        name = "report.txt",
        kind = NodeKind.FILE,
        state = NodeState.ACTIVE,
        createdAt = java.time.OffsetDateTime.parse("2026-09-30T00:00:00Z"),
        updatedAt = java.time.OffsetDateTime.parse("2026-09-30T00:00:00Z"),
        trashedAt = null,
        restoreDeadline = null,
        purgeEligible = false,
    )
}
