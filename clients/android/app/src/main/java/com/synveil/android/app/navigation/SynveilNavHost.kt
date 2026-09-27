package com.synveil.android.app.navigation

import androidx.compose.runtime.Composable
import androidx.navigation.NavType
import androidx.compose.ui.Modifier
import androidx.navigation.navArgument
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.data.profile.ServerProfileRepository
import com.synveil.android.feature.home.HomeScreen
import com.synveil.android.feature.profile.ProfileEditorScreen
import com.synveil.android.feature.profile.ServerProfilesScreen

private const val HomeRoute = "home"
private const val ProfilesRoute = "profiles"
private const val ProfileEditorRoute = "profiles/editor"
private const val ProfileIdArgument = "profileId"

@Composable
fun SynveilNavHost(
    repository: ServerProfileRepository,
    modifier: Modifier = Modifier,
) {
    val navController = rememberNavController()

    NavHost(
        navController = navController,
        startDestination = HomeRoute,
        modifier = modifier,
    ) {
        composable(HomeRoute) {
            HomeScreen(
                repository = repository,
                onOpenProfiles = { navController.navigate(ProfilesRoute) },
            )
        }
        composable(ProfilesRoute) {
            ServerProfilesScreen(
                repository = repository,
                onBack = navController::popBackStack,
                onAdd = { navController.navigate(ProfileEditorRoute) },
                onEdit = { profileId ->
                    navController.navigate("$ProfileEditorRoute?$ProfileIdArgument=$profileId")
                },
            )
        }
        composable(
            route = "$ProfileEditorRoute?$ProfileIdArgument={$ProfileIdArgument}",
            arguments = listOf(
                navArgument(ProfileIdArgument) {
                    type = NavType.StringType
                    nullable = true
                    defaultValue = null
                },
            ),
        ) { entry ->
            val profileId = entry.arguments?.getString(ProfileIdArgument)?.let {
                runCatching { ServerProfileId.parse(it) }.getOrNull()
            }
            ProfileEditorScreen(
                repository = repository,
                profileId = profileId,
                onBack = navController::popBackStack,
                onSaved = navController::popBackStack,
            )
        }
    }
}
