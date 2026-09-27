package com.synveil.android.feature.profile

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.synveil.android.core.model.CanonicalOriginError
import com.synveil.android.core.model.CanonicalOriginException
import com.synveil.android.core.model.DisplayLabelError
import com.synveil.android.core.model.DisplayLabelException
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.data.profile.ProfileOperationError
import com.synveil.android.data.profile.ProfileOperationException
import com.synveil.android.data.profile.ProfileRepositoryState
import com.synveil.android.data.profile.ServerProfileRepository
import com.synveil.android.data.network.ConnectionCheckResult
import com.synveil.android.data.network.SynveilHttpTransport
import com.synveil.android.data.network.SynveilTransportError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext
import kotlinx.coroutines.launch

data class ProfilesUiState(
    val profiles: List<ServerProfile> = emptyList(),
    val activeProfileId: ServerProfileId? = null,
    val isLoading: Boolean = true,
    val errorMessage: String? = null,
    val connectionStates: Map<ServerProfileId, ConnectionUiState> = emptyMap(),
)

sealed interface ConnectionUiState {
    data object Idle : ConnectionUiState
    data object Checking : ConnectionUiState
    data class Ready(val requestIds: List<String>) : ConnectionUiState
    data class AliveButNotReady(val code: String?) : ConnectionUiState
    data class Failed(val error: SynveilTransportError) : ConnectionUiState
}

class ServerProfilesViewModel(
    private val repository: ServerProfileRepository,
    private val transportFactory: (ServerProfile) -> SynveilHttpTransport,
    private val nowMillis: () -> Long = { System.currentTimeMillis() },
) : ViewModel() {
    private val actionError = MutableStateFlow<String?>(null)
    private val connectionStates = MutableStateFlow<Map<ServerProfileId, ConnectionUiState>>(emptyMap())
    private val checksInFlight = mutableSetOf<ServerProfileId>()

    private val repositoryState: StateFlow<ProfilesUiState> = repository.state
        .map { state ->
            when (state) {
                ProfileRepositoryState.NoServerConfigured -> ProfilesUiState(isLoading = false)
                is ProfileRepositoryState.Configured -> ProfilesUiState(
                    profiles = state.configuration.profiles,
                    activeProfileId = state.configuration.activeProfileId,
                    isLoading = false,
                )
                is ProfileRepositoryState.ConfigurationError -> ProfilesUiState(
                    isLoading = false,
                    errorMessage = "Saved server configuration is unavailable.",
                )
            }
        }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), ProfilesUiState())

    val uiState: StateFlow<ProfilesUiState> = combine(repositoryState, actionError, connectionStates) { base, actionMessage, checks ->
        base.copy(
            errorMessage = actionMessage ?: base.errorMessage,
            connectionStates = checks,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), ProfilesUiState())

    fun select(profileId: ServerProfileId) {
        runAction { repository.selectActiveProfile(profileId) }
    }

    fun remove(profileId: ServerProfileId) {
        runAction { repository.removeProfile(profileId) }
    }

    fun testConnection(profileId: ServerProfileId) {
        if (!checksInFlight.add(profileId)) return
        viewModelScope.launch {
            connectionStates.update { it + (profileId to ConnectionUiState.Checking) }
            val result = try {
                val profile = repository.state.first().let { state ->
                    (state as? ProfileRepositoryState.Configured)
                        ?.configuration
                        ?.profiles
                        ?.firstOrNull { it.profileId == profileId }
                }
                if (profile == null) {
                    ConnectionCheckResult.Failure(SynveilTransportError.ConfigurationError)
                } else {
                    withContext(Dispatchers.IO) { transportFactory(profile).checkServer() }
                }
            } catch (_: kotlinx.coroutines.CancellationException) {
                ConnectionCheckResult.Failure(SynveilTransportError.Cancelled)
            } catch (_: Exception) {
                ConnectionCheckResult.Failure(SynveilTransportError.ConfigurationError)
            }
            val uiState = when (result) {
                is ConnectionCheckResult.Ready -> {
                    try {
                        repository.recordSuccessfulConnection(profileId, nowMillis())
                        ConnectionUiState.Ready(
                            requestIds = listOfNotNull(result.livenessRequestId, result.readinessRequestId),
                        )
                    } catch (_: Exception) {
                        ConnectionUiState.Failed(SynveilTransportError.ConfigurationError)
                    }
                }
                is ConnectionCheckResult.AliveButNotReady -> ConnectionUiState.AliveButNotReady(result.code)
                is ConnectionCheckResult.Failure -> ConnectionUiState.Failed(result.error)
            }
            connectionStates.update { it + (profileId to uiState) }
            checksInFlight.remove(profileId)
        }
    }

    private fun runAction(action: suspend () -> Unit) {
        viewModelScope.launch {
            actionError.value = null
            try {
                action()
            } catch (error: Exception) {
                actionError.value = profileErrorMessage(error)
            }
        }
    }
}

data class ProfileEditorUiState(
    val displayLabel: String = "",
    val rawBaseUrl: String = "",
    val isSaving: Boolean = false,
    val isReady: Boolean = false,
    val saved: Boolean = false,
    val errorMessage: String? = null,
)

class ProfileEditorViewModel(
    private val repository: ServerProfileRepository,
    private val profileId: ServerProfileId?,
) : ViewModel() {
    private val state = MutableStateFlow(ProfileEditorUiState(isReady = profileId == null))
    val uiState: StateFlow<ProfileEditorUiState> = state.asStateFlow()

    init {
        if (profileId != null) {
            viewModelScope.launch {
                repository.state.collect { repositoryState ->
                    if (!state.value.isReady && repositoryState is ProfileRepositoryState.Configured) {
                        val profile = repositoryState.configuration.profiles.firstOrNull { it.profileId == profileId }
                        if (profile == null) {
                            state.update { it.copy(isReady = true, errorMessage = "Server profile was not found.") }
                        } else {
                            state.update {
                                it.copy(
                                    displayLabel = profile.displayLabel,
                                    rawBaseUrl = profile.canonicalBaseUrl.value,
                                    isReady = true,
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    fun updateDisplayLabel(value: String) {
        state.update { it.copy(displayLabel = value, errorMessage = null) }
    }

    fun updateBaseUrl(value: String) {
        state.update { it.copy(rawBaseUrl = value, errorMessage = null) }
    }

    fun save() {
        viewModelScope.launch {
            state.update { it.copy(isSaving = true, errorMessage = null) }
            try {
                if (profileId == null) {
                    repository.addProfile(state.value.displayLabel, state.value.rawBaseUrl)
                } else {
                    repository.updateProfile(profileId, state.value.displayLabel, state.value.rawBaseUrl)
                }
                state.update { it.copy(isSaving = false, saved = true) }
            } catch (error: Exception) {
                state.update {
                    it.copy(isSaving = false, errorMessage = profileErrorMessage(error))
                }
            }
        }
    }
}

fun profileErrorMessage(error: Throwable): String = when (error) {
    is DisplayLabelException -> when (error.reason) {
        DisplayLabelError.REQUIRED -> "Display name is required."
        DisplayLabelError.CONTROL_CHARACTER -> "Display name contains an invalid control character."
        DisplayLabelError.TOO_LONG -> "Display name is too long."
    }
    is CanonicalOriginException -> when (error.reason) {
        CanonicalOriginError.REQUIRED -> "Server URL is required."
        CanonicalOriginError.TOO_LONG -> "Server URL is too long."
        CanonicalOriginError.HTTPS_REQUIRED -> "HTTPS is required for server profiles."
        CanonicalOriginError.LOOPBACK_HTTP_NOT_ALLOWED -> "HTTP is limited to numeric loopback addresses in development."
        CanonicalOriginError.PATH_NOT_ALLOWED -> "Server URL must be an origin, not a path."
        CanonicalOriginError.QUERY_NOT_ALLOWED -> "Server URL cannot contain a query."
        CanonicalOriginError.FRAGMENT_NOT_ALLOWED -> "Server URL cannot contain a fragment."
        CanonicalOriginError.USERINFO_NOT_ALLOWED -> "Server URL cannot contain credentials."
        CanonicalOriginError.INVALID_PORT -> "Invalid server port."
        CanonicalOriginError.INVALID_HOST -> "Invalid server host."
        CanonicalOriginError.UNSUPPORTED_SCHEME -> "Server URL must use HTTPS."
        CanonicalOriginError.CONTROL_OR_WHITESPACE,
        CanonicalOriginError.BACKSLASH_NOT_ALLOWED,
        CanonicalOriginError.INVALID_URI -> "Invalid server URL."
        CanonicalOriginError.HOST_REQUIRED -> "Server URL must include a host."
    }
    is ProfileOperationException -> when (error.reason) {
        ProfileOperationError.DUPLICATE_ORIGIN -> "Server already exists."
        ProfileOperationError.PROFILE_NOT_FOUND -> "Server profile was not found."
        ProfileOperationError.ACTIVE_PROFILE_REQUIRED -> "The active profile selection is invalid."
        ProfileOperationError.PROFILE_LIMIT_REACHED -> "The maximum number of server profiles has been reached."
        ProfileOperationError.INVALID_PERSISTED_CONFIGURATION,
        ProfileOperationError.STORAGE_UNAVAILABLE -> "Server configuration could not be saved."
    }
    else -> "Server configuration could not be saved."
}
