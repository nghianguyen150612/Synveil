package com.synveil.android

import android.app.Application
import androidx.datastore.preferences.preferencesDataStoreFile
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import com.synveil.android.data.profile.DataStoreServerProfileRepository
import com.synveil.android.data.network.SynveilHttpTransport
import com.synveil.android.core.model.ServerProfile

class SynveilApplication : Application() {
    val serverProfileRepository by lazy {
        DataStoreServerProfileRepository(
            dataStore = PreferenceDataStoreFactory.create {
                applicationContext.preferencesDataStoreFile("server_profiles.preferences_pb")
            },
            allowLoopbackTestHttp = BuildConfig.DEBUG,
        )
    }

    val transportFactory: (ServerProfile) -> SynveilHttpTransport by lazy {
        { profile ->
            SynveilHttpTransport(
                profile = profile,
                userAgent = "Synveil Android/${BuildConfig.VERSION_NAME}",
                allowLoopbackTestHttp = BuildConfig.DEBUG,
            )
        }
    }
}
