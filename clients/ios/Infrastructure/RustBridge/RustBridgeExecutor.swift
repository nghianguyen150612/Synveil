import Foundation

/// Internal concurrency executor that executes bounded synchronous Rust FFI work
/// outside inherited actor isolation (e.g., `@MainActor`).
///
/// # Concurrency & Cancellation Policy
/// - Uses an encapsulated `Task.detached` worker to escape caller actor context.
/// - Task handle remains private and is never exposed to upper layers.
/// - Checks cancellation before scheduling, inside worker task, and after awaiting completion.
/// - On cancellation, cancels the worker task. In-flight synchronous Rust calls run to completion,
///   ensure all Rust-owned buffers are freed, and return a result which is discarded in favor
///   of `CancellationError`.
internal enum RustBridgeExecutor {
    /// Executes a bounded synchronous Rust bridge operation on a detached worker task
    /// off inherited actor context.
    ///
    /// - Parameters:
    ///   - priority: Optional task priority; defaults to `Task.currentPriority` if `nil`.
    ///   - operation: Bounded synchronous closure invoking Rust FFI operations.
    /// - Returns: The result produced by `operation`.
    /// - Throws: `CancellationError` if cancelled, or errors thrown by `operation`.
    internal static func run<T: Sendable>(
        priority: TaskPriority? = nil,
        _ operation: @escaping @Sendable () throws -> T
    ) async throws -> T {
        try Task.checkCancellation()

        let taskPriority = priority ?? Task.currentPriority
        let worker = Task.detached(priority: taskPriority) {
            try Task.checkCancellation()
            return try operation()
        }

        return try await withTaskCancellationHandler {
            let value = try await worker.value
            try Task.checkCancellation()
            return value
        } onCancel: {
            worker.cancel()
        }
    }
}
