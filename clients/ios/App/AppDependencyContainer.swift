import Foundation
import Observation

/// Top-level application dependency composition root.
///
/// Responsible for constructing application dependencies and managing top-level
/// singletons/controllers.
@Observable
@MainActor
public final class AppDependencyContainer {
    /// Non-secret application configuration.
    public let configuration: AppConfiguration

    /// Authoritative application session controller.
    public let sessionController: SessionController

    /// Authoritative Rust validator used by production enrollment and secure credential loading.
    public private(set) var rustBridge: (any RustBridgeProtocol)?

    /// Real Keychain store, available only after Rust validation initializes successfully.
    public private(set) var credentialSink: SecureCredentialSinkProtocol?

    /// Startup restoration service, composed from the same Rust-validated Keychain store.
    public private(set) var restorationService: SessionRestorationServiceProtocol?

    /// Local logout service, composed from the same active Keychain store.
    public private(set) var logoutService: SessionLogoutServiceProtocol?

    /// Authenticated catalog boundary injected into the native Library presentation.
    public private(set) var libraryCatalog: (any LibraryCatalogRepositoryProtocol)?

    /// Read-only Node foundation for the future native folder browser.
    public private(set) var nodeRepository: (any NodeRepositoryProtocol)?

    /// Live-first browser composition with authenticated reads from the durable P039 projection.
    private(set) var offlineNodeBrowserService: (any OfflineNodeBrowserServiceProtocol)?
    private(set) var savedMetadataInspection: (any SavedMetadataInspectionProtocol)?

    /// Foundation only: no durable authorizer is installed and no SwiftUI write interface is exposed.
    private(set) var clientMutationRepository: (any ClientMutationRepositoryProtocol)?

    /// Infrastructure only. Database failure leaves read-only browsing available and POST gated.
    private(set) var durableMutationQueue: DurableMutationQueue?
    private(set) var mutationDrainCoordinator: MutationDrainCoordinator?
    private(set) var syncCheckpointService: SyncCheckpointService?
    private(set) var syncFeedService: SyncFeedService?
    private(set) var nodeProjectionRepository: SQLiteNodeProjectionRepository?
    private(set) var syncFeedApplicationService: SyncFeedApplicationService?
    private(set) var rebaselineCoordinator: RebaselineCoordinator?
    private(set) var downloadCoordinator: FileDownloadCoordinator?
    private(set) var uploadCoordinator: FileUploadCoordinator?
    private var backgroundCancellationTransport: URLSessionBackgroundUploadTransport?
    private(set) var fileExportCoordinator: VerifiedFileExportCoordinator?
    private(set) var syncAckService: SyncAckService?
    private(set) var inboundSyncCoordinator: (any InboundSyncCoordinatorProtocol)?
    private(set) var metadataMutationFeature: (any MetadataMutationFeatureProtocol)?
    private(set) var mutationQueueFailure: MutationQueueFailure?
    private(set) var backgroundSyncCoordinator: BackgroundSyncCoordinator?
    private var sessionBootstrapTask: Task<Void, Never>?
    private var backgroundDatabase: MutationQueueSQLiteStore?

    /// The application delegate and metadata delivery share one startup pass, without a scene.
    func prepareApplicationSession() async {
        if let sessionBootstrapTask {
            await sessionBootstrapTask.value
            return
        }
        let task = Task { @MainActor in
            await self.prepareEnrollmentSecurity()
            await self.sessionController.start()
        }
        sessionBootstrapTask = task
        await task.value
    }

    func runBackgroundMetadata(_ delivery: BackgroundRefreshDelivery) async -> BackgroundSyncOutcome
    {
        await prepareApplicationSession()
        guard !delivery.isExpired, !Task.isCancelled else {
            return BackgroundSyncOutcome(state: .expired)
        }
        guard let backgroundSyncCoordinator else { return BackgroundSyncOutcome(state: .failed) }
        return await backgroundSyncCoordinator.run(delivery: delivery)
    }

    func backgroundSettingsIdentity() async throws -> BackgroundSyncIdentity {
        guard let credentialSink, let database = backgroundDatabase,
            sessionController.state == .authenticated,
            let endpoint = sessionController.serverEndpoint
        else { throw LibraryFailure.unauthenticated }
        let revision = sessionController.lifecycleRevision
        let session = try await credentialSink.load(expectedServerEndpoint: endpoint)
        let scope = LibraryRequestScope(session: session, revision: revision)
        try await database.requireBackgroundVerificationEligibility(scope)
        guard sessionController.state == .authenticated,
            revision == sessionController.lifecycleRevision,
            endpoint == sessionController.serverEndpoint
        else { throw LibraryFailure.staleSession }
        return BackgroundSyncIdentity(scope)
    }

    /// Explicit settings action: enrollment alone does not fabricate P047 verification evidence.
    func authorizeBackgroundOptIn() async throws -> BackgroundSyncIdentity {
        guard sessionController.state == .authenticated, let restorationService,
            let endpoint = sessionController.serverEndpoint, let credentialSink,
            let database = backgroundDatabase
        else { throw LibraryFailure.unauthenticated }
        let revision = sessionController.lifecycleRevision
        let stored = try await credentialSink.load(expectedServerEndpoint: endpoint)
        guard revision == sessionController.lifecycleRevision,
            sessionController.state == .authenticated
        else { throw LibraryFailure.staleSession }
        let session = LibraryRequestScope(session: stored, revision: revision)
        let owner = UUID()
        try await database.claimBackgroundOptInVerification(session: session, owner: owner)
        let result = await restorationService.restore(configuredServerEndpoint: endpoint)
        await database.finishInboundRun(owner: owner)
        guard sessionController.state == .authenticated,
            sessionController.lifecycleRevision == revision
        else { throw LibraryFailure.staleSession }
        switch result {
        case .remotelyVerifiedAuthorizedSession(let verified):
            guard verified == stored else { throw LibraryFailure.staleSession }
            return try await backgroundSettingsIdentity()
        case .locallyValidStoredSession(_, .authenticationRejected):
            sessionController.handleLibraryAuthenticationFailure(
                .authenticationRejected, revision: revision)
        case .locallyValidStoredSession(_, .deviceRevoked):
            sessionController.handleLibraryAuthenticationFailure(.deviceRevoked, revision: revision)
        default: break
        }
        throw LibraryFailure.unauthenticated
    }

    func backgroundSavedConfirmation() async throws -> BackgroundSyncConfirmation? {
        guard let credentialSink, let database = backgroundDatabase,
            let bridge = rustBridge, sessionController.state == .authenticated,
            let endpoint = sessionController.serverEndpoint
        else { throw LibraryFailure.unauthenticated }
        let revision = sessionController.lifecycleRevision
        let session = try await credentialSink.load(expectedServerEndpoint: endpoint)
        let identity = LibraryRequestScope(session: session, revision: revision)
        try await database.requireBackgroundAuthorization(identity)
        guard let raw = try await database.lastBackgroundConfirmedLibrary(session: identity) else {
            return nil
        }
        let id = try await LibraryId.validated(raw, using: bridge)
        let scope = try await identity.mutationScope(libraryId: id, bridge: bridge)
        let record = try await database.latestConfirmedInbound(
            scope: scope, credentialId: identity.credentialIdentifier, bridge: bridge)
        let current = try await credentialSink.load(expectedServerEndpoint: endpoint)
        guard current == session, sessionController.state == .authenticated,
            revision == sessionController.lifecycleRevision,
            endpoint == sessionController.serverEndpoint
        else { throw LibraryFailure.staleSession }
        try await database.requireBackgroundAuthorization(identity)
        return record.map {
            BackgroundSyncConfirmation(
                library: id, recordedAt: $0.updatedAt,
                position: SyncJournalPosition(epoch: $0.page.start.epoch, sequence: $0.page.through)
            )
        }
    }

    func backgroundRecoveryRequired() async throws -> Bool {
        guard let credentialSink, let database = backgroundDatabase,
            sessionController.state == .authenticated,
            let endpoint = sessionController.serverEndpoint
        else { throw LibraryFailure.unauthenticated }
        let revision = sessionController.lifecycleRevision
        let session = try await credentialSink.load(expectedServerEndpoint: endpoint)
        let required = try await database.backgroundRecoveryRequired(
            session: LibraryRequestScope(session: session, revision: revision))
        guard revision == sessionController.lifecycleRevision,
            sessionController.state == .authenticated
        else { throw LibraryFailure.staleSession }
        return required
    }

    private var enrollmentSecurityPrepared = false
    private var securityPreparationTask: Task<Void, Never>?

    /// Initializes application dependencies and constructs top-level services.
    ///
    /// - Parameter configuration: App configuration containing non-secret bootstrap settings.
    ///   Defaults to `AppConfiguration.load()`.
    public init(configuration: AppConfiguration = AppConfiguration.load()) {
        self.configuration = configuration
        self.sessionController = SessionController(configuration: configuration)
    }

    /// Initializes production enrollment security dependencies. Failure leaves enrollment closed.
    func restoreBackgroundTransfers() async {
        if let background = uploadCoordinator?.background {
            await background.restore()
        } else {
            // Incompatible schema or unavailable staging cannot authorize adoption. Reconnect
            // only to cancel system work; retain callbacks until durable storage is available.
            if backgroundCancellationTransport == nil {
                backgroundCancellationTransport = URLSessionBackgroundUploadTransport()
            }
            if let transport = backgroundCancellationTransport {
                for task in await transport.inventory() { await transport.cancel(task.identifier) }
            }
        }
    }

    public func prepareEnrollmentSecurity() async {
        if let securityPreparationTask {
            await securityPreparationTask.value
            return
        }
        guard !enrollmentSecurityPrepared else { return }

        let task = Task { @MainActor [weak self] in
            guard let self else { return }
            await self.initializeEnrollmentSecurity()
        }
        securityPreparationTask = task
        await task.value
        securityPreparationTask = nil
    }

    private func initializeEnrollmentSecurity() async {
        enrollmentSecurityPrepared = true

        do {
            let bridge = try await RustBridgeAsyncAdapter()
            let store = KeychainCredentialStore(rustBridge: bridge)
            let authenticatedProbe = AuthenticatedSessionValidationService(
                transport: URLSessionHTTPTransport(
                    requestTimeout: 10,
                    resourceTimeout: 15,
                    maxResponseBodyBytes: 16 * 1024
                )
            )
            var service = SessionRestorationService(
                credentialStore: store,
                authorizationValidator: authenticatedProbe,
                authorizationEvidence: SQLiteSessionAuthorizationEvidence(database: nil)
            )
            let logout = SessionLogoutService(credentialStore: store)

            let browserProvider = AuthenticatedLibraryRequestProvider(
                controller: sessionController,
                store: store,
                transport: URLSessionHTTPTransport(
                    requestTimeout: 10,
                    resourceTimeout: 15,
                    maxResponseBodyBytes: LibraryCatalogPolicy.maximumResponseBytes
                )
            )
            libraryCatalog = AuthenticatedLibraryCatalogRepository(
                provider: browserProvider, bridge: bridge)
            nodeRepository = AuthenticatedNodeRepository(provider: browserProvider, bridge: bridge)
            let mutationProvider = AuthenticatedLibraryRequestProvider(
                controller: sessionController, store: store,
                transport: URLSessionHTTPTransport(
                    requestTimeout: 10, resourceTimeout: 15,
                    maxResponseBodyBytes: ClientMutationPolicy.maximumResponseBytes))
            clientMutationRepository = AuthenticatedClientMutationRepository(
                provider: mutationProvider, bridge: bridge)
            do {
                let database = try await Task.detached {
                    try MutationQueueSQLiteStore(url: MutationQueueSQLiteStore.productionURL())
                }.value
                let queue = DurableMutationQueue(
                    store: database, provider: browserProvider, bridge: bridge)
                let guardedRestoration = SessionRestorationService(
                    credentialStore: store, authorizationValidator: authenticatedProbe,
                    authorizationEvidence: SQLiteSessionAuthorizationEvidence(database: database))
                service = guardedRestoration
                switch await queue.recoverInterruptedOperations() {
                case .recovered: break
                case .failed(let failure): throw failure
                }
                durableMutationQueue = queue
                backgroundDatabase = database
                let coordinator = MutationDrainCoordinator(
                    queue: queue, provider: mutationProvider, bridge: bridge)
                mutationDrainCoordinator = coordinator
                let checkpoint = SyncCheckpointService(
                    provider: browserProvider, queue: queue, bridge: bridge)
                syncCheckpointService = checkpoint
                let feedProvider = AuthenticatedLibraryRequestProvider(
                    controller: sessionController, store: store,
                    transport: URLSessionHTTPTransport(
                        requestTimeout: 10, resourceTimeout: 15,
                        maxResponseBodyBytes: SyncFeedPolicy.maximumResponseBytes))
                rebaselineCoordinator = RebaselineCoordinator(
                    database: database, queue: queue, provider: feedProvider, bridge: bridge)
                syncFeedService = SyncFeedService(
                    provider: feedProvider, queue: queue,
                    store: database, bridge: bridge)
                if let nodeRepository {
                    nodeProjectionRepository = SQLiteNodeProjectionRepository(
                        database: database, provider: feedProvider, bridge: bridge)
                    if let projection = nodeProjectionRepository,
                        let nodes = nodeRepository as? AuthenticatedNodeRepository
                    {
                        nodes.installReadDenialHandler { [weak self] libraryId, session in
                            // Projection denial fails closed internally; the HTTP 403 remains
                            // authoritative even when its local quarantine cannot be persisted.
                            do {
                                try await projection.rejectReadAuthorization(
                                    libraryId: libraryId, session: session)
                            } catch {
                                self?.mutationQueueFailure = DurableMutationQueue.classify(error)
                            }
                        }
                    }
                    if let projection = nodeProjectionRepository,
                        let catalog = libraryCatalog as? AuthenticatedLibraryCatalogRepository
                    {
                        catalog.installObservationHandler { [weak self] libraries, session in
                            do {
                                try await projection.rememberValidatedCatalog(
                                    libraries, session: session)
                            } catch {
                                self?.mutationQueueFailure = DurableMutationQueue.classify(error)
                            }
                        }
                    }
                    syncFeedApplicationService = SyncFeedApplicationService(
                        provider: feedProvider, queue: queue, database: database,
                        nodes: nodeRepository, bridge: bridge)
                    let projection = CommittedSQLiteSyncProjectionStorage(
                        database: database, bridge: bridge)
                    syncAckService = SyncAckService(
                        provider: feedProvider, projection: projection, bridge: bridge)
                    if let syncFeedService, let syncFeedApplicationService, let syncAckService,
                        let nodeProjectionRepository
                    {
                        inboundSyncCoordinator = InboundSyncCoordinator(
                            feed: syncFeedService, application: syncFeedApplicationService,
                            ack: syncAckService, projection: nodeProjectionRepository, queue: queue,
                            database: database, provider: feedProvider, bridge: bridge)
                    }
                }
                if let nodeRepository, let nodeProjectionRepository, let inboundSyncCoordinator {
                    offlineNodeBrowserService = OfflineNodeBrowserService(
                        liveRepository: nodeRepository,
                        projection: nodeProjectionRepository,
                        scopeProvider: inboundSyncCoordinator)
                    savedMetadataInspection = SavedMetadataInspectionService(
                        controller: sessionController, credentials: store, database: database,
                        bridge: bridge, liveRepository: nodeRepository)
                }
                let backgroundPolicy = BGAppRefreshScheduler.shared.policy
                let backgroundGate = BackgroundSyncRequestGate {
                    !ProcessInfo.processInfo.isLowPowerModeEnabled
                        && BGAppRefreshScheduler.shared.systemAllowsExecution
                }
                let backgroundTransport = BackgroundSyncHTTPTransport(
                    policy: backgroundPolicy, gate: backgroundGate)
                let backgroundProvider = AuthenticatedLibraryRequestProvider(
                    controller: sessionController, store: store, transport: backgroundTransport,
                    executionValidation: { session in
                        try backgroundGate.check()
                        try await database.requireBackgroundAuthorization(session)
                        try backgroundGate.check()
                    })
                let backgroundNodes = AuthenticatedNodeRepository(
                    provider: backgroundProvider, bridge: bridge)
                backgroundNodes.installReadDenialHandler { [weak self] library, session in
                    if let projection = self?.nodeProjectionRepository {
                        do {
                            try await projection.rejectReadAuthorization(
                                libraryId: library, session: session)
                        } catch {
                            self?.mutationQueueFailure = DurableMutationQueue.classify(error)
                        }
                    }
                }
                let backgroundProjection = SQLiteNodeProjectionRepository(
                    database: database, provider: backgroundProvider, bridge: bridge)
                let backgroundInbound = InboundSyncCoordinator(
                    feed: SyncFeedService(
                        provider: backgroundProvider, queue: queue, store: database, bridge: bridge),
                    application: SyncFeedApplicationService(
                        provider: backgroundProvider, queue: queue, database: database,
                        nodes: backgroundNodes, bridge: bridge),
                    ack: SyncAckService(
                        provider: backgroundProvider,
                        projection: CommittedSQLiteSyncProjectionStorage(
                            database: database, bridge: bridge),
                        bridge: bridge),
                    projection: backgroundProjection, queue: queue, database: database,
                    provider: backgroundProvider, bridge: bridge)
                let backgroundRestoration = SessionRestorationService(
                    credentialStore: store,
                    authorizationValidator: AuthenticatedSessionValidationService(
                        transport: backgroundTransport),
                    authorizationEvidence: SQLiteSessionAuthorizationEvidence(database: database))
                backgroundSyncCoordinator = BackgroundSyncCoordinator(
                    policy: backgroundPolicy,
                    work: BackgroundSyncWork(
                        provider: backgroundProvider, database: database, bridge: bridge,
                        restoration: backgroundRestoration, inbound: backgroundInbound,
                        sessionProvider: browserProvider),
                    gate: backgroundGate)
                var composedMetadataMutationService: MetadataMutationService?
                if let nodeRepository {
                    let feature = MetadataMutationService(
                        provider: browserProvider, queue: queue,
                        checkpointService: checkpoint, coordinator: coordinator,
                        nodeRepository: nodeRepository, bridge: bridge,
                        identityGenerator: bridge)
                    composedMetadataMutationService = feature
                    metadataMutationFeature = feature
                }
                let composedRebaseline = rebaselineCoordinator
                do {
                    let staging = try await Task.detached {
                        try DownloadStagingStore(root: DownloadStagingStore.productionURL())
                    }.value
                    try? await staging.cleanupOrphanExports()
                    downloadCoordinator = FileDownloadCoordinator(
                        repository: AuthenticatedFileDownloadRepository(
                            provider: browserProvider, bridge: bridge),
                        database: database, staging: staging)
                    let uploadRepository = AuthenticatedUploadRepository(
                        provider: browserProvider, bridge: bridge)
                    let uploadStaging = UploadStagingStore(shared: staging)
                    let backgroundUploads = BackgroundUploadCoordinator(
                        repository: uploadRepository,
                        database: database, staging: uploadStaging,
                        transport: URLSessionBackgroundUploadTransport())
                    BackgroundUploadEventDelivery.shared.handler = {
                        [backgroundUploads] task, evidence in
                        await backgroundUploads.receive(task, evidence: evidence)
                    }
                    BackgroundUploadEventDelivery.shared.activityHandler = {
                        [backgroundUploads] task in
                        await backgroundUploads.recordActivity(task)
                    }
                    BackgroundUploadEventDelivery.shared.processPendingEvents()
                    uploadCoordinator = FileUploadCoordinator(
                        repository: uploadRepository,
                        database: database, staging: uploadStaging, identityGenerator: bridge,
                        background: backgroundUploads)
                    if let downloadCoordinator {
                        fileExportCoordinator = VerifiedFileExportCoordinator(
                            downloads: downloadCoordinator)
                    }
                } catch { downloadCoordinator = nil }
                let downloads = downloadCoordinator
                let uploads = uploadCoordinator
                let exports = fileExportCoordinator
                sessionController.installMutationSessionInvalidator {
                    [
                        weak queue, weak composedMetadataMutationService, weak composedRebaseline,
                        weak downloads, weak uploads, weak exports
                    ] in
                    downloads?.invalidateSession()
                    uploads?.invalidateSession()
                    exports?.invalidateSession()
                    queue?.invalidateSession()
                    composedRebaseline?.invalidateSession()
                    composedMetadataMutationService?.invalidateSession()
                    BGAppRefreshScheduler.shared.invalidate(disable: true)
                }
            } catch {
                durableMutationQueue = nil
                backgroundDatabase = nil
                mutationDrainCoordinator = nil
                syncCheckpointService = nil
                syncFeedService = nil
                nodeProjectionRepository = nil
                offlineNodeBrowserService = nil
                syncFeedApplicationService = nil
                rebaselineCoordinator = nil
                downloadCoordinator = nil
                uploadCoordinator = nil
                fileExportCoordinator = nil
                syncAckService = nil
                inboundSyncCoordinator = nil
                backgroundSyncCoordinator = nil
                metadataMutationFeature = nil
                mutationQueueFailure = DurableMutationQueue.classify(error)
            }
            rustBridge = bridge
            credentialSink = store
            restorationService = service
            logoutService = logout
            sessionController.installRestorationService(service)
            sessionController.installLogoutService(logout)
        } catch {
            clientMutationRepository = nil
            libraryCatalog = nil
            nodeRepository = nil
            offlineNodeBrowserService = nil
            metadataMutationFeature = nil
            rustBridge = nil
            credentialSink = nil
            restorationService = nil
            logoutService = nil
            sessionController.markRestorationDependenciesUnavailable()
        }
    }
}
