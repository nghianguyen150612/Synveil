package com.synveil.android.feature.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.clickable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.synveil.android.data.library.Library
import com.synveil.android.data.library.LibraryFailure
import com.synveil.android.data.library.LibraryRepositoryResult
import com.synveil.android.data.session.DeviceSessionManager
import com.synveil.android.data.session.DeviceSessionState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class LibraryUiState(
    val session: DeviceSessionState = DeviceSessionState.NoProfile,
    val libraries: List<Library> = emptyList(),
    val isLoading: Boolean = false,
    val message: String? = null,
)

class LibraryViewModel(
    private val sessionManager: DeviceSessionManager,
) : ViewModel() {
    private val mutableState = MutableStateFlow(LibraryUiState())
    val uiState: StateFlow<LibraryUiState> = mutableState.asStateFlow()

    init {
        viewModelScope.launch {
            sessionManager.state.collect { session ->
                mutableState.value = mutableState.value.copy(session = session)
            }
        }
        refresh()
    }

    fun refresh() {
        if (mutableState.value.isLoading) return
        viewModelScope.launch {
            mutableState.value = mutableState.value.copy(isLoading = true, message = null)
            val result = withContext(Dispatchers.IO) { sessionManager.listLibraries() }
            mutableState.value = when (result) {
                is LibraryRepositoryResult.Loaded -> mutableState.value.copy(
                    libraries = result.libraries,
                    isLoading = false,
                    message = if (result.libraries.isEmpty()) "No libraries are available." else null,
                )
                is LibraryRepositoryResult.Failed -> mutableState.value.copy(
                    isLoading = false,
                    message = failureMessage(result.failure),
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(
    sessionManager: DeviceSessionManager,
    onBack: () -> Unit,
    onOpenLibrary: (Library) -> Unit,
    viewModel: LibraryViewModel = viewModel(
        factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T =
                LibraryViewModel(sessionManager) as T
        },
    ),
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Libraries") },
                navigationIcon = { TextButton(onClick = onBack) { Text("Back") } },
                actions = {
                    TextButton(onClick = viewModel::refresh, enabled = !uiState.isLoading) {
                        Text("Refresh")
                    }
                },
            )
        },
    ) { paddingValues ->
        Column(
            modifier = Modifier.fillMaxSize().padding(paddingValues).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            SessionStatus(uiState.session)
            if (uiState.isLoading) CircularProgressIndicator()
            uiState.message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (!uiState.isLoading && uiState.message == "No libraries are available.") {
                Text("The authenticated owner catalog is empty.")
            } else {
                LazyColumn(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    items(uiState.libraries, key = { it.id.value }) { library ->
                        LibraryRow(library, onClick = { onOpenLibrary(library) })
                    }
                }
            }
            Button(onClick = viewModel::refresh, enabled = !uiState.isLoading) {
                Text("Load libraries")
            }
            Text(
                "Open a library to browse its logical folders and files.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun SessionStatus(state: DeviceSessionState) {
    val message = when (state) {
        DeviceSessionState.NoProfile -> "Configure a server profile first."
        is DeviceSessionState.ProfileAvailable -> "Ready to check device enrollment for ${state.displayLabel}."
        is DeviceSessionState.NotEnrolled -> "This profile is configured but this device is not enrolled."
        is DeviceSessionState.LoadingCredential -> "Loading the secure device credential…"
        is DeviceSessionState.Ready -> "Authenticated as the enrolled device."
        is DeviceSessionState.AuthenticationRequired -> "The device credential was not accepted. Use the recovery workflow."
        is DeviceSessionState.DeviceRevoked -> "This device credential was revoked by the server."
        is DeviceSessionState.ServerUnavailable -> "The server is unavailable. Enrollment was not changed."
        is DeviceSessionState.TlsError -> "TLS verification failed."
        is DeviceSessionState.SecureStoreUnavailable -> "Secure credential storage is unavailable."
        is DeviceSessionState.RecoveryRequired -> "Enrollment recovery is required before authenticated access."
        is DeviceSessionState.ProtocolError -> "The server returned an invalid Synveil response."
    }
    Text(message, style = MaterialTheme.typography.bodyMedium)
}

@Composable
private fun LibraryRow(library: Library, onClick: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth().clickable(onClick = onClick)) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column {
                Text(library.name, style = MaterialTheme.typography.titleMedium)
                Text(library.status.name, style = MaterialTheme.typography.bodySmall)
            }
            Text(library.updatedAt.toString(), style = MaterialTheme.typography.bodySmall)
        }
    }
}

private fun failureMessage(failure: LibraryFailure): String = when (failure) {
    is LibraryFailure.Transport -> "Library request failed."
    LibraryFailure.NoActiveProfile -> "Configure an active server profile first."
    LibraryFailure.ResourceLimit -> "The library catalog exceeded the client safety limit."
    LibraryFailure.RepeatedCursor -> "The server returned an invalid pagination cursor."
}
