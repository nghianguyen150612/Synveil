package com.synveil.android.data.enrollment

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.spec.GCMParameterSpec
import kotlinx.coroutines.flow.first
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.util.Base64

sealed interface CredentialVaultException {
    data object Unavailable : CredentialVaultException
    data object Missing : CredentialVaultException
    data object Corrupt : CredentialVaultException
    data object ScopeMismatch : CredentialVaultException
}

class CredentialVaultFailure(
    val reason: CredentialVaultException,
) : IllegalStateException(reason.toString())

interface SecureCredentialVault {
    suspend fun preflight(profileId: String)
    suspend fun store(scope: CredentialScope, record: DeviceCredentialRecord)
    suspend fun load(profileId: String, expectedScope: CredentialScope): DeviceCredentialRecord?
    suspend fun delete(profileId: String)
}

class AndroidKeystoreCredentialVault(
    private val dataStore: DataStore<Preferences>,
    private val keyStoreProvider: () -> KeyStore = { KeyStore.getInstance(ANDROID_KEYSTORE) },
    private val keyGenerator: () -> KeyGenerator = {
        KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
    },
    private val random: SecureRandom = SecureRandom(),
) : SecureCredentialVault {
    override suspend fun preflight(profileId: String) {
        try {
            val key = getOrCreateKey(profileId)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key)
            cipher.doFinal(byteArrayOf(1, 2, 3))
        } catch (_: Exception) {
            throw CredentialVaultFailure(CredentialVaultException.Unavailable)
        }
    }

    override suspend fun store(scope: CredentialScope, record: DeviceCredentialRecord) {
        if (!scope.matches(record)) throw CredentialVaultFailure(CredentialVaultException.ScopeMismatch)
        val envelope = CredentialEnvelope(
            formatVersion = ENVELOPE_VERSION,
            profileId = scope.profileId,
            canonicalBaseUrl = scope.canonicalBaseUrl,
            transportPolicy = scope.transportPolicy,
            ownerUserId = record.ownerUserId,
            deviceId = record.deviceId,
            credentialId = record.credentialId,
            deviceCredential = record.credential.rawValue,
            createdAt = record.createdAt,
        )
        try {
            val key = getOrCreateKey(scope.profileId)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key)
            val iv = ByteArray(GCM_IV_BYTES).also(random::nextBytes)
            cipher.init(Cipher.ENCRYPT_MODE, key, GCMParameterSpec(GCM_TAG_BITS, iv))
            val credentialDigest = digest(record.credential.rawValue)
            cipher.updateAAD(scope.aad(credentialDigest))
            val ciphertext = cipher.doFinal(json.encodeToString(envelope).toByteArray(Charsets.UTF_8))
            val stored = StoredCredential(
                formatVersion = ENVELOPE_VERSION,
                profileId = scope.profileId,
                canonicalBaseUrl = scope.canonicalBaseUrl,
                transportPolicy = scope.transportPolicy,
                ownerUserId = scope.ownerUserId,
                deviceId = scope.deviceId,
                credentialId = scope.credentialId,
                credentialDigest = credentialDigest,
                keyAlias = alias(scope.profileId),
                iv = Base64.getEncoder().encodeToString(iv),
                ciphertext = Base64.getEncoder().encodeToString(ciphertext),
            )
            dataStore.edit { it[recordKey(scope.profileId)] = json.encodeToString(stored) }
        } catch (error: CredentialVaultFailure) {
            throw error
        } catch (_: Exception) {
            throw CredentialVaultFailure(CredentialVaultException.Unavailable)
        }
    }

    override suspend fun load(profileId: String, expectedScope: CredentialScope): DeviceCredentialRecord? {
        val encoded = dataStore.data.first()[recordKey(profileId)] ?: return null
        val stored = try {
            json.decodeFromString<StoredCredential>(encoded)
        } catch (_: Exception) {
            throw CredentialVaultFailure(CredentialVaultException.Corrupt)
        }
        if (stored.formatVersion != ENVELOPE_VERSION ||
            stored.profileId != expectedScope.profileId ||
            stored.canonicalBaseUrl != expectedScope.canonicalBaseUrl ||
            stored.transportPolicy != expectedScope.transportPolicy ||
            stored.ownerUserId != expectedScope.ownerUserId ||
            stored.deviceId != expectedScope.deviceId ||
            stored.credentialId != expectedScope.credentialId ||
            stored.keyAlias != alias(expectedScope.profileId) ||
            !HEX_DIGEST.matches(stored.credentialDigest)
        ) {
            throw CredentialVaultFailure(CredentialVaultException.ScopeMismatch)
        }
        try {
            val key = keyStoreProvider().apply { load(null) }.getKey(stored.keyAlias, null)
                ?: throw CredentialVaultFailure(CredentialVaultException.Missing)
            val iv = Base64.getDecoder().decode(stored.iv)
            if (iv.size != GCM_IV_BYTES) throw CredentialVaultFailure(CredentialVaultException.Corrupt)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(GCM_TAG_BITS, iv))
            cipher.updateAAD(expectedScope.aad(stored.credentialDigest))
            val envelope = json.decodeFromString<CredentialEnvelope>(
                cipher.doFinal(Base64.getDecoder().decode(stored.ciphertext)).toString(Charsets.UTF_8),
            )
            if (envelope.formatVersion != ENVELOPE_VERSION ||
                envelope.profileId != expectedScope.profileId ||
                envelope.canonicalBaseUrl != expectedScope.canonicalBaseUrl ||
                envelope.transportPolicy != expectedScope.transportPolicy ||
                envelope.ownerUserId != expectedScope.ownerUserId ||
                envelope.deviceId != expectedScope.deviceId ||
                envelope.credentialId != expectedScope.credentialId
            ) throw CredentialVaultFailure(CredentialVaultException.ScopeMismatch)
            val credential = DeviceCredential.parse(envelope.deviceCredential)
                ?: throw CredentialVaultFailure(CredentialVaultException.Corrupt)
            if (digest(credential.rawValue) != stored.credentialDigest) {
                throw CredentialVaultFailure(CredentialVaultException.Corrupt)
            }
            return DeviceCredentialRecord(
                ownerUserId = envelope.ownerUserId,
                deviceId = envelope.deviceId,
                credentialId = envelope.credentialId,
                credential = credential,
                createdAt = envelope.createdAt,
                requestId = null,
            )
        } catch (error: CredentialVaultFailure) {
            throw error
        } catch (_: Exception) {
            throw CredentialVaultFailure(CredentialVaultException.Corrupt)
        }
    }

    override suspend fun delete(profileId: String) {
        try {
            dataStore.edit { it.remove(recordKey(profileId)) }
            keyStoreProvider().apply {
                load(null)
                if (containsAlias(alias(profileId))) deleteEntry(alias(profileId))
            }
        } catch (_: Exception) {
            throw CredentialVaultFailure(CredentialVaultException.Unavailable)
        }
    }

    private fun getOrCreateKey(profileId: String): java.security.Key {
        val store = keyStoreProvider().apply { load(null) }
        store.getKey(alias(profileId), null)?.let { return it }
        val generator = keyGenerator()
        generator.init(
            KeyGenParameterSpec.Builder(
                alias(profileId),
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setKeySize(256)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .build(),
        )
        generator.generateKey()
        return keyStoreProvider().apply { load(null) }.getKey(alias(profileId), null)
            ?: throw IllegalStateException("keystore key unavailable")
    }

    private fun digest(value: String): String = MessageDigest.getInstance("SHA-256")
        .digest(value.toByteArray(Charsets.UTF_8))
        .joinToString("") { "%02x".format(it) }

    companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val ENVELOPE_VERSION = 1
        const val GCM_IV_BYTES = 12
        const val GCM_TAG_BITS = 128
        internal const val VAULT_RECORD_PREFIX = "secure_credential_v1_"
        val HEX_DIGEST = Regex("^[0-9a-f]{64}$")

        fun alias(profileId: String): String = "synveil.device.credential.v1.$profileId"
        fun recordKey(profileId: String) = stringPreferencesKey("$VAULT_RECORD_PREFIX$profileId")

        private val json = Json {
            encodeDefaults = true
            explicitNulls = true
            ignoreUnknownKeys = false
        }
    }

    @Serializable
    private data class CredentialEnvelope(
        val formatVersion: Int,
        val profileId: String,
        val canonicalBaseUrl: String,
        val transportPolicy: String,
        val ownerUserId: String,
        val deviceId: String,
        val credentialId: String,
        val deviceCredential: String,
        val createdAt: String,
    )

    @Serializable
    private data class StoredCredential(
        val formatVersion: Int,
        val profileId: String,
        val canonicalBaseUrl: String,
        val transportPolicy: String,
        val ownerUserId: String,
        val deviceId: String,
        val credentialId: String,
        val credentialDigest: String,
        val keyAlias: String,
        val iv: String,
        val ciphertext: String,
    )
}

class InMemoryCredentialVault(
    private val failPreflight: Boolean = false,
    private val failWrite: Boolean = false,
    private val failRead: Boolean = false,
    private val failDelete: Boolean = false,
) : SecureCredentialVault {
    private val records = mutableMapOf<String, Pair<CredentialScope, DeviceCredentialRecord>>()

    override suspend fun preflight(profileId: String) {
        if (failPreflight) throw CredentialVaultFailure(CredentialVaultException.Unavailable)
    }

    override suspend fun store(scope: CredentialScope, record: DeviceCredentialRecord) {
        if (failWrite) throw CredentialVaultFailure(CredentialVaultException.Unavailable)
        if (!scope.matches(record)) throw CredentialVaultFailure(CredentialVaultException.ScopeMismatch)
        records[scope.profileId] = scope to record
    }

    override suspend fun load(profileId: String, expectedScope: CredentialScope): DeviceCredentialRecord? {
        if (failRead) throw CredentialVaultFailure(CredentialVaultException.Corrupt)
        val stored = records[profileId] ?: return null
        if (stored.first != expectedScope) throw CredentialVaultFailure(CredentialVaultException.ScopeMismatch)
        return stored.second
    }

    override suspend fun delete(profileId: String) {
        if (failDelete) throw CredentialVaultFailure(CredentialVaultException.Unavailable)
        records.remove(profileId)
    }
}
