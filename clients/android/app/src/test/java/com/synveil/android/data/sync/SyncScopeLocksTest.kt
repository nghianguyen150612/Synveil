package com.synveil.android.data.sync

import com.synveil.android.data.library.LibraryId
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.withLock
import org.junit.Assert.assertSame
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import java.util.concurrent.atomic.AtomicInteger

class SyncScopeLocksTest {
    @Test
    fun sameScopeUsesOneSharedMutexAcrossCoordinatorInstances() {
        val library = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070811"))
        assertSame(
            SyncScopeLocks.forScope("profile-a", "device-a", library),
            SyncScopeLocks.forScope("profile-a", "device-a", library),
        )
    }

    @Test
    fun differentScopesUseDifferentMutexes() {
        val library = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070811"))
        val first = SyncScopeLocks.forScope("profile-a", "device-a", library)
        val second = SyncScopeLocks.forScope("profile-b", "device-b", library)
        assertNotSame(first, second)
    }

    @Test
    fun sameScopeNeverHasTwoMutatingEntries() = runBlocking {
        val library = checkNotNull(LibraryId.parse("018bcfe5-687b-7001-8203-040506070811"))
        val lock = SyncScopeLocks.forScope("profile-c", "device-c", library)
        val entered = CompletableDeferred<Unit>()
        val release = CompletableDeferred<Unit>()
        val active = AtomicInteger(0)
        val maximum = AtomicInteger(0)

        coroutineScope {
            val first = async {
                lock.withLock {
                    val current = active.incrementAndGet()
                    maximum.updateAndGet { value -> maxOf(value, current) }
                    entered.complete(Unit)
                    release.await()
                    active.decrementAndGet()
                }
            }
            entered.await()
            val second = async {
                lock.withLock {
                    val current = active.incrementAndGet()
                    maximum.updateAndGet { value -> maxOf(value, current) }
                    active.decrementAndGet()
                }
            }
            assertFalse(second.isCompleted)
            release.complete(Unit)
            listOf(first, second).awaitAll()
        }

        assertEquals(1, maximum.get())
        assertEquals(0, active.get())
    }
}
