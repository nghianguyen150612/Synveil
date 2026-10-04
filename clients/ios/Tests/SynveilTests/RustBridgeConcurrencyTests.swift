import Foundation
import XCTest
@testable import Synveil

final class RustBridgeConcurrencyTests: XCTestCase {

    // MARK: - MainActor & Thread Isolation Tests

    @MainActor
    func testExecutorEscapesMainThread() async throws {
        XCTAssertTrue(Thread.isMainThread, "Test runner starting condition")

        let workerIsMainThread = try await RustBridgeExecutor.run {
            Thread.isMainThread
        }

        XCTAssertFalse(
            workerIsMainThread,
            "RustBridgeExecutor worker must execute off MainActor / main thread"
        )
    }

    @MainActor
    func testMainActorRealBridgeCall() async throws {
        XCTAssertTrue(Thread.isMainThread)

        let adapter = try await RustBridgeAsyncAdapter()
        let canonical = "sha256:abababababababababababababababababababababababababababababababab"

        let digest = try await adapter.parseSHA256(canonical)
        XCTAssertEqual(digest.count, 32)
        XCTAssertEqual(digest, Data(repeating: 0xab, count: 32))

        let formatted = try await adapter.formatSHA256(digest)
        XCTAssertEqual(formatted, canonical)
    }

    // MARK: - Stress & Concurrency Tests

    func testConcurrentTaskGroupStress() async throws {
        let adapter = try await RustBridgeAsyncAdapter()
        let taskCount = 32
        let cyclesPerTask = 100

        try await withThrowingTaskGroup(of: Void.self) { group in
            for taskIdx in 0..<taskCount {
                group.addTask {
                    let hexByte = String(format: "%02x", (taskIdx % 16) * 16)
                    let hex64 = String(repeating: hexByte, count: 32)
                    let canonicalStr = "sha256:\(hex64)"

                    for _ in 0..<cyclesPerTask {
                        let digest = try await adapter.parseSHA256(canonicalStr)
                        XCTAssertEqual(digest.count, 32)

                        let formatted = try await adapter.formatSHA256(digest)
                        XCTAssertEqual(formatted, canonicalStr)
                    }
                }
            }

            try await group.waitForAll()
        }
    }

    func testConcurrentErrorIsolation() async throws {
        let adapter = try await RustBridgeAsyncAdapter()
        let taskCount = 32

        try await withThrowingTaskGroup(of: Bool.self) { group in
            for taskIdx in 0..<taskCount {
                let isMalformed = (taskIdx % 2 == 1)
                group.addTask {
                    if isMalformed {
                        let malformed = "sha256:invalid_hex_string_\(taskIdx)"
                        do {
                            _ = try await adapter.parseSHA256(malformed)
                            XCTFail("Malformed input should throw domainError")
                            return false
                        } catch let error as RustBridgeError {
                            XCTAssertEqual(error, .domainError)
                            return true
                        } catch {
                            XCTFail("Unexpected error type: \(error)")
                            return false
                        }
                    } else {
                        let canonical = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        let digest = try await adapter.parseSHA256(canonical)
                        XCTAssertEqual(digest.count, 32)
                        let formatted = try await adapter.formatSHA256(digest)
                        XCTAssertEqual(formatted, canonical)
                        return true
                    }
                }
            }

            var successCount = 0
            for try await result in group {
                if result {
                    successCount += 1
                }
            }

            XCTAssertEqual(successCount, taskCount)
        }
    }

    // MARK: - Cancellation Tests

    func testCancellationPreAndInFlight() async throws {
        // 1. Pre-cancelled task
        let preCancelledTask = Task {
            try Task.checkCancellation()
            return try await RustBridgeExecutor.run {
                "should_not_run"
            }
        }
        preCancelledTask.cancel()

        do {
            _ = try await preCancelledTask.value
            XCTFail("Pre-cancelled task should throw CancellationError")
        } catch {
            XCTAssertTrue(error is CancellationError)
        }

        // 2. Cancellation during worker sleep
        let inFlightTask = Task {
            try await RustBridgeExecutor.run {
                Thread.sleep(forTimeInterval: 0.1)
                return "completed"
            }
        }

        // Allow worker to start and enter sleep
        try await Task.sleep(nanoseconds: 10_000_000) // 10ms
        inFlightTask.cancel()

        do {
            _ = try await inFlightTask.value
            XCTFail("In-flight cancelled task should throw CancellationError")
        } catch {
            XCTAssertTrue(error is CancellationError)
        }
    }

    func testPostCancellationBridgeHealth() async throws {
        let adapter = try await RustBridgeAsyncAdapter()

        // Cancel a task on the executor
        let cancelledTask = Task {
            try await RustBridgeExecutor.run {
                Thread.sleep(forTimeInterval: 0.05)
                return "discarded"
            }
        }
        cancelledTask.cancel()

        do {
            _ = try await cancelledTask.value
        } catch {
            XCTAssertTrue(error is CancellationError)
        }

        // Prove bridge remains healthy and fully functional after cancellation
        let canonical = "sha256:fe34fe34fe34fe34fe34fe34fe34fe34fe34fe34fe34fe34fe34fe34fe34fe34"
        let digest = try await adapter.parseSHA256(canonical)
        XCTAssertEqual(digest.count, 32)

        let formatted = try await adapter.formatSHA256(digest)
        XCTAssertEqual(formatted, canonical)
    }

    // MARK: - Error Propagation Tests

    func testAsyncErrorPropagation() async throws {
        let adapter = try await RustBridgeAsyncAdapter()

        // Domain Error propagation
        do {
            _ = try await adapter.parseSHA256("sha256:not_a_hex_string")
            XCTFail("Parse should throw domainError")
        } catch let error as RustBridgeError {
            XCTAssertEqual(error, .domainError)
        }

        // Invalid UTF-8 propagation
        do {
            let invalidUtf8Bytes: [UInt8] = [0xFF, 0xFE, 0xFD]
            _ = try await adapter.parseSHA256UTF8Bytes(invalidUtf8Bytes)
            XCTFail("Parse invalid UTF-8 should throw invalidUtf8")
        } catch let error as RustBridgeError {
            XCTAssertEqual(error, .invalidUtf8)
        }
    }
}
