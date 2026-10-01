package com.synveil.android.core.model

data class AppIdentity(
    val displayName: String,
    val releaseChannel: String,
) {
    companion object {
        val androidClient = AppIdentity(
            displayName = "Synveil",
            releaseChannel = "Android client",
        )

        val development = AppIdentity(
            displayName = "Synveil",
            releaseChannel = "Development foundation",
        )
    }
}
