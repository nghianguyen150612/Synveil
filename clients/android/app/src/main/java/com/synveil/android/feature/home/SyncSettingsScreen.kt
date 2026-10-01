package com.synveil.android.feature.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.Row
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.compose.viewModel
import com.synveil.android.data.settings.SyncNetworkPolicy
import com.synveil.android.data.settings.SyncSettings
import com.synveil.android.data.settings.SyncSettingsStore
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

class SyncSettingsViewModel(private val store: SyncSettingsStore) : ViewModel() {
    private val state = MutableStateFlow(SyncSettings())
    val settings: StateFlow<SyncSettings> = state.asStateFlow()
    init { viewModelScope.launch { store.settings.collect { state.value = it } } }
    fun update(transform: (SyncSettings) -> SyncSettings) { viewModelScope.launch { store.update(transform) } }
}

@Composable
fun SyncSettingsScreen(store: SyncSettingsStore, onBack: () -> Unit, viewModel: SyncSettingsViewModel = viewModel(factory = object : ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST") override fun <T : ViewModel> create(modelClass: Class<T>): T = SyncSettingsViewModel(store) as T
})) {
    val settings by viewModel.settings.collectAsStateWithLifecycle()
    var menuOpen by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Text("Sync settings", style = MaterialTheme.typography.headlineSmall)
        TextButton(onClick = onBack) { Text("Back") }
        Text("Background sync is approximate and subject to Android scheduling.")
        RowSetting("Background sync", settings.backgroundEnabled) { viewModel.update { it.copy(backgroundEnabled = !it.backgroundEnabled) } }
        RowSetting("Battery not low", settings.batteryNotLow) { viewModel.update { it.copy(batteryNotLow = !it.batteryNotLow) } }
        Text("Network policy")
        Button(onClick = { menuOpen = true }) { Text(if (settings.networkPolicy == SyncNetworkPolicy.UNMETERED) "Unmetered only" else "Any connected network") }
        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
            DropdownMenuItem(text = { Text("Any connected network") }, onClick = { menuOpen = false; viewModel.update { it.copy(networkPolicy = SyncNetworkPolicy.CONNECTED) } })
            DropdownMenuItem(text = { Text("Unmetered only") }, onClick = { menuOpen = false; viewModel.update { it.copy(networkPolicy = SyncNetworkPolicy.UNMETERED) } })
        }
        Text("Periodic interval: ${settings.periodicMinutes} minutes minimum")
    }
}

@Composable
private fun RowSetting(label: String, checked: Boolean, onCheckedChange: () -> Unit) {
    Row(horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = { onCheckedChange() })
    }
}
