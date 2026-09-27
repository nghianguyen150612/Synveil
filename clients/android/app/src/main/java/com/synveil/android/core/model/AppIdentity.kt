package com.synveil.android.core.model

data class AppIdentity(
    val displayName: String,
    val releaseChannel: String,
) {
    companion object {
        val development = AppIdentity(
            displayName = "Synveil",
            releaseChannel = "Development foundation",
        )
    }
}
