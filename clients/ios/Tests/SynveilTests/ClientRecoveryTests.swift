import Foundation
import SwiftUI
import UIKit
import XCTest

@testable import Synveil

@MainActor
final class ClientRecoveryTests: XCTestCase {
    func testUnrecognizedTransportFailuresCannotAuthorizeOfflineFallback() {
        struct UnknownFailure: Error {}
        XCTAssertEqual(SynveilTransportError.classify(UnknownFailure()), .malformedResponse)
        XCTAssertEqual(
            AuthenticatedLibraryCatalogRepository.classify(UnknownFailure()), .protocolFailure)
        for code in [
            URLError.badURL, .badServerResponse, .cannotDecodeContentData, .fileDoesNotExist,
            .unknown,
        ] {
            let error = URLError(code)
            XCTAssertEqual(SynveilTransportError.classify(error), .malformedResponse)
            XCTAssertEqual(DownloadPolicy.classify(error), .invalidMetadata)
            XCTAssertEqual(UploadPolicy.classify(error), .invalidMetadata)
        }
        XCTAssertFalse(OfflineNodeBrowserService.isEligibleForFallback(.protocolFailure))
    }

    func testKnownConnectionFailuresAndCancellationRemainTyped() {
        for code in [
            URLError.notConnectedToInternet, .networkConnectionLost, .cannotConnectToHost,
            .dataNotAllowed,
        ] {
            XCTAssertEqual(SynveilTransportError.classify(URLError(code)), .offline)
            XCTAssertEqual(DownloadPolicy.classify(URLError(code)), .offline)
            XCTAssertEqual(UploadPolicy.classify(URLError(code)), .offline)
        }
        XCTAssertEqual(SynveilTransportError.classify(URLError(.dnsLookupFailed)), .dnsFailure)
        XCTAssertEqual(SynveilTransportError.classify(URLError(.timedOut)), .timeout)
        XCTAssertEqual(
            SynveilTransportError.classify(URLError(.serverCertificateUntrusted)), .tlsError)
        XCTAssertEqual(SynveilTransportError.classify(CancellationError()), .cancelled)
    }

    func testUnknownRestorationTransportFailureCannotEnableSavedInspection() async throws {
        struct UnknownFailure: Error {}
        struct UnknownTransport: HTTPTransportProtocol {
            func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
                throw UnknownFailure()
            }
        }
        let session = try queueSession(try await queueScope())
        let credentials = QueueCredentialStore(session)
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: session.serverEndpoint),
            restorationService: SessionRestorationService(
                credentialStore: credentials,
                authorizationValidator: AuthenticatedSessionValidationService(
                    transport: UnknownTransport())))
        await controller.start()
        XCTAssertEqual(controller.state, .recoveryRequired(.protocolFailure))
        XCTAssertFalse(controller.canInspectSavedMetadata)
        let retained = try await credentials.load(expectedServerEndpoint: session.serverEndpoint)
        XCTAssertEqual(retained, session)
    }

    func testPermissionAndIntegrityFailuresAreNotOffline() {
        XCTAssertEqual(
            ClientRecoveryPresentation.classification(.httpFailure(statusCode: 403)),
            .permissionDenied)
        XCTAssertEqual(
            DownloadPolicy.classify(LibraryFailure.httpFailure(statusCode: 403)), .permissionDenied)
        XCTAssertEqual(
            UploadPolicy.classify(LibraryFailure.httpFailure(statusCode: 403)), .permissionDenied)
        XCTAssertEqual(
            ClientRecoveryPresentation.download(.sha256Mismatch).classification, .integrityFailure)
        XCTAssertTrue(ClientRecoveryPresentation.download(.sha256Mismatch).actions.isEmpty)
        XCTAssertEqual(
            ClientRecoveryPresentation.download(.permissionDenied).classification, .permissionDenied
        )
        XCTAssertFalse(DownloadFailure.permissionDenied.state.canResume)
    }

    func testUnknownMutationsAndAcknowledgmentsRequireOriginalEvidence() throws {
        let mutation = try XCTUnwrap(ClientRecoveryPresentation.mutation(.outcomeUnknown))
        XCTAssertEqual(mutation.classification, .reconciliationRequired)
        XCTAssertEqual(mutation.actions, [.reconcileOriginalMutation])
        XCTAssertEqual(ClientRecoveryPresentation.mutation(.submitting)?.actions, [])
        let recovery = SyncRecoveryAction(position: try feedPosition(), attempts: 1)
        let ack = try XCTUnwrap(ClientRecoveryPresentation.sync(.offline, recovery: recovery))
        XCTAssertEqual(ack.classification, .reconciliationRequired)
        XCTAssertEqual(ack.actions, [.recoverOriginalAcknowledgement])
        XCTAssertEqual(
            ClientRecoveryPresentation.sync(
                .offline,
                recovery: SyncRecoveryAction(position: try feedPosition(), attempts: 8))?.actions,
            [.inspectLocalStatus])
    }

    func testIncompleteAndSnapshotProvenanceDoNotClaimFreshness() {
        let state = NodeProjectionState(
            completeness: .partial, incrementalAnchor: nil,
            locallyApplied: nil, serverConfirmed: nil, library: nil)
        let saved = NodeCachedDirectoryPresentation(
            nodes: [], knowledge: .missing,
            hasMore: false, projection: state, liveFailure: .offline)
        let notice = ClientRecoveryPresentation.saved(saved)
        XCTAssertEqual(notice.classification, .cachedDataIncomplete)
        XCTAssertEqual(notice.evidence, .committedProjection)
        XCTAssertTrue(notice.message.contains("Other server items may exist"))
        XCTAssertFalse(notice.message.contains("synced"))
    }

    func testRebaselineUncertaintyIsNotReclassifiedAsAnOfflineRetry() {
        let progress = RebaselineProgress(
            state: .outcomeUnknown, pages: 1,
            stagedNodes: 3, expectedNodes: 3, position: nil)
        let notice = ClientRecoveryPresentation.rebaseline(progress, failure: .transport(.timeout))
        XCTAssertEqual(notice?.classification, .reconciliationRequired)
        XCTAssertEqual(notice?.actions, [.confirmedRebaseline])
    }

    func testRecoveryNoticeRendersAtAccessibilitySize() {
        let host = UIHostingController(
            rootView: ClientRecoveryNotice(presentation: .download(.integrityFailure))
                .environment(\.dynamicTypeSize, .accessibility5))
        host.loadViewIfNeeded()
        host.view.frame = CGRect(x: 0, y: 0, width: 320, height: 1000)
        host.view.layoutIfNeeded()
        XCTAssertEqual(host.view.bounds.width, 320)
    }
}
