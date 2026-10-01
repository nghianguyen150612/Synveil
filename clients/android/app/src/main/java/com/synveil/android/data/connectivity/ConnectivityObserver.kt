package com.synveil.android.data.connectivity

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flowOf

enum class ConnectivityStatus {
    UNKNOWN,
    ONLINE,
    OFFLINE,
}

interface ConnectivityObserver {
    val status: Flow<ConnectivityStatus>
}

object UnknownConnectivityObserver : ConnectivityObserver {
    override val status: Flow<ConnectivityStatus> = flowOf(ConnectivityStatus.UNKNOWN)
}

class SystemConnectivityObserver(context: Context) : ConnectivityObserver {
    private val connectivityManager = context.getSystemService(ConnectivityManager::class.java)

    override val status: Flow<ConnectivityStatus> = callbackFlow {
        trySend(connectivityManager.currentStatus())
        var registered = false
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                trySend(connectivityManager.statusFor(network))
            }

            override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) {
                trySend(capabilities.toConnectivityStatus())
            }

            override fun onLost(network: Network) {
                trySend(connectivityManager.currentStatus())
            }
        }
        try {
            connectivityManager.registerDefaultNetworkCallback(callback)
            registered = true
        } catch (_: SecurityException) {
            trySend(ConnectivityStatus.UNKNOWN)
        }
        awaitClose {
            if (registered) connectivityManager.unregisterNetworkCallback(callback)
        }
    }.distinctUntilChanged()
}

internal fun connectivityStatusMessage(status: ConnectivityStatus): String = when (status) {
    ConnectivityStatus.UNKNOWN -> "Network status is unavailable; refresh uses bounded transport failures."
    ConnectivityStatus.ONLINE -> "Online"
    ConnectivityStatus.OFFLINE -> "Offline — cached metadata remains available; retry is explicit."
}

private fun ConnectivityManager.currentStatus(): ConnectivityStatus = runCatching {
    activeNetwork?.let(::statusFor) ?: ConnectivityStatus.OFFLINE
}.getOrDefault(ConnectivityStatus.UNKNOWN)

private fun ConnectivityManager.statusFor(network: Network): ConnectivityStatus = runCatching {
    getNetworkCapabilities(network)?.toConnectivityStatus() ?: ConnectivityStatus.OFFLINE
}.getOrDefault(ConnectivityStatus.UNKNOWN)

private fun NetworkCapabilities.toConnectivityStatus(): ConnectivityStatus = if (
    hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
    hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
) {
    ConnectivityStatus.ONLINE
} else {
    ConnectivityStatus.OFFLINE
}
