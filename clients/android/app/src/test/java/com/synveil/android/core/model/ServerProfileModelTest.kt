package com.synveil.android.core.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.charset.StandardCharsets

class ServerProfileModelTest {
    @Test
    fun productionHttpsOriginsAreCanonicalized() {
        val values = listOf(
            "https://example.com" to "https://example.com/",
            "https://example.com/" to "https://example.com/",
            "https://example.com:8443" to "https://example.com:8443/",
            "https://127.0.0.1" to "https://127.0.0.1/",
            "https://127.0.0.1:443" to "https://127.0.0.1:443/",
            "https://[2001:db8::1]" to "https://[2001:db8::1]/",
            "https://[2001:db8::1]:8443" to "https://[2001:db8::1]:8443/",
        )

        values.forEach { (raw, expected) ->
            val origin = CanonicalServerOrigin.parse(raw, allowLoopbackTestHttp = false)

            assertEquals(expected, origin.value)
            assertEquals(TransportPolicy.HTTPS, origin.transportPolicy)
            assertEquals(origin, CanonicalServerOrigin.parse(origin.value, false))
        }
    }

    @Test
    fun invalidProductionOriginsAreRejected() {
        listOf(
            "",
            " ",
            "example.com",
            "//example.com",
            "http://example.com",
            "ftp://example.com",
            "file:///tmp/server",
            "https://user@example.com",
            "https://user:password@example.com",
            "https://example.com/path",
            "https://example.com/path/",
            "https://example.com?q=1",
            "https://example.com/#fragment",
            "https://example.com:0",
            "https://example.com:",
            "https://example.com:65536",
            "https://example.com:abc",
            "https://example.com\\",
            "https://example.com bad",
            "https:// example.com",
        ).forEach { value ->
            assertThrows(CanonicalOriginException::class.java) {
                CanonicalServerOrigin.parse(value, allowLoopbackTestHttp = false)
            }
        }
    }

    @Test
    fun loopbackHttpRequiresExplicitDevelopmentPolicy() {
        listOf(
            "http://127.0.0.1" to "http://127.0.0.1/",
            "http://127.0.0.1:8080" to "http://127.0.0.1:8080/",
            "http://[::1]" to "http://[::1]/",
        ).forEach { (raw, expected) ->
            val origin = CanonicalServerOrigin.parse(raw, allowLoopbackTestHttp = true)
            assertEquals(expected, origin.value)
            assertEquals(TransportPolicy.LOOPBACK_TEST_HTTP, origin.transportPolicy)
        }

        listOf(
            "http://localhost",
            "http://synveil.local",
            "http://192.168.1.10",
            "http://10.0.0.1",
            "http://172.16.0.5",
        ).forEach { value ->
            assertThrows(CanonicalOriginException::class.java) {
                CanonicalServerOrigin.parse(value, allowLoopbackTestHttp = true)
            }
        }

        assertThrows(CanonicalOriginException::class.java) {
            CanonicalServerOrigin.parse("http://127.0.0.1", allowLoopbackTestHttp = false)
        }
    }

    @Test
    fun labelsUseTrimmedUtf8ByteLimitsAndRejectControls() {
        assertEquals("Synveil", validateDisplayLabel("  Synveil  "))
        assertThrows(DisplayLabelException::class.java) { validateDisplayLabel("") }
        assertThrows(DisplayLabelException::class.java) { validateDisplayLabel(" \t") }
        assertThrows(DisplayLabelException::class.java) { validateDisplayLabel("name\nserver") }
        assertEquals("é".repeat(128), validateDisplayLabel("é".repeat(128)))
        assertThrows(DisplayLabelException::class.java) { validateDisplayLabel("é".repeat(129)) }
        assertTrue("é".repeat(128).toByteArray(StandardCharsets.UTF_8).size <= MAX_PROFILE_LABEL_BYTES)
    }

    @Test
    fun uuidV7UsesInjectableDeterministicInputs() {
        val profileId = ServerProfileId.new(
            timestampMillis = { 1_700_000_000_123L },
            randomBytes = object : RandomBytes {
                override fun nextBytes(size: Int): ByteArray = ByteArray(size) { it.toByte() }
            },
        )

        assertEquals("018bcfe5-687b-7001-8203-040506070809", profileId.toString())
        assertEquals(profileId, ServerProfileId.parse(profileId.toString()))
        assertThrows(IllegalArgumentException::class.java) {
            ServerProfileId.parse(profileId.toString().uppercase())
        }
        assertThrows(IllegalArgumentException::class.java) {
            ServerProfileId.parse("018bcfe5-687b-4001-8203-040506070809")
        }
    }
}
