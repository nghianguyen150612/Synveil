import Foundation
import SwiftUI
import UIKit
import XCTest

@testable import Synveil

private struct InspectionAuthorization: AuthenticatedSessionValidationProtocol {
    let result: AuthenticatedSessionValidationResult
    func validateAuthorization(for session: DeviceCredentialSession) async
        -> AuthenticatedSessionValidationResult
    { result }
}

@MainActor
private final class InspectionLiveReader: NodeRepositoryProtocol {
    var requests = 0
    func listChildren(libraryId: LibraryId, parent: NodeParentScope) async -> NodeRepositoryResult {
        requests += 1
        return .failed(.protocolFailure)
    }
    func getNode(libraryId: LibraryId, nodeId: NodeId, expectedParent: NodeParentScope) async
        -> NodeDetailsRepositoryResult
    { .unavailable }
    func getNodeMetadata(libraryId: LibraryId, nodeId: NodeId) async -> NodeMetadataRepositoryResult
    { .unavailable }
}

@MainActor
final class SavedMetadataInspectionTests: XCTestCase {
    private func library(_ f: QueueFixture) async throws -> Library {
        Library(
            id: f.scope.libraryId, revision: try LibraryRevision(validating: "8"),
            name: "Private saved Library",
            rootNodeId: try await NodeId.validated(queueUUID(2), using: QueueValidator()),
            status: .active,
            createdAt: Date(timeIntervalSince1970: 1), updatedAt: Date(timeIntervalSince1970: 2))
    }

    private func pending(
        _ f: QueueFixture, result: AuthenticatedSessionValidationResult = .networkFailure
    ) async -> SessionController {
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: f.scope.serverEndpoint),
            restorationService: SessionRestorationService(
                credentialStore: f.credentials,
                authorizationValidator: InspectionAuthorization(result: result),
                authorizationEvidence: SQLiteSessionAuthorizationEvidence(database: f.database)),
            logoutService: SessionLogoutService(credentialStore: f.credentials))
        controller.installMutationSessionInvalidator { f.queue.invalidateSession() }
        await controller.start()
        return controller
    }

    func testRelaunchInspectsExistingProjectionWithoutAnyNetworkRequest() async throws {
        let f = try await queueFixture(self)
        let library = try await library(f)
        guard case .applied = try await projectionApply(f) else {
            return XCTFail("Projection must commit")
        }
        try await projectionCache(f).rememberLibrary(library, scope: f.scope)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let controller = await pending(f)
        let live = InspectionLiveReader()
        let service = SavedMetadataInspectionService(
            controller: controller, credentials: f.credentials,
            database: reopened, queue: f.queue, bridge: QueueValidator(), liveRepository: live)
        let before = await f.transport.requests()
        XCTAssertEqual(controller.state, .restorationVerificationPending)
        let saved = try await service.libraries()
        XCTAssertEqual(saved, [library])
        let result = await service.browse(
            library: library, parent: .libraryRoot(rootNodeId: library.rootNodeId))
        guard case .saved(let directory) = result else {
            return XCTFail("Expected saved projection")
        }
        XCTAssertEqual(directory.nodes.count, 1)
        XCTAssertEqual(directory.knowledge, .partial)
        XCTAssertEqual(directory.projection.serverConfirmed?.sequence.rawValue, "0")
        XCTAssertEqual(directory.projection.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(live.requests, 0)
        let after = await f.transport.requests()
        XCTAssertEqual(after, before)
        do {
            _ = try await AuthenticatedLibraryRequestProvider(
                controller: controller,
                store: f.credentials, transport: f.transport
            ).begin()
            XCTFail("Pending session cannot authorize network work")
        } catch { XCTAssertEqual(error as? LibraryFailure, .unauthenticated) }
    }

    func testOfflineConflictReviewUsesP047ScopeWithoutNetworkOrWrites() async throws {
        let f = try await queueFixture(self)
        let savedLibrary = try await library(f)
        let mutation = try await queuePrepared(id: 911, scope: f.scope)
        let queued = try await queueEnqueued(f, mutation: mutation)
        let lease = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: queued.mutation.id)
        try await f.queue.finish(lease, result: queueConflict(mutation))
        try await projectionCache(f).rememberLibrary(savedLibrary, scope: f.scope)
        let beforeRequests = await f.transport.requests()
        guard
            case .record(let before) = await f.queue.get(
                scope: f.scope, mutationId: mutation.id)
        else { return XCTFail("Expected durable conflict evidence") }

        let controller = await pending(f)
        let service = SavedMetadataInspectionService(
            controller: controller, credentials: f.credentials, database: f.database,
            queue: f.queue, bridge: QueueValidator(), liveRepository: InspectionLiveReader())
        XCTAssertEqual(controller.state, .restorationVerificationPending)

        guard
            case .loaded(let snapshot) = await service.conflictReview(
                library: savedLibrary, limit: 100)
        else { return XCTFail("Expected P047-authorized saved conflict inspection") }

        XCTAssertEqual(snapshot.knownConflictCount, 1)
        XCTAssertTrue(snapshot.isComplete)
        XCTAssertEqual(snapshot.provenance, .savedOnThisDevice)
        XCTAssertEqual(snapshot.items.first?.id, mutation.id.rawValue)
        XCTAssertEqual(snapshot.items.first?.state, .conflict)
        XCTAssertEqual(snapshot.items.first?.conflictReason, .revisionMismatch)
        let afterRequests = await f.transport.requests()
        XCTAssertEqual(afterRequests, beforeRequests)
        guard
            case .record(let after) = await f.queue.get(
                scope: f.scope, mutationId: mutation.id)
        else { return XCTFail("Saved review removed durable conflict evidence") }
        XCTAssertEqual(after, before)
    }

    func testOfflineConflictReviewDoesNotExposeFormerAccountAfterCredentialReplacement()
        async throws
    {
        let f = try await queueFixture(self)
        let savedLibrary = try await library(f)
        let mutation = try await queuePrepared(id: 912, scope: f.scope)
        let queued = try await queueEnqueued(f, mutation: mutation)
        let lease = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: queued.mutation.id)
        try await f.queue.finish(lease, result: queueConflict(mutation))
        try await projectionCache(f).rememberLibrary(savedLibrary, scope: f.scope)
        let controller = await pending(f)
        let service = SavedMetadataInspectionService(
            controller: controller, credentials: f.credentials, database: f.database,
            queue: f.queue, bridge: QueueValidator(), liveRepository: InspectionLiveReader())
        await f.credentials.replace(try queueSession(f.scope, credential: 913))

        guard case .failed = await service.conflictReview(library: savedLibrary, limit: 100)
        else { return XCTFail("A replaced credential must not read the former account") }
    }

    func testCredentialReplacementDuringConflictStorageReadDiscardsTheResult() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let savedLibrary = try await library(f)
        let mutation = try await queuePrepared(id: 914, scope: f.scope)
        let queued = try await queueEnqueued(f, mutation: mutation)
        let lease = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: queued.mutation.id)
        try await f.queue.finish(lease, result: queueConflict(mutation))
        try await projectionCache(f).rememberLibrary(savedLibrary, scope: f.scope)
        let controller = await pending(f)
        let service = SavedMetadataInspectionService(
            controller: controller, credentials: f.credentials, database: f.database,
            queue: f.queue, bridge: QueueValidator(), liveRepository: InspectionLiveReader())
        let gate = QueueStorageReadGate()
        fault.arm(.beforeConflictRead, action: { gate.suspend() })
        let read = Task {
            await service.conflictReview(library: savedLibrary, limit: 100)
        }

        await gate.waitUntilEntered()
        await f.credentials.replace(try queueSession(f.scope, credential: 915))
        gate.release()

        guard case .failed = await read.value else {
            return XCTFail("The stale credential must not publish a completed conflict read")
        }
        let stillConflict = try await f.database.record(
            scope: f.scope, id: mutation.id.rawValue)
        XCTAssertEqual(stillConflict?.state, .conflict)
    }

    func testOnlyTransientVerificationFailuresEnableSavedInspection() async throws {
        let f = try await queueFixture(self)
        for result in [
            AuthenticatedSessionValidationResult.networkFailure, .dnsFailure, .timeout,
            .serverUnavailable,
        ] {
            let controller = await pending(f, result: result)
            XCTAssertTrue(controller.canInspectSavedMetadata)
        }
        for result in [
            AuthenticatedSessionValidationResult.authenticationRejected, .deviceRevoked,
            .tlsFailure, .unexpectedProtocolResponse, .unexpectedContentType, .redirectRejected,
            .cancelled,
        ] {
            let controller = await pending(f, result: result)
            XCTAssertFalse(controller.canInspectSavedMetadata)
            do {
                _ = try await SavedMetadataSessionProvider(
                    controller: controller, credentials: f.credentials,
                    bridge: QueueValidator(), authorizationStore: f.database
                ).begin()
                XCTFail("Saved access must be denied")
            } catch {}
        }
    }

    func testCredentialReplacementDuringReadCannotPublishSavedMetadata() async throws {
        let f = try await queueFixture(self)
        let controller = await pending(f)
        let provider = SavedMetadataSessionProvider(
            controller: controller, credentials: f.credentials, bridge: QueueValidator(),
            authorizationStore: f.database)
        let gate = QueueGate()
        await f.credentials.suspendNextLoad(gate)
        let task = Task { try await provider.begin() }
        await gate.wait()
        await f.credentials.replace(try queueSession(f.scope, credential: 192))
        await gate.release()
        do {
            _ = try await task.value
            XCTFail("Replaced credential cannot inspect the old account")
        } catch { XCTAssertEqual(error as? LibraryFailure, .staleSession) }
    }

    func testLogoutDuringSavedReadFencesResultAndQuarantinesEvidence() async throws {
        let f = try await queueFixture(self)
        let library = try await library(f)
        try await projectionCache(f).rememberLibrary(library, scope: f.scope)
        let controller = await pending(f)
        let provider = SavedMetadataSessionProvider(
            controller: controller, credentials: f.credentials, bridge: QueueValidator(),
            authorizationStore: f.database)
        let gate = QueueGate()
        await f.credentials.suspendNextLoad(gate)
        let task = Task { try await provider.begin() }
        await gate.wait()
        await controller.requestLogout()
        await gate.release()
        do {
            _ = try await task.value
            XCTFail("Logout must invalidate the read")
        } catch {}
        XCTAssertFalse(controller.canInspectSavedMetadata)
        _ = await f.queue.pending(scope: f.scope, limit: 1)
        do {
            _ = try await f.database.cachedProjectionState(
                scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
            XCTFail("Quarantined metadata must remain inaccessible")
        } catch {}
    }

    func testOtherCredentialAndQuarantinedScopesCannotAppearInSavedCatalog() async throws {
        let f = try await queueFixture(self)
        let library = try await library(f)
        try await projectionCache(f).rememberLibrary(library, scope: f.scope)
        let controller = await pending(f)
        let provider = SavedMetadataSessionProvider(
            controller: controller, credentials: f.credentials, bridge: QueueValidator(),
            authorizationStore: f.database)
        let session = try await provider.begin()
        try await f.database.quarantineSessions()
        let saved = try await f.database.savedLibraries(session: session, bridge: QueueValidator())
        XCTAssertTrue(saved.isEmpty)
    }

    func testPreviouslyBoundScopeIsNotVisibleToAnotherOwnerOrCredential() async throws {
        let f = try await queueFixture(self)
        try await projectionCache(f).rememberLibrary(try await library(f), scope: f.scope)
        let other = try await queueScope(device: 191, owner: 190)
        await f.credentials.replace(try queueSession(other, credential: 192))
        let controller = await pending(f)
        let service = SavedMetadataInspectionService(
            controller: controller, credentials: f.credentials,
            database: f.database, queue: f.queue, bridge: QueueValidator(),
            liveRepository: InspectionLiveReader())
        let saved = try await service.libraries()
        XCTAssertTrue(saved.isEmpty)
    }

    func testRetryVerificationFencesAnOlderSavedReadWithoutPromotingAuthentication() async throws {
        let f = try await queueFixture(self)
        let controller = await pending(f)
        let provider = SavedMetadataSessionProvider(
            controller: controller, credentials: f.credentials, bridge: QueueValidator(),
            authorizationStore: f.database)
        let gate = QueueGate()
        await f.credentials.suspendNextLoad(gate)
        let task = Task { try await provider.begin() }
        await gate.wait()
        await controller.retrySessionRestoration()
        await gate.release()
        do {
            _ = try await task.value
            XCTFail("Old read must not survive a newer verification lifecycle")
        } catch { XCTAssertEqual(error as? LibraryFailure, .staleSession) }
        XCTAssertEqual(controller.state, .restorationVerificationPending)
    }

    func testStartupAuthorizationRejectionQuarantinesPriorMetadataAcrossRelaunch() async throws {
        let f = try await queueFixture(self)
        try await projectionCache(f).rememberLibrary(try await library(f), scope: f.scope)
        let rejected = await pending(f, result: .authenticationRejected)
        XCTAssertFalse(rejected.canInspectSavedMetadata)
        _ = await f.queue.pending(scope: f.scope, limit: 1)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let offline = await pending(f)
        let service = SavedMetadataInspectionService(
            controller: offline, credentials: f.credentials,
            database: reopened, queue: f.queue, bridge: QueueValidator(),
            liveRepository: InspectionLiveReader())
        XCTAssertTrue(offline.savedAuthorizationEvidenceUnresolved)
        do {
            _ = try await service.libraries()
            XCTFail("Unresolved authorization cannot expose saved metadata")
        } catch { XCTAssertEqual(error as? LibraryFailure, .unauthenticated) }
    }

    func testAuthoritativeMetadataPermissionDenialBlocksSavedScopeAcrossReopen() async throws {
        let f = try await queueFixture(self)
        let library = try await library(f)
        try await projectionCache(f).rememberLibrary(library, scope: f.scope)
        let nodes = AuthenticatedNodeRepository(provider: f.provider, bridge: QueueValidator())
        let cache = projectionCache(f)
        nodes.installReadDenialHandler { id, session in
            do { try await cache.rejectReadAuthorization(libraryId: id, session: session) } catch {
                XCTFail("Read denial must be durable: \(error)")
            }
        }
        await f.transport.set(try inboundError("permission_denied", status: 403))
        let result = await nodes.listChildren(
            libraryId: library.id,
            parent: .libraryRoot(rootNodeId: library.rootNodeId))
        XCTAssertEqual(result, .failed(.httpFailure(statusCode: 403)))
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let controller = await pending(f)
        let service = SavedMetadataInspectionService(
            controller: controller, credentials: f.credentials,
            database: reopened, queue: f.queue, bridge: QueueValidator(),
            liveRepository: InspectionLiveReader())
        let saved = try await service.libraries()
        XCTAssertTrue(saved.isEmpty)
    }

    func testQuarantinePersistenceFailureClosesProjectionReadAuthority() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let library = try await library(f)
        try await projectionCache(f).rememberLibrary(library, scope: f.scope)
        fault.arm(.beforeQuarantine, failure: .diskFull)
        do {
            try await f.database.quarantineMetadataScope(
                scope: f.scope, credentialId: queueUUID(92))
            XCTFail("Expected the durable failure")
        } catch { XCTAssertEqual(error as? MutationQueueFailure, .diskFull) }
        let cached = await projectionCache(f).projectionState(scope: f.scope)
        XCTAssertEqual(cached, .unavailable(.storage(.unavailable)))
    }

    func testLiveCatalogObservationUsesOnlyExistingScopesAndSupportsIncrementalSavedInspection()
        async throws
    {
        let f = try await queueFixture(self)
        let library = try await library(f)
        let session = try await f.provider.begin()
        try await projectionCache(f).rememberValidatedCatalog([library], session: session)
        let controller = await pending(f)
        let service = SavedMetadataInspectionService(
            controller: controller, credentials: f.credentials,
            database: f.database, queue: f.queue, bridge: QueueValidator(),
            liveRepository: InspectionLiveReader())
        let saved = try await service.libraries()
        XCTAssertEqual(saved, [library])
        let state = try await f.database.cachedProjectionState(
            scope: f.scope,
            credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(state.completeness, .uninitialized)
        let newScope = try await queueScope(library: 180)
        let newLibrary = Library(
            id: newScope.libraryId, revision: library.revision,
            name: "New Library without a sync base", rootNodeId: library.rootNodeId,
            status: .active, createdAt: library.createdAt, updatedAt: library.updatedAt)
        // Upload-only scopes can be bound without any synchronization base or Node projection.
        try await f.database.bindSession(scope: newScope, credentialId: queueUUID(92))
        try await projectionCache(f).rememberValidatedCatalog(
            [library, newLibrary], session: session)
        let base = try await f.database.syncBase(scope: newScope)
        XCTAssertNil(base)
        let after = try await service.libraries()
        XCTAssertEqual(after, [library])
    }

    func testCompleteCatalogRemovalQuarantinesOldSavedLibraryWithoutDeletingEvidence() async throws
    {
        let f = try await queueFixture(self)
        let session = try await f.provider.begin()
        try await projectionCache(f).rememberValidatedCatalog(
            [try await library(f)], session: session)
        let originalBase = try await f.database.syncBase(scope: f.scope)
        try await projectionCache(f).rememberValidatedCatalog([], session: session)
        let baseAfter = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(baseAfter?.responseBody, originalBase?.responseBody)
        let saved = try await f.database.savedLibraries(session: session, bridge: QueueValidator())
        XCTAssertTrue(saved.isEmpty)
    }

    func testAuthenticatedCatalogOutageStillAllowsExplicitLocalSavedInspection() async throws {
        let f = try await queueFixture(self)
        let library = try await library(f)
        try await projectionCache(f).rememberLibrary(library, scope: f.scope)
        let live = InspectionLiveReader()
        let service = SavedMetadataInspectionService(
            controller: f.controller, credentials: f.credentials,
            database: f.database, queue: f.queue, bridge: QueueValidator(), liveRepository: live)
        let before = await f.transport.requests()
        let model = SavedMetadataInspectionViewModel(service: service, controller: f.controller)
        await model.load()
        XCTAssertEqual(model.libraries, [library])
        XCTAssertEqual(f.controller.state, .authenticated)
        XCTAssertEqual(live.requests, 0)
        let after = await f.transport.requests()
        XCTAssertEqual(before, after)
        let view = LibraryCatalogView(
            repository: nil, savedMetadataInspection: service, sessionController: f.controller)
        XCTAssertNotNil(view.body)
    }

    func testCatalogObservationCannotRetargetAnExistingProjectionRoot() async throws {
        let f = try await queueFixture(self)
        let original = try await library(f)
        let session = try await f.provider.begin()
        try await projectionCache(f).rememberValidatedCatalog([original], session: session)
        let changed = Library(
            id: original.id, revision: try LibraryRevision(validating: "9"),
            name: original.name,
            rootNodeId: try await NodeId.validated(queueUUID(302), using: QueueValidator()),
            status: original.status, createdAt: original.createdAt, updatedAt: original.updatedAt)
        do {
            try await projectionCache(f).rememberValidatedCatalog([changed], session: session)
            XCTFail("Root identity must remain pinned to the existing projection")
        } catch { XCTAssertEqual(error as? SyncProjectionFailure, .scopeMismatch) }
        let state = try await f.database.cachedProjectionState(
            scope: f.scope,
            credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(state.library?.library.rootNodeId, original.rootNodeId)
    }

    func testMissingSavedCatalogIsUnavailableRatherThanEmptyServerCatalog() async throws {
        let f = try await queueFixture(self)
        let controller = await pending(f)
        let service = SavedMetadataInspectionService(
            controller: controller, credentials: f.credentials,
            database: f.database, queue: f.queue, bridge: QueueValidator(),
            liveRepository: InspectionLiveReader())
        let model = SavedMetadataInspectionViewModel(service: service, controller: controller)
        await model.load()
        XCTAssertTrue(model.libraries.isEmpty)
        XCTAssertTrue(model.isCurrent)
        let host = UIHostingController(
            rootView: SavedMetadataInspectionView(service: service, controller: controller)
                .environment(\.dynamicTypeSize, .accessibility5))
        host.loadViewIfNeeded()
        XCTAssertNotNil(host.view)
    }
}
