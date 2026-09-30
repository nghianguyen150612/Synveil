package com.synveil.android.data.profile

import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import com.synveil.android.data.enrollment.CredentialCleanupResult
import com.synveil.android.data.enrollment.CredentialLifecycle
import com.synveil.android.core.model.ServerProfileId
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.flow.first
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files

class CredentialFencingTest {
    @Test
    fun originEditFencesBeforeCommitAndFailureLeavesOriginUnchanged(): Unit = runBlocking {
        val fixture = Fixture()
        val fencer = RecordingFencer()
        val repository = fixture.repository(fencer)
        val created = (repository.addProfile("Primary", "https://one.example.com") as ProfileMutationResult.Created).profile

        repository.updateProfile(created.profileId, "Primary", "https://two.example.com")
        assertEquals(listOf(created.profileId.toString()), fencer.fenced)
        val updated = (repository.state.first() as ProfileRepositoryState.Configured).configuration.profiles.single()
        assertEquals("https://two.example.com/", updated.canonicalBaseUrl.value)

        fencer.fail = true
        assertThrows(ProfileOperationException::class.java) {
            runBlocking { repository.updateProfile(created.profileId, "Primary", "https://three.example.com") }
        }
        val unchanged = (repository.state.first() as ProfileRepositoryState.Configured).configuration.profiles.single()
        assertEquals("https://two.example.com/", unchanged.canonicalBaseUrl.value)
        fixture.close()
    }

    @Test
    fun profileDeleteIsBlockedWhenCredentialCleanupFails(): Unit = runBlocking {
        val fixture = Fixture()
        val fencer = RecordingFencer(fail = true)
        val repository = fixture.repository(fencer)
        val created = (repository.addProfile("Primary", "https://one.example.com") as ProfileMutationResult.Created).profile

        val error = assertThrows(ProfileOperationException::class.java) {
            runBlocking { repository.removeProfile(created.profileId) }
        }
        assertEquals(ProfileOperationError.CREDENTIAL_CLEANUP_FAILED, error.reason)
        assertTrue((repository.state.first() as ProfileRepositoryState.Configured).configuration.profiles.any { it.profileId == created.profileId })
        fixture.close()
    }

    private class RecordingFencer(
        var fail: Boolean = false,
    ) : CredentialLifecycle {
        val fenced = mutableListOf<String>()

        override suspend fun fence(profileId: String): CredentialCleanupResult {
            fenced += profileId
            return if (fail) CredentialCleanupResult.Failed("test failure") else CredentialCleanupResult.Success
        }
    }

    private class Fixture {
        private val directory = Files.createTempDirectory("synveil-fencing-test")
        private val store = PreferenceDataStoreFactory.create {
            directory.resolve("profiles.preferences_pb").toFile()
        }

        fun repository(fencer: CredentialLifecycle) = DataStoreServerProfileRepository(
            dataStore = store,
            allowLoopbackTestHttp = false,
            idGenerator = { ServerProfileId.parse("018bcfe5-687b-7001-8203-040506070809") },
            credentialLifecycle = fencer,
        )

        fun close() = directory.toFile().deleteRecursively()
    }
}
