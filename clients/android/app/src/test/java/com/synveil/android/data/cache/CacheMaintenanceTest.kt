package com.synveil.android.data.cache

import org.junit.Assert.assertEquals
import org.junit.Test

class CacheMaintenanceTest {
    @Test
    fun classifiesRecoverableStorageFailuresWithoutExposingDetails() {
        assertEquals(CacheRecoveryState.LOW_DISK_SPACE, CacheRecoveryStateClassifier.classify(IllegalStateException("write failed"), 1024L))
        assertEquals(CacheRecoveryState.STORAGE_LOCKED, CacheRecoveryStateClassifier.classify(IllegalStateException("database is locked"), null))
        assertEquals(CacheRecoveryState.MIGRATION_FAILED, CacheRecoveryStateClassifier.classify(IllegalStateException("Migration didn't properly handle"), null))
        assertEquals(CacheRecoveryState.CORRUPT, CacheRecoveryStateClassifier.classify(IllegalStateException("file is not a database"), null))
        assertEquals(CacheRecoveryState.MAINTENANCE_FAILED, CacheRecoveryStateClassifier.classify(IllegalStateException("unexpected failure"), null))
    }

    @Test
    fun maintenanceLimitsAreBoundedAndPositive() {
        assertEquals(500, CacheMaintenancePolicy.DEFAULT_BATCH_LIMIT)
        assertEquals(500, CacheMaintenancePolicy.batchLimit(0))
        assertEquals(100, CacheMaintenancePolicy.batchLimit(100))
        assertEquals(500, CacheMaintenancePolicy.batchLimit(1000))
    }
}
