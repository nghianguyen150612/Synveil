import Foundation
import XCTest

@testable import Synveil

@MainActor
final class RebaselineManifestTests: XCTestCase {
    private func decode(_ change: (inout [String: Any]) -> Void) async throws
        -> RebaselineManifestPage
    {
        let scope = try await queueScope()
        let decoder = RebaselineResponseDecoder(bridge: QueueValidator())
        let bootstrap = try await decoder.start(
            queueHTTP(snapshotEnvelope(snapshotBootstrapObject(scope))), scope: scope)
        var object = snapshotPageObject(scope, rows: snapshotRows())
        var data = object["data"] as! [String: Any]
        change(&data)
        object["data"] = data
        return try await decoder.page(queueHTTP(object), expected: bootstrap)
    }
    func testTerminalAndNonterminalPages() async throws {
        let terminal = try await decode { _ in }
        XCTAssertEqual(terminal.nodes.count, 3)
        XCTAssertNotNil(terminal.evidence)
        let nonterminal = try await decode {
            $0.removeValue(forKey: "completion_token")
            $0["has_more"] = true
            $0["next_cursor"] = "opaque"
        }
        XCTAssertEqual(nonterminal.nextCursor, "opaque")
        XCTAssertNil(nonterminal.evidence)
    }
    func testContradictoryPageProofCombinations() async {
        for change: (inout [String: Any]) -> Void in [
            { $0["has_more"] = true },
            {
                $0["has_more"] = true
                $0["next_cursor"] = "opaque"
            },
            { $0["next_cursor"] = "opaque" },
            { $0.removeValue(forKey: "completion_token") },
            { $0["completion_token"] = NSNull() },
            { $0["next_cursor"] = NSNull() },
            { $0["has_more"] = "false" },
            { $0["nodes"] = "bad" },
            { $0["unexpected"] = 1 },
        ] { await snapshotAssertFailure { try await self.decode(change) } }
    }
    func testAllBootstrapGenerationFieldsMustMatch() async {
        for (key, value) in [
            "bootstrap_id": queueUUID(701), "generation": "2", "snapshot_epoch": "2",
            "snapshot_resume_sequence": "26", "manifest_item_count": "4",
            "device_id": queueUUID(91 + 1), "library_id": queueUUID(81),
        ] {
            await snapshotAssertFailure {
                try await self.decode { data in
                    var bootstrap = data["bootstrap"] as! [String: Any]
                    bootstrap[key] = value
                    data["bootstrap"] = bootstrap
                }
            }
        }
    }
    func testDuplicateAndOutOfOrderNodesRejected() async {
        await snapshotAssertFailure {
            try await self.decode { $0["nodes"] = [snapshotRows()[0], snapshotRows()[0]] }
        }
        await snapshotAssertFailure {
            try await self.decode { $0["nodes"] = Array(snapshotRows().reversed()) }
        }
    }
    func testInvalidNodeFieldsRejected() async {
        for (key, value): (String, Any) in [
            ("node_id", "bad"), ("parent_node_id", NSNull()), ("parent_node_id", queueUUID(3)),
            ("kind", "SYMLINK"), ("state", "PURGING"), ("revision", "0"), ("revision", "01"),
            ("revision", "18446744073709551616"), ("name", ""), ("current_version_id", NSNull()),
            ("current_content", NSNull()), ("extra", "bad"),
        ] {
            await snapshotAssertFailure {
                try await self.decode { data in
                    var rows = snapshotRows()
                    rows[2][key] = value
                    data["nodes"] = rows
                }
            }
        }
    }
    func testFileContentMustBeCompleteAndDirectoryContentForbidden() async throws {
        let content: [String: Any] = [
            "byte_length": "18446744073709551615", "sha256": String(repeating: "a", count: 64),
        ]
        let page = try await decode { data in
            var rows = snapshotRows()
            rows[2]["current_version_id"] = queueUUID(600)
            rows[2]["current_content"] = content
            data["nodes"] = rows
        }
        XCTAssertEqual(page.nodes[2].content?.byteLength.rawValue, "18446744073709551615")
        XCTAssertEqual(page.nodes[2].name, "Exact e\u{301} 📂 3")
        for change: (inout [String: Any]) -> Void in [
            { $0["current_version_id"] = queueUUID(600) },
            { $0["current_content"] = content },
            {
                $0["current_version_id"] = queueUUID(600)
                $0["current_content"] = [
                    "byte_length": "-1", "sha256": String(repeating: "a", count: 64),
                ]
            },
            {
                $0["current_version_id"] = queueUUID(600)
                $0["current_content"] = [
                    "byte_length": "1", "sha256": String(repeating: "A", count: 64),
                ]
            },
            {
                $0["kind"] = "DIRECTORY"
                $0["current_version_id"] = queueUUID(600)
                $0["current_content"] = content
            },
        ] {
            await snapshotAssertFailure {
                try await self.decode { data in
                    var rows = snapshotRows()
                    change(&rows[2])
                    data["nodes"] = rows
                }
            }
        }
    }
    func testOpaqueBoundsAndEmptyTerminalPage() async throws {
        await snapshotAssertFailure {
            try await self.decode { $0["completion_token"] = String(repeating: "a", count: 337) }
        }
        await snapshotAssertFailure {
            try await self.decode {
                $0.removeValue(forKey: "completion_token")
                $0["has_more"] = true
                $0["next_cursor"] = String(repeating: "a", count: 321)
            }
        }
        let terminal = try await decode { $0["nodes"] = [] }
        XCTAssertTrue(terminal.nodes.isEmpty)
        XCTAssertNotNil(terminal.evidence)
        await snapshotAssertFailure {
            try await self.decode {
                $0["nodes"] = []
                $0.removeValue(forKey: "completion_token")
                $0["has_more"] = true
                $0["next_cursor"] = "x"
            }
        }
    }
    func testFirstAndSubsequentPagePreserveOpaqueCursor() async throws {
        let f = try await queueFixture(self)
        let coordinator = snapshotCoordinator(f)
        try await snapshotStart(f, coordinator: coordinator)
        await f.transport.set(
            try queueHTTP(
                snapshotPageObject(f.scope, rows: Array(snapshotRows().prefix(2)), more: true)))
        try await coordinator.download(scope: f.scope, maximumPages: 1)
        let observed1 = await f.transport.requests().last
        let first = try XCTUnwrap(observed1)
        let firstQuery = URLComponents(url: first.url, resolvingAgainstBaseURL: false)!.queryItems!
        XCTAssertEqual(firstQuery.first(where: { $0.name == "limit" })?.value, "200")
        XCTAssertNil(firstQuery.first(where: { $0.name == "cursor" }))
        await f.transport.set(
            try queueHTTP(snapshotPageObject(f.scope, rows: Array(snapshotRows().suffix(1)))))
        try await coordinator.download(scope: f.scope, maximumPages: 1)
        let observed2 = await f.transport.requests().last
        let next = try XCTUnwrap(observed2)
        XCTAssertEqual(
            URLComponents(url: next.url, resolvingAgainstBaseURL: false)?.queryItems?.first(where: {
                $0.name == "cursor"
            })?.value, "opaque.server.cursor+_")
        let status = try await coordinator.status(scope: f.scope)
        XCTAssertEqual(status?.pages, 2)
        XCTAssertEqual(status?.stagedNodes, 3)
    }
    func testRequestPageSizeLimits() async throws {
        let f = try await queueFixture(self)
        let session = try await f.provider.begin()
        let bootstrap = try await RebaselineResponseDecoder(bridge: QueueValidator()).start(
            queueHTTP(snapshotEnvelope(snapshotBootstrapObject(f.scope))), scope: f.scope)
        for limit in [0, -1, 1001] {
            await snapshotAssertFailure {
                try await f.provider.requestRebaselinePage(
                    bootstrap: bootstrap, cursor: nil, limit: limit, session: session)
            }
        }
        _ = try await f.provider.requestRebaselinePage(
            bootstrap: bootstrap, cursor: nil, limit: 1000, session: session)
        await snapshotAssertFailure {
            try await f.provider.requestRebaselinePage(
                bootstrap: bootstrap, cursor: String(repeating: "a", count: 321), limit: 200,
                session: session)
        }
    }
    func testRepeatedCursorAndCrossPageNodeRegressionDoNotCommit() async throws {
        for repeated in [true, false] {
            let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
            try await snapshotStart(f, coordinator: coordinator)
            await f.transport.set(
                try queueHTTP(snapshotPageObject(f.scope, rows: [snapshotRows()[0]], more: true)))
            try await coordinator.download(scope: f.scope, maximumPages: 1)
            let rows = repeated ? [snapshotRows()[1]] : [snapshotRows()[0]]
            await f.transport.set(
                try queueHTTP(
                    snapshotPageObject(
                        f.scope, rows: rows, more: true,
                        cursor: repeated ? "opaque.server.cursor+_" : "different.cursor")))
            await snapshotAssertFailure(.protocolFailure) {
                try await coordinator.download(scope: f.scope, maximumPages: 1)
            }
            let status = try await coordinator.status(scope: f.scope)
            XCTAssertEqual(status?.pages, 1)
            XCTAssertEqual(status?.stagedNodes, 1)
        }
    }
}
