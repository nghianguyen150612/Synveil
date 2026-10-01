package com.synveil.android.data.enrollment

import com.synveil.android.core.model.ServerProfile

interface EnrollmentExchangeClient {
    fun exchange(token: EnrollmentToken): EnrollmentExchangeResult
}

sealed interface CredentialCleanupResult {
    data object Success : CredentialCleanupResult
    data class Failed(val reason: String) : CredentialCleanupResult
}

interface CredentialLifecycle {
    suspend fun fence(profileId: String): CredentialCleanupResult
}

class EnrollmentManager(
    private val metadataStore: EnrollmentMetadataStore,
    private val vault: SecureCredentialVault,
    private val onFenced: suspend (String) -> Unit = {},
) : CredentialLifecycle {
    suspend fun enroll(
        profile: ServerProfile,
        rawToken: String,
        exchange: EnrollmentExchangeClient,
    ): EnrollmentState {
        val token = EnrollmentToken.parse(rawToken)
            ?: return EnrollmentState(EnrollmentStateKind.INVALID_TOKEN, "Enrollment token format is invalid.")
        return try {
            vault.preflight(profile.profileId.toString())
            when (val result = exchange.exchange(token)) {
                is EnrollmentExchangeResult.Success -> finalize(profile, result.record)
                is EnrollmentExchangeResult.Rejected -> EnrollmentState(
                    EnrollmentStateKind.NOT_ENROLLED,
                    "The enrollment grant was rejected by the server.",
                )
                is EnrollmentExchangeResult.RecoveryRequired -> EnrollmentState(
                    EnrollmentStateKind.RECOVERY_REQUIRED,
                    "The enrollment result is unknown. Use the trusted owner recovery workflow before trying again.",
                )
                is EnrollmentExchangeResult.Failed -> EnrollmentState(
                    EnrollmentStateKind.RECOVERY_REQUIRED,
                    "Enrollment could not be completed safely.",
                )
            }
        } catch (error: CredentialVaultFailure) {
            EnrollmentState(
                EnrollmentStateKind.SECURE_STORE_UNAVAILABLE,
                "Secure credential storage is unavailable; no enrollment request was sent.",
            )
        } catch (_: Exception) {
            EnrollmentState(
                EnrollmentStateKind.RECOVERY_REQUIRED,
                "Enrollment could not be completed safely; recovery is required.",
            )
        }
    }

    suspend fun recover(profile: ServerProfile): EnrollmentState {
        val profileId = profile.profileId.toString()
        val pending = metadataStore.pending(profileId)
        if (pending != null) {
            if (pending.canonicalBaseUrl != profile.canonicalBaseUrl.value ||
                pending.transportPolicy != profile.transportPolicy.name
            ) {
                return EnrollmentState(
                    EnrollmentStateKind.RECOVERY_REQUIRED,
                    "Pending enrollment belongs to a different server origin.",
                )
            }
            return try {
                val record = vault.load(profileId, pending.scope())
                    ?: return EnrollmentState(
                        EnrollmentStateKind.RECOVERY_REQUIRED,
                        "Enrollment was interrupted before the credential was safely stored.",
                    )
                metadataStore.commitActive(pending)
                EnrollmentState(EnrollmentStateKind.ENROLLED)
            } catch (_: Exception) {
                EnrollmentState(
                    EnrollmentStateKind.RECOVERY_REQUIRED,
                    "Enrollment metadata is pending but the credential cannot be verified.",
                )
            }
        }
        val active = metadataStore.active(profileId) ?: return EnrollmentState(EnrollmentStateKind.NOT_ENROLLED)
        if (active.canonicalBaseUrl != profile.canonicalBaseUrl.value ||
            active.transportPolicy != profile.transportPolicy.name
        ) {
            return EnrollmentState(EnrollmentStateKind.RECOVERY_REQUIRED, "The stored credential is bound to another origin.")
        }
        return try {
            val record = vault.load(profileId, active.scope())
            if (record == null) {
                EnrollmentState(EnrollmentStateKind.RECOVERY_REQUIRED, "The stored credential is missing.")
            } else {
                EnrollmentState(EnrollmentStateKind.ENROLLED)
            }
        } catch (_: CredentialVaultFailure) {
            EnrollmentState(EnrollmentStateKind.RECOVERY_REQUIRED, "The stored credential is corrupt or out of scope.")
        }
    }

    suspend fun forget(profile: ServerProfile): CredentialCleanupResult = fence(profile.profileId.toString())

    override suspend fun fence(profileId: String): CredentialCleanupResult = try {
        vault.delete(profileId)
        metadataStore.clear(profileId)
        onFenced(profileId)
        CredentialCleanupResult.Success
    } catch (error: Exception) {
        CredentialCleanupResult.Failed(error.message ?: "secure credential cleanup failed")
    }

    private suspend fun finalize(profile: ServerProfile, record: DeviceCredentialRecord): EnrollmentState {
        val scope = CredentialScope.forProfile(profile, record)
        val metadata = EnrollmentMetadata(
            profileId = scope.profileId,
            canonicalBaseUrl = scope.canonicalBaseUrl,
            transportPolicy = scope.transportPolicy,
            ownerUserId = record.ownerUserId,
            deviceId = record.deviceId,
            credentialId = record.credentialId,
            createdAt = record.createdAt,
        )
        return try {
            metadataStore.writePending(metadata)
            vault.store(scope, record)
            val readBack = vault.load(profile.profileId.toString(), scope)
                ?: return EnrollmentState(
                    EnrollmentStateKind.RECOVERY_REQUIRED,
                    "The credential could not be read back after storage.",
                )
            if (!scope.matches(readBack) || readBack.credential.rawValue != record.credential.rawValue) {
                return EnrollmentState(
                    EnrollmentStateKind.RECOVERY_REQUIRED,
                    "The credential scope failed verification after storage.",
                )
            }
            metadataStore.commitActive(metadata)
            EnrollmentState(EnrollmentStateKind.ENROLLED)
        } catch (_: Exception) {
            EnrollmentState(
                EnrollmentStateKind.RECOVERY_REQUIRED,
                "Enrollment storage finalization failed; recovery is required.",
            )
        }
    }

    private fun EnrollmentMetadata.scope(): CredentialScope = CredentialScope(
        profileId = profileId,
        canonicalBaseUrl = canonicalBaseUrl,
        transportPolicy = transportPolicy,
        ownerUserId = ownerUserId,
        deviceId = deviceId,
        credentialId = credentialId,
    )
}

class NoOpCredentialLifecycle : CredentialLifecycle {
    override suspend fun fence(profileId: String): CredentialCleanupResult = CredentialCleanupResult.Success
}
