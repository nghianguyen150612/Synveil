package com.synveil.android.data.session

import com.synveil.android.core.model.ServerProfile
import com.synveil.android.data.enrollment.CredentialScope
import com.synveil.android.data.enrollment.CredentialVaultException
import com.synveil.android.data.enrollment.CredentialVaultFailure
import com.synveil.android.data.enrollment.EnrollmentMetadata
import com.synveil.android.data.enrollment.EnrollmentMetadataStore
import com.synveil.android.data.enrollment.SecureCredentialVault
import com.synveil.android.data.library.AuthenticatedLibraryRepository
import com.synveil.android.data.library.LibraryFailure
import com.synveil.android.data.library.LibraryRepositoryResult
import com.synveil.android.data.network.AuthenticatedSynveilTransport
import com.synveil.android.data.network.SynveilTransportError
import com.synveil.android.data.network.TransportTimeouts
import com.synveil.android.data.profile.ProfileRepositoryState
import com.synveil.android.data.profile.ServerProfileRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

sealed interface DeviceSessionState {
    data object NoProfile : DeviceSessionState
    data class ProfileAvailable(val profileId: String, val displayLabel: String) : DeviceSessionState
    data class NotEnrolled(val profileId: String, val displayLabel: String) : DeviceSessionState
    data class LoadingCredential(val profileId: String, val displayLabel: String) : DeviceSessionState
    data class Ready(val profileId: String, val displayLabel: String) : DeviceSessionState
    data class AuthenticationRequired(val profileId: String, val displayLabel: String) : DeviceSessionState
    data class DeviceRevoked(val profileId: String, val displayLabel: String) : DeviceSessionState
    data class ServerUnavailable(val profileId: String, val displayLabel: String) : DeviceSessionState
    data class TlsError(val profileId: String, val displayLabel: String) : DeviceSessionState
    data class SecureStoreUnavailable(val profileId: String, val displayLabel: String) : DeviceSessionState
    data class RecoveryRequired(val profileId: String, val displayLabel: String) : DeviceSessionState
    data class ProtocolError(val profileId: String, val displayLabel: String) : DeviceSessionState
}

class DeviceSessionManager(
    private val profileRepository: ServerProfileRepository,
    private val metadataStore: EnrollmentMetadataStore,
    private val vault: SecureCredentialVault,
    private val userAgent: String,
    private val allowLoopbackTestHttp: Boolean,
    scope: CoroutineScope,
    private val timeouts: TransportTimeouts = TransportTimeouts(),
) {
    private val mutableState = MutableStateFlow<DeviceSessionState>(DeviceSessionState.NoProfile)
    val state: StateFlow<DeviceSessionState> = mutableState.asStateFlow()
    private val contextMutex = Mutex()
    private val refreshMutex = Mutex()
    private var context: AuthenticatedContext? = null
    private var observedProfileId: String? = null

    init {
        scope.launch(Dispatchers.IO) {
            profileRepository.state.collectLatest { repositoryState ->
                val profile = (repositoryState as? ProfileRepositoryState.Configured)
                    ?.configuration?.activeProfile
                contextMutex.withLock {
                    val profileId = profile?.profileId?.toString()
                    if (profileId != observedProfileId) {
                        context = null
                        observedProfileId = profileId
                    }
                    mutableState.value = if (profile == null) {
                        DeviceSessionState.NoProfile
                    } else {
                        DeviceSessionState.ProfileAvailable(profileId!!, profile.displayLabel)
                    }
                }
            }
        }
    }

    suspend fun listLibraries(): LibraryRepositoryResult = refreshMutex.withLock {
        val current = activeProfile() ?: run {
            mutableState.value = DeviceSessionState.NoProfile
            return LibraryRepositoryResult.Failed(LibraryFailure.NoActiveProfile)
        }
        val authenticated = authenticatedContext(current) ?: return LibraryRepositoryResult.Failed(
            LibraryFailure.Transport(SynveilTransportError.ConfigurationError),
        )
        when (val result = authenticated.repository.listLibraries()) {
            is LibraryRepositoryResult.Loaded -> {
                mutableState.value = DeviceSessionState.Ready(current.profileId.toString(), current.displayLabel)
                result
            }
            is LibraryRepositoryResult.Failed -> {
                updateFailureState(current, result.failure)
                result
            }
        }
    }

    private suspend fun authenticatedContext(profile: ServerProfile): AuthenticatedContext? = contextMutex.withLock {
        val profileId = profile.profileId.toString()
        context?.takeIf {
            it.profileId == profileId &&
                it.canonicalBaseUrl == profile.canonicalBaseUrl.value &&
                it.transportPolicy == profile.transportPolicy.name
        }?.let {
            mutableState.value = DeviceSessionState.Ready(profileId, profile.displayLabel)
            return@withLock it
        }
        mutableState.value = DeviceSessionState.LoadingCredential(profileId, profile.displayLabel)
        val metadata = metadataStore.active(profileId) ?: run {
            mutableState.value = DeviceSessionState.NotEnrolled(profileId, profile.displayLabel)
            return@withLock null
        }
        if (!metadata.matches(profile)) {
            mutableState.value = DeviceSessionState.RecoveryRequired(profileId, profile.displayLabel)
            return@withLock null
        }
        val scope = metadata.scope()
        val record = try {
            vault.load(profileId, scope)
        } catch (error: CredentialVaultFailure) {
            mutableState.value = when (error.reason) {
                CredentialVaultException.Unavailable -> DeviceSessionState.SecureStoreUnavailable(profileId, profile.displayLabel)
                CredentialVaultException.Missing,
                CredentialVaultException.Corrupt,
                CredentialVaultException.ScopeMismatch -> DeviceSessionState.RecoveryRequired(profileId, profile.displayLabel)
            }
            return@withLock null
        } catch (_: Exception) {
            mutableState.value = DeviceSessionState.SecureStoreUnavailable(profileId, profile.displayLabel)
            return@withLock null
        }
        if (record == null || !scope.matches(record)) {
            mutableState.value = DeviceSessionState.RecoveryRequired(profileId, profile.displayLabel)
            return@withLock null
        }
        val transport = AuthenticatedSynveilTransport(
            profile = profile,
            credential = record.credential,
            userAgent = userAgent,
            timeouts = timeouts,
            allowLoopbackTestHttp = allowLoopbackTestHttp,
        )
        AuthenticatedContext(
            profileId = profileId,
            canonicalBaseUrl = profile.canonicalBaseUrl.value,
            transportPolicy = profile.transportPolicy.name,
            repository = AuthenticatedLibraryRepository(transport::listLibrariesPage),
        ).also { context = it }
    }

    private suspend fun activeProfile(): ServerProfile? = when (val value = profileRepository.state.first()) {
        ProfileRepositoryState.NoServerConfigured -> null
        is ProfileRepositoryState.ConfigurationError -> null
        is ProfileRepositoryState.Configured -> value.configuration.activeProfile
    }

    private fun updateFailureState(profile: ServerProfile, failure: LibraryFailure) {
        val profileId = profile.profileId.toString()
        mutableState.value = when (failure) {
            is LibraryFailure.Transport -> when (val error = failure.error) {
                is SynveilTransportError.HttpError -> when {
                    error.statusCode == 401 && error.code == "authentication_failed" ->
                        DeviceSessionState.AuthenticationRequired(profileId, profile.displayLabel)
                    error.statusCode == 401 && error.code == "device_revoked" ->
                        DeviceSessionState.DeviceRevoked(profileId, profile.displayLabel)
                    error.statusCode == 503 -> DeviceSessionState.ServerUnavailable(profileId, profile.displayLabel)
                    else -> DeviceSessionState.ProtocolError(profileId, profile.displayLabel)
                }
                SynveilTransportError.TlsError -> DeviceSessionState.TlsError(profileId, profile.displayLabel)
                SynveilTransportError.Offline,
                SynveilTransportError.DnsFailure,
                SynveilTransportError.Timeout -> DeviceSessionState.ServerUnavailable(profileId, profile.displayLabel)
                is SynveilTransportError.ProtocolError,
                SynveilTransportError.BodyLimitExceeded,
                is SynveilTransportError.UnexpectedContentType,
                SynveilTransportError.MalformedResponse,
                is SynveilTransportError.RedirectRejected,
                SynveilTransportError.ConfigurationError,
                SynveilTransportError.Cancelled -> DeviceSessionState.ProtocolError(profileId, profile.displayLabel)
            }
            LibraryFailure.NoActiveProfile -> DeviceSessionState.NoProfile
            LibraryFailure.ResourceLimit,
            LibraryFailure.RepeatedCursor -> DeviceSessionState.ProtocolError(profileId, profile.displayLabel)
        }
    }

    private data class AuthenticatedContext(
        val profileId: String,
        val canonicalBaseUrl: String,
        val transportPolicy: String,
        val repository: AuthenticatedLibraryRepository,
    )
}

private fun EnrollmentMetadata.matches(profile: ServerProfile): Boolean =
    profileId == profile.profileId.toString() &&
        canonicalBaseUrl == profile.canonicalBaseUrl.value &&
        transportPolicy == profile.transportPolicy.name

private fun EnrollmentMetadata.scope(): CredentialScope = CredentialScope(
    profileId = profileId,
    canonicalBaseUrl = canonicalBaseUrl,
    transportPolicy = transportPolicy,
    ownerUserId = ownerUserId,
    deviceId = deviceId,
    credentialId = credentialId,
)
