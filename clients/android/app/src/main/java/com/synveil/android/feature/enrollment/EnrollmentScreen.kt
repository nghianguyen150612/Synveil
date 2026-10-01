package com.synveil.android.feature.enrollment

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.data.enrollment.EnrollmentExchangeClient
import com.synveil.android.data.enrollment.EnrollmentManager
import com.synveil.android.data.enrollment.EnrollmentStateKind

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EnrollmentScreen(
    profile: ServerProfile,
    manager: EnrollmentManager,
    exchangeClient: EnrollmentExchangeClient,
    onBack: () -> Unit,
    enrollmentViewModel: EnrollmentViewModel = viewModel(
        key = "enrollment-${profile.profileId}",
        factory = enrollmentViewModelFactory(profile, manager, exchangeClient),
    ),
) {
    val uiState by enrollmentViewModel.uiState.collectAsStateWithLifecycle()
    val presentation = enrollmentPresentationFor(uiState.state)
    var token by remember { mutableStateOf("") }
    var confirmForget by remember { mutableStateOf(false) }
    LaunchedEffect(uiState.tokenClearGeneration) {
        token = ""
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Enroll device") },
                navigationIcon = { TextButton(onClick = onBack) { Text("Back") } },
            )
        },
    ) { paddingValues ->
        Column(
            modifier = Modifier.fillMaxSize().padding(paddingValues).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(profile.displayLabel, style = MaterialTheme.typography.titleLarge)
            Text(presentation.statusLabel, style = MaterialTheme.typography.titleMedium)
            Text(presentation.description)
            if (presentation.primaryAction == EnrollmentAction.SUBMIT_TOKEN) {
                Text("Paste the one-time grant from the trusted owner workflow. It is used only in memory and is cleared after submission.")
                OutlinedTextField(
                    value = token,
                    onValueChange = {
                        token = it
                        enrollmentViewModel.updateToken(it)
                    },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("Enrollment token") },
                    supportingText = { Text("Required format: sve1_ followed by 64 lowercase hexadecimal characters") },
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    singleLine = true,
                )
            } else if (presentation.canForgetLocally) {
                Text("To enroll a replacement credential, first forget the local credential. This does not revoke the server credential.")
            }
            uiState.message?.let {
                Text(
                    it,
                    color = if (uiState.state == EnrollmentStateKind.ENROLLED) {
                        MaterialTheme.colorScheme.primary
                    } else {
                        MaterialTheme.colorScheme.error
                    },
                )
            }
            when (presentation.primaryAction) {
                EnrollmentAction.SUBMIT_TOKEN -> Button(
                    onClick = enrollmentViewModel::enroll,
                    enabled = !uiState.isSubmitting && uiState.hasToken,
                ) {
                    if (uiState.isSubmitting) CircularProgressIndicator() else Text(presentation.primaryActionLabel)
                }
                EnrollmentAction.RETRY_RECOVERY -> OutlinedButton(
                    onClick = enrollmentViewModel::retryRecovery,
                    enabled = !uiState.isSubmitting,
                ) {
                    if (uiState.isSubmitting) CircularProgressIndicator() else Text(presentation.primaryActionLabel)
                }
                EnrollmentAction.NONE -> Unit
            }
            if (presentation.canForgetLocally) {
                OutlinedButton(onClick = { confirmForget = true }, enabled = !uiState.isSubmitting) {
                    Text("Forget on this device")
                }
            }
            Text("Enrollment has no automatic retry. If the result is unknown, use the trusted owner/browser recovery workflow. Libraries use the enrolled DeviceBearer only; browser cookies and CSRF are not used.", style = MaterialTheme.typography.bodySmall)
        }
    }
    if (confirmForget) {
        AlertDialog(
            onDismissRequest = { confirmForget = false },
            title = { Text("Forget local enrollment?") },
            text = { Text("This removes the credential from this device only. The server credential is not revoked, and you will need a new trusted-owner grant to enroll again.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmForget = false
                        enrollmentViewModel.forget()
                    },
                ) { Text("Forget locally") }
            },
            dismissButton = {
                TextButton(onClick = { confirmForget = false }) { Text("Cancel") }
            },
        )
    }
}

private fun enrollmentViewModelFactory(
    profile: ServerProfile,
    manager: EnrollmentManager,
    exchangeClient: EnrollmentExchangeClient,
): ViewModelProvider.Factory = object : ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <T : ViewModel> create(modelClass: Class<T>): T =
        EnrollmentViewModel(profile, manager, exchangeClient) as T
}
