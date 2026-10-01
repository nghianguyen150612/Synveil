package com.synveil.android.feature.accessibility

import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.activity.compose.setContent
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.synveil.android.core.ui.SynveilTheme
import com.synveil.android.feature.shared.AdaptiveStatusCard
import com.synveil.android.app.MainActivity
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AdaptiveAccessibilityInstrumentationTest {
    @get:Rule
    val composeRule = createAndroidComposeRule<MainActivity>()

    @Test
    fun statusCardExposesReadableStateAndActionSemantics() {
        var clicked = false
        composeRule.activity.runOnUiThread {
            composeRule.activity.setContent {
                SynveilTheme {
                    AdaptiveStatusCard(
                        title = "Connectivity",
                        message = "Offline — cached metadata",
                        actionLabel = "Retry",
                        onAction = { clicked = true },
                    )
                }
            }
        }

        composeRule.onNodeWithText("Connectivity").assertIsDisplayed()
        composeRule.onNodeWithText("Offline — cached metadata").assertIsDisplayed()
        composeRule.onNodeWithContentDescription("Retry").assertHasClickAction().performClick()
        assertTrue(clicked)
    }

    @Test
    fun statusCardRendersAcrossThemesAndLargeFontScale() {
        composeRule.activity.runOnUiThread {
            composeRule.activity.setContent {
                CompositionLocalProvider(LocalDensity provides Density(1f, fontScale = 1.5f)) {
                    SynveilTheme(darkTheme = true, dynamicColor = false) {
                        AdaptiveStatusCard("Dark theme", "Readable at larger text scale")
                    }
                }
            }
        }
        composeRule.onNodeWithText("Dark theme").assertIsDisplayed()
        composeRule.onNodeWithText("Readable at larger text scale").assertIsDisplayed()

        composeRule.activity.runOnUiThread {
            composeRule.activity.setContent {
                SynveilTheme(darkTheme = false, dynamicColor = false) {
                    AdaptiveStatusCard("Light theme", "Readable in system light mode")
                }
            }
        }
        composeRule.onNodeWithText("Light theme").assertIsDisplayed()
    }
}
