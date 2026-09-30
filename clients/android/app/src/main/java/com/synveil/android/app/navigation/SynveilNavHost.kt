package com.synveil.android.app.navigation

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.material3.Text
import androidx.navigation.NavType
import androidx.compose.ui.Modifier
import androidx.navigation.navArgument
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.synveil.android.core.model.ServerProfileId
import com.synveil.android.data.profile.ServerProfileRepository
import com.synveil.android.data.profile.ProfileRepositoryState
import com.synveil.android.data.network.SynveilHttpTransport
import com.synveil.android.feature.home.HomeScreen
import com.synveil.android.feature.profile.ProfileEditorScreen
import com.synveil.android.feature.profile.ServerProfilesScreen
import com.synveil.android.data.enrollment.EnrollmentManager
import com.synveil.android.feature.enrollment.EnrollmentScreen
import com.synveil.android.data.session.DeviceSessionManager
import com.synveil.android.feature.library.LibraryScreen

private const val HomeRoute = "home"
private const val ProfilesRoute = "profiles"
private const val ProfileEditorRoute = "profiles/editor"
private const val ProfileIdArgument = "profileId"
private const val EnrollmentRoute = "enrollment"
private const val LibrariesRoute = "libraries"

@Composable
fun SynveilNavHost(
    repository: ServerProfileRepository,
    transportFactory: (com.synveil.android.core.model.ServerProfile) -> SynveilHttpTransport,
    enrollmentManager: EnrollmentManager,
    deviceSessionManager: DeviceSessionManager,
    modifier: Modifier = Modifier,
) {
    val navController = rememberNavController()
    val repositoryState by repository.state.collectAsStateWithLifecycle(ProfileRepositoryState.NoServerConfigured)

    NavHost(
        navController = navController,
        startDestination = HomeRoute,
        modifier = modifier,
    ) {
        composable(HomeRoute) {
            HomeScreen(
                repository = repository,
                onOpenProfiles = { navController.navigate(ProfilesRoute) },
                onOpenLibraries = { navController.navigate(LibrariesRoute) },
            )
        }
        composable(LibrariesRoute) {
            LibraryScreen(
                sessionManager = deviceSessionManager,
                onBack = navController::popBackStack,
            )
        }
        composable(ProfilesRoute) {
            ServerProfilesScreen(
                repository = repository,
                transportFactory = transportFactory,
                onBack = navController::popBackStack,
                onAdd = { navController.navigate(ProfileEditorRoute) },
                onEdit = { profileId ->
                    navController.navigate("$ProfileEditorRoute?$ProfileIdArgument=$profileId")
                },
                onEnroll = { profileId -> navController.navigate("$EnrollmentRoute/$profileId") },
            )
        }
        composable(
            route = "$EnrollmentRoute/{$ProfileIdArgument}",
            arguments = listOf(navArgument(ProfileIdArgument) { type = NavType.StringType }),
        ) { entry ->
            val profileId = entry.arguments?.getString(ProfileIdArgument)?.let {
                runCatching { ServerProfileId.parse(it) }.getOrNull()
            }
            val profile = (repositoryState as? ProfileRepositoryState.Configured)
                ?.configuration?.profiles?.firstOrNull { it.profileId == profileId }
            if (profile == null) {
                Text("Server profile was not found.")
            } else {
                EnrollmentScreen(
                    profile = profile,
                    manager = enrollmentManager,
                    exchangeClient = transportFactory(profile),
                    onBack = navController::popBackStack,
                )
            }
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
