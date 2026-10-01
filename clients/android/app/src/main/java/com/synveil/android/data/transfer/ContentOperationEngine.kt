package com.synveil.android.data.transfer

import com.synveil.android.data.cache.CacheRepository
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.network.AuthenticatedSynveilTransport

class ContentOperationEngine(
    private val cache: CacheRepository,
    private val transport: AuthenticatedSynveilTransport,
    private val profileId: String,
    private val deviceId: String,
    private val libraryId: LibraryId,
) {
    suspend fun drain(maxOperations: Int = 2): Boolean {
        var transient = false
        cache.contentOperations(profileId, deviceId, libraryId)
            .filter { it.state in setOf("READY", "CREATING_SESSION", "UPLOADING", "OUTCOME_UNKNOWN", "COMPLETING") }
            .take(maxOperations)
            .forEach { operation ->
                when (val result = TransferOperations.resumeContentOperation(cache, operation, transport)) {
                    is TransferResult.Failed -> {
                        transient = result.error is com.synveil.android.data.network.SynveilTransportError.Offline ||
                            result.error is com.synveil.android.data.network.SynveilTransportError.Timeout ||
                            result.error is com.synveil.android.data.network.SynveilTransportError.DnsFailure
                    }
                    else -> Unit
                }
            }
        return transient
    }
}
