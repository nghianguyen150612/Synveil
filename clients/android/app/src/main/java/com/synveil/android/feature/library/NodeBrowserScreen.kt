package com.synveil.android.feature.library

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.library.NodeId
import com.synveil.android.data.cache.CacheRepository
import com.synveil.android.data.connectivity.ConnectivityObserver
import com.synveil.android.data.connectivity.ConnectivityStatus
import com.synveil.android.data.connectivity.UnknownConnectivityObserver
import com.synveil.android.data.node.Node
import com.synveil.android.data.node.NodeFailure
import com.synveil.android.data.node.NodeRepositoryResult
import com.synveil.android.data.session.DeviceSessionManager
import com.synveil.android.data.session.DeviceSessionState
import com.synveil.android.data.transfer.TransferOperations
import com.synveil.android.data.transfer.TransferProgress
import com.synveil.android.data.mutation.MutationEngine
import com.synveil.android.data.mutation.MutationIntent
import com.synveil.android.data.mutation.QueueResult
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class Breadcrumb(val label: String, val parentId: NodeId?)

data class NodeBrowserUiState(
    val session: DeviceSessionState = DeviceSessionState.NoProfile,
    val nodes: List<Node> = emptyList(),
    val breadcrumbs: List<Breadcrumb> = emptyList(),
    val currentParent: NodeId? = null,
    val loading: Boolean = false,
    val message: String? = null,
    val transfer: TransferProgress? = null,
    val lastSavedUri: Uri? = null,
    val lastSavedMime: String? = null,
    val showingCachedData: Boolean = false,
    val pendingOperations: Int = 0,
    val connectivity: ConnectivityStatus = ConnectivityStatus.UNKNOWN,
)

class NodeBrowserViewModel(
    private val sessionManager: DeviceSessionManager,
    private val cache: CacheRepository,
    private val libraryId: LibraryId,
    private val rootNodeId: NodeId,
    private val connectivity: ConnectivityObserver = UnknownConnectivityObserver,
) : ViewModel() {
    private val mutableState = MutableStateFlow(NodeBrowserUiState())
    val uiState: StateFlow<NodeBrowserUiState> = mutableState.asStateFlow()
    private var transferJob: Job? = null

    init {
        viewModelScope.launch {
            connectivity.status.collect { status ->
                mutableState.value = mutableState.value.copy(connectivity = status)
            }
        }
        viewModelScope.launch {
            sessionManager.state.collect { session -> mutableState.value = mutableState.value.copy(session = session) }
        }
        refresh()
    }

    fun refresh() = load(mutableState.value.currentParent, mutableState.value.breadcrumbs)

    fun openDirectory(node: Node) {
        if (node.kind != com.synveil.android.data.node.NodeKind.DIRECTORY) return
        val crumbs = mutableState.value.breadcrumbs + Breadcrumb(node.name, node.nodeId)
        load(node.nodeId, crumbs)
    }

    fun goUp() {
        val crumbs = mutableState.value.breadcrumbs
        if (crumbs.isEmpty()) return
        val parent = if (crumbs.size == 1) null else crumbs[crumbs.size - 2].parentId
        load(parent, crumbs.dropLast(1))
    }

    fun download(node: Node, context: Context, destination: Uri) {
        transferJob?.cancel()
        transferJob = viewModelScope.launch {
            val transport = sessionManager.authenticatedTransport()
                ?: return@launch setMessage("This profile is not ready for authenticated file access.")
            mutableState.value = mutableState.value.copy(transfer = TransferProgress.Preparing, message = null)
            val result = TransferOperations.downloadTo(
                transport,
                node.nodeId,
                context.contentResolver,
                destination,
            ) { progress -> mutableState.value = mutableState.value.copy(transfer = progress) }
            if (result is com.synveil.android.data.transfer.TransferResult.Saved) {
                mutableState.value = mutableState.value.copy(lastSavedUri = result.uri, lastSavedMime = "application/octet-stream")
            } else if (result is com.synveil.android.data.transfer.TransferResult.Failed) {
                setMessage("Download failed: ${transportFailureMessage(result.error)}")
            }
        }
    }

    fun upload(context: Context, source: Uri) {
        transferJob?.cancel()
        transferJob = viewModelScope.launch {
            val transport = sessionManager.authenticatedTransport()
                ?: return@launch setMessage("This profile is not ready for authenticated file access.")
            val parent = mutableState.value.currentParent ?: rootNodeId
            val name = TransferOperations.suggestedName(context.contentResolver, source, "upload")
            mutableState.value = mutableState.value.copy(transfer = TransferProgress.Preparing, message = null)
            val result = TransferOperations.uploadCreateFile(
                context,
                transport,
                context.contentResolver,
                source,
                libraryId,
                parent,
                name,
            ) { progress -> mutableState.value = mutableState.value.copy(transfer = progress) }
            when (result) {
                is com.synveil.android.data.transfer.TransferResult.Uploaded -> {
                    mutableState.value = mutableState.value.copy(message = "Uploaded $name.")
                    refresh()
                }
                is com.synveil.android.data.transfer.TransferResult.Failed -> setMessage("Upload failed: ${transportFailureMessage(result.error)}")
                is com.synveil.android.data.transfer.TransferResult.Rejected -> setMessage("Upload unavailable: ${transferRejectionMessage(result.reason)}")
                else -> Unit
            }
        }
    }

    fun replaceContent(context: Context, node: Node, source: Uri) {
        transferJob?.cancel()
        transferJob = viewModelScope.launch {
            val scope = sessionManager.authenticatedScope()
                ?: return@launch setMessage("This profile is not ready for content replacement.")
            mutableState.value = mutableState.value.copy(transfer = TransferProgress.Preparing, message = null)
            val result = TransferOperations.replaceContent(
                context, cache, scope.profileId, scope.deviceId, scope.transport, context.contentResolver,
                source, libraryId, node.nodeId, node.revision.value,
            ) { progress -> mutableState.value = mutableState.value.copy(transfer = progress) }
            when (result) {
                is com.synveil.android.data.transfer.TransferResult.Uploaded -> { setMessage("Content replacement committed; waiting for inbound sync."); refresh() }
                is com.synveil.android.data.transfer.TransferResult.Failed -> setMessage("Replacement failed: ${transportFailureMessage(result.error)}")
                is com.synveil.android.data.transfer.TransferResult.Rejected -> setMessage("Replacement unavailable: ${transferRejectionMessage(result.reason)}")
                else -> Unit
            }
        }
    }

    fun cancelTransfer() {
        transferJob?.cancel()
        transferJob = null
    }

    fun clearSaved() { mutableState.value = mutableState.value.copy(lastSavedUri = null) }

    fun createDirectory(name: String) = enqueueMutation { parent, parentRevision ->
        MutationIntent.CreateDirectory(parent, parentRevision, name.trim())
    }

    fun rename(node: Node, name: String) = enqueueMutation { _, _ ->
        MutationIntent.RenameNode(node.nodeId, node.revision.value, name.trim())
    }

    fun trash(node: Node) = enqueueMutation { _, _ -> MutationIntent.TrashNode(node.nodeId, node.revision.value) }

    fun moveToCurrentDirectory(node: Node) = enqueueMutation { parent, parentRevision ->
        if (node.parentId == parent) null else MutationIntent.MoveNode(node.nodeId, node.revision.value, parent, parentRevision)
    }

    fun restore(node: Node) = enqueueMutation { _, parentRevision ->
        val parent = node.parentId ?: return@enqueueMutation null
        MutationIntent.RestoreNode(node.nodeId, node.revision.value, parent, parentRevision)
    }

    private fun enqueueMutation(factory: suspend (NodeId, String) -> MutationIntent?) {
        viewModelScope.launch {
            val scope = sessionManager.authenticatedScope()
            val profileId = mutableState.value.session.profileIdOrNull()
            if (scope == null || profileId == null) return@launch setMessage("Enrollment is required before changing metadata.")
            val parent = mutableState.value.currentParent ?: rootNodeId
            val nodes = withContext(Dispatchers.IO) { cache.activeNodes(profileId, libraryId) }
            val parentNode = nodes.firstOrNull { it.nodeId == parent.value }
            val parentRevision = parentNode?.revision ?: return@launch setMessage("Refresh this directory before changing it.")
            val result = MutationEngine(cache, scope.transport, profileId, scope.deviceId, libraryId).enqueue(factory(parent, parentRevision) ?: return@launch)
            when (result) {
                is QueueResult.Enqueued -> { setMessage("Pending sync — the server has not changed yet."); updatePendingCount() }
                is QueueResult.Rejected -> setMessage("Change not queued: ${result.reason.name.lowercase().replace('_', ' ')}.")
            }
        }
    }

    private suspend fun updatePendingCount() {
        val scope = sessionManager.authenticatedScope() ?: return
        val count = cache.mutations(scope.profileId, scope.deviceId, libraryId).count { it.state in setOf("PENDING", "SUBMITTING", "OUTCOME_UNKNOWN", "BLOCKED_REBASELINE", "CONFLICT") }
        mutableState.value = mutableState.value.copy(pendingOperations = count)
    }

    private fun load(parent: NodeId?, breadcrumbs: List<Breadcrumb>) {
        if (mutableState.value.loading) return
        viewModelScope.launch {
            mutableState.value = mutableState.value.copy(currentParent = parent, breadcrumbs = breadcrumbs, loading = true, message = null)
            val profileId = mutableState.value.session.profileIdOrNull()
            updatePendingCount()
            val cached = profileId?.let {
                withContext(Dispatchers.IO) { cache.observeChildren(it, libraryId, parent).first() }
            }.orEmpty()
            if (cached.isNotEmpty()) {
                mutableState.value = mutableState.value.copy(nodes = cached, showingCachedData = true)
            }
            if (mutableState.value.connectivity == ConnectivityStatus.OFFLINE) {
                mutableState.value = mutableState.value.copy(
                    loading = false,
                    message = if (cached.isNotEmpty()) {
                        "Offline — showing cached metadata. Reconnect and retry."
                    } else {
                        "Offline — no cached metadata is available. Reconnect and retry."
                    },
                    showingCachedData = cached.isNotEmpty(),
                )
                return@launch
            }
            val result = withContext(Dispatchers.IO) { sessionManager.listChildren(libraryId, parent) }
            mutableState.value = when (result) {
                is NodeRepositoryResult.Loaded -> mutableState.value.copy(nodes = result.nodes, loading = false, showingCachedData = false)
                is NodeRepositoryResult.Failed -> mutableState.value.copy(loading = false, message = if (cached.isNotEmpty()) "Offline — showing cached metadata. ${nodeFailureMessage(result.failure)}" else nodeFailureMessage(result.failure), showingCachedData = cached.isNotEmpty())
            }
        }
    }

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

    private fun setMessage(value: String) { mutableState.value = mutableState.value.copy(message = value) }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NodeBrowserScreen(
    sessionManager: DeviceSessionManager,
    cache: CacheRepository,
    libraryId: LibraryId,
    rootNodeId: NodeId,
    onBack: () -> Unit,
    connectivityObserver: ConnectivityObserver = UnknownConnectivityObserver,
    viewModel: NodeBrowserViewModel = viewModel(
        factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T = NodeBrowserViewModel(sessionManager, cache, libraryId, rootNodeId, connectivityObserver) as T
        },
    ),
) {
    val state by viewModel.uiState.collectAsStateWithLifecycle()
    val context = LocalContext.current
    var pendingDownload by remember { mutableStateOf<Node?>(null) }
    var renameNode by remember { mutableStateOf<Node?>(null) }
    var trashNode by remember { mutableStateOf<Node?>(null) }
    var renameText by remember { mutableStateOf("") }
    var createFolder by remember { mutableStateOf(false) }
    var folderText by remember { mutableStateOf("") }
    var pendingReplacement by remember { mutableStateOf<Node?>(null) }
    val saveLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        pendingDownload?.let { node -> if (uri != null) viewModel.download(node, context, uri) }
        pendingDownload = null
    }
    val uploadLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) viewModel.upload(context, uri)
    }
    val replaceLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val node = pendingReplacement
        pendingReplacement = null
        if (uri != null && node != null) viewModel.replaceContent(context, node, uri)
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(state.breadcrumbs.lastOrNull()?.label ?: "Files") },
                navigationIcon = {
                    TextButton(onClick = if (state.breadcrumbs.isEmpty()) onBack else viewModel::goUp) {
                        Text(if (state.breadcrumbs.isEmpty()) "Back" else "Up")
                    }
                },
                actions = {
                    TextButton(onClick = viewModel::refresh, enabled = !state.loading) { Text("Refresh") }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(state.breadcrumbs.joinToString(" / ") { it.label }.ifEmpty { "Library root" }, style = MaterialTheme.typography.titleMedium)
            Text(
                when (state.connectivity) {
                    ConnectivityStatus.UNKNOWN -> "Network status unavailable"
                    ConnectivityStatus.ONLINE -> "Online"
                    ConnectivityStatus.OFFLINE -> "Offline — cached metadata only; retry is explicit."
                },
                style = MaterialTheme.typography.bodySmall,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = { uploadLauncher.launch(arrayOf("*/*")) }, enabled = !state.loading) { Text("Upload") }
                Button(onClick = { folderText = ""; createFolder = true }, enabled = !state.loading) { Text("New folder") }
            }
            if (state.pendingOperations > 0) Text("${state.pendingOperations} pending change(s) — server state may differ", color = MaterialTheme.colorScheme.tertiary)
            if (state.loading) CircularProgressIndicator()
            if (state.showingCachedData) Text("Offline — cached metadata", color = MaterialTheme.colorScheme.tertiary)
            state.message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (!state.loading && state.nodes.isEmpty()) Text("This folder is empty.")
            LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                items(state.nodes, key = { it.nodeId.value }) { node ->
                    NodeRow(node, onOpen = { viewModel.openDirectory(node) }, onRename = { renameNode = node; renameText = node.name }, onTrash = { trashNode = node }, onRestore = { viewModel.restore(node) }, onMove = { viewModel.moveToCurrentDirectory(node) }, onReplace = { pendingReplacement = node; replaceLauncher.launch(arrayOf("*/*")) }, onDownload = {
                        pendingDownload = node
                        saveLauncher.launch(TransferOperations.safeLogicalName(node.name))
                    })
                }
            }
            state.lastSavedUri?.let { uri ->
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { context.startActivity(TransferOperations.openIntent(uri, state.lastSavedMime)) }) { Text("Open") }
                    Button(onClick = { context.startActivity(Intent.createChooser(TransferOperations.shareIntent(uri, state.lastSavedMime), "Share file")) }) { Text("Share") }
                    TextButton(onClick = viewModel::clearSaved) { Text("Dismiss") }
                }
            }
            state.transfer?.let { progress -> Text(transferProgressLabel(progress)) }
            if (state.transfer is TransferProgress.Preparing ||
                state.transfer is TransferProgress.Downloading ||
                state.transfer is TransferProgress.Uploading ||
                state.transfer is TransferProgress.Verifying
            ) {
                TextButton(onClick = viewModel::cancelTransfer) { Text("Cancel transfer") }
            }
        }
    }
    if (createFolder) AlertDialog(
        onDismissRequest = { createFolder = false },
        title = { Text("New folder") },
        text = { OutlinedTextField(value = folderText, onValueChange = { folderText = it }, label = { Text("Name") }, singleLine = true) },
        confirmButton = { TextButton(onClick = { createFolder = false; viewModel.createDirectory(folderText) }, enabled = folderText.trim().isNotEmpty()) { Text("Queue") } },
        dismissButton = { TextButton(onClick = { createFolder = false }) { Text("Cancel") } },
    )
    renameNode?.let { node -> AlertDialog(
        onDismissRequest = { renameNode = null },
        title = { Text("Rename") },
        text = { OutlinedTextField(value = renameText, onValueChange = { renameText = it }, label = { Text("Name") }, singleLine = true) },
        confirmButton = { TextButton(onClick = { renameNode = null; viewModel.rename(node, renameText) }, enabled = renameText.trim().isNotEmpty()) { Text("Queue") } },
        dismissButton = { TextButton(onClick = { renameNode = null }) { Text("Cancel") } },
    ) }
    trashNode?.let { node -> AlertDialog(
        onDismissRequest = { trashNode = null },
        title = { Text("Move to trash?") },
        text = { Text("${node.name} will be queued for trash. The server has not changed until synchronization applies the mutation.") },
        confirmButton = {
            TextButton(onClick = { trashNode = null; viewModel.trash(node) }) { Text("Move to trash") }
        },
        dismissButton = { TextButton(onClick = { trashNode = null }) { Text("Cancel") } },
    ) }
}

@Composable
private fun NodeRow(node: Node, onOpen: () -> Unit, onRename: () -> Unit, onTrash: () -> Unit, onRestore: () -> Unit, onMove: () -> Unit, onReplace: () -> Unit, onDownload: () -> Unit) {
    Card(
        Modifier
            .fillMaxWidth()
            .sizeIn(minHeight = 48.dp)
            .semantics { contentDescription = "${node.name}, ${node.kind.name.lowercase()}, ${node.state.name.lowercase()}" },
    ) {
        Row(Modifier.fillMaxWidth().padding(12.dp), horizontalArrangement = Arrangement.SpaceBetween) {
            Column(Modifier.weight(1f)) {
                Text(node.name, style = MaterialTheme.typography.titleMedium)
                Text(node.kind.name, style = MaterialTheme.typography.bodySmall)
            }
            if (node.kind == com.synveil.android.data.node.NodeKind.DIRECTORY) TextButton(onClick = onOpen) { Text("Open") }
            else { TextButton(onClick = onDownload) { Text("Save") }; TextButton(onClick = onReplace) { Text("Replace") } }
            if (node.state == com.synveil.android.data.node.NodeState.TRASHED) TextButton(onClick = onRestore) { Text("Restore") }
            else { TextButton(onClick = onRename) { Text("Rename") }; TextButton(onClick = onMove) { Text("Move here") }; TextButton(onClick = onTrash) { Text("Trash") } }
        }
    }
}

internal fun transferProgressLabel(progress: TransferProgress): String = when (progress) {
    TransferProgress.Preparing -> "Preparing transfer…"
    is TransferProgress.Downloading -> "Downloading ${progress.transferred}${progress.total?.let { "/$it" } ?: ""} bytes"
    is TransferProgress.Uploading -> "Uploading ${progress.transferred}/${progress.total} bytes"
    TransferProgress.Verifying -> "Verifying upload…"
    TransferProgress.Completed -> "Transfer complete"
    is TransferProgress.Failed -> progress.message
    TransferProgress.Cancelled -> "Transfer cancelled"
}

internal fun nodeFailureMessage(failure: NodeFailure): String = when (failure) {
    is NodeFailure.Transport -> transportFailureMessage(failure.error)
    NodeFailure.ResourceLimit, NodeFailure.RepeatedCursor, NodeFailure.InvalidScope -> "The server returned an invalid directory page."
}

private fun transportFailureMessage(error: com.synveil.android.data.network.SynveilTransportError): String = when (error) {
    is com.synveil.android.data.network.SynveilTransportError.HttpError -> when (error.code) {
        "device_revoked" -> "This device was revoked."
        "authentication_failed" -> "Authentication is required."
        else -> "The server rejected the request (${error.statusCode})."
    }
    com.synveil.android.data.network.SynveilTransportError.Timeout,
    com.synveil.android.data.network.SynveilTransportError.Offline,
    com.synveil.android.data.network.SynveilTransportError.DnsFailure -> "The server is unavailable."
    else -> "The transfer response was invalid."
}

private fun transferRejectionMessage(reason: com.synveil.android.data.transfer.TransferRejectionReason): String = when (reason) {
    com.synveil.android.data.transfer.TransferRejectionReason.STAGING_QUOTA_EXCEEDED -> "local staging quota exceeded"
    com.synveil.android.data.transfer.TransferRejectionReason.INSUFFICIENT_STORAGE -> "free storage is too low"
    com.synveil.android.data.transfer.TransferRejectionReason.SOURCE_UNAVAILABLE -> "the selected source is unavailable"
}
