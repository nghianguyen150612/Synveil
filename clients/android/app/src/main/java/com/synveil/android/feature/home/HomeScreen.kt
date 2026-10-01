package com.synveil.android.feature.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.compose.viewModel
import com.synveil.android.R
import com.synveil.android.data.profile.ServerProfileRepository
import com.synveil.android.data.session.DeviceSessionManager
import com.synveil.android.feature.startup.StartupPrimaryAction
import com.synveil.android.feature.startup.StartupViewModel

@Composable
fun HomeScreen(
    repository: ServerProfileRepository,
    sessionManager: DeviceSessionManager,
    onOpenProfiles: () -> Unit,
    onOpenActiveProfileEnrollment: () -> Unit,
    onOpenLibraries: () -> Unit,
    onOpenSyncSettings: () -> Unit,
    startupViewModel: StartupViewModel = viewModel(
        factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return StartupViewModel(repository, sessionManager) as T
            }
        },
    ),
) {
    val uiState by startupViewModel.uiState.collectAsStateWithLifecycle()

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(
            text = uiState.identity.displayName,
            style = MaterialTheme.typography.headlineLarge,
        )
        Text(
            text = uiState.identity.releaseChannel,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.primary,
        )
        Card {
            Column(modifier = Modifier.padding(20.dp)) {
                Text(
                    text = uiState.statusMessage,
                    style = MaterialTheme.typography.titleMedium,
                )
                Text(
                    text = uiState.productBoundaryMessage,
                    modifier = Modifier.padding(top = 8.dp),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
        Button(
            onClick = when (uiState.primaryAction) {
                StartupPrimaryAction.CONFIGURE_PROFILE,
                StartupPrimaryAction.MANAGE_PROFILE,
                    -> onOpenProfiles
                StartupPrimaryAction.ENROLL_DEVICE,
                StartupPrimaryAction.RECOVER_ENROLLMENT,
                StartupPrimaryAction.REENROLL_DEVICE,
                    -> onOpenActiveProfileEnrollment
                StartupPrimaryAction.OPEN_LIBRARIES -> onOpenLibraries
            },
        ) {
            Text(text = uiState.primaryActionLabel)
        }
        OutlinedButton(onClick = onOpenProfiles) {
            Text(text = "Manage server profiles")
        }
        OutlinedButton(onClick = onOpenLibraries) {
            Text(text = "Open libraries")
        }
        OutlinedButton(onClick = onOpenSyncSettings) {
            Text(text = "Sync settings")
        }
        Text(
            text = stringResource(R.string.foundation_boundary_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
