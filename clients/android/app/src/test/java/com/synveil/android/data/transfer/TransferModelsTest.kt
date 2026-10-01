package com.synveil.android.data.transfer

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class TransferModelsTest {
    private val json = Json { ignoreUnknownKeys = false; explicitNulls = false }

    @Test
    fun parsesUploadSessionAndValidatesAuthoritativeOffset() {
        val session = UploadWireParser.parse(json, sessionJson().toByteArray(), "5")
        assertEquals(5L, session.receivedBytes)
        assertEquals(10L, session.expectedBytes)
        assertEquals(UploadState.OPEN, session.state)
        assertEquals("018bcfe5-687b-7001-8203-040506070810", session.target.libraryId)
        assertThrows(RuntimeException::class.java) {
            UploadWireParser.parse(json, sessionJson().toByteArray(), "4")
        }
    }

    @Test
    fun rejectsUnknownStateAndNonCanonicalDecimal() {
        assertThrows(RuntimeException::class.java) {
            UploadWireParser.parse(json, sessionJson().replace("\"OPEN\"", "\"UNKNOWN\"").toByteArray(), "5")
        }
        assertThrows(IllegalArgumentException::class.java) {
            UploadWireParser.parse(json, sessionJson().replace("\"received_bytes\":\"5\"", "\"received_bytes\":\"05\"").toByteArray(), "5")
        }
    }

    @Test
    fun parsesCanonicalCompletion() {
        val completion = UploadWireParser.parseCompletion(json, completionJson().toByteArray())
        assertEquals(10L, completion.bytes)
        assertEquals("a".repeat(64), completion.sha256)
    }

    private fun sessionJson(): String =
        """{"data":{"id":"018bcfe5-687b-7001-8203-040506070814","type":"upload_session","attributes":{"operation":"CREATE_FILE","state":"OPEN","received_bytes":"5","expected_bytes":"10","created_at":"2026-09-30T00:00:00Z","updated_at":"2026-09-30T00:00:00Z","expires_at":"2026-09-30T01:00:00Z","target":{"operation":"CREATE_FILE","library_id":"018bcfe5-687b-7001-8203-040506070810","parent_id":"018bcfe5-687b-7001-8203-040506070811","node_id":"018bcfe5-687b-7001-8203-040506070812","name":"report.txt"}}},"meta":{"request_id":"request-01"}}"""

    private fun completionJson(): String =
        """{"data":{"id":"018bcfe5-687b-7001-8203-040506070814","type":"upload_completion","attributes":{"node_id":"018bcfe5-687b-7001-8203-040506070812","file_version_id":"018bcfe5-687b-7001-8203-040506070813","node_revision":"2","bytes":"10","sha256":"${"a".repeat(64)}","committed_at":"2026-09-30T01:00:00Z"}},"meta":{"request_id":"request-01"}}"""
}
