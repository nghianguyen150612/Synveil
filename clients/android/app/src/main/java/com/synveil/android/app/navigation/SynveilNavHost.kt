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
import com.synveil.android.feature.home.SyncSettingsScreen
import com.synveil.android.data.settings.SyncSettingsStore
import com.synveil.android.feature.library.NodeBrowserScreen
import com.synveil.android.data.library.LibraryId
import com.synveil.android.data.library.NodeId
import com.synveil.android.data.cache.CacheRepository
import com.synveil.android.data.connectivity.ConnectivityObserver

private const val HomeRoute = "home"
private const val ProfilesRoute = "profiles"
private const val ProfileEditorRoute = "profiles/editor"
private const val ProfileIdArgument = "profileId"
private const val EnrollmentRoute = "enrollment"
private const val LibrariesRoute = "libraries"
private const val LibraryBrowserRoute = "libraries/{libraryId}/{rootNodeId}"

@Composable
fun SynveilNavHost(
    repository: ServerProfileRepository,
    transportFactory: (com.synveil.android.core.model.ServerProfile) -> SynveilHttpTransport,
    enrollmentManager: EnrollmentManager,
    deviceSessionManager: DeviceSessionManager,
    cache: CacheRepository,
    syncSettingsStore: SyncSettingsStore,
    connectivityObserver: ConnectivityObserver,
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
                sessionManager = deviceSessionManager,
                onOpenProfiles = { navController.navigate(ProfilesRoute) },
                onOpenActiveProfileEnrollment = {
                    val activeProfileId = (repositoryState as? ProfileRepositoryState.Configured)
                        ?.configuration?.activeProfile?.profileId
                    if (activeProfileId == null) {
                        navController.navigate(ProfilesRoute)
                    } else {
                        navController.navigate("$EnrollmentRoute/$activeProfileId")
                    }
                },
                onOpenLibraries = { navController.navigate(LibrariesRoute) },
                onOpenSyncSettings = { navController.navigate("sync-settings") },
            )
        }
        composable(LibrariesRoute) {
            LibraryScreen(
                sessionManager = deviceSessionManager,
                cache = cache,
                connectivityObserver = connectivityObserver,
                onBack = navController::popBackStack,
                onOpenLibrary = { library ->
                    navController.navigate("libraries/${library.id.value}/${library.rootNodeId.value}")
                },
            )
        }
        composable("sync-settings") {
            SyncSettingsScreen(syncSettingsStore, navController::popBackStack)
        }
        composable(
            LibraryBrowserRoute,
            arguments = listOf(
                navArgument("libraryId") { type = NavType.StringType },
                navArgument("rootNodeId") { type = NavType.StringType },
            ),
        ) { entry ->
            val libraryId = entry.arguments?.getString("libraryId")?.let(LibraryId::parse)
            val rootNodeId = entry.arguments?.getString("rootNodeId")?.let(NodeId::parse)
            if (libraryId == null || rootNodeId == null) {
                Text("The library identity was invalid.")
            } else {
                NodeBrowserScreen(
                    sessionManager = deviceSessionManager,
                    cache = cache,
                    libraryId = libraryId,
                    rootNodeId = rootNodeId,
                    onBack = navController::popBackStack,
                    connectivityObserver = connectivityObserver,
                )
            }
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
