package com.synveil.android.feature.enrollment

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.data.enrollment.EnrollmentExchangeClient
import com.synveil.android.data.enrollment.EnrollmentManager
import com.synveil.android.data.enrollment.EnrollmentState
import com.synveil.android.data.enrollment.EnrollmentStateKind
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class EnrollmentUiState(
    val state: EnrollmentStateKind = EnrollmentStateKind.NOT_ENROLLED,
    val message: String? = null,
    val isSubmitting: Boolean = false,
    val tokenClearGeneration: Int = 0,
    val hasToken: Boolean = false,
) {
    override fun toString(): String =
        "EnrollmentUiState(state=$state, message=$message, isSubmitting=$isSubmitting, token=[REDACTED])"
}

enum class EnrollmentAction {
    SUBMIT_TOKEN,
    RETRY_RECOVERY,
    NONE,
}

data class EnrollmentPresentation(
    val statusLabel: String,
    val description: String,
    val primaryAction: EnrollmentAction,
    val primaryActionLabel: String,
    val canForgetLocally: Boolean,
)

fun enrollmentPresentationFor(kind: EnrollmentStateKind): EnrollmentPresentation = when (kind) {
    EnrollmentStateKind.NOT_ENROLLED -> EnrollmentPresentation(
        statusLabel = "Not enrolled",
        description = "Use a one-time grant from the trusted owner workflow to enroll this device.",
        primaryAction = EnrollmentAction.SUBMIT_TOKEN,
        primaryActionLabel = "Exchange once",
        canForgetLocally = false,
    )
    EnrollmentStateKind.INVALID_TOKEN -> EnrollmentPresentation(
        statusLabel = "Token rejected",
        description = "The token was not accepted. Request a fresh one-time grant from the trusted owner workflow.",
        primaryAction = EnrollmentAction.SUBMIT_TOKEN,
        primaryActionLabel = "Exchange once",
        canForgetLocally = false,
    )
    EnrollmentStateKind.PENDING -> EnrollmentPresentation(
        statusLabel = "Enrollment pending recovery",
        description = "The exchange may have completed. Re-check secure storage before attempting another enrollment.",
        primaryAction = EnrollmentAction.RETRY_RECOVERY,
        primaryActionLabel = "Re-check secure enrollment",
        canForgetLocally = true,
    )
    EnrollmentStateKind.ENROLLED -> EnrollmentPresentation(
        statusLabel = "Device enrolled",
        description = "This profile has a usable credential protected by Android Keystore.",
        primaryAction = EnrollmentAction.NONE,
        primaryActionLabel = "",
        canForgetLocally = true,
    )
    EnrollmentStateKind.RECOVERY_REQUIRED -> EnrollmentPresentation(
        statusLabel = "Recovery required",
        description = "The credential could not be verified safely. Re-check secure storage or forget the local enrollment before trying a new grant.",
        primaryAction = EnrollmentAction.RETRY_RECOVERY,
        primaryActionLabel = "Re-check secure enrollment",
        canForgetLocally = true,
    )
    EnrollmentStateKind.SECURE_STORE_UNAVAILABLE -> EnrollmentPresentation(
        statusLabel = "Secure storage unavailable",
        description = "Android Keystore is unavailable. No enrollment request was sent and no credential was accepted.",
        primaryAction = EnrollmentAction.NONE,
        primaryActionLabel = "",
        canForgetLocally = false,
    )
}

class EnrollmentViewModel(
    private val profile: ServerProfile,
    private val manager: EnrollmentManager,
    private val exchangeClient: EnrollmentExchangeClient,
) : ViewModel() {
    private val state = MutableStateFlow(EnrollmentUiState())
    private var pendingToken = ""
    val uiState: StateFlow<EnrollmentUiState> = state.asStateFlow()

    init {
        viewModelScope.launch {
            val recovered = withContext(Dispatchers.IO) { manager.recover(profile) }
            state.value = state.value.copy(state = recovered.kind, message = recovered.message)
        }
    }

    fun updateToken(value: String) {
        pendingToken = value
        state.value = state.value.copy(hasToken = value.isNotEmpty(), message = null)
    }

    fun enroll() {
        if (state.value.isSubmitting) return
        val token = pendingToken
        if (token.isEmpty()) return
        pendingToken = ""
        viewModelScope.launch {
            state.value = state.value.copy(
                isSubmitting = true,
                message = null,
                hasToken = false,
                tokenClearGeneration = state.value.tokenClearGeneration + 1,
            )
            val result = withContext(Dispatchers.IO) { manager.enroll(profile, token, exchangeClient) }
            state.value = state.value.copy(
                state = result.kind,
                message = result.message,
                isSubmitting = false,
            )
        }
    }

    fun retryRecovery() {
        if (state.value.isSubmitting) return
        viewModelScope.launch {
            state.value = state.value.copy(isSubmitting = true, message = null)
            val result = withContext(Dispatchers.IO) { manager.recover(profile) }
            state.value = state.value.copy(
                state = result.kind,
                message = result.message,
                isSubmitting = false,
            )
        }
    }

    fun forget() {
        if (state.value.isSubmitting) return
        viewModelScope.launch {
            state.value = state.value.copy(isSubmitting = true, message = null)
            val result = withContext(Dispatchers.IO) { manager.forget(profile) }
            state.value = state.value.copy(
                state = if (result is com.synveil.android.data.enrollment.CredentialCleanupResult.Success) {
                    EnrollmentStateKind.NOT_ENROLLED
                } else {
                    EnrollmentStateKind.RECOVERY_REQUIRED
                },
                message = if (result is com.synveil.android.data.enrollment.CredentialCleanupResult.Success) {
                    "Credential forgotten on this device. The server credential was not revoked."
                } else {
                    "Secure credential cleanup failed; nothing was forgotten."
                },
                hasToken = false,
                tokenClearGeneration = state.value.tokenClearGeneration + 1,
                isSubmitting = false,
            )
            pendingToken = ""
        }
    }
}
