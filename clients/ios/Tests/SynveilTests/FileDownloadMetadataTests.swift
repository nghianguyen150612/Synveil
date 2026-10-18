import Foundation
import XCTest

@testable import Synveil

final class FileDownloadMetadataTests: XCTestCase {
    private func decode(
        _ object: [String: Any], headers: [String: String] = ["Content-Type": "application/json"]
    )
        async throws -> FileVersionMetadata
    {
        let version = try await FileVersionId.validated(queueUUID(501), using: QueueValidator())
        let node = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        return try await FileVersionResponseDecoder(bridge: QueueValidator()).decode(
            queueHTTP(object, headers: headers), version: version, node: node)
    }
    private func rejected(_ path: [String], _ value: Any?) async throws {
        var object = downloadVersionObject()
        if path.count == 1 {
            object[path[0]] = value
        } else if path[0] == "meta" {
            var meta = object["meta"] as! [String: Any]
            meta[path[1]] = value
            object["meta"] = meta
        } else {
            var resource = object["data"] as! [String: Any]
            if path.count == 2 {
                resource[path[1]] = value
            } else {
                var attributes = resource["attributes"] as! [String: Any]
                attributes[path[2]] = value
                resource["attributes"] = attributes
            }
            object["data"] = resource
        }
        do {
            _ = try await decode(object)
            XCTFail("Accepted invalid FileVersion metadata: \(path)")
        } catch { XCTAssertEqual(DownloadPolicy.classify(error), .invalidMetadata) }
    }
    func testValidImmutableMetadata() async throws {
        let result = try await decode(downloadVersionObject())
        XCTAssertEqual(result.id.rawValue, queueUUID(501))
        XCTAssertEqual(result.nodeId.rawValue, queueUUID(1))
        XCTAssertEqual(result.expected.byteLength, 5)
        XCTAssertTrue(result.isCurrent)
    }
    func testWrongVersion() async throws { try await rejected(["data", "id"], queueUUID(502)) }
    func testWrongNode() async throws {
        try await rejected(["data", "attributes", "node_id"], queueUUID(2))
    }
    func testNoncanonicalVersion() async throws {
        try await rejected(["data", "id"], queueUUID(501).uppercased())
    }
    func testWrongResourceType() async throws { try await rejected(["data", "type"], "node") }
    func testDecimalLeadingZero() async throws {
        try await rejected(["data", "attributes", "byte_length"], "05")
    }
    func testDecimalNegative() async throws {
        try await rejected(["data", "attributes", "byte_length"], "-5")
    }
    func testDecimalOverflow() async throws {
        try await rejected(["data", "attributes", "byte_length"], "18446744073709551616")
    }
    func testNumericLengthRejected() async throws {
        try await rejected(["data", "attributes", "byte_length"], 5)
    }
    func testFractionalLengthRejected() async throws {
        try await rejected(["data", "attributes", "byte_length"], "5.0")
    }
    func testExponentRejected() async throws {
        try await rejected(["data", "attributes", "byte_length"], "5e0")
    }
    func testInvalidDigest() async throws {
        try await rejected(["data", "attributes", "sha256"], "sha256:00")
    }
    func testUppercaseDigest() async throws {
        try await rejected(
            ["data", "attributes", "sha256"], "sha256:" + String(repeating: "A", count: 64))
    }
    func testMissingLength() async throws {
        try await rejected(["data", "attributes", "byte_length"], nil)
    }
    func testUnknownAttribute() async throws {
        try await rejected(["data", "attributes", "unknown"], "value")
    }
    func testUnknownEnvelope() async throws { try await rejected(["unknown"], "value") }
    func testUnknownResource() async throws { try await rejected(["data", "unknown"], "value") }
    func testUnknownMeta() async throws { try await rejected(["meta", "unknown"], "value") }
    func testExplicitNull() async throws {
        try await rejected(["data", "attributes", "sha256"], NSNull())
    }
    func testInvalidTimestamp() async throws {
        try await rejected(["data", "attributes", "created_at"], "2026-02-30T00:00:00Z")
    }
    func testNumericBoolean() async throws {
        try await rejected(["data", "attributes", "is_current"], 1)
    }
    func testStringBoolean() async throws {
        try await rejected(["data", "attributes", "is_current"], "true")
    }
    func testInvalidRequestId() async throws { try await rejected(["meta", "request_id"], "x") }
    func testRequestIdHeaderMismatch() async throws {
        do {
            _ = try await decode(
                downloadVersionObject(),
                headers: ["Content-Type": "application/json", "X-Request-ID": "wrong-request-01"])
            XCTFail()
        } catch { XCTAssertEqual(DownloadPolicy.classify(error), .invalidMetadata) }
    }
    func testUInt64ExactWithoutFloatingPoint() throws {
        let expected = try DownloadExpectedContent(
            length: "18446744073709551615", sha256: DownloadPolicy.emptyDigest)
        XCTAssertEqual(expected.byteLength, UInt64.max)
    }
    func testEmptyFileMetadata() async throws {
        let result = try await decode(downloadVersionObject(data: Data()))
        XCTAssertEqual(result.expected.byteLength, 0)
        XCTAssertEqual(result.expected.sha256, DownloadPolicy.emptyDigest)
    }
}

final class FileDownloadHTTPPolicyTests: XCTestCase {
    private func expected() throws -> DownloadExpectedContent {
        try DownloadExpectedContent(length: "5", sha256: downloadDigest(Data("hello".utf8)))
    }
    private func reject(
        status: Int = 200, range: DownloadRange? = nil,
        key: String? = nil, value: String? = nil, failure: DownloadFailure
    ) throws {
        let expected = try expected()
        var headers = downloadHeaders(expected, range: range)
        if let key { headers[key] = value }
        XCTAssertThrowsError(
            try DownloadResponseMetadata.validate(
                status: status, headers: headers,
                expected: expected, range: range)
        ) { XCTAssertEqual($0 as? DownloadFailure, failure) }
    }
    func testValid200() throws {
        let expected = try expected()
        XCTAssertEqual(
            try DownloadResponseMetadata.validate(
                status: 200, headers: downloadHeaders(expected),
                expected: expected, range: nil
            ).length, 5)
    }
    func testValid206InclusiveRange() throws {
        let range = try DownloadRange(start: 1, end: 4, total: 5)
        let expected = try expected()
        XCTAssertEqual(range.header, "bytes=1-4")
        XCTAssertEqual(
            try DownloadResponseMetadata.validate(
                status: 206, headers: downloadHeaders(expected, range: range),
                expected: expected, range: range
            ).length, 4)
    }
    private func badRange(_ value: String?) throws {
        try reject(
            status: 206, range: DownloadRange(start: 1, end: 4, total: 5),
            key: "Content-Range", value: value, failure: .invalidRange)
    }
    func testWrongRangeStart() throws { try badRange("bytes 0-4/5") }
    func testWrongRangeEnd() throws { try badRange("bytes 1-3/5") }
    func testWrongTotalLength() throws { try badRange("bytes 1-4/6") }
    func testMissingRange() throws { try badRange(nil) }
    func testMultipleRanges() throws { try badRange("bytes 1-2/5,3-4/5") }
    func testUnexpected200ForRange() throws {
        try reject(range: DownloadRange(start: 1, end: 4, total: 5), failure: .invalidRange)
    }
    func testRange416() throws {
        try reject(
            status: 416, range: DownloadRange(start: 1, end: 4, total: 5), key: "Content-Range",
            value: "bytes */5", failure: .invalidRange)
    }
    func testMalformed416() throws { try reject(status: 416, failure: .unexpectedStatus) }
    func test304CannotFillPartial() throws { try reject(status: 304, failure: .unexpectedStatus) }
    func testDedicated304() throws {
        let expected = try expected()
        XCTAssertTrue(
            try DownloadResponseMetadata.validate(
                status: 304, headers: ["ETag": expected.etag],
                expected: expected, range: nil, revalidating: true
            ).notModified)
    }
    func test304RejectsRange() throws {
        let expected = try expected()
        XCTAssertThrowsError(
            try DownloadResponseMetadata.validate(
                status: 304, headers: ["ETag": expected.etag],
                expected: expected, range: DownloadRange(start: 1, end: 4, total: 5),
                revalidating: true))
    }
    func test304RejectsBodyLength() throws {
        let expected = try expected()
        XCTAssertThrowsError(
            try DownloadResponseMetadata.validate(
                status: 304, headers: ["ETag": expected.etag, "Content-Length": "5"],
                expected: expected, range: nil, revalidating: true))
    }
    func testMissingLength() throws {
        try reject(key: "Content-Length", value: nil, failure: .invalidContentLength)
    }
    func testWrongLength() throws {
        try reject(key: "Content-Length", value: "4", failure: .invalidContentLength)
    }
    func testNoncanonicalLength() throws {
        try reject(key: "Content-Length", value: "05", failure: .invalidContentLength)
    }
    func testMissingETag() throws { try reject(key: "ETag", value: nil, failure: .etagMismatch) }
    func testWrongETag() throws {
        try reject(key: "ETag", value: "\"\(DownloadPolicy.emptyDigest)\"", failure: .etagMismatch)
    }
    func testUnquotedETag() throws {
        try reject(key: "ETag", value: try expected().sha256, failure: .etagMismatch)
    }
    func testWeakETag() throws {
        try reject(key: "ETag", value: "W/" + expected().etag, failure: .etagMismatch)
    }
    func testMultipartRejected() throws {
        try reject(key: "Content-Type", value: "multipart/byteranges", failure: .unexpectedStatus)
    }
    func testJSONErrorNotContent() throws {
        try reject(
            status: 404, key: "Content-Type", value: "application/json", failure: .sourceUnavailable
        )
    }
    func testServer503NotDeleted() throws { try reject(status: 503, failure: .serverUnavailable) }
    func testDeviceRevocation() throws { try reject(status: 403, failure: .deviceRevoked) }
    func testAuthenticationRejected() throws { try reject(status: 401, failure: .unauthenticated) }
    func testRedirectRejected() throws { try reject(status: 302, failure: .redirectRejected) }
    func testGzipRejected() throws {
        try reject(key: "Content-Encoding", value: "gzip", failure: .unexpectedStatus)
    }
    func testHeaderInjectionRejected() throws {
        try reject(
            key: "Content-Disposition", value: "attachment\r\nInjected: yes",
            failure: .unexpectedStatus)
    }
    func testTraversalDispositionIgnored() throws {
        let expected = try expected()
        var headers = downloadHeaders(expected)
        headers["Content-Disposition"] = "attachment; filename=\"../../escaped\""
        XCTAssertEqual(
            try DownloadResponseMetadata.validate(
                status: 200, headers: headers,
                expected: expected, range: nil
            ).length, 5)
    }
    func testInvalidRangeBounds() throws {
        let ranges: [(UInt64, UInt64, UInt64)] = [
            (0, 0, 0), (4, 1, 5), (0, 5, 5), (UInt64.max, UInt64.max, UInt64.max),
        ]
        for (start, end, total) in ranges {
            XCTAssertThrowsError(try DownloadRange(start: start, end: end, total: total))
        }
    }
    func testTLSErrorDistinct() {
        XCTAssertEqual(DownloadPolicy.classify(URLError(.serverCertificateUntrusted)), .tlsFailure)
        XCTAssertEqual(DownloadPolicy.classify(URLError(.timedOut)), .timeout)
        XCTAssertEqual(DownloadPolicy.classify(URLError(.dnsLookupFailed)), .dnsFailure)
    }
}
