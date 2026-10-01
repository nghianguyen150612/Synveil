package com.synveil.android.core.model

import java.net.Inet6Address
import java.net.InetAddress
import java.net.URI
import java.net.URISyntaxException
import java.net.IDN
import java.nio.ByteBuffer
import java.nio.charset.StandardCharsets
import java.util.Locale
import java.util.UUID
import java.security.SecureRandom

const val MAX_BASE_URL_BYTES = 2_048
const val MAX_PROFILE_LABEL_BYTES = 256
const val MAX_PROFILES = 1_024

enum class TransportPolicy {
    HTTPS,
    LOOPBACK_TEST_HTTP,
}

enum class CanonicalOriginError {
    REQUIRED,
    TOO_LONG,
    CONTROL_OR_WHITESPACE,
    BACKSLASH_NOT_ALLOWED,
    INVALID_URI,
    UNSUPPORTED_SCHEME,
    HTTPS_REQUIRED,
    LOOPBACK_HTTP_NOT_ALLOWED,
    USERINFO_NOT_ALLOWED,
    PATH_NOT_ALLOWED,
    QUERY_NOT_ALLOWED,
    FRAGMENT_NOT_ALLOWED,
    HOST_REQUIRED,
    INVALID_HOST,
    INVALID_PORT,
}

class CanonicalOriginException(
    val reason: CanonicalOriginError,
) : IllegalArgumentException(reason.name)

enum class DisplayLabelError {
    REQUIRED,
    CONTROL_CHARACTER,
    TOO_LONG,
}

class DisplayLabelException(
    val reason: DisplayLabelError,
) : IllegalArgumentException(reason.name)

fun validateDisplayLabel(value: String): String {
    val trimmed = value.trim()
    if (trimmed.isEmpty()) {
        throw DisplayLabelException(DisplayLabelError.REQUIRED)
    }
            if (trimmed.any(Char::isISOControl)) {
        throw DisplayLabelException(DisplayLabelError.CONTROL_CHARACTER)
    }
    if (trimmed.toByteArray(StandardCharsets.UTF_8).size > MAX_PROFILE_LABEL_BYTES) {
        throw DisplayLabelException(DisplayLabelError.TOO_LONG)
    }
    return trimmed
}

data class CanonicalServerOrigin private constructor(
    val value: String,
    val transportPolicy: TransportPolicy,
) {
    companion object {
        fun parse(
            rawValue: String,
            allowLoopbackTestHttp: Boolean,
        ): CanonicalServerOrigin {
            if (rawValue.isEmpty()) {
                throw CanonicalOriginException(CanonicalOriginError.REQUIRED)
            }
            if (rawValue.toByteArray(StandardCharsets.UTF_8).size > MAX_BASE_URL_BYTES) {
                throw CanonicalOriginException(CanonicalOriginError.TOO_LONG)
            }
            if (rawValue.any { it.isISOControl() || it.isWhitespace() }) {
                throw CanonicalOriginException(CanonicalOriginError.CONTROL_OR_WHITESPACE)
            }
            if ('\\' in rawValue) {
                throw CanonicalOriginException(CanonicalOriginError.BACKSLASH_NOT_ALLOWED)
            }

            val uri = try {
                URI(rawValue)
            } catch (_: URISyntaxException) {
                throw CanonicalOriginException(CanonicalOriginError.INVALID_URI)
            }
            val scheme = uri.scheme?.lowercase(Locale.ROOT)
                ?: throw CanonicalOriginException(CanonicalOriginError.UNSUPPORTED_SCHEME)
            val transportPolicy = when (scheme) {
                "https" -> TransportPolicy.HTTPS
                "http" -> TransportPolicy.LOOPBACK_TEST_HTTP
                else -> throw CanonicalOriginException(CanonicalOriginError.UNSUPPORTED_SCHEME)
            }
            if (transportPolicy == TransportPolicy.LOOPBACK_TEST_HTTP && !allowLoopbackTestHttp) {
                throw CanonicalOriginException(CanonicalOriginError.HTTPS_REQUIRED)
            }
            if (uri.rawUserInfo != null) {
                throw CanonicalOriginException(CanonicalOriginError.USERINFO_NOT_ALLOWED)
            }
            if (uri.rawQuery != null) {
                throw CanonicalOriginException(CanonicalOriginError.QUERY_NOT_ALLOWED)
            }
            if (uri.rawFragment != null) {
                throw CanonicalOriginException(CanonicalOriginError.FRAGMENT_NOT_ALLOWED)
            }
            val path = uri.rawPath.orEmpty()
            if (path.isNotEmpty() && path != "/") {
                throw CanonicalOriginException(CanonicalOriginError.PATH_NOT_ALLOWED)
            }

            val authority = uri.rawAuthority
                ?: throw CanonicalOriginException(CanonicalOriginError.HOST_REQUIRED)
            val authorityParts = parseAuthority(authority)
            if (authorityParts.explicitPort && authorityParts.portText.isEmpty()) {
                throw CanonicalOriginException(CanonicalOriginError.INVALID_PORT)
            }
            val port = if (authorityParts.explicitPort) {
                authorityParts.portText.toIntOrNull()?.takeIf { it in 1..65_535 }
                    ?: throw CanonicalOriginException(CanonicalOriginError.INVALID_PORT)
            } else {
                -1
            }
            if (uri.port != -1 && uri.port != port) {
                throw CanonicalOriginException(CanonicalOriginError.INVALID_PORT)
            }

            val canonicalHost = canonicalHost(authorityParts.host)
            val isNumericLoopback = isNumericLoopback(authorityParts.host)
            if (transportPolicy == TransportPolicy.LOOPBACK_TEST_HTTP && !isNumericLoopback) {
                throw CanonicalOriginException(CanonicalOriginError.LOOPBACK_HTTP_NOT_ALLOWED)
            }

            val portSuffix = if (port == -1) "" else ":$port"
            return CanonicalServerOrigin(
                value = "$scheme://$canonicalHost$portSuffix/",
                transportPolicy = transportPolicy,
            )
        }

        internal fun fromPersisted(
            rawValue: String,
            transportPolicy: TransportPolicy,
            allowLoopbackTestHttp: Boolean,
        ): CanonicalServerOrigin {
            val parsed = parse(rawValue, allowLoopbackTestHttp)
            if (parsed.transportPolicy != transportPolicy || parsed.value != rawValue) {
                throw CanonicalOriginException(CanonicalOriginError.INVALID_URI)
            }
            return parsed
        }

        private fun parseAuthority(authority: String): AuthorityParts {
            if (authority.isEmpty() || authority.contains('@')) {
                throw CanonicalOriginException(CanonicalOriginError.USERINFO_NOT_ALLOWED)
            }
            if (authority.startsWith("[")) {
                val closingBracket = authority.indexOf(']')
                if (closingBracket <= 1) {
                    throw CanonicalOriginException(CanonicalOriginError.INVALID_HOST)
                }
                val remainder = authority.substring(closingBracket + 1)
                if (remainder.isNotEmpty() && !remainder.startsWith(":")) {
                    throw CanonicalOriginException(CanonicalOriginError.INVALID_PORT)
                }
                return AuthorityParts(
                    host = authority.substring(0, closingBracket + 1),
                    explicitPort = remainder.isNotEmpty(),
                    portText = remainder.removePrefix(":"),
                )
            }
            val colon = authority.lastIndexOf(':')
            if (colon >= 0) {
                return AuthorityParts(
                    host = authority.substring(0, colon),
                    explicitPort = true,
                    portText = authority.substring(colon + 1),
                )
            }
            return AuthorityParts(host = authority, explicitPort = false, portText = "")
        }

        private fun canonicalHost(host: String): String {
            if (host.isEmpty()) {
                throw CanonicalOriginException(CanonicalOriginError.HOST_REQUIRED)
            }
            if (host.startsWith("[") && host.endsWith("]")) {
                val address = numericAddress(host.substring(1, host.length - 1))
                if (address !is Inet6Address) {
                    throw CanonicalOriginException(CanonicalOriginError.INVALID_HOST)
                }
                return "[${host.substring(1, host.length - 1).lowercase(Locale.ROOT)}]"
            }
            if (host.contains(':')) {
                throw CanonicalOriginException(CanonicalOriginError.INVALID_HOST)
            }
            if (looksLikeIpv4(host)) {
                val address = numericAddress(host)
                if (address !is InetAddress || address.address.size != 4) {
                    throw CanonicalOriginException(CanonicalOriginError.INVALID_HOST)
                }
                return host
            }
            val asciiHost = try {
                IDN.toASCII(host, IDN.USE_STD3_ASCII_RULES)
            } catch (_: IllegalArgumentException) {
                throw CanonicalOriginException(CanonicalOriginError.INVALID_HOST)
            }
            if (asciiHost.isEmpty() || asciiHost.any(Char::isWhitespace)) {
                throw CanonicalOriginException(CanonicalOriginError.INVALID_HOST)
            }
            return asciiHost.lowercase(Locale.ROOT)
        }

        private fun isNumericLoopback(host: String): Boolean {
            return if (host.startsWith("[") && host.endsWith("]")) {
                numericAddress(host.substring(1, host.length - 1))?.isLoopbackAddress == true
            } else {
                host == "127.0.0.1"
            }
        }

        private fun looksLikeIpv4(value: String): Boolean {
            return value.isNotEmpty() && value.all { it.isDigit() || it == '.' }
        }

        private fun numericAddress(value: String): InetAddress? {
            if (value.isEmpty() || value.contains('%')) return null
            if (value.contains(':') && value.any { it.isLetter() && it.lowercaseChar() !in 'a'..'f' }) {
                return null
            }
            if (!value.contains(':') && value.split('.').size != 4) return null
            if (!value.contains(':') && value.split('.').any { it.isEmpty() || it.toIntOrNull() !in 0..255 }) {
                return null
            }
            return try {
                InetAddress.getByName(value)
            } catch (_: Exception) {
                null
            }
        }

        private data class AuthorityParts(
            val host: String,
            val explicitPort: Boolean,
            val portText: String,
        )
    }
}

interface RandomBytes {
    fun nextBytes(size: Int): ByteArray
}

private object SecureRandomBytes : RandomBytes {
    private val random = SecureRandom()

    override fun nextBytes(size: Int): ByteArray = ByteArray(size).also(random::nextBytes)
}

class ServerProfileId private constructor(
    private val uuid: UUID,
) {
    override fun equals(other: Any?): Boolean = other is ServerProfileId && uuid == other.uuid

    override fun hashCode(): Int = uuid.hashCode()

    override fun toString(): String = uuid.toString()

    companion object {
        fun new(
            timestampMillis: () -> Long = { System.currentTimeMillis() },
            randomBytes: RandomBytes = SecureRandomBytes,
        ): ServerProfileId {
            val timestamp = timestampMillis()
            require(timestamp >= 0) { "timestamp must not be negative" }
            val random = randomBytes.nextBytes(10)
            require(random.size == 10) { "UUIDv7 randomness must contain ten bytes" }
            val randomA = ByteBuffer.wrap(byteArrayOf(0, 0, random[0], random[1])).int and 0x0fff
            val mostSignificant = ((timestamp and 0x0000ffffffffffffL) shl 16) or
                0x7000L or randomA.toLong()
            val randomB = ByteBuffer.wrap(random.copyOfRange(2, 10)).long and 0x3fffffffffffffffL
            val leastSignificant = randomB or Long.MIN_VALUE
            return ServerProfileId(UUID(mostSignificant, leastSignificant))
        }

        fun parse(value: String): ServerProfileId {
            val parsed = try {
                UUID.fromString(value)
            } catch (_: IllegalArgumentException) {
                throw IllegalArgumentException("invalid profile id")
            }
            if (parsed.version() != 7 || parsed.variant() != 2 || parsed.toString() != value) {
                throw IllegalArgumentException("invalid profile id")
            }
            return ServerProfileId(parsed)
        }
    }
}

data class ServerProfile private constructor(
    val profileId: ServerProfileId,
    val canonicalBaseUrl: CanonicalServerOrigin,
    val displayLabel: String,
    val createdAt: Long,
    val lastConnectedAt: Long?,
) {
    val transportPolicy: TransportPolicy
        get() = canonicalBaseUrl.transportPolicy

    fun edit(
        displayLabel: String,
        canonicalBaseUrl: CanonicalServerOrigin,
    ): ServerProfile {
        val validatedLabel = validateDisplayLabel(displayLabel)
        return copy(
            displayLabel = validatedLabel,
            canonicalBaseUrl = canonicalBaseUrl,
            lastConnectedAt = if (this.canonicalBaseUrl == canonicalBaseUrl) lastConnectedAt else null,
        )
    }

    fun markConnected(atMillis: Long): ServerProfile {
        require(atMillis >= 0) { "lastConnectedAt must not be negative" }
        return copy(lastConnectedAt = atMillis)
    }

    companion object {
        fun create(
            profileId: ServerProfileId,
            displayLabel: String,
            canonicalBaseUrl: CanonicalServerOrigin,
            createdAt: Long,
        ): ServerProfile {
            require(createdAt >= 0) { "createdAt must not be negative" }
            return ServerProfile(
                profileId = profileId,
                canonicalBaseUrl = canonicalBaseUrl,
                displayLabel = validateDisplayLabel(displayLabel),
                createdAt = createdAt,
                lastConnectedAt = null,
            )
        }

        internal fun fromPersisted(
            profileId: ServerProfileId,
            displayLabel: String,
            canonicalBaseUrl: CanonicalServerOrigin,
            createdAt: Long,
            lastConnectedAt: Long?,
        ): ServerProfile {
            if (createdAt < 0 || lastConnectedAt != null && lastConnectedAt < 0) {
                throw IllegalArgumentException("invalid timestamps")
            }
            return ServerProfile(
                profileId = profileId,
                canonicalBaseUrl = canonicalBaseUrl,
                displayLabel = validateDisplayLabel(displayLabel),
                createdAt = createdAt,
                lastConnectedAt = lastConnectedAt,
            )
        }
    }
}
