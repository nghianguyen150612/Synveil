package com.synveil.android.feature.profile

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.data.profile.ServerProfileRepository
import com.synveil.android.data.network.SynveilHttpTransport
import com.synveil.android.data.network.SynveilTransportError

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServerProfilesScreen(
    repository: ServerProfileRepository,
    transportFactory: (ServerProfile) -> SynveilHttpTransport,
    onBack: () -> Unit,
    onAdd: () -> Unit,
    onEdit: (ServerProfileId) -> Unit,
    onEnroll: (ServerProfileId) -> Unit,
    profilesViewModel: ServerProfilesViewModel = viewModel(
        factory = profilesViewModelFactory(repository, transportFactory),
    ),
) {
    val uiState by profilesViewModel.uiState.collectAsStateWithLifecycle()
    var profileToRemove by remember { mutableStateOf<ServerProfile?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Server profiles") },
                navigationIcon = { TextButton(onClick = onBack) { Text("Back") } },
                actions = { TextButton(onClick = onAdd) { Text("Add") } },
            )
        },
    ) { paddingValues ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
                .padding(horizontal = 16.dp),
        ) {
            uiState.errorMessage?.let { message ->
                Text(
                    text = message,
                    modifier = Modifier.padding(vertical = 12.dp),
                    color = MaterialTheme.colorScheme.error,
                )
            }
            if (uiState.isLoading) {
                CircularProgressIndicator(modifier = Modifier.padding(24.dp))
            } else if (uiState.profiles.isEmpty()) {
                Column(
                    modifier = Modifier.padding(top = 24.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text("No server configured", style = MaterialTheme.typography.titleLarge)
                    Text("Add a Synveil server origin to configure this app.")
                    Button(onClick = onAdd) { Text("Add server profile") }
                }
            } else {
                LazyColumn(
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                    modifier = Modifier.padding(top = 12.dp),
                ) {
                    items(uiState.profiles, key = { it.profileId.toString() }) { profile ->
                        ProfileCard(
                            profile = profile,
                            isActive = profile.profileId == uiState.activeProfileId,
                            connectionState = uiState.connectionStates[profile.profileId] ?: ConnectionUiState.Idle,
                            onSelect = { profilesViewModel.select(profile.profileId) },
                            onEdit = { onEdit(profile.profileId) },
                            onEnroll = { onEnroll(profile.profileId) },
                            onRemove = { profileToRemove = profile },
                            onTestConnection = { profilesViewModel.testConnection(profile.profileId) },
                        )
                    }
                }
            }
        }
    }

    profileToRemove?.let { profile ->
        AlertDialog(
            onDismissRequest = { profileToRemove = null },
            title = { Text("Remove server profile?") },
            text = { Text("Remove ${profile.displayLabel} from this device? Any local credential is fenced before removal; server revocation is not claimed.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        profilesViewModel.remove(profile.profileId)
                        profileToRemove = null
                    },
                ) { Text("Remove") }
            },
            dismissButton = {
                TextButton(onClick = { profileToRemove = null }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun ProfileCard(
    profile: ServerProfile,
    isActive: Boolean,
    connectionState: ConnectionUiState,
    onSelect: () -> Unit,
    onEdit: () -> Unit,
    onEnroll: () -> Unit,
    onRemove: () -> Unit,
    onTestConnection: () -> Unit,
) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(profile.displayLabel, style = MaterialTheme.typography.titleMedium)
            Text(profile.canonicalBaseUrl.value, style = MaterialTheme.typography.bodyMedium)
            Text(
                text = if (isActive) {
                    "Active profile · Server configured"
                } else {
                    "Server configured · ${profile.transportPolicy.name}"
                },
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.primary,
            )
            ConnectionStatusText(connectionState)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (!isActive) {
                    OutlinedButton(onClick = onSelect) { Text("Use") }
                }
                OutlinedButton(onClick = onEdit) { Text("Edit") }
                OutlinedButton(onClick = onEnroll) { Text("Enroll") }
                OutlinedButton(
                    onClick = onTestConnection,
                    enabled = connectionState != ConnectionUiState.Checking,
                ) {
                    if (connectionState == ConnectionUiState.Checking) {
                        CircularProgressIndicator()
                    } else {
                        Text("Test connection")
                    }
                }
                TextButton(onClick = onRemove) { Text("Remove") }
            }
        }
    }
}

@Composable
private fun ConnectionStatusText(state: ConnectionUiState) {
    val message = when (state) {
        ConnectionUiState.Idle -> "Connection not tested"
        ConnectionUiState.Checking -> "Checking server…"
        is ConnectionUiState.Ready -> if (state.requestIds.isEmpty()) {
            "Ready"
        } else {
            "Ready · request ID available"
        }
        is ConnectionUiState.AliveButNotReady -> "Server is running but not ready"
        is ConnectionUiState.Failed -> connectionErrorMessage(state.error)
    }
    Text(
        text = message,
        style = MaterialTheme.typography.bodySmall,
        color = if (state is ConnectionUiState.Failed) {
            MaterialTheme.colorScheme.error
        } else {
            MaterialTheme.colorScheme.onSurfaceVariant
        },
    )
}

private fun connectionErrorMessage(error: SynveilTransportError): String = when (error) {
    SynveilTransportError.Offline -> "Server unavailable"
    SynveilTransportError.DnsFailure -> "Server address could not be resolved"
    SynveilTransportError.Timeout -> "Connection timed out"
    SynveilTransportError.TlsError -> "TLS verification failed"
    is SynveilTransportError.RedirectRejected -> "Server redirect was rejected"
    is SynveilTransportError.HttpError -> "Server returned HTTP ${error.statusCode}"
    SynveilTransportError.BodyLimitExceeded -> "Server response was too large"
    is SynveilTransportError.UnexpectedContentType -> "Invalid Synveil response type"
    SynveilTransportError.MalformedResponse -> "Invalid Synveil response"
    is SynveilTransportError.ProtocolError -> "Invalid Synveil response"
    SynveilTransportError.ConfigurationError -> "Server configuration is unavailable"
    SynveilTransportError.Cancelled -> "Connection check cancelled"
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProfileEditorScreen(
    repository: ServerProfileRepository,
    profileId: ServerProfileId?,
    onBack: () -> Unit,
    onSaved: () -> Unit,
    editorViewModel: ProfileEditorViewModel = viewModel(
        key = profileId?.toString() ?: "new",
        factory = profileEditorViewModelFactory(repository, profileId),
    ),
) {
    val uiState by editorViewModel.uiState.collectAsStateWithLifecycle()

    if (uiState.saved) {
        LaunchedEffect(Unit) { onSaved() }
        return
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(if (profileId == null) "Add server profile" else "Edit server profile") },
                navigationIcon = { TextButton(onClick = onBack) { Text("Back") } },
            )
        },
    ) { paddingValues ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            OutlinedTextField(
                value = uiState.displayLabel,
                onValueChange = editorViewModel::updateDisplayLabel,
                modifier = Modifier.fillMaxWidth(),
                label = { Text("Display name") },
                singleLine = true,
            )
            OutlinedTextField(
                value = uiState.rawBaseUrl,
                onValueChange = editorViewModel::updateBaseUrl,
                modifier = Modifier.fillMaxWidth(),
                label = { Text("Server URL") },
                supportingText = { Text("Use an HTTPS origin, such as https://synveil.example.com/") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                singleLine = true,
            )
            uiState.errorMessage?.let { message ->
                Text(text = message, color = MaterialTheme.colorScheme.error)
            }
            Button(
                onClick = editorViewModel::save,
                enabled = uiState.isReady && !uiState.isSaving,
            ) {
                if (uiState.isSaving) {
                    CircularProgressIndicator()
                } else {
                    Text("Save profile")
                }
            }
            Text(
                "Profiles store only non-secret configuration. Connection checks use unauthenticated health probes; authenticated library access uses the secure device vault.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

private fun profilesViewModelFactory(
    repository: ServerProfileRepository,
    transportFactory: (ServerProfile) -> SynveilHttpTransport,
): ViewModelProvider.Factory =
    object : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T =
            ServerProfilesViewModel(repository, transportFactory) as T
    }

private fun profileEditorViewModelFactory(
    repository: ServerProfileRepository,
    profileId: ServerProfileId?,
): ViewModelProvider.Factory = object : ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <T : ViewModel> create(modelClass: Class<T>): T =
        ProfileEditorViewModel(repository, profileId) as T
}
