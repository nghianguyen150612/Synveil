package com.synveil.android.data.sync

import com.synveil.android.data.network.ProtocolErrorKind
import com.synveil.android.data.network.SynveilTransportError
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SyncModelsTest {
    private val ids = listOf(
        "018bcfe5-687b-7001-8203-040506070810",
        "018bcfe5-687b-7001-8203-040506070811",
    )

    @Test
    fun parsesStrictFeedAndRejectsOutOfOrderEvents() {
        val body = """{"data":{"device_id":"${ids[0]}","library_id":"${ids[1]}","epoch":"2","from_sequence":"0","through_sequence":"2","high_watermark":"2","has_more":false,"changes":[{"event_id":"018bcfe5-687b-7001-8203-040506070812","schema_version":1,"sequence":"1","resource_kind":"NODE","resource_id":"018bcfe5-687b-7001-8203-040506070813","change_kind":"NODE_CREATED","resource_revision":"1","occurred_at":"2026-10-01T00:00:00Z"},{"event_id":"018bcfe5-687b-7001-8203-040506070814","schema_version":1,"sequence":"2","resource_kind":"NODE","resource_id":"018bcfe5-687b-7001-8203-040506070813","change_kind":"NODE_RENAMED","resource_revision":"2","occurred_at":"2026-10-01T00:00:01Z"}],"ack_token":"ack-token"},"meta":{"request_id":"request-01"}}"""
        val result = SyncWireParser.parseFeed(body.toByteArray(), ids[0], ids[1])
        assertTrue("unexpected parser result: $result", result is SyncResult.Success)
        assertEquals("2", (result as SyncResult.Success).value.throughSequence.value)

        val invalid = SyncWireParser.parseFeed(body.replace("\"sequence\":\"2\"", "\"sequence\":\"1\"").toByteArray(), ids[0], ids[1])
        assertEquals(ProtocolErrorKind.INVALID_SYNC_RESPONSE, ((invalid as SyncResult.Failure).error as SynveilTransportError.ProtocolError).kind)
    }

    @Test
    fun parsesRebaselineTerminalPageOnlyWithCompletionToken() {
        val body = """{"data":{"bootstrap":{"bootstrap_id":"${ids[0]}","device_id":"${ids[0]}","library_id":"${ids[1]}","state":"OPEN","generation":"1","snapshot_epoch":"2","snapshot_resume_sequence":"4","manifest_item_count":"0","created_at":"2026-10-01T00:00:00Z","expires_at":"2026-10-01T01:00:00Z"},"nodes":[],"has_more":false,"completion_token":"completion"},"meta":{"request_id":"request-01"}}"""
        val result = SyncWireParser.parsePage(body.toByteArray(), ids[0], ids[1], ids[0])
        assertTrue(result is SyncResult.Success)
    }

    @Test
    fun rejectsUnknownFeedFieldsAndNonCanonicalSequence() {
        val body = """{"data":{"device_id":"${ids[0]}","library_id":"${ids[1]}","epoch":"2","from_sequence":"00","through_sequence":"00","high_watermark":"0","has_more":false,"changes":[],"ack_token":null},"meta":{"request_id":"request-01"}}"""
        val result = SyncWireParser.parseFeed(body.toByteArray(), ids[0], ids[1])
        assertEquals(ProtocolErrorKind.INVALID_SYNC_RESPONSE, ((result as SyncResult.Failure).error as SynveilTransportError.ProtocolError).kind)

        val withUnknown = body.replace("\"ack_token\":null", "\"ack_token\":null,\"unexpected\":true")
        val unknownResult = SyncWireParser.parseFeed(withUnknown.toByteArray(), ids[0], ids[1])
        assertEquals(ProtocolErrorKind.INVALID_SYNC_RESPONSE, ((unknownResult as SyncResult.Failure).error as SynveilTransportError.ProtocolError).kind)
    }

    @Test
    fun leavesManifestCountValidationToTheEngineAndRejectsUnknownState() {
        val body = """{"data":{"bootstrap":{"bootstrap_id":"${ids[0]}","device_id":"${ids[0]}","library_id":"${ids[1]}","state":"OPEN","generation":"1","snapshot_epoch":"2","snapshot_resume_sequence":"4","manifest_item_count":"1","created_at":"2026-10-01T00:00:00Z","expires_at":"2026-10-01T01:00:00Z"},"nodes":[],"has_more":false,"completion_token":"completion"},"meta":{"request_id":"request-01"}}"""
        val result = SyncWireParser.parsePage(body.toByteArray(), ids[0], ids[1], ids[0])
        assertTrue(result is SyncResult.Success)
        val unknownState = body.replace("\"state\":\"OPEN\"", "\"state\":\"MYSTERY\"")
        val unknownResult = SyncWireParser.parsePage(unknownState.toByteArray(), ids[0], ids[1], ids[0])
        assertEquals(ProtocolErrorKind.INVALID_REBASELINE_RESPONSE, ((unknownResult as SyncResult.Failure).error as SynveilTransportError.ProtocolError).kind)
    }
}
