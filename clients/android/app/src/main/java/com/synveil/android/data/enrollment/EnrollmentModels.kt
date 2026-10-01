package com.synveil.android.data.enrollment

import com.synveil.android.core.model.ServerProfile
import java.time.OffsetDateTime
import java.time.format.DateTimeFormatter

private val ENROLLMENT_TOKEN_PATTERN = Regex("^sve1_[0-9a-f]{64}$")
private val DEVICE_CREDENTIAL_PATTERN = Regex("^svd1_[0-9a-f]{64}$")
private val OPAQUE_ID_PATTERN = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")

class EnrollmentToken private constructor(internal val rawValue: String) {
    override fun toString(): String = "[REDACTED_ENROLLMENT_TOKEN]"

    companion object {
        fun parse(rawValue: String): EnrollmentToken? =
            rawValue.takeIf { ENROLLMENT_TOKEN_PATTERN.matches(it) }?.let(::EnrollmentToken)

        fun isValid(rawValue: String): Boolean = ENROLLMENT_TOKEN_PATTERN.matches(rawValue)
    }
}

class DeviceCredential private constructor(internal val rawValue: String) {
    override fun toString(): String = "[REDACTED_DEVICE_CREDENTIAL]"

    companion object {
        fun parse(rawValue: String): DeviceCredential? =
            rawValue.takeIf { DEVICE_CREDENTIAL_PATTERN.matches(it) }?.let(::DeviceCredential)

        fun isValid(rawValue: String): Boolean = DEVICE_CREDENTIAL_PATTERN.matches(rawValue)
    }
}

data class DeviceCredentialRecord(
    val ownerUserId: String,
    val deviceId: String,
    val credentialId: String,
    val credential: DeviceCredential,
    val createdAt: String,
    val requestId: String?,
) {
    init {
        require(OPAQUE_ID_PATTERN.matches(ownerUserId))
        require(OPAQUE_ID_PATTERN.matches(deviceId))
        require(OPAQUE_ID_PATTERN.matches(credentialId))
        require(runCatching { OffsetDateTime.parse(createdAt, DateTimeFormatter.ISO_OFFSET_DATE_TIME) }.isSuccess)
    }
}

data class CredentialScope(
    val profileId: String,
    val canonicalBaseUrl: String,
    val transportPolicy: String,
    val ownerUserId: String,
    val deviceId: String,
    val credentialId: String,
) {
    fun matches(record: DeviceCredentialRecord): Boolean =
        record.ownerUserId == ownerUserId &&
            record.deviceId == deviceId &&
            record.credentialId == credentialId

    fun aad(deviceCredentialDigest: String? = null): ByteArray = listOf(
        "synveil-device-credential-v1",
        profileId,
        canonicalBaseUrl,
        transportPolicy,
        ownerUserId,
        deviceId,
        credentialId,
        deviceCredentialDigest.orEmpty(),
    ).joinToString("\u001f").toByteArray(Charsets.UTF_8)

    companion object {
        fun forProfile(profile: ServerProfile, record: DeviceCredentialRecord): CredentialScope =
            CredentialScope(
                profileId = profile.profileId.toString(),
                canonicalBaseUrl = profile.canonicalBaseUrl.value,
                transportPolicy = profile.transportPolicy.name,
                ownerUserId = record.ownerUserId,
                deviceId = record.deviceId,
                credentialId = record.credentialId,
            )
    }
}

sealed interface EnrollmentExchangeResult {
    data class Success(val record: DeviceCredentialRecord) : EnrollmentExchangeResult
    data class Rejected(val code: String?, val requestId: String?) : EnrollmentExchangeResult
    data class RecoveryRequired(val reason: EnrollmentRecoveryReason) : EnrollmentExchangeResult
    data class Failed(val reason: EnrollmentFailureReason) : EnrollmentExchangeResult
}

enum class EnrollmentRecoveryReason {
    TIMEOUT,
    DISCONNECT,
    RESPONSE_LOSS,
    HTTP_503,
    REDIRECT,
    MALFORMED_RESPONSE,
    OVERSIZED_RESPONSE,
    WRONG_CONTENT_TYPE,
}

enum class EnrollmentFailureReason {
    INVALID_CONFIGURATION,
    CANCELED,
    TRANSPORT,
}

enum class EnrollmentStateKind {
    NOT_ENROLLED,
    INVALID_TOKEN,
    PENDING,
    ENROLLED,
    RECOVERY_REQUIRED,
    SECURE_STORE_UNAVAILABLE,
}

data class EnrollmentState(
    val kind: EnrollmentStateKind,
    val message: String? = null,
)
