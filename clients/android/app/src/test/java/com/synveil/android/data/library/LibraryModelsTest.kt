package com.synveil.android.data.library

import com.synveil.android.data.network.LibraryPageResult
import com.synveil.android.data.network.ProtocolErrorKind
import com.synveil.android.data.network.SynveilTransportError
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LibraryModelsTest {
    private val json = Json { ignoreUnknownKeys = false; explicitNulls = false }

    @Test
    fun validatesCanonicalIdsRevisionsTimestampsNamesAndStatuses() {
        val result = LibraryWireParser.parse(json, validPage().toByteArray(), "request-01")
        val page = (result as LibraryPageResult.Success).page
        assertEquals("18446744073709551616", page.libraries.single().revision.value)
        assertEquals(LibraryStatus.ACTIVE, page.libraries.single().status)
        assertEquals("request-01", page.requestId)
    }

    @Test
    fun rejectsMalformedResourceValuesAndUnexpectedFields() {
        listOf(
            validPage().replace("018bcfe5-687b-7001-8203-040506070810", "not-a-uuid"),
            validPage().replace("18446744073709551616", "01"),
            validPage().replace("ACTIVE", "UNKNOWN"),
            validPage().replace("2026-09-30T00:00:00Z", "not-a-time"),
            validPage().replace("Documents", ""),
            validPage().replace("\"updated_at\":\"2026-09-30T01:00:00Z\"", "\"unexpected\":true,\"updated_at\":\"2026-09-30T01:00:00Z\""),
        ).forEach { body ->
            val result = LibraryWireParser.parse(json, body.toByteArray(), "request-01")
            assertEquals(
                LibraryPageResult.Failure(SynveilTransportError.ProtocolError(ProtocolErrorKind.INVALID_LIBRARY_RESPONSE)),
                result,
            )
        }
    }

    @Test
    fun enforcesPaginationCoherenceAndRepositoryBudgets() {
        val missingCursor = LibraryWireParser.parse(
            json,
            validPage(hasMore = true).replace(",\"next_cursor\":\"cursor-1\"", "").toByteArray(),
            "request-01",
        )
        assertTrue(missingCursor is LibraryPageResult.Failure)
        val repeated = AuthenticatedLibraryRepository { cursor ->
            LibraryPageResult.Success(
                LibraryCollectionPage(libraryList(), "same", true, "request-01"),
            )
        }.listLibraries()
        assertEquals(LibraryRepositoryResult.Failed(LibraryFailure.RepeatedCursor), repeated)

        val tooManyPages = AuthenticatedLibraryRepository {
            LibraryPageResult.Success(LibraryCollectionPage(emptyList(), "next-${itCount++}", true, "request-01"))
        }.listLibraries()
        assertEquals(LibraryRepositoryResult.Failed(LibraryFailure.ResourceLimit), tooManyPages)
    }

    private var itCount = 0

    private fun validPage(hasMore: Boolean = false): String =
        """{"data":[{"id":"018bcfe5-687b-7001-8203-040506070810","type":"library","revision":"18446744073709551616","attributes":{"name":"Documents","root_node_id":"018bcfe5-687b-7001-8203-040506070811","status":"ACTIVE","created_at":"2026-09-30T00:00:00Z","updated_at":"2026-09-30T01:00:00Z"}}],"page":{"has_more":$hasMore${if (hasMore) ",\"next_cursor\":\"cursor-1\"" else ""}},"meta":{"request_id":"request-01"}}"""

    private fun libraryList() = listOf(
        Library(
            id = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070810")),
            revision = checkNotNull(LibraryRevision.parse("0")),
            name = "Documents",
            rootNodeId = checkNotNull(NodeId.parse("018bcfe5-687b-7001-8203-040506070811")),
            status = LibraryStatus.ACTIVE,
            createdAt = java.time.OffsetDateTime.parse("2026-09-30T00:00:00Z"),
            updatedAt = java.time.OffsetDateTime.parse("2026-09-30T00:00:00Z"),
        ),
    )
}
