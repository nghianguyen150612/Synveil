package com.synveil.android

import android.app.Application
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.preferencesDataStoreFile
import com.synveil.android.data.profile.DataStoreServerProfileRepository
import com.synveil.android.data.network.SynveilHttpTransport
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.data.enrollment.AndroidKeystoreCredentialVault
import com.synveil.android.data.enrollment.DataStoreEnrollmentMetadataStore
import com.synveil.android.data.enrollment.EnrollmentManager
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch

class SynveilApplication : Application() {
    private val applicationScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private val enrollmentMetadataStore by lazy {
        DataStoreEnrollmentMetadataStore(
            PreferenceDataStoreFactory.create {
                applicationContext.preferencesDataStoreFile("enrollment_metadata.preferences_pb")
            },
        )
    }

    private val credentialVault by lazy {
        AndroidKeystoreCredentialVault(
            PreferenceDataStoreFactory.create {
                applicationContext.preferencesDataStoreFile("device_credentials.preferences_pb")
            },
        )
    }

    val enrollmentManager by lazy {
        EnrollmentManager(enrollmentMetadataStore, credentialVault)
    }

    val serverProfileRepository by lazy {
        DataStoreServerProfileRepository(
            dataStore = PreferenceDataStoreFactory.create {
                applicationContext.preferencesDataStoreFile("server_profiles.preferences_pb")
            },
            allowLoopbackTestHttp = BuildConfig.DEBUG,
            credentialLifecycle = enrollmentManager,
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

    override fun onCreate() {
        super.onCreate()
        applicationScope.launch {
            serverProfileRepository.state.collectLatest { state ->
                val configured = state as? com.synveil.android.data.profile.ProfileRepositoryState.Configured
                configured?.configuration?.profiles?.forEach { profile ->
                    enrollmentManager.recover(profile)
                }
            }
        }
    }
}
