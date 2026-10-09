import Foundation
import XCTest

@testable import Synveil

@MainActor
final class LibraryCatalogViewModelTests: XCTestCase {
    func testInitialStateIsIdle() throws {
        let fixture = try makeFixture()
        XCTAssertEqual(fixture.viewModel.state, .idle)
        XCTAssertEqual(fixture.repository.requestCount, 0)
    }

    func testAuthenticatedEntryStartsLoadingAndLoadsLibraries() async throws {
        let library = try await makeLibrary(1, name: "Archive")
        let fixture = try makeFixture(results: [.loaded([library])])

        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)
        XCTAssertEqual(fixture.viewModel.state, .loading)
        await load.value

        XCTAssertEqual(fixture.viewModel.state, .loaded([library]))
    }

    func testEmptyResponseProducesEmptyState() async throws {
        let fixture = try makeFixture(results: [.loaded([])])

        await fixture.viewModel.loadIfNeeded()

        XCTAssertEqual(fixture.viewModel.state, .empty)
    }

    func testFailureResponseProducesTypedUIFailure() async throws {
        let fixture = try makeFixture(results: [.failed(.offline)])

        await fixture.viewModel.loadIfNeeded()

        XCTAssertEqual(fixture.viewModel.state, .failed(.library(.offline)))
    }

    func testRepeatedInitialLoadDoesNotRepeatRequest() async throws {
        let fixture = try makeFixture(results: [.loaded([])])

        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.loadIfNeeded()

        XCTAssertEqual(fixture.repository.requestCount, 1)
    }

    func testDuplicateInitialLoadIsSuppressedWhileRequestIsPending() async throws {
        let library = try await makeLibrary(1, name: "Archive")
        let fixture = try makeFixture(suspendedRequests: [1])
        let firstLoad = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)

        await fixture.viewModel.loadIfNeeded()

        XCTAssertEqual(fixture.repository.requestCount, 1)
        fixture.repository.complete(request: 1, with: .loaded([library]))
        await firstLoad.value
        XCTAssertEqual(fixture.viewModel.state, .loaded([library]))
    }

    func testRefreshStartsANewRepositoryRequest() async throws {
        let first = try await makeLibrary(1, name: "Archive")
        let second = try await makeLibrary(2, name: "Photos")
        let fixture = try makeFixture(results: [.loaded([first]), .loaded([second])])
        await fixture.viewModel.loadIfNeeded()

        await fixture.viewModel.refresh()

        XCTAssertEqual(fixture.repository.requestCount, 2)
        XCTAssertEqual(fixture.viewModel.state, .loaded([second]))
    }

    func testDuplicateRefreshIsSuppressedWhileRequestIsPending() async throws {
        let initial = try await makeLibrary(1, name: "Archive")
        let refreshed = try await makeLibrary(2, name: "Photos")
        let fixture = try makeFixture(results: [.loaded([initial])], suspendedRequests: [2])
        await fixture.viewModel.loadIfNeeded()
        let refresh = Task { await fixture.viewModel.refresh() }
        await fixture.repository.waitForRequestCount(2)

        let duplicate = Task { await fixture.viewModel.refresh() }
        await duplicate.value

        XCTAssertEqual(fixture.repository.requestCount, 2)
        XCTAssertEqual(fixture.viewModel.state, .refreshing([initial]))
        fixture.repository.complete(request: 2, with: .loaded([refreshed]))
        await refresh.value
        XCTAssertEqual(fixture.viewModel.state, .loaded([refreshed]))
    }

    func testRefreshCompletionClearsProgressState() async throws {
        let initial = try await makeLibrary(1, name: "Archive")
        let refreshed = try await makeLibrary(2, name: "Photos")
        let fixture = try makeFixture(results: [.loaded([initial]), .loaded([refreshed])])
        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()

        XCTAssertFalse(fixture.viewModel.isRequestInProgress)
        XCTAssertEqual(fixture.viewModel.state, .loaded([refreshed]))
    }

    func testRefreshFailureIsDisplayedWithPreviousDataClearlyDistinguished() async throws {
        let initial = try await makeLibrary(1, name: "Archive")
        let fixture = try makeFixture(results: [.loaded([initial]), .failed(.offline)])
        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()

        XCTAssertEqual(fixture.viewModel.state, .refreshFailed([initial], .library(.offline)))
        XCTAssertEqual(fixture.viewModel.visibleLibraries, [initial])
    }

    func testRetainedDataIsTransientAndExplicitlyPrevious() async throws {
        let initial = try await makeLibrary(1, name: "Archive")
        let fixture = try makeFixture(results: [.loaded([initial]), .failed(.timeout)])
        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()

        guard case .refreshFailed(let displayed, _) = fixture.viewModel.state else {
            return XCTFail("A failed refresh should explicitly identify previously loaded data")
        }
        XCTAssertEqual(displayed, [initial])
        XCTAssertFalse(
            String(describing: fixture.viewModel.state).localizedCaseInsensitiveContains("cache"))
    }

    func testSessionChangeClearsTransientCatalogImmediately() async throws {
        let initial = try await makeLibrary(1, name: "Archive")
        let fixture = try makeFixture(results: [.loaded([initial])])
        await fixture.viewModel.loadIfNeeded()

        fixture.sessionController.requireRecovery(.credential)
        fixture.viewModel.sessionDidChange()

        XCTAssertEqual(fixture.viewModel.state, .invalidated)
        XCTAssertNil(fixture.viewModel.visibleLibraries)
    }

    func testLogoutInvalidatesPendingCatalogRequest() async throws {
        let fixture = try makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)

        await fixture.sessionController.requestLogout()
        fixture.viewModel.sessionDidChange()

        XCTAssertEqual(fixture.viewModel.state, .invalidated)
        fixture.repository.complete(request: 1, with: .loaded([]))
        await load.value
        XCTAssertEqual(fixture.viewModel.state, .invalidated)
    }

    func testLateSuccessAfterLogoutIsIgnored() async throws {
        let library = try await makeLibrary(1, name: "Archive")
        let fixture = try makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)

        await fixture.sessionController.requestLogout()
        fixture.viewModel.sessionDidChange()
        fixture.repository.complete(request: 1, with: .loaded([library]))
        await load.value

        XCTAssertEqual(fixture.viewModel.state, .invalidated)
        XCTAssertNil(fixture.viewModel.library(with: library.id))
    }

    func testLateFailureAfterLogoutIsIgnored() async throws {
        let fixture = try makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)

        await fixture.sessionController.requestLogout()
        fixture.viewModel.sessionDidChange()
        fixture.repository.complete(request: 1, with: .failed(.offline))
        await load.value

        XCTAssertEqual(fixture.viewModel.state, .invalidated)
    }

    func testCredentialReplacementFailureClearsPreviouslyLoadedData() async throws {
        let initial = try await makeLibrary(1, name: "Archive")
        let fixture = try makeFixture(results: [.loaded([initial]), .failed(.staleSession)])
        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()

        XCTAssertEqual(fixture.viewModel.state, .failed(.library(.staleSession)))
        XCTAssertNil(fixture.viewModel.visibleLibraries)
    }

    func testLifecycleRevisionChangeCannotPublishLateAuthenticatedResult() async throws {
        let library = try await makeLibrary(1, name: "Archive")
        let fixture = try makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)

        // Recovery changes the authoritative lifecycle revision and removes the authenticated scope.
        fixture.sessionController.requireRecovery(.scopeMismatch)
        fixture.viewModel.sessionDidChange()
        fixture.repository.complete(request: 1, with: .loaded([library]))
        await load.value

        XCTAssertEqual(fixture.viewModel.state, .invalidated)
        XCTAssertNil(fixture.viewModel.visibleLibraries)
        XCTAssertEqual(fixture.repository.wasCancelled(request: 1), true)
    }

    func testCancellationDoesNotProduceFalseSuccess() async throws {
        let fixture = try makeFixture(results: [.failed(.cancelled)])

        await fixture.viewModel.loadIfNeeded()

        XCTAssertEqual(fixture.viewModel.state, .cancelled)
    }

    func testRecoveryTransitionRemovesAuthenticatedCatalogContent() async throws {
        let library = try await makeLibrary(1, name: "Archive")
        let fixture = try makeFixture(results: [.loaded([library])])
        await fixture.viewModel.loadIfNeeded()

        fixture.sessionController.requireRecovery(.deviceRevoked)
        fixture.viewModel.sessionDidChange()

        XCTAssertEqual(fixture.viewModel.state, .invalidated)
        XCTAssertNil(fixture.viewModel.library(with: library.id))
    }

    func testOfflineAndHTTP503FailuresDoNotInitiateLogout() async throws {
        let logoutRecorder = LogoutCallRecorder()
        let offlineFixture = try makeFixture(
            results: [.failed(.offline)],
            logoutService: logoutRecorder
        )
        await offlineFixture.viewModel.loadIfNeeded()
        XCTAssertEqual(offlineFixture.sessionController.state, .authenticated)
        XCTAssertTrue(offlineFixture.viewModel.canRefresh)

        let unavailableFixture = try makeFixture(
            results: [.failed(.serverUnavailable)],
            logoutService: logoutRecorder
        )
        await unavailableFixture.viewModel.loadIfNeeded()
        XCTAssertEqual(unavailableFixture.sessionController.state, .authenticated)
        let logoutCalls = await logoutRecorder.callCount()
        XCTAssertEqual(logoutCalls, 0)
    }

    func testDeviceRevocationDoesNotOfferGenericRetry() {
        let revoked = LibraryCatalogUIFailure.library(.deviceRevoked)
        XCTAssertFalse(revoked.canRetry)
        XCTAssertTrue(revoked.message.contains("revoked"))
    }

    func testPermanentAuthenticationFailuresDisableRefreshRetry() async throws {
        for failure in [LibraryFailure.authenticationRejected, .deviceRevoked] {
            let fixture = try makeFixture(results: [.failed(failure)])
            await fixture.viewModel.loadIfNeeded()

            XCTAssertFalse(fixture.viewModel.canRefresh)
        }
    }

    func testEveryLibraryFailureHasConciseSafePresentation() {
        let failures: [LibraryFailure] = [
            .unauthenticated,
            .credentialUnavailable,
            .invalidCredential,
            .originMismatch,
            .staleSession,
            .authenticationRejected,
            .deviceRevoked,
            .offline,
            .dnsFailure,
            .timeout,
            .tlsFailure,
            .redirectRejected,
            .serverUnavailable,
            .httpFailure(statusCode: 503),
            .unexpectedContentType,
            .protocolFailure,
            .resourceLimit,
            .repeatedCursor,
            .cancelled,
        ]

        for failure in failures {
            let presentation = LibraryCatalogUIFailure.library(failure)
            XCTAssertFalse(presentation.title.isEmpty)
            XCTAssertFalse(presentation.message.isEmpty)
            XCTAssertFalse(presentation.message.contains("svd1_"))
            XCTAssertFalse(presentation.accessibilityDescription.isEmpty)
        }
    }

    func testOnlyTemporaryNetworkFailuresOfferSafeRetryAndMayRetainData() {
        for failure in [LibraryFailure.offline, .dnsFailure, .timeout, .serverUnavailable] {
            let presentation = LibraryCatalogUIFailure.library(failure)
            XCTAssertTrue(presentation.canRetry)
            XCTAssertTrue(presentation.mayRetainPreviouslyLoadedData)
        }

        for failure in [
            LibraryFailure.credentialUnavailable,
            .invalidCredential,
            .staleSession,
            .authenticationRejected,
            .deviceRevoked,
        ] {
            let presentation = LibraryCatalogUIFailure.library(failure)
            XCTAssertFalse(presentation.canRetry)
            XCTAssertFalse(presentation.mayRetainPreviouslyLoadedData)
        }
    }

    func testLibraryStatusesMapToDistinctLabelsAndSymbols() {
        let active = LibraryStatusPresentation.make(for: .active)
        let readOnly = LibraryStatusPresentation.make(for: .readOnly)
        let quarantined = LibraryStatusPresentation.make(for: .quarantined)

        XCTAssertEqual(active.label, "Active")
        XCTAssertEqual(readOnly.label, "Read Only")
        XCTAssertEqual(quarantined.label, "Quarantined")
        XCTAssertNotEqual(active.symbol, readOnly.symbol)
        XCTAssertNotEqual(active.symbol, quarantined.symbol)
    }

    func testLibraryIDsRemainStableAcrossPresentationOrdering() async throws {
        let archive = try await makeLibrary(1, name: "Archive")
        let photos = try await makeLibrary(2, name: "Photos")
        let fixture = try makeFixture(results: [.loaded([photos, archive])])

        await fixture.viewModel.loadIfNeeded()

        XCTAssertEqual(fixture.viewModel.visibleLibraries?.map(\.id), [archive.id, photos.id])
        XCTAssertEqual(archive.id.rawValue, "018f9b9f-5c21-722e-8b1a-000000000001")
    }

    func testLongUTF8LibraryNameDoesNotChangeDomainIdentity() async throws {
        let name = String(repeating: "界", count: 340)
        let library = try await makeLibrary(1, name: name)
        let fixture = try makeFixture(results: [.loaded([library])])

        await fixture.viewModel.loadIfNeeded()

        XCTAssertEqual(fixture.viewModel.visibleLibraries?.first?.name, name)
        XCTAssertEqual(fixture.viewModel.visibleLibraries?.first?.id, library.id)
    }

    func testInvalidatedViewModelCannotPublishNewCatalogData() async throws {
        let library = try await makeLibrary(1, name: "Archive")
        let fixture = try makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)
        fixture.viewModel.invalidate()
        fixture.repository.complete(request: 1, with: .loaded([library]))
        await load.value

        XCTAssertEqual(fixture.viewModel.state, .invalidated)
        XCTAssertNil(fixture.viewModel.visibleLibraries)
    }

    func testMissingRepositoryFailsClosedWithoutDemoData() async throws {
        let controller = try makeAuthenticatedController()
        let viewModel = LibraryCatalogViewModel(repository: nil, sessionController: controller)

        await viewModel.loadIfNeeded()

        XCTAssertEqual(viewModel.state, .failed(.repositoryUnavailable))
        XCTAssertNil(viewModel.visibleLibraries)
    }

    private func makeFixture(
        results: [LibraryRepositoryResult] = [],
        suspendedRequests: Set<Int> = [],
        logoutService: any SessionLogoutServiceProtocol = ImmediateLogoutService()
    ) throws -> Fixture {
        let sessionController = try makeAuthenticatedController(logoutService: logoutService)
        let repository = ControlledCatalogRepository(
            results: results,
            suspendedRequests: suspendedRequests
        )
        let viewModel = LibraryCatalogViewModel(
            repository: repository,
            sessionController: sessionController
        )
        return Fixture(
            sessionController: sessionController,
            repository: repository,
            viewModel: viewModel
        )
    }

    private func makeAuthenticatedController(
        logoutService: any SessionLogoutServiceProtocol = ImmediateLogoutService()
    ) throws -> SessionController {
        let endpoint = try ServerEndpoint(validating: "https://catalog.synveil.example")
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint),
            logoutService: logoutService
        )
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(endpoint)
        controller.requireEnrollment()

        let record = try DeviceCredentialRecord(
            ownerUserId: testCatalogID(90),
            deviceId: testCatalogID(91),
            credentialId: testCatalogID(92),
            credential: DeviceCredential(
                validatedRawValue: "svd1_" + String(repeating: "a", count: 64)
            ),
            createdAt: "2026-10-09T12:00:00Z"
        )
        controller.markAuthenticated(
            after: SecureCredentialPersistenceReceipt(
                session: DeviceCredentialSession(serverEndpoint: endpoint, record: record)
            )
        )
        XCTAssertEqual(controller.state, .authenticated)
        return controller
    }

    private func makeLibrary(
        _ number: Int,
        name: String,
        status: LibraryStatus = .active
    ) async throws -> Library {
        let validator = CatalogViewModelTestValidator()
        return Library(
            id: try await LibraryId.validated(testCatalogID(number), using: validator),
            revision: try LibraryRevision(validating: "42"),
            name: name,
            rootNodeId: try await NodeId.validated(
                testCatalogID(number + 10_000), using: validator),
            status: status,
            createdAt: Date(timeIntervalSince1970: 1_760_000_000),
            updatedAt: Date(timeIntervalSince1970: 1_760_000_100)
        )
    }

    private func testCatalogID(_ number: Int) -> String {
        "018f9b9f-5c21-722e-8b1a-" + String(format: "%012x", number)
    }
}

private struct Fixture {
    let sessionController: SessionController
    let repository: ControlledCatalogRepository
    let viewModel: LibraryCatalogViewModel
}

@MainActor
private final class ControlledCatalogRepository: LibraryCatalogRepositoryProtocol {
    private var queuedResults: [LibraryRepositoryResult]
    private var suspendedRequests: Set<Int>
    private var responseContinuations: [Int: CheckedContinuation<LibraryRepositoryResult, Never>] =
        [:]
    private var requestWaiters: [(Int, CheckedContinuation<Void, Never>)] = []
    private var cancellationStates: [Int: Bool] = [:]
    private(set) var requestCount = 0

    init(results: [LibraryRepositoryResult], suspendedRequests: Set<Int>) {
        queuedResults = results
        self.suspendedRequests = suspendedRequests
    }

    func listLibraries() async -> LibraryRepositoryResult {
        requestCount += 1
        let request = requestCount
        resumeRequestWaiters()

        if suspendedRequests.remove(request) != nil {
            let result = await withCheckedContinuation { responseContinuations[request] = $0 }
            cancellationStates[request] = Task.isCancelled
            return result
        }
        if !queuedResults.isEmpty { return queuedResults.removeFirst() }
        return .failed(.offline)
    }

    func waitForRequestCount(_ count: Int) async {
        if requestCount >= count { return }
        await withCheckedContinuation { continuation in
            requestWaiters.append((count, continuation))
        }
    }

    func complete(request: Int, with result: LibraryRepositoryResult) {
        responseContinuations.removeValue(forKey: request)?.resume(returning: result)
    }

    func wasCancelled(request: Int) -> Bool? { cancellationStates[request] }

    private func resumeRequestWaiters() {
        let ready = requestWaiters.filter { $0.0 <= requestCount }
        requestWaiters.removeAll { $0.0 <= requestCount }
        for (_, continuation) in ready { continuation.resume() }
    }
}

private struct ImmediateLogoutService: SessionLogoutServiceProtocol {
    func logoutLocally() async -> SessionLogoutResult { .credentialAbsent }
}

private actor LogoutCallRecorder: SessionLogoutServiceProtocol {
    private var calls = 0

    func logoutLocally() async -> SessionLogoutResult {
        calls += 1
        return .credentialAbsent
    }

    func callCount() -> Int { calls }
}

private struct CatalogViewModelTestValidator: RustBridgeProtocol {
    func parseSHA256(_ canonical: String) async throws -> Data { Data() }
    func formatSHA256(_ digest: Data) async throws -> String { "" }
    func validateEnrollmentToken(_ token: String) async throws -> Bool { false }
    func validateDeviceBearerToken(_ token: String) async throws -> Bool {
        DeviceCredential.isValid(token)
    }
    func validateLibraryID(_ value: String) async throws -> Bool {
        LibraryWireValidation.matches(
            value,
            pattern: "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
        )
    }
    func validateNodeID(_ value: String) async throws -> Bool {
        try await validateLibraryID(value)
    }
    func validateLogicalName(_ value: String) async throws -> Bool {
        !value.isEmpty && value.utf8.count <= 1024
    }
}
