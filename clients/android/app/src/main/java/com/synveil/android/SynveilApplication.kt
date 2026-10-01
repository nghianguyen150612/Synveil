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
import com.synveil.android.data.enrollment.CredentialCleanupResult
import com.synveil.android.data.enrollment.CredentialLifecycle
import com.synveil.android.data.session.DeviceSessionManager
import com.synveil.android.data.cache.SynveilCacheDatabase
import com.synveil.android.data.cache.CacheRepository
import com.synveil.android.data.sync.SyncCoordinator
import com.synveil.android.work.SyncWorkScheduler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

class SynveilApplication : Application() {
    private val applicationScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    val cacheDatabase by lazy { SynveilCacheDatabase.create(applicationContext) }
    val cacheRepository by lazy { CacheRepository(cacheDatabase.cacheDao()) }

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
        EnrollmentManager(
            enrollmentMetadataStore,
            credentialVault,
            onFenced = { profileId ->
                cacheRepository.clearProfile(profileId)
                SyncWorkScheduler.cancelProfile(this, profileId)
            },
        )
    }

    private val cacheAwareCredentialLifecycle by lazy {
        object : CredentialLifecycle {
            override suspend fun fence(profileId: String): CredentialCleanupResult {
                val result = enrollmentManager.fence(profileId)
                return result
            }
        }
    }

    val serverProfileRepository by lazy {
        DataStoreServerProfileRepository(
            dataStore = PreferenceDataStoreFactory.create {
                applicationContext.preferencesDataStoreFile("server_profiles.preferences_pb")
            },
            allowLoopbackTestHttp = BuildConfig.DEBUG,
            credentialLifecycle = cacheAwareCredentialLifecycle,
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

    val deviceSessionManager by lazy {
        DeviceSessionManager(
            profileRepository = serverProfileRepository,
            metadataStore = enrollmentMetadataStore,
            vault = credentialVault,
            userAgent = "Synveil Android/${BuildConfig.VERSION_NAME}",
            allowLoopbackTestHttp = BuildConfig.DEBUG,
            scope = applicationScope,
            cache = cacheRepository,
        )
    }

    val syncCoordinator by lazy {
        SyncCoordinator(
            profileRepository = serverProfileRepository,
            metadataStore = enrollmentMetadataStore,
            vault = credentialVault,
            cache = cacheRepository,
            userAgent = "Synveil Android/${BuildConfig.VERSION_NAME}",
            allowLoopbackTestHttp = BuildConfig.DEBUG,
        )
    }

}
