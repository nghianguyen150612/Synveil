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
    private(set) var syncAckService: SyncAckService?
    private(set) var inboundSyncCoordinator: (any InboundSyncCoordinatorProtocol)?
    private(set) var metadataMutationFeature: (any MetadataMutationFeatureProtocol)?
    private(set) var mutationQueueFailure: MutationQueueFailure?

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
            let service = SessionRestorationService(
                credentialStore: store,
                authorizationValidator: authenticatedProbe
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
                switch await queue.recoverInterruptedOperations() {
                case .recovered: break
                case .failed(let failure): throw failure
                }
                durableMutationQueue = queue
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
                }
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
                sessionController.installMutationSessionInvalidator {
                    [weak queue, weak composedMetadataMutationService, weak composedRebaseline] in
                    queue?.invalidateSession()
                    composedRebaseline?.invalidateSession()
                    composedMetadataMutationService?.invalidateSession()
                }
            } catch {
                durableMutationQueue = nil
                mutationDrainCoordinator = nil
                syncCheckpointService = nil
                syncFeedService = nil
                nodeProjectionRepository = nil
                offlineNodeBrowserService = nil
                syncFeedApplicationService = nil
                rebaselineCoordinator = nil
                syncAckService = nil
                inboundSyncCoordinator = nil
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
