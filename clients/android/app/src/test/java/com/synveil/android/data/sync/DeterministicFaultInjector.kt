package com.synveil.android.data.sync

class SimulatedProcessDeath(val point: String) : RuntimeException(point)

class DeterministicFaultInjector(private val armedPoints: Set<String>) {
    private val observedPoints = linkedSetOf<String>()

    val points: Set<String>
        get() = observedPoints.toSet()

    fun checkpoint(point: String) {
        observedPoints += point
        if (point in armedPoints) throw SimulatedProcessDeath(point)
    }
}
