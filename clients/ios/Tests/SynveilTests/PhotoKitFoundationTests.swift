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

    // 3. Asset Observation SQLite Persistence & Source Availability Updates
    func testAssetObservationPersistence() async throws {
        let profileId = UUID()
        let ownerId = "owner_123"
        let obs1 = PhotoAssetObservation(
            localIdentifier: "asset_001",
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
        let obs2 = PhotoAssetObservation(
            localIdentifier: "asset_002",
            assetKind: .livePhoto,
            creationDate: Date(),
            modificationDate: Date(),
            pixelWidth: 3840,
            pixelHeight: 2160,
            isFavorite: false,
            isHidden: false,
            discoveryGeneration: 1,
            observedAt: Date(),
            permissionScope: .authorizedFull,
            sourceAvailability: .available
        )

        try await store.saveObservations([obs1, obs2], profileId: profileId, ownerId: ownerId)

        let fetched = try await store.fetchObservations(profileId: profileId, ownerId: ownerId)
        XCTAssertEqual(fetched.count, 2)

        try await store.markSourceUnavailable(localIdentifier: "asset_001", reason: .sourceUnavailable, profileId: profileId, ownerId: ownerId)

        let updated = try await store.fetchObservations(profileId: profileId, ownerId: ownerId)
        let asset1 = updated.first(where: { $0.localIdentifier == "asset_001" })
        XCTAssertEqual(asset1?.sourceAvailability, .sourceUnavailable)
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
        try await store.saveObservations([obs], profileId: profileId, ownerId: ownerId)

        try await store.clearObservations(profileId: profileId)
        let fetched = try await store.fetchObservations(profileId: profileId, ownerId: ownerId)
        XCTAssertTrue(fetched.isEmpty)
    }

    // 5. Transfer Candidate Handoff Descriptor Validation
    func testTransferCandidateDescriptor() {
        let profileId = UUID()
        let candidate = PhotoTransferCandidateDescriptor(
            profileId: profileId,
            ownerId: "owner_789",
            deviceId: "device_001",
            localIdentifier: "asset_200",
            assetKind: .livePhoto,
            resourceRole: .primaryImage,
            proposedFileName: "IMG_0001.HEIC",
            estimatedSizeBytes: 2_048_000,
            observationGeneration: 1
        )

        XCTAssertEqual(candidate.profileId, profileId)
        XCTAssertEqual(candidate.localIdentifier, "asset_200")
        XCTAssertEqual(candidate.assetKind, .livePhoto)
        XCTAssertEqual(candidate.resourceRole, .primaryImage)
        XCTAssertEqual(candidate.proposedFileName, "IMG_0001.HEIC")
        XCTAssertEqual(candidate.estimatedSizeBytes, 2_048_000)
    }

    // 6. Live Photo Resource Descriptor Roles & Non-Destructive Availability
    func testLivePhotoResourceDescriptors() {
        let stillRes = PhotoAssetResourceDescriptor(
            localIdentifier: "live_001",
            role: .primaryImage,
            originalFilename: "IMG_0010.HEIC",
            uniformTypeIdentifier: "public.heic",
            isLocallyAvailable: true,
            fileSizeEstimateBytes: 3_000_000
        )
        let motionRes = PhotoAssetResourceDescriptor(
            localIdentifier: "live_001",
            role: .motionVideo,
            originalFilename: "IMG_0010.MOV",
            uniformTypeIdentifier: "com.apple.quicktime-movie",
            isLocallyAvailable: false,
            fileSizeEstimateBytes: 8_000_000
        )

        XCTAssertEqual(stillRes.id, "live_001_primaryImage")
        XCTAssertEqual(motionRes.id, "live_001_motionVideo")
        XCTAssertTrue(stillRes.isLocallyAvailable)
        XCTAssertFalse(motionRes.isLocallyAvailable)
    }

    // 7. Mock Discovery Filtering (Videos Included vs Excluded)
    func testMockDiscoveryFiltering() async throws {
        struct MockDiscovery: PhotoLibraryDiscoveryProtocol {
            func discoverAssets(configuration: PhotoBackupConfiguration, generation: Int64, batchSize: Int) async throws -> [PhotoAssetObservation] {
                let still = PhotoAssetObservation(
                    localIdentifier: "id_still",
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
                let video = PhotoAssetObservation(
                    localIdentifier: "id_video",
                    assetKind: .video,
                    creationDate: Date(),
                    modificationDate: nil,
                    pixelWidth: 1920,
                    pixelHeight: 1080,
                    isFavorite: false,
                    isHidden: false,
                    discoveryGeneration: generation,
                    observedAt: Date(),
                    permissionScope: .authorizedFull,
                    sourceAvailability: .available
                )
                return configuration.includeVideos ? [still, video] : [still]
            }

            func discoverResourceDescriptors(for localIdentifier: String) async throws -> [PhotoAssetResourceDescriptor] { [] }
        }

        struct MockPermission: PhotoLibraryPermissionChecking {
            func currentPermissionState() async -> PhotoPermissionState { .authorizedFull }
            func requestPermission() async -> PhotoPermissionState { .authorizedFull }
        }

        let profileId = UUID()
        let ownerId = "owner_test"

        let configWithVideos = PhotoBackupConfiguration(isOptedIn: true, includeVideos: true)
        try await store.saveConfiguration(configWithVideos, profileId: profileId)

        let coordinator = PhotoLibraryObservationCoordinator(
            discoveryService: MockDiscovery(),
            store: store,
            permissionAdapter: MockPermission()
        )

        let result1 = try await coordinator.startObservation(profileId: profileId, ownerId: ownerId)
        if case .completed(let count, _) = result1 {
            XCTAssertEqual(count, 2)
        } else {
            XCTFail("Expected completed discovery with 2 items")
        }

        await coordinator.stopObservation()

        let configNoVideos = PhotoBackupConfiguration(isOptedIn: true, includeVideos: false)
        try await store.saveConfiguration(configNoVideos, profileId: profileId)

        let result2 = try await coordinator.startObservation(profileId: profileId, ownerId: ownerId)
        if case .completed(let count, _) = result2 {
            XCTAssertEqual(count, 1)
        } else {
            XCTFail("Expected completed discovery with 1 item")
        }
    }
}
