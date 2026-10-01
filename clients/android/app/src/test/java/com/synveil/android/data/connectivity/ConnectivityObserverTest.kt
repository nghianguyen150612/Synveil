package com.synveil.android.data.connectivity

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Test

class ConnectivityObserverTest {
    @Test
    fun statusPresentationDistinguishesOnlineOfflineAndUnknown() {
        assertEquals("Online", connectivityStatusMessage(ConnectivityStatus.ONLINE))
        assertEquals(
            "Offline — cached metadata remains available; retry is explicit.",
            connectivityStatusMessage(ConnectivityStatus.OFFLINE),
        )
        assertEquals(
            "Network status is unavailable; refresh uses bounded transport failures.",
            connectivityStatusMessage(ConnectivityStatus.UNKNOWN),
        )
    }

    @Test
    fun observerConsumersCanHandleOfflineToOnlineTransitions() = runBlocking {
        val statuses = MutableStateFlow(ConnectivityStatus.OFFLINE)
        val observer = object : ConnectivityObserver {
            override val status = statuses
        }

        assertEquals(ConnectivityStatus.OFFLINE, observer.status.first())
        statuses.value = ConnectivityStatus.ONLINE
        assertEquals(ConnectivityStatus.ONLINE, observer.status.first())
    }
}
