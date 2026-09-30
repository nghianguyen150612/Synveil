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
    val token: String = "",
    val state: EnrollmentStateKind = EnrollmentStateKind.NOT_ENROLLED,
    val message: String? = null,
    val isSubmitting: Boolean = false,
) {
    override fun toString(): String =
        "EnrollmentUiState(state=$state, message=$message, isSubmitting=$isSubmitting, token=[REDACTED])"
}

class EnrollmentViewModel(
    private val profile: ServerProfile,
    private val manager: EnrollmentManager,
    private val exchangeClient: EnrollmentExchangeClient,
) : ViewModel() {
    private val state = MutableStateFlow(EnrollmentUiState())
    val uiState: StateFlow<EnrollmentUiState> = state.asStateFlow()

    init {
        viewModelScope.launch {
            val recovered = withContext(Dispatchers.IO) { manager.recover(profile) }
            state.value = state.value.copy(state = recovered.kind, message = recovered.message)
        }
    }

    fun updateToken(value: String) {
        state.value = state.value.copy(token = value, message = null)
    }

    fun enroll() {
        if (state.value.isSubmitting) return
        val token = state.value.token
        viewModelScope.launch {
            state.value = state.value.copy(isSubmitting = true, message = null)
            val result = withContext(Dispatchers.IO) { manager.enroll(profile, token, exchangeClient) }
            state.value = state.value.copy(
                token = "",
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
                isSubmitting = false,
            )
        }
    }
}
