package com.synveil.android.feature.startup

import androidx.lifecycle.ViewModel
import com.synveil.android.core.model.AppIdentity
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

data class StartupUiState(
    val identity: AppIdentity,
    val statusMessage: String,
    val unsupportedFeaturesMessage: String,
) {
    companion object {
        fun foundation(): StartupUiState = StartupUiState(
            identity = AppIdentity.development,
            statusMessage = "Native Android foundation ready",
            unsupportedFeaturesMessage = "Authentication, networking, sync, uploads, backups, and file browsing are not implemented yet.",
        )
    }
}

class StartupViewModel : ViewModel() {
    private val state = MutableStateFlow(StartupUiState.foundation())

    val uiState: StateFlow<StartupUiState> = state.asStateFlow()
}
