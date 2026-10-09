import Foundation
import XCTest

@testable import Synveil

@MainActor
final class NodeBrowserTests: XCTestCase {
    private let bridge = NodeTestValidator()

    func testCanonicalLibraryIdAcceptedThroughRealRustBridge() async throws {
        let bridge = try await RustBridgeAsyncAdapter()
        let id = try await LibraryId.validated(nodeId(1), using: bridge)
        XCTAssertEqual(id.rawValue, nodeId(1))
    }

    func testCanonicalNodeIdAcceptedThroughRealRustBridge() async throws {
        let bridge = try await RustBridgeAsyncAdapter()
        let id = try await NodeId.validated(nodeId(2), using: bridge)
        XCTAssertEqual(id.rawValue, nodeId(2))
    }

    func testValidResponseThroughRealRustBridge() async throws {
        let bridge = try await RustBridgeAsyncAdapter()
        let page = try await NodeResponseDecoder(bridge: bridge).decode(nodeBody())
        XCTAssertEqual(page.nodes.first?.name, "Logical/A 🚀")
    }

    func testRevisionPreservesExactTextBeyondUInt64() throws {
        let text = "184467440737095516160000000000000000000"
        XCTAssertEqual(try NodeRevision(validating: text).rawValue, text)
    }

    func testValidRevision() throws {
        XCTAssertEqual(try NodeRevision(validating: "0").rawValue, "0")
        XCTAssertEqual(try NodeRevision(validating: "123").rawValue, "123")
    }

    func testInvalidRevisions() {
        for value in ["-1", "01", "+1", "1.0", "1e3", " 1", "1\n", ""] {
            XCTAssertThrowsError(try NodeRevision(validating: value))
        }
    }

    func testSupportedLifecycleStates() async throws {
        for state in ["ACTIVE", "TRASHED", "PURGING"] {
            let page = try await decodeResource { resource in
                var attributes = resource["attributes"] as! [String: Any]
                attributes["state"] = state
                resource["attributes"] = attributes
            }
            XCTAssertEqual(page.nodes.first?.state.rawValue, state)
        }
    }

    func testTimestampExplicitOffsetAndFraction() throws {
        let utc = try LibraryWireValidation.timestamp("2026-10-09T12:30:00.125Z")
        let offset = try LibraryWireValidation.timestamp("2026-10-09T14:30:00.125+02:00")
        XCTAssertEqual(utc, offset)
    }

    func testInvalidTimestampCalendarAndSyntax() {
        for value in [
            "garbage", "2026-02-30T12:00:00Z", "2026-13-01T00:00:00Z", "2026-10-09",
            "2026-10-09T25:00:00Z", "2026-10-09T12:00:00", "2026-10-09T12:00:00+24:00",
        ] {
            XCTAssertThrowsError(try LibraryWireValidation.timestamp(value))
        }
    }

    func testLogicalNameUTF8BoundaryAndNoPathNormalization() async throws {
        let name = String(repeating: "a", count: 1020) + "🚀"
        let page = try await decodeResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            attributes["name"] = name
            resource["attributes"] = attributes
        }
        XCTAssertEqual(page.nodes.first?.name, name)
        let ordinary = try await NodeResponseDecoder(bridge: bridge).decode(nodeBody())
        XCTAssertEqual(ordinary.nodes.first?.name, "Logical/A 🚀")
    }

    func testMalformedNodeIdRejected() async throws {
        await assertInvalidResource { resource in
            let attributes = resource["attributes"] as! [String: Any]
            resource["id"] = "garbage"
            resource["attributes"] = attributes
        }
    }

    func testUppercaseNodeIdRejected() async throws {
        await assertInvalidResource { resource in
            let attributes = resource["attributes"] as! [String: Any]
            resource["id"] = nodeId(1).uppercased()
            resource["attributes"] = attributes
        }
    }

    func testWrongVersionNodeIdRejected() async throws {
        await assertInvalidResource { resource in
            let attributes = resource["attributes"] as! [String: Any]
            resource["id"] = "018f9b9f-5c21-422e-8b1a-9f4a0b2c3d4e"
            resource["attributes"] = attributes
        }
    }

    func testInvalidParentNodeIdRejected() async throws {
        await assertInvalidResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            attributes["parent_id"] = "garbage"
            resource["attributes"] = attributes
        }
    }

    func testUppercaseParentNodeIdRejected() async throws {
        await assertInvalidResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            attributes["parent_id"] = nodeId(2).uppercased()
            resource["attributes"] = attributes
        }
    }

    func testWrongVersionParentNodeIdRejected() async throws {
        await assertInvalidResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            attributes["parent_id"] = "018f9b9f-5c21-422e-8b1a-9f4a0b2c3d4e"
            resource["attributes"] = attributes
        }
    }

    func testNegativeRevisionRejected() async throws {
        await assertInvalidResource { resource in
            let attributes = resource["attributes"] as! [String: Any]
            resource["revision"] = "-1"
            resource["attributes"] = attributes
        }
    }

    func testLeadingZeroRevisionRejected() async throws {
        await assertInvalidResource { resource in
            let attributes = resource["attributes"] as! [String: Any]
            resource["revision"] = "01"
            resource["attributes"] = attributes
        }
    }

    func testNumericRevisionRejected() async throws {
        await assertInvalidResource { resource in
            let attributes = resource["attributes"] as! [String: Any]
            resource["revision"] = 1
            resource["attributes"] = attributes
        }
    }

    func testUnknownStateRejected() async throws {
        await assertInvalidResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            attributes["state"] = "UNKNOWN"
            resource["attributes"] = attributes
        }
    }

    func testEmptyNameRejected() async throws {
        await assertInvalidResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            attributes["name"] = ""
            resource["attributes"] = attributes
        }
    }

    func testOversizedUTF8NameRejected() async throws {
        await assertInvalidResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            attributes["name"] = String(repeating: "🚀", count: 257)
            resource["attributes"] = attributes
        }
    }

    func testMalformedTimestampRejected() async throws {
        await assertInvalidResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            attributes["updated_at"] = "garbage"
            resource["attributes"] = attributes
        }
    }

    func testMissingRequiredIdRejected() async throws {
        await assertInvalidResource { resource in
            let attributes = resource["attributes"] as! [String: Any]
            resource.removeValue(forKey: "id")
            resource["attributes"] = attributes
        }
    }

    func testMissingRequiredAttributeRejected() async throws {
        await assertInvalidResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            attributes.removeValue(forKey: "created_at")
            resource["attributes"] = attributes
        }
    }

    func testWrongResourceTypeRejected() async throws {
        await assertInvalidResource { resource in
            let attributes = resource["attributes"] as! [String: Any]
            resource["type"] = "library"
            resource["attributes"] = attributes
        }
    }

    func testUnknownResourceKeyRejected() async throws {
        await assertInvalidResource { resource in
            let attributes = resource["attributes"] as! [String: Any]
            resource["extra"] = true
            resource["attributes"] = attributes
        }
    }

    func testUnknownAttributeKeyRejected() async throws {
        await assertInvalidResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            attributes["extra"] = true
            resource["attributes"] = attributes
        }
    }

    func testMalformedJSONRejected() async {
        await assertInvalidBody(Data("{bad".utf8))
    }

    func testInvalidMetadataRejected() async throws {
        for metadata: [String: Any] in [
            ["request_id": "x"], ["request_id": "request-123", "extra": true], [:],
            ["request_id": "request-123\n"],
        ] {
            await assertInvalidBody(try nodeBody(meta: metadata))
        }
    }

    func testUnknownEnvelopeKeyRejected() async throws {
        var root = try JSONSerialization.jsonObject(with: nodeBody()) as! [String: Any]
        root["extra"] = true
        await assertInvalidBody(try JSONSerialization.data(withJSONObject: root))
    }

    func testMissingRequiredEnvelopeFieldRejected() async throws {
        for key in ["data", "page", "meta"] {
            var root = try JSONSerialization.jsonObject(with: nodeBody()) as! [String: Any]
            root.removeValue(forKey: key)
            await assertInvalidBody(try JSONSerialization.data(withJSONObject: root))
        }
    }

    func testInvalidPaginationRejected() async throws {
        let pages: [[String: Any]] = [
            ["has_more": true], ["has_more": true, "next_cursor": ""],
            ["has_more": true, "next_cursor": String(repeating: "x", count: 513)],
            ["has_more": false, "next_cursor": "next"],
            ["has_more": false, "next_cursor": NSNull()],
            ["has_more": "false"], ["has_more": false, "extra": true], [:],
        ]
        for page in pages { await assertInvalidBody(try nodeBody(page: page)) }
    }

    func testCursorScalarLengthAndOpacity() {
        XCTAssertTrue(LibraryWireValidation.validCursor(String(repeating: "🚀", count: 512)))
        XCTAssertFalse(LibraryWireValidation.validCursor(String(repeating: "🚀", count: 513)))
        XCTAssertTrue(LibraryWireValidation.validCursor(" "))
    }

    func testEmptySuccessfulDirectory() async throws {
        let fixture = try fixture(responses: [nodeResponse(ids: [])])
        let result = await list(fixture)
        XCTAssertEqual(result, .loaded([]))
    }

    func testOnePageDirectorySucceeds() async throws {
        let fixture = try fixture(responses: [nodeResponse()])
        let result = await list(fixture)
        guard case .loaded(let nodes) = result else {
            return XCTFail("Expected complete node")
        }
        XCTAssertEqual(nodes.count, 1)
    }

    func testMultiplePagesAndSafelyEncodedCursor() async throws {
        let cursor = "opaque&limit=1+#?/%雪"
        let fixture = try fixture(responses: [
            nodeResponse(ids: [1], cursor: cursor), nodeResponse(ids: [2]),
        ])
        let result = await list(fixture)
        guard case .loaded(let nodes) = result else {
            return XCTFail("Expected complete node")
        }
        XCTAssertEqual(nodes.count, 2)
        let requests = await fixture.transport.requests()
        XCTAssertEqual(requests.count, 2)
        XCTAssertTrue(requests[1].url.absoluteString.contains("%2B"))
        XCTAssertFalse(requests[1].url.absoluteString.contains("+"))
        let items = URLComponents(url: requests[1].url, resolvingAgainstBaseURL: false)?.queryItems
        XCTAssertEqual(
            items,
            [
                URLQueryItem(name: "limit", value: "100"),
                URLQueryItem(name: "cursor", value: cursor),
            ])
    }

    func testRequestSecurityHeadersAndOrigin() async throws {
        let fixture = try fixture(responses: [nodeResponse()])
        _ = await list(fixture)
        let requests = await fixture.transport.requests()
        let request = try XCTUnwrap(requests.first)
        XCTAssertEqual(request.headers["Authorization"], "Bearer " + nodeBearer)
        XCTAssertEqual(request.headers["Accept"], "application/json")
        XCTAssertEqual(request.headers["Accept-Encoding"], "identity")
        XCTAssertEqual(request.headers["User-Agent"], "Synveil/0.1.0 (iOS)")
        XCTAssertNil(request.headers["Cookie"])
        XCTAssertNil(request.headers["X-CSRF-Token"])
        XCTAssertEqual(request.headers.count, 4)
        XCTAssertEqual(request.method, .get)
        XCTAssertNil(request.body)
        XCTAssertFalse(request.url.absoluteString.contains(nodeBearer))
        XCTAssertEqual(request.url.host, fixture.endpoint.host)
        XCTAssertEqual(request.url.port, fixture.endpoint.port)
        XCTAssertEqual(request.url.path, "/synveil/api/v1/libraries/\(nodeId(7000))/nodes")
        XCTAssertEqual(
            URLComponents(url: request.url, resolvingAgainstBaseURL: false)?.queryItems,
            [URLQueryItem(name: "limit", value: "100")])
    }

    func testOpaqueCursorsUseExactBytesForLoopDetection() async throws {
        let fixture = try fixture(responses: [
            nodeResponse(ids: [1], cursor: "é"),
            nodeResponse(ids: [2], cursor: "e\u{301}"),
            nodeResponse(ids: [3]),
        ])
        let result = await list(fixture)
        guard case .loaded(let nodes) = result else {
            return XCTFail("Expected byte-distinct cursors to succeed")
        }
        XCTAssertEqual(nodes.count, 3)
    }

    func testRepeatedCursorRejected() async throws {
        let fixture = try fixture(responses: [
            nodeResponse(ids: [1], cursor: "same"), nodeResponse(ids: [2], cursor: "same"),
        ])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.repeatedCursor))
        let count = await fixture.transport.requests().count
        XCTAssertEqual(count, 2)
    }

    func testMaximum64PagesEnforcedWithoutTruncation() async throws {
        let responses = try (0..<65).map { try nodeResponse(ids: [], cursor: "cursor-\($0)") }
        let fixture = try fixture(responses: responses)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.resourceLimit))
        let count = await fixture.transport.requests().count
        XCTAssertEqual(count, 64)
    }

    func testMaximum4096NodesEnforcedWithoutTruncation() async throws {
        var responses: [HTTPTransportResponse] = []
        for page in 0..<41 {
            responses.append(
                try nodeResponse(
                    ids: Array((page * 100 + 1)...(page * 100 + 100)), cursor: "cursor-\(page)"))
        }
        let fixture = try fixture(responses: responses)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.resourceLimit))
        let count = await fixture.transport.requests().count
        XCTAssertEqual(count, 41)
    }

    func testExactly4096NodesAccepted() async throws {
        var responses: [HTTPTransportResponse] = []
        for page in 0..<40 {
            responses.append(
                try nodeResponse(
                    ids: Array((page * 100 + 1)...(page * 100 + 100)), cursor: "cursor-\(page)"))
        }
        responses.append(try nodeResponse(ids: Array(4001...4096)))
        let fixture = try fixture(responses: responses)
        let result = await list(fixture)
        guard case .loaded(let nodes) = result else {
            return XCTFail("Expected full bounded node")
        }
        XCTAssertEqual(nodes.count, 4096)
    }

    func testOversizedPageRejected() async throws {
        let fixture = try fixture(responses: [nodeResponse(ids: Array(1...101))])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.resourceLimit))
    }

    func testDuplicateIdsWithinPageRejected() async throws {
        let fixture = try fixture(responses: [nodeResponse(ids: [1, 1])])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.protocolFailure))
    }

    func testDuplicateIdsAcrossPagesRejected() async throws {
        let fixture = try fixture(responses: [nodeResponse(cursor: "next"), nodeResponse()])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.protocolFailure))
    }

    func testSecondPageFailureNeverReturnsPartialSuccess() async throws {
        let fixture = try fixture(responses: [
            nodeResponse(cursor: "next"), nodeError(status: 503, code: "server_unavailable"),
        ])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.serverUnavailable))
        XCTAssertEqual(fixture.controller.state, .authenticated)
    }

    func testNoRequestWhileUnauthenticated() async throws {
        let fixture = try fixture(responses: [], authenticated: false)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.unauthenticated))
        let requests = await fixture.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    func testMissingKeychainFailsClosed() async throws {
        let fixture = try fixture(responses: [])
        await fixture.store.replace(nil)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.credentialUnavailable))
        let requests = await fixture.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    func testOriginMismatchFailsClosed() async throws {
        let fixture = try fixture(responses: [])
        let other = try ServerEndpoint(validating: "https://another.example")
        await fixture.store.replace(try nodeSession(endpoint: other))
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.originMismatch))
        let requests = await fixture.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    func testCleartextEndpointFailsClosed() async throws {
        let fixture = try fixture(
            responses: [], endpoint: ServerEndpoint(validating: "http://127.0.0.1"))
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.originMismatch))
        let requests = await fixture.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    func testAuthenticationFailedClassificationAndCredentialRetention() async throws {
        let fixture = try fixture(responses: [
            nodeError(status: 401, code: "authentication_failed")
        ])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.authenticationRejected))
        XCTAssertEqual(fixture.controller.state, .recoveryRequired(.authentication))
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testDeviceRevokedClassificationAndCredentialRetention() async throws {
        let fixture = try fixture(responses: [nodeError(status: 401, code: "device_revoked")])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.deviceRevoked))
        XCTAssertEqual(fixture.controller.state, .recoveryRequired(.deviceRevoked))
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testServerUnavailableClassificationAndCredentialRetention() async throws {
        let fixture = try fixture(responses: [nodeError(status: 503, code: "server_unavailable")]
        )
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.serverUnavailable))
        XCTAssertEqual(fixture.controller.state, .authenticated)
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testUnknown401CodeClassificationAndCredentialRetention() async throws {
        let fixture = try fixture(responses: [nodeError(status: 401, code: "other_error")])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.protocolFailure))
        XCTAssertEqual(fixture.controller.state, .authenticated)
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testForbiddenClassificationAndCredentialRetention() async throws {
        let fixture = try fixture(responses: [nodeError(status: 403, code: "forbidden")])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.httpFailure(statusCode: 403)))
        XCTAssertEqual(fixture.controller.state, .authenticated)
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testOfflineNeverDeletesOrRevokesCredential() async throws {
        let fixture = try fixture(responses: [], error: .offline)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.offline))
        XCTAssertEqual(fixture.controller.state, .authenticated)
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testDNSNeverDeletesOrRevokesCredential() async throws {
        let fixture = try fixture(responses: [], error: .dnsFailure)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.dnsFailure))
        XCTAssertEqual(fixture.controller.state, .authenticated)
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testTimeoutNeverDeletesOrRevokesCredential() async throws {
        let fixture = try fixture(responses: [], error: .timeout)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.timeout))
        XCTAssertEqual(fixture.controller.state, .authenticated)
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testTLSNeverDeletesOrRevokesCredential() async throws {
        let fixture = try fixture(responses: [], error: .tlsError)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.tlsFailure))
        XCTAssertEqual(fixture.controller.state, .authenticated)
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testTransportRedirectNeverDeletesOrRevokesCredential() async throws {
        let fixture = try fixture(responses: [], error: .redirectRejected(statusCode: 302))
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.redirectRejected))
        XCTAssertEqual(fixture.controller.state, .authenticated)
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testTransportBodyLimitNeverDeletesOrRevokesCredential() async throws {
        let fixture = try fixture(responses: [], error: .bodyLimitExceeded)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.resourceLimit))
        XCTAssertEqual(fixture.controller.state, .authenticated)
        let deletions = await fixture.store.deleteCount()
        XCTAssertEqual(deletions, 0)
    }

    func testRedirectResponseCannotForwardBearer() async throws {
        let response = HTTPTransportResponse(
            statusCode: 302, headers: ["Location": "https://evil.example"], body: Data())
        let fixture = try fixture(responses: [response])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.redirectRejected))
        let requests = await fixture.transport.requests()
        XCTAssertEqual(requests.count, 1)
        XCTAssertEqual(requests.first?.url.host, fixture.endpoint.host)
    }

    func testWrongContentTypeRejected() async throws {
        let response = HTTPTransportResponse(
            statusCode: 200, headers: ["Content-Type": "text/html"], body: try nodeBody())
        let fixture = try fixture(responses: [response])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.unexpectedContentType))
    }

    func testSuccessAndErrorResponseOver1MiBRejected() async throws {
        for status in [200, 401, 503] {
            let response = HTTPTransportResponse(
                statusCode: status, headers: ["Content-Type": "application/json"],
                body: Data(repeating: 0x20, count: 1024 * 1024 + 1))
            let fixture = try fixture(responses: [response])
            let result = await list(fixture)
            XCTAssertEqual(result, .failed(.resourceLimit))
            XCTAssertEqual(fixture.controller.state, .authenticated)
        }
    }

    func testMalformedErrorDoesNotRevokeSession() async throws {
        let response = HTTPTransportResponse(
            statusCode: 401, headers: ["Content-Type": "application/json"], body: Data("{bad".utf8))
        let fixture = try fixture(responses: [response])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.protocolFailure))
        XCTAssertEqual(fixture.controller.state, .authenticated)
    }

    func testRepeatedRequestsNeverExchangeEnrollment() async throws {
        let fixture = try fixture(responses: [nodeResponse(), nodeResponse()])
        _ = await list(fixture)
        _ = await list(fixture)
        let requests = await fixture.transport.requests()
        XCTAssertEqual(requests.count, 2)
        XCTAssertTrue(
            requests.allSatisfy {
                $0.method == .get && $0.url.path.hasSuffix("/nodes") && $0.body == nil
            })
    }

    func testCancellationStopsAdditionalRequestsAndDiscardsLateResponse() async throws {
        let fixture = try fixture(responses: [], suspended: true)
        let task = Task { await list(fixture) }
        await fixture.transport.waitForRequest()
        task.cancel()
        await fixture.transport.complete(try nodeResponse(cursor: "next"))
        let result = await task.value
        XCTAssertEqual(result, .failed(.cancelled))
        let requests = await fixture.transport.requests()
        XCTAssertEqual(requests.count, 1)
        XCTAssertEqual(fixture.controller.state, .authenticated)
    }

    func testCancelledBeforeBeginMakesNoRequest() async throws {
        let fixture = try fixture(responses: [])
        let task = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return await list(fixture)
        }
        let result = await task.value
        XCTAssertEqual(result, .failed(.cancelled))
        let requests = await fixture.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    func testLateResponseAfterLogoutDiscardedAndNewRequestBlocked() async throws {
        let fixture = try fixture(responses: [], suspended: true)
        let task = Task { await list(fixture) }
        await fixture.transport.waitForRequest()
        await fixture.controller.requestLogout()
        let next = await list(fixture)
        XCTAssertEqual(next, .failed(.unauthenticated))
        await fixture.transport.complete(try nodeResponse(cursor: "next"))
        let result = await task.value
        XCTAssertEqual(result, .failed(.staleSession))
        XCTAssertEqual(fixture.controller.state, .readyForServerValidation)
        let requests = await fixture.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }

    func testLate401AfterLogoutCannotChangeRecovery() async throws {
        let fixture = try fixture(responses: [], suspended: true)
        let task = Task { await list(fixture) }
        await fixture.transport.waitForRequest()
        await fixture.controller.requestLogout()
        await fixture.transport.complete(try nodeError(status: 401, code: "device_revoked"))
        let result = await task.value
        XCTAssertEqual(result, .failed(.staleSession))
        XCTAssertEqual(fixture.controller.state, .readyForServerValidation)
    }

    func testStaleResponseAfterOriginChangeDiscarded() async throws {
        let fixture = try fixture(responses: [], suspended: true)
        let task = Task { await list(fixture) }
        await fixture.transport.waitForRequest()
        let other = try ServerEndpoint(validating: "https://other.example")
        await fixture.store.replace(try nodeSession(endpoint: other))
        await fixture.transport.complete(try nodeResponse())
        let result = await task.value
        XCTAssertEqual(result, .failed(.originMismatch))
        XCTAssertEqual(fixture.controller.serverEndpoint, fixture.endpoint)
    }

    func testCredentialReplacementDiscardsResponse() async throws {
        let fixture = try fixture(responses: [], suspended: true)
        let task = Task { await list(fixture) }
        await fixture.transport.waitForRequest()
        await fixture.store.replace(
            try nodeSession(
                endpoint: fixture.endpoint, bearer: "svd1_" + String(repeating: "b", count: 64)))
        await fixture.transport.complete(try nodeResponse())
        let result = await task.value
        XCTAssertEqual(result, .failed(.staleSession))
    }

    func testNoRequestDuringLogoutCleanup() async throws {
        let fixture = try fixture(responses: [])
        await fixture.store.suspendDeletion()
        let logout = Task { await fixture.controller.requestLogout() }
        await fixture.store.waitForDeletion()
        XCTAssertEqual(fixture.controller.state, .logoutInProgress)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.unauthenticated))
        let requests = await fixture.transport.requests()
        XCTAssertTrue(requests.isEmpty)
        await fixture.store.completeDeletion()
        await logout.value
    }

    func testKeychainLoadCompletingAfterLogoutCannotSendRequest() async throws {
        let fixture = try fixture(responses: [])
        await fixture.store.suspendNextLoad()
        let listing = Task { await list(fixture) }
        await fixture.store.waitForLoad()
        await fixture.controller.requestLogout()
        await fixture.store.completeLoad()
        let result = await listing.value
        XCTAssertEqual(result, .failed(.staleSession))
        let requests = await fixture.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    func testInvalidStoredCredentialFailsClosed() async throws {
        let fixture = try fixture(responses: [])
        await fixture.store.replace(
            try nodeSession(endpoint: fixture.endpoint, bearer: "invalid"))
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.invalidCredential))
        let requests = await fixture.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    func testRequestDebugDescriptionsRedactAuthorization() throws {
        let request = HTTPTransportRequest(
            url: URL(string: "https://node.example")!,
            headers: ["Authorization": "Bearer " + nodeBearer])
        XCTAssertFalse(String(describing: request).contains(nodeBearer))
        XCTAssertFalse(String(reflecting: request).contains(nodeBearer))
    }

    func testCredentialCannotAppearInFailureOrScopeDescriptions() async throws {
        let fixture = try fixture(responses: [])
        let scope = try await fixture.provider.begin()
        XCTAssertFalse(String(describing: scope).contains(nodeBearer))
        XCTAssertFalse(String(reflecting: scope).contains(nodeBearer))
        for failure in [
            NodeFailure.authenticationRejected, .deviceRevoked, .protocolFailure,
            .httpFailure(statusCode: 500), .staleSession,
        ] {
            XCTAssertFalse(String(describing: failure).contains(nodeBearer))
            XCTAssertFalse(String(reflecting: failure).contains(nodeBearer))
        }
    }

    func testFileAndDirectoryKindsUseOnlyServerKind() async throws {
        for kind in ["FILE", "DIRECTORY"] {
            let page = try await decodeAttributes {
                $0["kind"] = kind
                $0["name"] = "directory.txt"
            }
            XCTAssertEqual(page.nodes.first?.kind.rawValue, kind)
            XCTAssertNil(page.nodes.first?.currentVersionId)
        }
    }

    func testUnknownKindRejected() async {
        await assertInvalidAttributes { $0["kind"] = "SYMLINK" }
    }

    func testInvalidOwningLibraryIdsRejected() async {
        for value in ["bad", nodeId(7000).uppercased(), "018f9b9f-5c21-422e-8b1a-000000001b58"] {
            await assertInvalidAttributes { $0["library_id"] = value }
        }
    }

    func testInvalidUUIDVariantRejectedByRealRust() async throws {
        let bridge = try await RustBridgeAsyncAdapter()
        do {
            _ = try await NodeId.validated("018f9b9f-5c21-722e-0b1a-000000000001", using: bridge)
            XCTFail("Expected invalid variant")
        } catch { XCTAssertEqual(error as? NodeFailure, .protocolFailure) }
    }

    func testOptionalAttributesCanAllBeOmitted() async throws {
        let page = try await decodeAttributes { $0.removeValue(forKey: "parent_id") }
        let node = try XCTUnwrap(page.nodes.first)
        XCTAssertNil(node.parentId)
        XCTAssertNil(node.currentVersionId)
        XCTAssertNil(node.trashedAt)
        XCTAssertNil(node.restoreDeadline)
        XCTAssertFalse(node.purgeEligible)
    }

    func testOptionalParentPreservesValidatedIdentity() async throws {
        let page = try await decodeAttributes { $0["parent_id"] = nodeId(42) }
        XCTAssertEqual(page.nodes.first?.parentId?.rawValue, nodeId(42))
    }

    func testFileVersionIdIsDistinctAndRustValidated() async throws {
        let bridge = try await RustBridgeAsyncAdapter()
        let page = try await NodeResponseDecoder(bridge: bridge).decode(
            nodeBody(resources: [
                changedNode {
                    $0["kind"] = "FILE"
                    $0["current_version_id"] = nodeId(50)
                }
            ]))
        let version: FileVersionId = try XCTUnwrap(page.nodes.first?.currentVersionId)
        XCTAssertEqual(version.rawValue, nodeId(50))
    }

    func testInvalidOptionalIdentifiersRejected() async {
        for key in ["parent_id", "current_version_id"] {
            for value in ["bad", nodeId(50).uppercased(), "018f9b9f-5c21-422e-8b1a-000000000032"] {
                await assertInvalidAttributes { $0[key] = value }
            }
        }
    }

    func testExplicitNullOptionalFieldsRejected() async {
        for key in ["parent_id", "current_version_id", "trashed_at", "restore_deadline"] {
            await assertInvalidAttributes { $0[key] = NSNull() }
        }
    }

    func testWrongTypeOptionalFieldsRejected() async {
        for key in ["parent_id", "current_version_id", "trashed_at", "restore_deadline"] {
            await assertInvalidAttributes { $0[key] = 42 }
        }
    }

    func testTrashMetadataAndPurgeHintPreservedInDomain() async throws {
        for state in ["TRASHED", "PURGING"] {
            let page = try await decodeAttributes {
                $0["state"] = state
                $0["trashed_at"] = "2026-10-01T12:00:00Z"
                $0["restore_deadline"] = "2026-11-01T12:00:00Z"
                $0["purge_eligible"] = true
            }
            XCTAssertEqual(page.nodes.first?.state.rawValue, state)
            XCTAssertNotNil(page.nodes.first?.trashedAt)
            XCTAssertNotNil(page.nodes.first?.restoreDeadline)
            XCTAssertEqual(page.nodes.first?.purgeEligible, true)
        }
    }

    func testEveryTimestampValidated() async {
        for key in ["created_at", "updated_at", "trashed_at", "restore_deadline"] {
            for value in ["2026-02-30T12:00:00Z", "2026-10-09T12:00:00+02:99", "bad"] {
                await assertInvalidAttributes { $0[key] = value }
            }
        }
    }

    func testPurgeEligibleMustBeRequiredBoolean() async {
        await assertInvalidAttributes { $0.removeValue(forKey: "purge_eligible") }
        for value: Any in [NSNull(), "false", 0, 1] {
            await assertInvalidAttributes { $0["purge_eligible"] = value }
        }
    }

    func testAllRequiredResourceAndAttributeFieldsEnforced() async {
        for key in ["id", "type", "revision", "attributes"] {
            await assertInvalidResource { $0.removeValue(forKey: key) }
        }
        for key in [
            "library_id", "name", "kind", "state", "created_at", "updated_at", "purge_eligible",
        ] {
            await assertInvalidAttributes { $0.removeValue(forKey: key) }
        }
    }

    func testLogicalNameWhitespaceAndUnicodeArePreservedExactly() async throws {
        let name = "  ../e\u{301}/report\\draft 🚀  "
        let page = try await decodeAttributes { $0["name"] = name }
        XCTAssertEqual(Array(try XCTUnwrap(page.nodes.first?.name).utf8), Array(name.utf8))
    }

    func testRootListingOmitsParentAndChecksRealRootParent() async throws {
        let fixture = try fixture(responses: [nodeResponse()])
        let result = await list(fixture)
        guard case .loaded(let nodes) = result else { return XCTFail("Expected root children") }
        XCTAssertEqual(nodes.first?.parentId?.rawValue, nodeId(10000))
        let requests = await fixture.transport.requests()
        let request = try XCTUnwrap(requests.first)
        let query = URLComponents(url: request.url, resolvingAgainstBaseURL: false)?.queryItems
        XCTAssertEqual(query, [URLQueryItem(name: "limit", value: "100")])
    }

    func testDirectoryListingIncludesExactParentOnEveryPage() async throws {
        let directory = try await NodeId.validated(nodeId(42), using: bridge)
        let fixture = try fixture(responses: [
            nodeHTTP(
                nodeBody(
                    resources: [changedNode { $0["parent_id"] = nodeId(42) }],
                    page: ["has_more": true, "next_cursor": "next"])),
            nodeHTTP(
                nodeBody(resources: [changedNode(number: 2) { $0["parent_id"] = nodeId(42) }])),
        ])
        let result = await list(fixture, parent: .directory(directory))
        guard case .loaded(let nodes) = result else {
            return XCTFail("Expected directory children")
        }
        XCTAssertEqual(nodes.count, 2)
        let requests = await fixture.transport.requests()
        for request in requests {
            let items = URLComponents(url: request.url, resolvingAgainstBaseURL: false)?.queryItems
            XCTAssertEqual(items?.first { $0.name == "parent_id" }?.value, directory.rawValue)
        }
    }

    func testCrossLibraryResponseRejectedWithoutPartialData() async throws {
        let response = try nodeHTTP(
            nodeBody(resources: [
                nodeResource(1), changedNode(number: 2) { $0["library_id"] = nodeId(8000) },
            ]))
        let fixture = try fixture(responses: [response])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.protocolFailure))
    }

    func testWrongDirectoryParentAndMissingParentRejected() async throws {
        let directory = try await NodeId.validated(nodeId(42), using: bridge)
        for resources in [[nodeResource(1)], [changedNode { $0.removeValue(forKey: "parent_id") }]]
        {
            let fixture = try fixture(responses: [nodeHTTP(nodeBody(resources: resources))])
            let result = await list(fixture, parent: .directory(directory))
            XCTAssertEqual(result, .failed(.protocolFailure))
        }
    }

    func testWrongRootParentRejected() async throws {
        let fixture = try fixture(responses: [
            nodeHTTP(nodeBody(resources: [changedNode { $0["parent_id"] = nodeId(42) }]))
        ])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.protocolFailure))
    }

    func testParentCannotAppearAsItsOwnChild() async throws {
        let fixture = try fixture(responses: [nodeResponse(ids: [10000])])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.protocolFailure))
    }

    func testNonActiveChildrenRejected() async throws {
        for state in ["TRASHED", "PURGING"] {
            let fixture = try fixture(responses: [
                nodeHTTP(nodeBody(resources: [changedNode { $0["state"] = state }]))
            ])
            let result = await list(fixture)
            XCTAssertEqual(result, .failed(.protocolFailure))
        }
    }

    func testActiveListingRejectsTrashTimestamps() async throws {
        for key in ["trashed_at", "restore_deadline"] {
            let fixture = try fixture(responses: [
                nodeHTTP(nodeBody(resources: [changedNode { $0[key] = "2026-10-01T12:00:00Z" }]))
            ])
            let result = await list(fixture)
            XCTAssertEqual(result, .failed(.protocolFailure))
        }
    }

    func testDuplicateDisplayNamesAllowed() async throws {
        let fixture = try fixture(responses: [nodeResponse(ids: [1, 2])])
        let result = await list(fixture)
        guard case .loaded(let nodes) = result else { return XCTFail("Expected both identities") }
        XCTAssertEqual(nodes.count, 2)
        XCTAssertEqual(nodes[0].name, nodes[1].name)
        XCTAssertNotEqual(nodes[0].id, nodes[1].id)
    }

    func testCursorsDoNotEscapeIntoAnotherOperation() async throws {
        let fixture = try fixture(responses: [
            nodeResponse(ids: [1], cursor: "old"), nodeResponse(ids: [2]), nodeResponse(ids: []),
        ])
        _ = await list(fixture)
        let directory = try await NodeId.validated(nodeId(42), using: bridge)
        let library = try await LibraryId.validated(nodeId(8000), using: bridge)
        _ = await list(fixture, parent: .directory(directory), libraryId: library)
        let requests = await fixture.transport.requests()
        XCTAssertEqual(requests.count, 3)
        let query = URLComponents(url: requests[2].url, resolvingAgainstBaseURL: false)?.queryItems
        XCTAssertNil(query?.first { $0.name == "cursor" })
        XCTAssertTrue(requests[2].url.path.contains(nodeId(8000)))
        XCTAssertEqual(query?.first { $0.name == "parent_id" }?.value, directory.rawValue)
    }

    func testKeychainReadFailureFailsClosed() async throws {
        let fixture = try fixture(responses: [])
        await fixture.store.failLoads(.readFailure)
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.credentialUnavailable))
        let requests = await fixture.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    func testLateResponseDuringRustValidationAfterLogoutRejected() async throws {
        let gate = NodeValidationGate()
        let fixture = try fixture(
            responses: [nodeResponse()], validator: NodeTestValidator(gate: gate))
        let task = Task { await list(fixture) }
        await gate.wait()
        await fixture.controller.requestLogout()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .failed(.staleSession))
    }

    func testCredentialReplacementDuringRustValidationRejected() async throws {
        let gate = NodeValidationGate()
        let fixture = try fixture(
            responses: [nodeResponse()], validator: NodeTestValidator(gate: gate))
        let task = Task { await list(fixture) }
        await gate.wait()
        await fixture.store.replace(
            try nodeSession(
                endpoint: fixture.endpoint, bearer: "svd1_" + String(repeating: "b", count: 64)))
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .failed(.staleSession))
    }

    func testCancellationDuringRustValidationRejected() async throws {
        let gate = NodeValidationGate()
        let fixture = try fixture(
            responses: [nodeResponse(cursor: "next")], validator: NodeTestValidator(gate: gate))
        let task = Task { await list(fixture) }
        await gate.wait()
        task.cancel()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .failed(.cancelled))
        let count = await fixture.transport.requests().count
        XCTAssertEqual(count, 1)
        XCTAssertEqual(fixture.controller.state, .authenticated)
    }

    func testNotFoundDoesNotBecomeRevocation() async throws {
        let fixture = try fixture(responses: [nodeError(status: 404, code: "device_revoked")])
        let result = await list(fixture)
        XCTAssertEqual(result, .failed(.httpFailure(statusCode: 404)))
        XCTAssertEqual(fixture.controller.state, .authenticated)
    }

    func testExactly64PagesCanComplete() async throws {
        var responses = try (0..<63).map { try nodeResponse(ids: [], cursor: "cursor-\($0)") }
        responses.append(try nodeResponse(ids: []))
        let fixture = try fixture(responses: responses)
        let result = await list(fixture)
        XCTAssertEqual(result, .loaded([]))
        let count = await fixture.transport.requests().count
        XCTAssertEqual(count, 64)
    }

    private func decodeAttributes(_ change: (inout [String: Any]) -> Void) async throws
        -> NodeCollectionPage
    {
        try await NodeResponseDecoder(bridge: bridge).decode(
            nodeBody(resources: [changedNode(change)]))
    }

    private func assertInvalidAttributes(_ change: (inout [String: Any]) -> Void) async {
        await assertInvalidResource { resource in
            var attributes = resource["attributes"] as! [String: Any]
            change(&attributes)
            resource["attributes"] = attributes
        }
    }

    private func list(
        _ fixture: Fixture, parent: NodeParentScope? = nil, libraryId: LibraryId? = nil
    ) async -> NodeRepositoryResult {
        do {
            let library: LibraryId
            if let libraryId {
                library = libraryId
            } else {
                library = try await LibraryId.validated(nodeId(7000), using: bridge)
            }
            let scope: NodeParentScope
            if let parent {
                scope = parent
            } else {
                let root = try await NodeId.validated(nodeId(10000), using: bridge)
                scope = .libraryRoot(rootNodeId: root)
            }
            return await fixture.repository.listChildren(libraryId: library, parent: scope)
        } catch { return .failed(.protocolFailure) }
    }

    private func decodeResource(_ change: (inout [String: Any]) -> Void) async throws
        -> NodeCollectionPage
    {
        var resource = nodeResource(1)
        change(&resource)
        return try await NodeResponseDecoder(bridge: bridge).decode(
            nodeBody(resources: [resource]))
    }

    private func assertInvalidResource(_ change: (inout [String: Any]) -> Void) async {
        do {
            _ = try await decodeResource(change)
            XCTFail("Expected protocol failure")
        } catch {
            XCTAssertEqual(error as? NodeFailure, .protocolFailure)
        }
    }

    private func assertInvalidBody(_ body: Data) async {
        do {
            _ = try await NodeResponseDecoder(bridge: bridge).decode(body)
            XCTFail("Expected protocol failure")
        } catch {
            XCTAssertEqual(error as? NodeFailure, .protocolFailure)
        }
    }

    private struct Fixture {
        let endpoint: ServerEndpoint
        let controller: SessionController
        let store: NodeTestStore
        let transport: NodeTestTransport
        let provider: AuthenticatedLibraryRequestProvider
        let repository: AuthenticatedNodeRepository
    }

    private func fixture(
        responses: [HTTPTransportResponse], authenticated: Bool = true,
        error: SynveilTransportError? = nil, suspended: Bool = false,
        endpoint: ServerEndpoint? = nil, validator: (any RustBridgeProtocol)? = nil
    ) throws -> Fixture {
        let endpoint =
            try endpoint ?? ServerEndpoint(validating: "https://node.example:8443/synveil")
        let session = try nodeSession(endpoint: endpoint)
        let store = NodeTestStore(session)
        let controller = SessionController(
            logoutService: SessionLogoutService(credentialStore: store))
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(endpoint)
        controller.requireEnrollment()
        if authenticated {
            controller.markAuthenticated(
                after: SecureCredentialPersistenceReceipt(session: session))
        }
        let transport = NodeTestTransport(
            responses: responses, error: error, suspended: suspended)
        let provider = AuthenticatedLibraryRequestProvider(
            controller: controller, store: store, transport: transport)
        return Fixture(
            endpoint: endpoint, controller: controller, store: store, transport: transport,
            provider: provider,
            repository: AuthenticatedNodeRepository(provider: provider, bridge: validator ?? bridge)
        )
    }
}

private let nodeBearer = "svd1_" + String(repeating: "a", count: 64)

private func nodeId(_ number: Int) -> String {
    "018f9b9f-5c21-722e-8b1a-" + String(format: "%012x", number)
}

private func nodeResource(_ number: Int) -> [String: Any] {
    [
        "id": nodeId(number), "type": "node", "revision": "42",
        "attributes": [
            "library_id": nodeId(7000), "parent_id": nodeId(10000),
            "name": "Logical/A 🚀", "kind": "DIRECTORY", "state": "ACTIVE",
            "created_at": "2026-10-09T12:00:00Z", "updated_at": "2026-10-09T12:00:00.125+02:00",
            "purge_eligible": false,
        ],
    ]
}

private func nodeBody(
    resources: [[String: Any]]? = nil, page: [String: Any] = ["has_more": false],
    meta: [String: Any] = ["request_id": "request-123"]
) throws -> Data {
    try JSONSerialization.data(withJSONObject: [
        "data": resources ?? [nodeResource(1)], "page": page, "meta": meta,
    ])
}

private func nodeResponse(ids: [Int] = [1], cursor: String? = nil) throws
    -> HTTPTransportResponse
{
    var page: [String: Any] = ["has_more": cursor != nil]
    if let cursor { page["next_cursor"] = cursor }
    return HTTPTransportResponse(
        statusCode: 200, headers: ["Content-Type": "application/json; charset=utf-8"],
        body: try nodeBody(resources: ids.map(nodeResource), page: page))
}

private func nodeError(status: Int, code: String) throws -> HTTPTransportResponse {
    let body = try JSONSerialization.data(withJSONObject: [
        "error": [
            "code": code, "message": "Safe message", "request_id": "request-123",
            "retryable": false,
        ]
    ])
    return HTTPTransportResponse(
        statusCode: status, headers: ["Content-Type": "application/json"], body: body)
}

private func nodeSession(endpoint: ServerEndpoint, bearer: String = nodeBearer) throws
    -> DeviceCredentialSession
{
    let record = try DeviceCredentialRecord(
        ownerUserId: nodeId(90), deviceId: nodeId(91), credentialId: nodeId(92),
        credential: DeviceCredential(validatedRawValue: bearer), createdAt: "2026-10-09T12:00:00Z")
    return DeviceCredentialSession(serverEndpoint: endpoint, record: record)
}

private struct NodeTestValidator: RustBridgeProtocol {
    func parseSHA256(_ canonical: String) async throws -> Data { Data() }
    func formatSHA256(_ digest: Data) async throws -> String { "" }
    func validateEnrollmentToken(_ token: String) async throws -> Bool { false }
    func validateDeviceBearerToken(_ token: String) async throws -> Bool {
        DeviceCredential.isValid(token)
    }
    func validateLibraryID(_ value: String) async throws -> Bool {
        LibraryWireValidation.matches(
            value, pattern: "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
    }
    func validateNodeID(_ value: String) async throws -> Bool { try await validateLibraryID(value) }
    let gate: NodeValidationGate?
    init(gate: NodeValidationGate? = nil) { self.gate = gate }
    func validateLogicalName(_ value: String) async throws -> Bool {
        if let gate { await gate.suspend() }
        return !value.isEmpty && value.utf8.count <= 1024
    }
}

private actor NodeTestStore: SecureCredentialSinkProtocol {
    private var session: DeviceCredentialSession?
    private var deletes = 0
    private var loadFailure: SecureCredentialSinkError?
    private var deletionSuspended = false
    private var deletionStarted = false
    private var deletionWaiter: CheckedContinuation<Void, Never>?
    private var deletionContinuation: CheckedContinuation<Void, Never>?
    private var nextLoadSuspended = false
    private var loadStarted = false
    private var loadWaiter: CheckedContinuation<Void, Never>?
    private var loadContinuation: CheckedContinuation<Void, Never>?
    init(_ session: DeviceCredentialSession) { self.session = session }
    func preflight() async throws {}
    func store(_ record: DeviceCredentialRecord, for endpoint: ServerEndpoint) async throws
        -> SecureCredentialPersistenceReceipt
    { throw SecureCredentialSinkError.writeFailure }
    func update(_ record: DeviceCredentialRecord, for endpoint: ServerEndpoint) async throws
        -> SecureCredentialPersistenceReceipt
    { throw SecureCredentialSinkError.writeFailure }
    func load(expectedServerEndpoint: ServerEndpoint?) async throws -> DeviceCredentialSession {
        if let loadFailure { throw loadFailure }
        guard let session else { throw SecureCredentialSinkError.itemNotFound }
        if nextLoadSuspended {
            nextLoadSuspended = false
            loadStarted = true
            loadWaiter?.resume()
            loadWaiter = nil
            await withCheckedContinuation { loadContinuation = $0 }
        }
        if let expectedServerEndpoint, session.serverEndpoint != expectedServerEndpoint {
            throw SecureCredentialSinkError.scopeMismatch
        }
        return session
    }
    func delete() async throws {
        deletes += 1
        deletionStarted = true
        deletionWaiter?.resume()
        deletionWaiter = nil
        if deletionSuspended {
            await withCheckedContinuation { deletionContinuation = $0 }
        }
        session = nil
    }
    func suspendDeletion() { deletionSuspended = true }
    func waitForDeletion() async {
        if deletionStarted { return }
        await withCheckedContinuation { deletionWaiter = $0 }
    }
    func completeDeletion() {
        deletionContinuation?.resume()
        deletionContinuation = nil
    }
    func suspendNextLoad() { nextLoadSuspended = true }
    func waitForLoad() async {
        if loadStarted { return }
        await withCheckedContinuation { loadWaiter = $0 }
    }
    func completeLoad() {
        loadContinuation?.resume()
        loadContinuation = nil
    }
    func replace(_ session: DeviceCredentialSession?) { self.session = session }
    func deleteCount() -> Int { deletes }
    func failLoads(_ error: SecureCredentialSinkError) { loadFailure = error }
}

private actor NodeTestTransport: HTTPTransportProtocol {
    private var responses: [HTTPTransportResponse]
    private var recorded: [HTTPTransportRequest] = []
    private let error: SynveilTransportError?
    private let suspended: Bool
    private var waiter: CheckedContinuation<Void, Never>?
    private var continuation: CheckedContinuation<HTTPTransportResponse, Never>?
    init(responses: [HTTPTransportResponse], error: SynveilTransportError?, suspended: Bool) {
        self.responses = responses
        self.error = error
        self.suspended = suspended
    }
    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        recorded.append(request)
        waiter?.resume()
        waiter = nil
        if suspended { return await withCheckedContinuation { continuation = $0 } }
        if let error { throw error }
        guard !responses.isEmpty else { throw SynveilTransportError.offline }
        return responses.removeFirst()
    }
    func requests() -> [HTTPTransportRequest] { recorded }
    func waitForRequest() async {
        if !recorded.isEmpty { return }
        await withCheckedContinuation { waiter = $0 }
    }
    func complete(_ response: HTTPTransportResponse) {
        continuation?.resume(returning: response)
        continuation = nil
    }
}

private actor NodeValidationGate {
    private var entered = false
    private var waiter: CheckedContinuation<Void, Never>?
    private var continuation: CheckedContinuation<Void, Never>?
    func suspend() async {
        entered = true
        waiter?.resume()
        waiter = nil
        await withCheckedContinuation { continuation = $0 }
    }
    func wait() async {
        if entered { return }
        await withCheckedContinuation { waiter = $0 }
    }
    func release() {
        continuation?.resume()
        continuation = nil
    }
}

private func changedNode(number: Int = 1, _ change: (inout [String: Any]) -> Void) -> [String: Any]
{
    var resource = nodeResource(number)
    var attributes = resource["attributes"] as! [String: Any]
    change(&attributes)
    resource["attributes"] = attributes
    return resource
}

private func nodeHTTP(_ body: Data) -> HTTPTransportResponse {
    HTTPTransportResponse(
        statusCode: 200, headers: ["Content-Type": "application/json"], body: body)
}
