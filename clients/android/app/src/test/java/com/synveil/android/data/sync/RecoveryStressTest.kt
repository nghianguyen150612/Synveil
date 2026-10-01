package com.synveil.android.data.sync

import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicInteger

class RecoveryStressTest {
    @Test
    fun syntheticWorkloadUsesBoundedPagesAndStableQueueOrder() {
        val profiles = listOf("profile-a", "profile-b")
        val libraries = listOf("library-a", "library-b", "library-c")
        val nodes = profiles.flatMap { profile ->
            libraries.flatMap { library ->
                (0 until 5_000).map { index -> "$profile/$library/node-$index" }
            }
        }
        val mutations = (0 until 1_000).map { index ->
            Triple("library-${'a' + index % 3}", index.toLong(), "mutation-%04d".format(index))
        }.sortedWith(compareBy<Triple<String, Long, String>> { it.second }.thenBy { it.third })
        val conflicts = (0 until 100).map { "conflict-%03d".format(it) }
        val changeEvents = (0 until 500).map { "event-%03d".format(it) }

        assertEquals(30_000, nodes.size)
        assertEquals(1_000, mutations.size)
        assertEquals(500, changeEvents.size)
        assertEquals(100, conflicts.size)
        assertEquals(mutations, mutations.sortedWith(compareBy<Triple<String, Long, String>> { it.second }.thenBy { it.third }))

        val appliedNodeIds = nodes.asSequence().chunked(250).flatMap { page -> page.asSequence() }.toSet()
        assertEquals(nodes.size, appliedNodeIds.size)
        assertTrue(mutations.zipWithNext().all { (first, second) -> first.second <= second.second })
    }

    @Test
    fun sameScopeMutatingLoopHasOneCriticalSection() = runBlocking {
        val scopeLock = Mutex()
        val active = AtomicInteger(0)
        val maximum = AtomicInteger(0)

        coroutineScope {
            (0 until 8).map {
                async {
                    repeat(100) {
                        scopeLock.withLock {
                            val current = active.incrementAndGet()
                            maximum.updateAndGet { value -> maxOf(value, current) }
                            active.decrementAndGet()
                        }
                    }
                }
            }.awaitAll()
        }

        assertEquals(1, maximum.get())
        assertEquals(0, active.get())
    }

    @Test
    fun faultInjectorIsDeterministicAndRecordsEveryBoundary() {
        val injector = DeterministicFaultInjector(setOf("after-send"))
        injector.checkpoint("before-send")
        try {
            injector.checkpoint("after-send")
        } catch (error: SimulatedProcessDeath) {
            assertEquals("after-send", error.point)
        }
        assertEquals(listOf("before-send", "after-send"), injector.points.toList())
    }
}
