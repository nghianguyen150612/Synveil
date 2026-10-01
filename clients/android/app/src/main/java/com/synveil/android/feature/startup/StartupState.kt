package com.synveil.android.feature.startup

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.synveil.android.core.model.AppIdentity
import com.synveil.android.data.profile.ProfileRepositoryState
import com.synveil.android.data.profile.ServerProfileRepository
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn

data class StartupUiState(
    val identity: AppIdentity,
    val statusMessage: String,
    val unsupportedFeaturesMessage: String,
    val configurationState: StartupConfigurationState,
) {
    companion object {
        fun foundation(): StartupUiState = StartupUiState(
            identity = AppIdentity.development,
            statusMessage = "Native Android foundation ready",
            unsupportedFeaturesMessage = "Device authentication, file browsing, and foreground transfers are available; sync, backups, replace-content editing, and background transfer remain future work.",
            configurationState = StartupConfigurationState.NoServerConfigured,
        )
    }
}

sealed interface StartupConfigurationState {
    data object NoServerConfigured : StartupConfigurationState

    data class ServerConfigured(
        val displayLabel: String,
    ) : StartupConfigurationState

    data object ConfigurationError : StartupConfigurationState
}

class StartupViewModel(
    repository: ServerProfileRepository? = null,
) : ViewModel() {
    private val fallbackState = MutableStateFlow(StartupUiState.foundation())

    val uiState: StateFlow<StartupUiState> = repository?.state
        ?.map(::startupUiStateFor)
        ?.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), StartupUiState.foundation())
        ?: fallbackState.asStateFlow()
}

fun startupUiStateFor(state: ProfileRepositoryState): StartupUiState {
    val configurationState = when (state) {
        ProfileRepositoryState.NoServerConfigured -> StartupConfigurationState.NoServerConfigured
        is ProfileRepositoryState.Configured -> state.configuration.activeProfile?.let {
            StartupConfigurationState.ServerConfigured(it.displayLabel)
        } ?: StartupConfigurationState.NoServerConfigured
        is ProfileRepositoryState.ConfigurationError -> StartupConfigurationState.ConfigurationError
    }
    val message = when (configurationState) {
        StartupConfigurationState.NoServerConfigured -> "No server configured"
        is StartupConfigurationState.ServerConfigured -> "Server configured: ${configurationState.displayLabel}"
        StartupConfigurationState.ConfigurationError -> "Server configuration needs attention"
    }
    return StartupUiState(
        identity = AppIdentity.development,
        statusMessage = message,
        unsupportedFeaturesMessage = "Device authentication, file browsing, and foreground transfers are available; sync, backups, replace-content editing, and background transfer remain future work.",
        configurationState = configurationState,
    )
}
