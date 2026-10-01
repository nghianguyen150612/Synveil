package com.synveil.android.data.settings

import org.junit.Assert.assertEquals
import org.junit.Test

class SyncSettingsTest {
    @Test
    fun defaultPeriodicIntervalMeetsWorkManagerMinimum() {
        assertEquals(15, SyncSettings().periodicMinutes)
    }
}
