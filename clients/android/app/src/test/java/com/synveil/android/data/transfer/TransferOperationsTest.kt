package com.synveil.android.data.transfer

import org.junit.Assert.assertEquals
import org.junit.Test

class TransferOperationsTest {
    @Test
    fun sanitizesLogicalNamesAndBuildsContentUriIntents() {
        assertEquals("report.txt", TransferOperations.safeLogicalName("../report.txt"))
        assertEquals("report.txt", TransferOperations.safeLogicalName("folder\\report.txt"))
        assertEquals("download", TransferOperations.safeLogicalName("../"))
    }
}
