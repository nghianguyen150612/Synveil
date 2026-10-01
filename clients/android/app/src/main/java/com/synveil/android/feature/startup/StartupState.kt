package com.synveil.android.feature.startup

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.synveil.android.core.model.AppIdentity
import com.synveil.android.data.profile.ProfileRepositoryState
import com.synveil.android.data.profile.ServerProfileRepository
import com.synveil.android.data.session.DeviceSessionManager
import com.synveil.android.data.session.DeviceSessionState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.stateIn

enum class StartupPrimaryAction {
    CONFIGURE_PROFILE,
    MANAGE_PROFILE,
    ENROLL_DEVICE,
    RECOVER_ENROLLMENT,
    REENROLL_DEVICE,
    OPEN_LIBRARIES,
}

sealed interface StartupSessionState {
    val profileId: String?
    val displayLabel: String?

    data object NoProfile : StartupSessionState {
        override val profileId: String? = null
        override val displayLabel: String? = null
    }

    data class ProfileConfigured(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState

    data class LoadingCredential(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState

    data class NotEnrolled(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState

    data class RecoveryRequired(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState

    data class AuthenticationRequired(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState

    data class DeviceRevoked(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState

    data class ServerUnavailable(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState

    data class TlsError(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState

    data class SecureStoreUnavailable(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState

    data class ProtocolError(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState

    data class Ready(
        override val profileId: String,
        override val displayLabel: String,
    ) : StartupSessionState
}

data class StartupUiState(
    val identity: AppIdentity,
    val statusMessage: String,
    val productBoundaryMessage: String,
    val configurationState: StartupConfigurationState,
    val sessionState: StartupSessionState,
    val primaryAction: StartupPrimaryAction,
    val primaryActionLabel: String,
) {
    companion object {
        fun foundation(): StartupUiState = StartupUiState(
            identity = AppIdentity.androidClient,
            statusMessage = "No server profile configured",
            productBoundaryMessage = "Authenticated library access uses the enrolled DeviceBearer. Conflict inspection and resolution remain owner/web review.",
            configurationState = StartupConfigurationState.NoServerConfigured,
            sessionState = StartupSessionState.NoProfile,
            primaryAction = StartupPrimaryAction.CONFIGURE_PROFILE,
            primaryActionLabel = "Set up server profile",
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
    sessionManager: DeviceSessionManager? = null,
) : ViewModel() {
    private val fallbackState = MutableStateFlow(StartupUiState.foundation())

    val uiState: StateFlow<StartupUiState> = if (repository == null) {
        fallbackState.asStateFlow()
    } else {
        combine(
            repository.state,
            sessionManager?.state ?: flowOf(DeviceSessionState.NoProfile),
            ::startupUiStateFor,
        ).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), StartupUiState.foundation())
    }
}

fun startupUiStateFor(
    state: ProfileRepositoryState,
    sessionState: DeviceSessionState = DeviceSessionState.NoProfile,
): StartupUiState {
    val configurationState = when (state) {
        ProfileRepositoryState.NoServerConfigured -> StartupConfigurationState.NoServerConfigured
        is ProfileRepositoryState.Configured -> state.configuration.activeProfile?.let {
            StartupConfigurationState.ServerConfigured(it.displayLabel)
        } ?: StartupConfigurationState.NoServerConfigured
        is ProfileRepositoryState.ConfigurationError -> StartupConfigurationState.ConfigurationError
    }
    val activeProfile = (state as? ProfileRepositoryState.Configured)?.configuration?.activeProfile
    val presentation = startupSessionPresentation(
        sessionState,
        configuredProfileId = activeProfile?.profileId?.toString(),
        configuredDisplayLabel = activeProfile?.displayLabel,
    )
    val configurationError = configurationState == StartupConfigurationState.ConfigurationError
    return StartupUiState(
        identity = AppIdentity.androidClient,
        statusMessage = if (configurationError) {
            "Server configuration needs attention"
        } else {
            presentation.statusMessage
        },
        productBoundaryMessage = "Authenticated library access uses the enrolled DeviceBearer. Conflict inspection and resolution remain owner/web review.",
        configurationState = configurationState,
        sessionState = presentation.sessionState,
        primaryAction = if (configurationError) StartupPrimaryAction.MANAGE_PROFILE else presentation.primaryAction,
        primaryActionLabel = if (configurationError) "Review server profiles" else presentation.primaryActionLabel,
    )
}

private data class StartupSessionPresentation(
    val sessionState: StartupSessionState,
    val statusMessage: String,
    val primaryAction: StartupPrimaryAction,
    val primaryActionLabel: String,
)

private fun startupSessionPresentation(
    state: DeviceSessionState,
    configuredProfileId: String?,
    configuredDisplayLabel: String?,
): StartupSessionPresentation {
    val fallback = if (configuredProfileId != null && configuredDisplayLabel != null) {
        StartupSessionState.ProfileConfigured(configuredProfileId, configuredDisplayLabel)
    } else {
        StartupSessionState.NoProfile
    }
    val session = when {
        state == DeviceSessionState.NoProfile || state is DeviceSessionState.ProfileAvailable -> fallback
        configuredProfileId == null || state.profileIdOrNull() != configuredProfileId -> fallback
        else -> when (state) {
            DeviceSessionState.NoProfile,
            is DeviceSessionState.ProfileAvailable,
                -> fallback
            is DeviceSessionState.LoadingCredential -> StartupSessionState.LoadingCredential(state.profileId, state.displayLabel)
            is DeviceSessionState.NotEnrolled -> StartupSessionState.NotEnrolled(state.profileId, state.displayLabel)
            is DeviceSessionState.RecoveryRequired -> StartupSessionState.RecoveryRequired(state.profileId, state.displayLabel)
            is DeviceSessionState.AuthenticationRequired -> StartupSessionState.AuthenticationRequired(state.profileId, state.displayLabel)
            is DeviceSessionState.DeviceRevoked -> StartupSessionState.DeviceRevoked(state.profileId, state.displayLabel)
            is DeviceSessionState.ServerUnavailable -> StartupSessionState.ServerUnavailable(state.profileId, state.displayLabel)
            is DeviceSessionState.TlsError -> StartupSessionState.TlsError(state.profileId, state.displayLabel)
            is DeviceSessionState.SecureStoreUnavailable -> StartupSessionState.SecureStoreUnavailable(state.profileId, state.displayLabel)
            is DeviceSessionState.ProtocolError -> StartupSessionState.ProtocolError(state.profileId, state.displayLabel)
            is DeviceSessionState.Ready -> StartupSessionState.Ready(state.profileId, state.displayLabel)
        }
    }
    return when (session) {
        StartupSessionState.NoProfile -> StartupSessionPresentation(
            session,
            "No server profile configured",
            StartupPrimaryAction.CONFIGURE_PROFILE,
            "Set up server profile",
        )
        is StartupSessionState.ProfileConfigured -> StartupSessionPresentation(
            session,
            "Server configured: ${session.displayLabel}",
            StartupPrimaryAction.MANAGE_PROFILE,
            "Review server profile",
        )
        is StartupSessionState.LoadingCredential -> StartupSessionPresentation(
            session,
            "Checking secure device access for ${session.displayLabel}",
            StartupPrimaryAction.MANAGE_PROFILE,
            "Review device access",
        )
        is StartupSessionState.NotEnrolled -> StartupSessionPresentation(
            session,
            "Device enrollment required for ${session.displayLabel}",
            StartupPrimaryAction.ENROLL_DEVICE,
            "Enroll this device",
        )
        is StartupSessionState.RecoveryRequired -> StartupSessionPresentation(
            session,
            "Enrollment recovery required for ${session.displayLabel}",
            StartupPrimaryAction.RECOVER_ENROLLMENT,
            "Review enrollment recovery",
        )
        is StartupSessionState.AuthenticationRequired -> StartupSessionPresentation(
            session,
            "Device authentication required",
            StartupPrimaryAction.REENROLL_DEVICE,
            "Re-enroll this device",
        )
        is StartupSessionState.DeviceRevoked -> StartupSessionPresentation(
            session,
            "This device was revoked by the server",
            StartupPrimaryAction.REENROLL_DEVICE,
            "Re-enroll this device",
        )
        is StartupSessionState.ServerUnavailable -> StartupSessionPresentation(
            session,
            "Server unavailable for ${session.displayLabel}",
            StartupPrimaryAction.MANAGE_PROFILE,
            "Review server profile",
        )
        is StartupSessionState.TlsError -> StartupSessionPresentation(
            session,
            "Secure connection unavailable for ${session.displayLabel}",
            StartupPrimaryAction.MANAGE_PROFILE,
            "Review server profile",
        )
        is StartupSessionState.SecureStoreUnavailable -> StartupSessionPresentation(
            session,
            "Secure storage unavailable on this device",
            StartupPrimaryAction.MANAGE_PROFILE,
            "Review device access",
        )
        is StartupSessionState.ProtocolError -> StartupSessionPresentation(
            session,
            "Server response needs attention for ${session.displayLabel}",
            StartupPrimaryAction.MANAGE_PROFILE,
            "Review server profile",
        )
        is StartupSessionState.Ready -> StartupSessionPresentation(
            session,
            "Device ready for ${session.displayLabel}",
            StartupPrimaryAction.OPEN_LIBRARIES,
            "Open libraries",
        )
    }
}

private fun DeviceSessionState.profileIdOrNull(): String? = when (this) {
    DeviceSessionState.NoProfile -> null
    is DeviceSessionState.ProfileAvailable -> profileId
    is DeviceSessionState.LoadingCredential -> profileId
    is DeviceSessionState.NotEnrolled -> profileId
    is DeviceSessionState.RecoveryRequired -> profileId
    is DeviceSessionState.AuthenticationRequired -> profileId
    is DeviceSessionState.DeviceRevoked -> profileId
    is DeviceSessionState.ServerUnavailable -> profileId
    is DeviceSessionState.TlsError -> profileId
    is DeviceSessionState.SecureStoreUnavailable -> profileId
    is DeviceSessionState.ProtocolError -> profileId
    is DeviceSessionState.Ready -> profileId
}
