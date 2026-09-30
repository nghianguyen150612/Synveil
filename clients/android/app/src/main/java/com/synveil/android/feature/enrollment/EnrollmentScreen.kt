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
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
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
            Text("Paste the one-time grant from the trusted owner workflow. It is used only in memory and is cleared after submission.")
            OutlinedTextField(
                value = uiState.token,
                onValueChange = enrollmentViewModel::updateToken,
                modifier = Modifier.fillMaxWidth(),
                label = { Text("Enrollment token") },
                supportingText = { Text("Required format: sve1_ followed by 64 lowercase hexadecimal characters") },
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                singleLine = true,
            )
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
            Button(onClick = enrollmentViewModel::enroll, enabled = !uiState.isSubmitting && uiState.token.isNotEmpty()) {
                if (uiState.isSubmitting) CircularProgressIndicator() else Text("Exchange once")
            }
            if (uiState.state == EnrollmentStateKind.ENROLLED) {
                OutlinedButton(onClick = enrollmentViewModel::forget, enabled = !uiState.isSubmitting) {
                    Text("Forget on this device")
                }
            }
            Text("Enrollment has no automatic retry. If the result is unknown, use the trusted owner/browser recovery workflow. Libraries use the enrolled DeviceBearer only; browser cookies and CSRF are not used.", style = MaterialTheme.typography.bodySmall)
        }
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
