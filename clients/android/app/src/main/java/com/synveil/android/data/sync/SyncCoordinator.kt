package com.synveil.android.data.sync

import com.synveil.android.core.model.ServerProfile
import com.synveil.android.data.cache.CacheRepository
import com.synveil.android.data.enrollment.CredentialVaultFailure
import com.synveil.android.data.enrollment.CredentialVaultException
import com.synveil.android.data.enrollment.CredentialScope
import com.synveil.android.data.enrollment.EnrollmentMetadataStore
import com.synveil.android.data.enrollment.SecureCredentialVault
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.network.AuthenticatedSynveilTransport
import com.synveil.android.data.network.TransportTimeouts
import com.synveil.android.data.mutation.MutationEngine
import com.synveil.android.data.transfer.ContentOperationEngine
import com.synveil.android.data.profile.ProfileRepositoryState
import com.synveil.android.data.profile.ServerProfileRepository
import kotlinx.coroutines.flow.first

class SyncCoordinator(
    private val profileRepository: ServerProfileRepository,
    private val metadataStore: EnrollmentMetadataStore,
    private val vault: SecureCredentialVault,
    private val cache: CacheRepository,
    private val userAgent: String,
    private val allowLoopbackTestHttp: Boolean,
    private val timeouts: TransportTimeouts = TransportTimeouts(),
) {
    suspend fun synchronize(profileId: String, libraryId: LibraryId): SyncOutcome {
        val profile = profile(profileId) ?: return SyncOutcome(SyncOutcomeKind.AUTHENTICATION_REQUIRED)
        val metadata = metadataStore.active(profileId) ?: return SyncOutcome(SyncOutcomeKind.AUTHENTICATION_REQUIRED)
        if (metadata.canonicalBaseUrl != profile.canonicalBaseUrl.value || metadata.transportPolicy != profile.transportPolicy.name) {
            return SyncOutcome(SyncOutcomeKind.AUTHENTICATION_REQUIRED)
        }
        val scope = CredentialScope(profileId, metadata.canonicalBaseUrl, metadata.transportPolicy, metadata.ownerUserId, metadata.deviceId, metadata.credentialId)
        val record = try { vault.load(profileId, scope) } catch (_: CredentialVaultFailure) { return SyncOutcome(SyncOutcomeKind.AUTHENTICATION_REQUIRED) }
            ?: return SyncOutcome(SyncOutcomeKind.AUTHENTICATION_REQUIRED)
        if (!scope.matches(record)) return SyncOutcome(SyncOutcomeKind.AUTHENTICATION_REQUIRED)
        val transport = AuthenticatedSynveilTransport(profile, record.credential, userAgent, timeouts, allowLoopbackTestHttp)
        val engine = SyncEngine(transport, cache, profileId, metadata.deviceId, libraryId)
        val state = cache.state(profileId, metadata.deviceId, libraryId)
        val inbound = if (state?.state == SyncStateKind.REBASELINE_REQUIRED.name) {
            engine.rebaseline()
        } else {
            val result = engine.synchronize()
            if (result.kind == SyncOutcomeKind.REBASELINE_REQUIRED) engine.rebaseline() else result
        }
        if (inbound.kind !in setOf(SyncOutcomeKind.SUCCESS, SyncOutcomeKind.MORE_WORK)) return inbound
        if (cache.state(profileId, metadata.deviceId, libraryId)?.state == SyncStateKind.READY.name) {
            cache.releaseBlockedMutations(profileId, metadata.deviceId, libraryId)
        }
        val drain = MutationEngine(cache, transport, profileId, metadata.deviceId, libraryId).drain()
        val contentTransient = ContentOperationEngine(cache, transport, profileId, metadata.deviceId, libraryId).drain()
        val afterOutbound = engine.synchronize()
        return if (afterOutbound.kind == SyncOutcomeKind.SUCCESS && (drain.transientFailure || contentTransient)) {
            SyncOutcome(SyncOutcomeKind.TRANSIENT_ERROR)
        } else if (afterOutbound.kind == SyncOutcomeKind.SUCCESS && (drain.attempted == 16 || drain.blocked > 0)) {
            SyncOutcome(SyncOutcomeKind.MORE_WORK)
        } else {
            afterOutbound
        }
    }

    private suspend fun profile(profileId: String): ServerProfile? = when (val state = profileRepository.state.first()) {
        is ProfileRepositoryState.Configured -> state.configuration.profiles.firstOrNull { it.profileId.toString() == profileId }
        else -> null
    }
}
