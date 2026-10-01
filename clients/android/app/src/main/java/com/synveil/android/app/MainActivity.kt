package com.synveil.android.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import com.synveil.android.SynveilApplication
import com.synveil.android.app.navigation.SynveilNavHost
import com.synveil.android.core.ui.SynveilTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val repository = (application as SynveilApplication).serverProfileRepository
        setContent {
            SynveilTheme {
                Surface(
                    modifier = Modifier
                        .fillMaxSize()
                        .safeDrawingPadding(),
                ) {
                        SynveilNavHost(
                            repository = repository,
                            transportFactory = (application as SynveilApplication).transportFactory,
                            enrollmentManager = (application as SynveilApplication).enrollmentManager,
                            deviceSessionManager = (application as SynveilApplication).deviceSessionManager,
                            cache = (application as SynveilApplication).cacheRepository,
                            syncSettingsStore = (application as SynveilApplication).syncSettingsStore,
                            connectivityObserver = (application as SynveilApplication).connectivityObserver,
                        )
                }
            }
        }
    }
}
