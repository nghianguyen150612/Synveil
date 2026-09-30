package com.synveil.android.data.enrollment

import android.content.Context
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.core.edit
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.synveil.android.core.model.CanonicalServerOrigin
import com.synveil.android.core.model.ServerProfile
import com.synveil.android.core.model.ServerProfileId
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

@RunWith(AndroidJUnit4::class)
class AndroidKeystoreCredentialVaultTest {
    private val context = ApplicationProvider.getApplicationContext<Context>()
    private val file = File(context.filesDir, "prompt4-keystore-test.preferences_pb")

    @After
    fun cleanup() {
        file.delete()
        runCatching {
            java.security.KeyStore.getInstance(AndroidKeystoreCredentialVault.ANDROID_KEYSTORE).apply {
                load(null)
                deleteEntry(AndroidKeystoreCredentialVault.alias(profile().profileId.toString()))
            }
        }
    }

    @Test
    fun credentialSurvivesStoreRecreationIsEncryptedAndCanBeDeleted() = runBlocking {
        val dataStore = PreferenceDataStoreFactory.create { file }
        val scope = scope()
        val record = record()
        AndroidKeystoreCredentialVault(dataStore).store(scope, record)
        val persisted = dataStore.data.first()[AndroidKeystoreCredentialVault.recordKey(scope.profileId)]
        assertTrue(persisted != null)
        assertFalse(persisted!!.contains(record.credential.rawValue))

        val recreated = AndroidKeystoreCredentialVault(dataStore)
        assertEquals(record.credential.rawValue, recreated.load(scope.profileId, scope)!!.credential.rawValue)
        recreated.delete(scope.profileId)
        assertEquals(null, dataStore.data.first()[AndroidKeystoreCredentialVault.recordKey(scope.profileId)])
    }

    @Test
    fun wrongScopeMalformedCiphertextAndMissingKeyFailClosed() = runBlocking {
        val dataStore = PreferenceDataStoreFactory.create { file }
        val scope = scope()
        val vault = AndroidKeystoreCredentialVault(dataStore)
        vault.store(scope, record())

        val wrongScope = scope.copy(canonicalBaseUrl = "https://other.example.com/")
        assertEquals(CredentialVaultException.ScopeMismatch, runCatching { vault.load(scope.profileId, wrongScope) }.exceptionOrNull()?.let { (it as CredentialVaultFailure).reason })

        dataStore.edit { it[AndroidKeystoreCredentialVault.recordKey(scope.profileId)] = "{}" }
        assertEquals(CredentialVaultException.Corrupt, runCatching { vault.load(scope.profileId, scope) }.exceptionOrNull()?.let { (it as CredentialVaultFailure).reason })

        vault.store(scope, record())
        java.security.KeyStore.getInstance(AndroidKeystoreCredentialVault.ANDROID_KEYSTORE).apply {
            load(null)
            deleteEntry(AndroidKeystoreCredentialVault.alias(scope.profileId))
        }
        assertEquals(CredentialVaultException.Missing, runCatching { vault.load(scope.profileId, scope) }.exceptionOrNull()?.let { (it as CredentialVaultFailure).reason })
    }

    private fun profile() = ServerProfile.create(
        profileId = ServerProfileId.parse("018bcfe5-687b-7001-8203-040506070809"),
        displayLabel = "Test",
        canonicalBaseUrl = CanonicalServerOrigin.parse("https://example.com", false),
        createdAt = 1_700_000_000_000L,
    )

    private fun scope() = CredentialScope(
        profileId = profile().profileId.toString(),
        canonicalBaseUrl = profile().canonicalBaseUrl.value,
        transportPolicy = profile().transportPolicy.name,
        ownerUserId = "018bcfe5-687b-7001-8203-040506070810",
        deviceId = "018bcfe5-687b-7001-8203-040506070811",
        credentialId = "018bcfe5-687b-7001-8203-040506070812",
    )

    private fun record() = DeviceCredentialRecord(
        ownerUserId = "018bcfe5-687b-7001-8203-040506070810",
        deviceId = "018bcfe5-687b-7001-8203-040506070811",
        credentialId = "018bcfe5-687b-7001-8203-040506070812",
        credential = checkNotNull(DeviceCredential.parse("svd1_" + "b".repeat(64))),
        createdAt = "2026-09-30T00:00:00Z",
        requestId = null,
    )
}
