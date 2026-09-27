package com.synveil.android

import android.app.Application
import androidx.datastore.preferences.preferencesDataStoreFile
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import com.synveil.android.data.profile.DataStoreServerProfileRepository

class SynveilApplication : Application() {
    val serverProfileRepository by lazy {
        DataStoreServerProfileRepository(
            dataStore = PreferenceDataStoreFactory.create {
                applicationContext.preferencesDataStoreFile("server_profiles.preferences_pb")
            },
            allowLoopbackTestHttp = BuildConfig.DEBUG,
        )
    }
}
