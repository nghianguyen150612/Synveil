package com.synveil.android.feature.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
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
import com.synveil.android.feature.startup.StartupViewModel

@Composable
fun HomeScreen(
    repository: ServerProfileRepository,
    onOpenProfiles: () -> Unit,
    startupViewModel: StartupViewModel = viewModel(
        factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return StartupViewModel(repository) as T
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
                    text = uiState.unsupportedFeaturesMessage,
                    modifier = Modifier.padding(top = 8.dp),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
        Button(onClick = onOpenProfiles) {
            Text(text = "Manage server profiles")
        }
        Text(
            text = stringResource(R.string.foundation_boundary_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
