import Foundation
import XCTest

@testable import Synveil

@MainActor
final class FileUploadTests: XCTestCase {
    private let validator = QueueValidator()
    private let libraryRaw = queueUUID(80)
    private let parentRaw = queueUUID(2)
    private let nodeRaw = queueUUID(3)
    private let sessionRaw = queueUUID(700)
    private let emptySHA256 = DownloadPolicy.emptyDigest

    func testCreateRequestIsCanonicalAndRecoveryRejectsChangedBytes() async throws {
        let library = try await LibraryId.validated(libraryRaw, using: validator)
        let parent = try await NodeId.validated(parentRaw, using: validator)
        let expected = try UploadExpectedContent(byteLength: 0, sha256: emptySHA256)
        let target = UploadTarget.createFile(library: library, parent: parent, name: "report 🚀.txt")
        let intent = try await UploadIntent.prepare(
            target: target, expected: expected,
            generator: FixedUploadIDGenerator(value: sessionRaw), bridge: validator)

        XCTAssertEqual(intent.idempotencyKey.rawValue, sessionRaw)
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: intent.requestBytes) as? [String: String])
        XCTAssertEqual(object["operation"], "CREATE_FILE")
        XCTAssertEqual(object["idempotency_key"], sessionRaw)
        XCTAssertEqual(object["library_id"], libraryRaw)
        XCTAssertEqual(object["parent_id"], parentRaw)
        XCTAssertEqual(object["name"], "report 🚀.txt")
        XCTAssertEqual(object["expected_bytes"], "0")
        XCTAssertEqual(object["expected_sha256"], emptySHA256)
        XCTAssertEqual(
            try UploadIntent.restore(
                idempotencyKey: intent.idempotencyKey, target: target,
                expected: expected, requestBytes: intent.requestBytes), intent)

        var changed = intent.requestBytes
        changed.append(0x20)
        XCTAssertThrowsError(
            try UploadIntent.restore(
                idempotencyKey: intent.idempotencyKey, target: target,
                expected: expected, requestBytes: changed))
    }

    func testReplaceRequestFreezesTheRequestedRevision() async throws {
        let library = try await LibraryId.validated(libraryRaw, using: validator)
        let node = try await NodeId.validated(nodeRaw, using: validator)
        let revision = try NodeRevision(validating: "18446744073709551617")
        let expected = try UploadExpectedContent(byteLength: 0, sha256: emptySHA256)
        let target = UploadTarget.replaceContent(
            library: library, node: node, expectedRevision: revision)
        let intent = try await UploadIntent.prepare(
            target: target, expected: expected,
            generator: FixedUploadIDGenerator(value: sessionRaw), bridge: validator)
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: intent.requestBytes) as? [String: String])

        XCTAssertEqual(object["operation"], "REPLACE_CONTENT")
        XCTAssertEqual(object["node_id"], nodeRaw)
        XCTAssertEqual(object["expected_revision"], revision.rawValue)
        XCTAssertNil(object["parent_id"])
        XCTAssertNil(object["name"])
    }

    func testCreateResponseChecksLocationAndOffsetHeaderAgainstBody() async throws {
        let intent = try await createIntent()
        let decoder = UploadSessionResponseDecoder(bridge: validator)
        let valid = try sessionResponse(intent: intent, received: "0")
        let evidence = try await decoder.decode(
            valid, intent: intent, expectedSessionId: sessionRaw,
            expectedOffset: 0, requireLocation: true)
        XCTAssertEqual(evidence.id, sessionRaw)
        XCTAssertEqual(evidence.state, .open)
        XCTAssertEqual(evidence.receivedBytes, 0)

        let wrongOffset = try sessionResponse(intent: intent, received: "0", headerOffset: "1")
        do {
            _ = try await decoder.decode(wrongOffset, intent: intent, requireLocation: true)
            XCTFail("Expected mismatched offset to be rejected")
        } catch { XCTAssertEqual(error as? UploadFailure, .invalidMetadata) }

        let wrongLocation = try sessionResponse(
            intent: intent, received: "0", location: "https://attacker.example/session")
        do {
            _ = try await decoder.decode(wrongLocation, intent: intent, requireLocation: true)
            XCTFail("Expected external Location to be rejected")
        } catch { XCTAssertEqual(error as? UploadFailure, .invalidMetadata) }
    }

    func testAllDocumentedServerStatesDecodeWithoutInventingCommitEvidence() async throws {
        let intent = try await createIntent()
        let decoder = UploadSessionResponseDecoder(bridge: validator)
        for state in UploadServerState.allCases {
            let response = try sessionResponse(
                intent: intent, received: "0", state: state.rawValue,
                status: 200, includeCompletion: state == .committed)
            let evidence = try await decoder.decode(
                response, intent: intent, expectedSessionId: sessionRaw)
            XCTAssertEqual(evidence.state, state)
            XCTAssertEqual(evidence.completion != nil, state == .committed)
        }
    }

    func testPatchOffsetRequiresCanonicalAuthoritativeHeaderAndNoBody() throws {
        let success = HTTPTransportResponse(
            statusCode: 204,
            headers: ["Upload-Offset": "4194304", "X-Request-ID": "upload-patch-01"])
        XCTAssertEqual(
            try UploadSessionResponseDecoder.patchOffset(
                success, prior: 0, chunkBytes: UploadPolicy.chunkBytes,
                expectedBytes: UInt64(UploadPolicy.chunkBytes)),
            UInt64(UploadPolicy.chunkBytes))

        let nonCanonical = HTTPTransportResponse(
            statusCode: 204,
            headers: ["Upload-Offset": "04194304", "X-Request-ID": "upload-patch-01"])
        XCTAssertThrowsError(
            try UploadSessionResponseDecoder.patchOffset(
                nonCanonical, prior: 0, chunkBytes: UploadPolicy.chunkBytes,
                expectedBytes: UInt64(UploadPolicy.chunkBytes)))

        let unexpectedBody = HTTPTransportResponse(
            statusCode: 204,
            headers: ["Upload-Offset": "1", "X-Request-ID": "upload-patch-01"],
            body: Data([1]))
        XCTAssertThrowsError(
            try UploadSessionResponseDecoder.patchOffset(
                unexpectedBody, prior: 0, chunkBytes: 1, expectedBytes: 1))
    }

    func testRawUploadTransportAcceptsOnlyBoundedUploadRoutes() async throws {
        let scripted = UploadRecordingHTTPTransport()
        let transport = URLSessionRawUploadTransport(transport: scripted)
        let body = Data("{}".utf8)
        let request = HTTPTransportRequest(
            url: URL(string: "https://upload.example/prefix/api/v1/upload-sessions")!,
            method: .post,
            headers: [
                "Authorization": "Bearer test-only", "Content-Type": "application/json",
                "Cache-Control": "no-store",
            ], body: body)
        _ = try await transport.send(request)
        let captured = await scripted.requests()
        XCTAssertEqual(captured, [request])
        XCTAssertNil(captured[0].headers["Cookie"])
        XCTAssertNil(captured[0].headers["X-CSRF-Token"])

        let invalid = HTTPTransportRequest(
            url: URL(string: "https://upload.example/api/v1/upload-sessions/extra")!,
            method: .post, headers: ["Cache-Control": "no-store"], body: Data())
        do {
            _ = try await transport.send(invalid)
            XCTFail("Expected unsupported route to be rejected")
        } catch { XCTAssertEqual(error as? UploadFailure, .scopeMismatch) }
        let requestsAfterRejection = await scripted.requests()
        XCTAssertEqual(requestsAfterRejection.count, 1)

        let unauthenticated = HTTPTransportRequest(
            url: URL(string: "https://upload.example/api/v1/upload-sessions/\(sessionRaw)")!,
            method: .get, headers: ["Cache-Control": "no-store"])
        do {
            _ = try await transport.send(unauthenticated)
            XCTFail("UploadSession calls must carry the authenticated DeviceBearer")
        } catch { XCTAssertEqual(error as? UploadFailure, .scopeMismatch) }
        let requestsAfterUnauthenticatedRejection = await scripted.requests()
        XCTAssertEqual(requestsAfterUnauthenticatedRejection.count, 1)
    }

    private func createIntent() async throws -> UploadIntent {
        let library = try await LibraryId.validated(libraryRaw, using: validator)
        let parent = try await NodeId.validated(parentRaw, using: validator)
        return try await UploadIntent.prepare(
            target: .createFile(library: library, parent: parent, name: "sample.txt"),
            expected: UploadExpectedContent(byteLength: 0, sha256: emptySHA256),
            generator: FixedUploadIDGenerator(value: sessionRaw), bridge: validator)
    }

    private func sessionResponse(
        intent: UploadIntent, received: String, headerOffset: String? = nil,
        location: String = "/api/v1/upload-sessions/\(queueUUID(700))",
        state: String = "OPEN", status: Int = 201, includeCompletion: Bool = false
    ) throws -> HTTPTransportResponse {
        let target: [String: String] = [
            "operation": "CREATE_FILE", "library_id": libraryRaw,
            "parent_id": parentRaw, "node_id": nodeRaw, "name": "sample.txt",
        ]
        var attributes: [String: Any] = [
            "operation": "CREATE_FILE", "state": state,
            "received_bytes": received, "expected_bytes": "0",
            "expected_sha256": emptySHA256,
            "created_at": "2026-10-11T12:00:00Z",
            "updated_at": "2026-10-11T12:00:00Z",
            "expires_at": "2026-10-12T12:00:00Z", "target": target,
        ]
        if includeCompletion {
            attributes["completion"] = [
                "id": sessionRaw, "type": "upload_completion",
                "attributes": [
                    "node_id": nodeRaw, "file_version_id": queueUUID(702),
                    "node_revision": "4", "bytes": "0", "sha256": emptySHA256,
                    "committed_at": "2026-10-11T12:01:00Z",
                ],
            ]
        }
        let object: [String: Any] = [
            "data": [
                "id": sessionRaw, "type": "upload_session",
                "attributes": attributes,
            ],
            "meta": ["request_id": "upload-response-01"],
        ]
        var headers = [
            "Content-Type": "application/json", "Location": location,
            "X-Request-ID": "upload-response-01",
            "Upload-Offset": headerOffset ?? received,
        ]
        if intent.target.operation != "CREATE_FILE" { headers["Location"] = nil }
        return HTTPTransportResponse(
            statusCode: status, headers: headers,
            body: try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]))
    }
}

struct FixedUploadIDGenerator: ClientMutationIdentityGeneratorProtocol {
    let value: String
    func generateClientMutationID() async throws -> String { value }
}

private actor UploadRecordingHTTPTransport: HTTPTransportProtocol {
    private var recorded: [HTTPTransportRequest] = []

    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        recorded.append(request)
        return HTTPTransportResponse(statusCode: 201, body: Data("{}".utf8))
    }

    func requests() -> [HTTPTransportRequest] { recorded }
}
