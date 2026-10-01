package com.synveil.android.feature.library

import android.content.Context
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
import androidx.compose.runtime.LaunchedEffect
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
import com.synveil.android.data.cache.CacheRepository
import com.synveil.android.data.session.DeviceSessionManager
import com.synveil.android.data.session.DeviceSessionState
import com.synveil.android.work.SyncWorkScheduler
import com.synveil.android.SynveilApplication
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class LibraryUiState(
    val session: DeviceSessionState = DeviceSessionState.NoProfile,
    val libraries: List<Library> = emptyList(),
    val isLoading: Boolean = false,
    val message: String? = null,
    val showingCachedData: Boolean = false,
    val syncStates: Map<String, String> = emptyMap(),
)

private fun DeviceSessionState.profileIdOrNull(): String? = when (this) {
    is DeviceSessionState.ProfileAvailable -> profileId
    is DeviceSessionState.NotEnrolled -> profileId
    is DeviceSessionState.LoadingCredential -> profileId
    is DeviceSessionState.Ready -> profileId
    is DeviceSessionState.AuthenticationRequired -> profileId
    is DeviceSessionState.DeviceRevoked -> profileId
    is DeviceSessionState.ServerUnavailable -> profileId
    is DeviceSessionState.TlsError -> profileId
    is DeviceSessionState.SecureStoreUnavailable -> profileId
    is DeviceSessionState.RecoveryRequired -> profileId
    is DeviceSessionState.ProtocolError -> profileId
    DeviceSessionState.NoProfile -> null
}

class LibraryViewModel(
    private val sessionManager: DeviceSessionManager,
    private val cache: CacheRepository,
) : ViewModel() {
    private val mutableState = MutableStateFlow(LibraryUiState())
    val uiState: StateFlow<LibraryUiState> = mutableState.asStateFlow()
    private var cacheJob: Job? = null

    init {
        viewModelScope.launch {
            sessionManager.state.collectLatest { session ->
                mutableState.value = mutableState.value.copy(session = session)
                cacheJob?.cancel()
                val profileId = session.profileIdOrNull()
                if (profileId != null) {
                    cacheJob = launch {
                        cache.observeLibraries(profileId).collect { libraries ->
                            if (libraries.isNotEmpty() && !mutableState.value.isLoading) {
                                mutableState.value = mutableState.value.copy(
                                    libraries = libraries,
                                    showingCachedData = true,
                                )
                            }
                        }
                    }
                    launch {
                        cache.observeSyncStates(profileId).collect { states ->
                            mutableState.value = mutableState.value.copy(
                                syncStates = states.associate { it.libraryId to it.state },
                            )
                        }
                    }
                }
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
                    showingCachedData = false,
                )
                is LibraryRepositoryResult.Failed -> mutableState.value.copy(
                    isLoading = false,
                    message = if (mutableState.value.libraries.isNotEmpty()) {
                        "Offline — showing cached libraries. ${failureMessage(result.failure)}"
                    } else {
                        failureMessage(result.failure)
                    },
                    showingCachedData = mutableState.value.libraries.isNotEmpty(),
                )
            }
        }
    }

    fun syncNow(context: Context, library: Library) {
        val profileId = (mutableState.value.session as? DeviceSessionState.Ready)?.profileId
            ?: (mutableState.value.session as? DeviceSessionState.ProfileAvailable)?.profileId
            ?: return
        SyncWorkScheduler.enqueueNow(context, profileId, library.id)
        mutableState.value = mutableState.value.copy(message = "Sync queued for ${library.name}.")
    }

}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(
    sessionManager: DeviceSessionManager,
    cache: CacheRepository,
    onBack: () -> Unit,
    onOpenLibrary: (Library) -> Unit,
    viewModel: LibraryViewModel = viewModel(
        factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T =
                LibraryViewModel(sessionManager, cache) as T
        },
    ),
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val context = androidx.compose.ui.platform.LocalContext.current
    LaunchedEffect(uiState.session, uiState.libraries) {
        val profileId = uiState.session.profileIdOrNull()
        if (profileId != null) {
            val settings = (context.applicationContext as SynveilApplication).syncSettingsStore.settings.first()
            uiState.libraries.forEach { library ->
                if (settings.backgroundEnabled) {
                    SyncWorkScheduler.enqueuePeriodic(context, profileId, library.id, settings.periodicMinutes, settings.networkPolicy, settings.batteryNotLow)
                }
            }
        }
    }
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
            if (uiState.showingCachedData) {
                Text("Offline — cached metadata", color = MaterialTheme.colorScheme.tertiary)
            }
            if (uiState.isLoading) CircularProgressIndicator()
            uiState.message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (!uiState.isLoading && uiState.message == "No libraries are available.") {
                Text("The authenticated owner catalog is empty.")
            } else {
                LazyColumn(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    items(uiState.libraries, key = { it.id.value }) { library ->
                        LibraryRow(
                            library,
                            syncState = uiState.syncStates[library.id.value],
                            onClick = { onOpenLibrary(library) },
                            onSync = { viewModel.syncNow(context, library) },
                        )
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
private fun LibraryRow(library: Library, syncState: String?, onClick: () -> Unit, onSync: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth().clickable(onClick = onClick)) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column {
                Text(library.name, style = MaterialTheme.typography.titleMedium)
                Text(library.status.name, style = MaterialTheme.typography.bodySmall)
                Text(syncStateLabel(syncState), style = MaterialTheme.typography.bodySmall)
            }
            Column {
                Text(library.updatedAt.toString(), style = MaterialTheme.typography.bodySmall)
                TextButton(onClick = onSync) { Text("Sync now") }
            }
        }
    }
}

private fun syncStateLabel(value: String?): String = when (value) {
    null, "UNINITIALIZED" -> "Never synced"
    "READY" -> "Up to date"
    "SYNCING" -> "Syncing"
    "REBASELINE_REQUIRED" -> "Rebaseline required"
    "REBASELINING" -> "Rebaselining"
    "PAUSED_AUTH" -> "Authentication required"
    "ERROR_TRANSIENT" -> "Offline / retry pending"
    else -> "Sync error"
}

private fun failureMessage(failure: LibraryFailure): String = when (failure) {
    is LibraryFailure.Transport -> "Library request failed."
    LibraryFailure.NoActiveProfile -> "Configure an active server profile first."
    LibraryFailure.ResourceLimit -> "The library catalog exceeded the client safety limit."
    LibraryFailure.RepeatedCursor -> "The server returned an invalid pagination cursor."
}
