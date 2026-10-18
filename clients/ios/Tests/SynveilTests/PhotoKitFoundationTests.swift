import XCTest

@testable import Synveil

final class PhotoKitFoundationTests: XCTestCase {
    var dbURL: URL!
    var store: PhotoObservationSQLiteStore!

    override func setUpWithError() throws {
        try super.setUpWithError()
        let tempDir = FileManager.default.temporaryDirectory
        dbURL = tempDir.appendingPathComponent("photo_test_\(UUID().uuidString).sqlite")
        store = try PhotoObservationSQLiteStore(databaseURL: dbURL)
    }

    override func tearDownWithError() throws {
        store = nil
        if let url = dbURL {
            try? FileManager.default.removeItem(at: url)
        }
        try super.tearDownWithError()
    }

    func testPhotoObservationSchemaMigratesVersionOneWithoutLosingObservations() throws {
        let legacyURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("photo_v1_migration_\(UUID().uuidString).sqlite")
        addTeardownBlock { try? FileManager.default.removeItem(at: legacyURL) }
        let profileId = "11111111-1111-4111-8111-111111111111"
        let ownerId = "legacy-photo-owner"
        let localIdentifier = "legacy-photo-asset"

        try queueRawSQL(
            legacyURL,
            """
            CREATE TABLE photo_asset_observations (
                local_identifier TEXT NOT NULL,
                profile_id TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                asset_kind TEXT NOT NULL,
                creation_date DOUBLE,
                modification_date DOUBLE,
                pixel_width INTEGER NOT NULL,
                pixel_height INTEGER NOT NULL,
                is_favorite INTEGER NOT NULL,
                is_hidden INTEGER NOT NULL,
                discovery_generation INTEGER NOT NULL,
                observed_at DOUBLE NOT NULL,
                permission_scope TEXT NOT NULL,
                source_availability TEXT NOT NULL,
                is_excluded INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (local_identifier, profile_id)
            )
            """)
        try queueRawSQL(
            legacyURL,
            """
            CREATE TABLE photo_backup_configurations (
                profile_id TEXT PRIMARY KEY,
                is_opted_in INTEGER NOT NULL,
                scope TEXT NOT NULL,
                include_videos INTEGER NOT NULL,
                pause_discovery INTEGER NOT NULL,
                updated_at DOUBLE NOT NULL
            )
            """)
        try queueRawSQL(
            legacyURL,
            """
            INSERT INTO photo_asset_observations (
                local_identifier, profile_id, owner_id, asset_kind, pixel_width,
                pixel_height, is_favorite, is_hidden, discovery_generation,
                observed_at, permission_scope, source_availability, is_excluded
            ) VALUES (
                '\(localIdentifier)', '\(profileId)', '\(ownerId)', 'stillPhoto',
                1920, 1080, 1, 0, 7, 1234.5, 'authorizedLimited', 'available', 0
            )
            """)
        try queueRawSQL(legacyURL, "PRAGMA user_version = 1")

        let migratedStore = try PhotoObservationSQLiteStore(databaseURL: legacyURL)
        XCTAssertEqual(try queueRawScalar(legacyURL, "PRAGMA user_version"), "3")
        XCTAssertEqual(
            try queueRawScalar(
                legacyURL,
                "SELECT COUNT(*) FROM photo_asset_observations WHERE local_identifier = '\(localIdentifier)'"
            ),
            "1")
        XCTAssertEqual(
            try queueRawScalar(
                legacyURL,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'photo_resource_extractions'"
            ),
            "1")
        XCTAssertEqual(
            try queueRawScalar(
                legacyURL,
                "SELECT COUNT(*) FROM pragma_table_info('photo_backup_configurations') WHERE name='automatic_foreground_backup_enabled'"
            ),
            "1")
        XCTAssertEqual(
            try queueRawScalar(
                legacyURL,
                "SELECT COUNT(*) FROM pragma_table_info('photo_backup_candidates') WHERE name='file_version_identifier'"
            ),
            "1")
        _ = migratedStore
    }

    // 1. Full & Limited Permission Mapping
    func testPermissionStateMapping() {
        XCTAssertTrue(PhotoPermissionState.authorizedFull.isAuthorized)
        XCTAssertTrue(PhotoPermissionState.authorizedLimited.isAuthorized)
        XCTAssertFalse(PhotoPermissionState.denied.isAuthorized)
        XCTAssertFalse(PhotoPermissionState.restricted.isAuthorized)
        XCTAssertFalse(PhotoPermissionState.notDetermined.isAuthorized)
        XCTAssertFalse(PhotoPermissionState.unavailable.isAuthorized)
        XCTAssertFalse(PhotoPermissionState.unknownFutureStatus.isAuthorized)
    }

    // 2. Configuration Store Persistence & Default State
    func testConfigurationPersistence() async throws {
        let profileId = UUID()
        let defaultConfig = try await store.fetchConfiguration(profileId: profileId)
        XCTAssertFalse(defaultConfig.isOptedIn)
        XCTAssertEqual(defaultConfig.scope, .allAuthorized)
        XCTAssertTrue(defaultConfig.includeVideos)
        XCTAssertFalse(defaultConfig.pauseDiscovery)

        let newConfig = PhotoBackupConfiguration(
            isOptedIn: true,
            scope: .limitedSelection,
            includeVideos: false,
            pauseDiscovery: true
        )
        try await store.saveConfiguration(newConfig, profileId: profileId)

        let fetched = try await store.fetchConfiguration(profileId: profileId)
        XCTAssertTrue(fetched.isOptedIn)
        XCTAssertEqual(fetched.scope, .limitedSelection)
        XCTAssertFalse(fetched.includeVideos)
        XCTAssertTrue(fetched.pauseDiscovery)
    }

    // 3. Asset Observation SQLite Persistence & Owner-Scoped Source Availability
    func testAssetObservationPersistenceAndOwnerScoping() async throws {
        let profileId = UUID()
        let owner1 = "owner_123"
        let owner2 = "owner_456"

        let obs1 = PhotoAssetObservation(
            localIdentifier: "asset_shared",
            assetKind: .stillPhoto,
            creationDate: Date(),
            modificationDate: Date(),
            pixelWidth: 1920,
            pixelHeight: 1080,
            isFavorite: true,
            isHidden: false,
            discoveryGeneration: 1,
            observedAt: Date(),
            permissionScope: .authorizedFull,
            sourceAvailability: .available
        )

        let authorization = PhotoAuthorizationSession(profileId: profileId, ownerId: owner1)
        try await store.activateAuthorization(authorization)
        try await store.commitObservations([obs1], authorization: authorization)

        // Mark source unavailable for owner1
        try await store.markSourceUnavailable(
            localIdentifier: "asset_shared",
            reason: .sourceUnavailable,
            profileId: profileId,
            ownerId: owner1
        )

        let fetchedOwner1 = try await store.fetchObservations(
            profileId: profileId,
            ownerId: owner1
        )
        XCTAssertEqual(fetchedOwner1.first?.sourceAvailability, .sourceUnavailable)

        let fetchedOwner2 = try await store.fetchObservations(
            profileId: profileId,
            ownerId: owner2
        )
        XCTAssertTrue(fetchedOwner2.isEmpty)
    }

    // 4. Clear Observations on Logout / Profile Teardown
    func testClearObservationsOnLogout() async throws {
        let profileId = UUID()
        let ownerId = "owner_456"
        let obs = PhotoAssetObservation(
            localIdentifier: "asset_100",
            assetKind: .video,
            creationDate: Date(),
            modificationDate: Date(),
            pixelWidth: 1280,
            pixelHeight: 720,
            isFavorite: false,
            isHidden: false,
            discoveryGeneration: 1,
            observedAt: Date(),
            permissionScope: .authorizedFull,
            sourceAvailability: .available
        )
        let authorization = PhotoAuthorizationSession(profileId: profileId, ownerId: ownerId)
        try await store.activateAuthorization(authorization)
        try await store.commitObservations([obs], authorization: authorization)

        try await store.clearObservations(profileId: profileId)
        let fetched = try await store.fetchObservations(profileId: profileId, ownerId: ownerId)
        XCTAssertTrue(fetched.isEmpty)
    }

    // 5. Session Fencing Stops Delayed Scan Writes
    func testSessionFencingFencesDelayedScanWrites() async throws {
        struct DelayedMockDiscovery: PhotoLibraryDiscoveryProtocol {
            func discoverAssets(
                configuration: PhotoBackupConfiguration,
                generation: Int64,
                batchSize: Int
            ) async throws -> PhotoDiscoveryBatchResult {
                try await Task.sleep(nanoseconds: 100_000_000)
                let obs = PhotoAssetObservation(
                    localIdentifier: "asset_delayed",
                    assetKind: .stillPhoto,
                    creationDate: Date(),
                    modificationDate: nil,
                    pixelWidth: 100,
                    pixelHeight: 100,
                    isFavorite: false,
                    isHidden: false,
                    discoveryGeneration: generation,
                    observedAt: Date(),
                    permissionScope: .authorizedFull,
                    sourceAvailability: .available
                )
                return PhotoDiscoveryBatchResult(
                    observations: [obs],
                    isComplete: true,
                    totalInScope: 1
                )
            }

            func discoverResourceDescriptors(
                for localIdentifier: String
            ) async throws -> [PhotoAssetResourceDescriptor] { [] }
        }

        struct MockPermission: PhotoLibraryPermissionChecking {
            func currentPermissionState() async -> PhotoPermissionState { .authorizedFull }
            func requestPermission() async -> PhotoPermissionState { .authorizedFull }
        }

        let profileId = UUID()
        let ownerId = "owner_fenced"
        let config = PhotoBackupConfiguration(isOptedIn: true)
        try await store.saveConfiguration(config, profileId: profileId)

        let coordinator = PhotoLibraryObservationCoordinator(
            discoveryService: DelayedMockDiscovery(),
            store: store,
            permissionAdapter: MockPermission()
        )

        let scanTask = Task {
            try await coordinator.startObservation(profileId: profileId, ownerId: ownerId)
        }

        // Stop observation before delayed discovery finishes
        try await Task.sleep(nanoseconds: 10_000_000)
        try await coordinator.stopObservation()

        let _ = try await scanTask.value

        let fetched = try await store.fetchObservations(profileId: profileId, ownerId: ownerId)
        XCTAssertTrue(fetched.isEmpty, "Stale observation should be fenced and not written")
        let scanResult = await coordinator.currentScanResult()
        XCTAssertEqual(scanResult, .disabled)
    }

    // 6. Partial Discovery Classification
    func testPartialScanClassification() async throws {
        struct PartialMockDiscovery: PhotoLibraryDiscoveryProtocol {
            func discoverAssets(
                configuration: PhotoBackupConfiguration,
                generation: Int64,
                batchSize: Int
            ) async throws -> PhotoDiscoveryBatchResult {
                let obs = PhotoAssetObservation(
                    localIdentifier: "asset_part1",
                    assetKind: .stillPhoto,
                    creationDate: Date(),
                    modificationDate: nil,
                    pixelWidth: 100,
                    pixelHeight: 100,
                    isFavorite: false,
                    isHidden: false,
                    discoveryGeneration: generation,
                    observedAt: Date(),
                    permissionScope: .authorizedFull,
                    sourceAvailability: .available
                )
                return PhotoDiscoveryBatchResult(
                    observations: [obs],
                    isComplete: false,
                    totalInScope: 1500
                )
            }

            func discoverResourceDescriptors(
                for localIdentifier: String
            ) async throws -> [PhotoAssetResourceDescriptor] { [] }
        }

        struct MockPermission: PhotoLibraryPermissionChecking {
            func currentPermissionState() async -> PhotoPermissionState { .authorizedFull }
            func requestPermission() async -> PhotoPermissionState { .authorizedFull }
        }

        let profileId = UUID()
        let ownerId = "owner_partial"
        let config = PhotoBackupConfiguration(isOptedIn: true)
        try await store.saveConfiguration(config, profileId: profileId)

        let coordinator = PhotoLibraryObservationCoordinator(
            discoveryService: PartialMockDiscovery(),
            store: store,
            permissionAdapter: MockPermission()
        )

        let result = try await coordinator.startObservation(profileId: profileId, ownerId: ownerId)
        if case .partial(let observed, let total, _) = result {
            XCTAssertEqual(observed, 1)
            XCTAssertEqual(total, 1500)
        } else {
            XCTFail("Expected partial discovery result")
        }
    }
}
