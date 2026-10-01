package com.synveil.android.data.transfer

import android.content.Intent
import android.net.Uri
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class TransferIntentInstrumentationTest {
    @Test
    fun openAndShareUseGrantedContentUris() {
        val uri = Uri.parse("content://com.example.provider/document/1")
        val open = TransferOperations.openIntent(uri, "text/plain")
        assertEquals(Intent.ACTION_VIEW, open.action)
        assertEquals(uri, open.data)
        assertTrue(open.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0)
        assertTrue(!uri.toString().startsWith("file:"))

        val share = TransferOperations.shareIntent(uri, "text/plain")
        assertEquals(Intent.ACTION_SEND, share.action)
        assertEquals(uri, share.getParcelableExtra(Intent.EXTRA_STREAM))
        assertTrue(share.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0)
    }
}
