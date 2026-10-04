import Foundation

/// Asynchronous, Swift-facing concurrency boundary wrapping synchronous `RustBridgeAdapter`.
///
/// # Concurrency & Isolation Guarantee
/// - Conforms to `Sendable`.
/// - Guarantees synchronous Rust FFI operations run off inherited caller actor context
///   (`@MainActor`) via `RustBridgeExecutor`.
/// - Exposes only native Swift value types (`Data`, `String`) and errors.
public struct RustBridgeAsyncAdapter: RustBridgeProtocol, Sendable {
    /// Low-level synchronous adapter primitive.
    private let syncAdapter: RustBridgeAdapter

    /// Initializes the asynchronous adapter off MainActor, executing ABI probing via
    /// `RustBridgeExecutor`.
    public init() async throws {
        self.syncAdapter = try await RustBridgeExecutor.run {
            try RustBridgeAdapter()
        }
    }

    /// Internal initializer allowing injection of an existing `RustBridgeAdapter`
    /// (primarily for testing).
    internal init(syncAdapter: RustBridgeAdapter) {
        self.syncAdapter = syncAdapter
    }

    /// Asynchronously parses a canonical SHA-256 string (e.g. `sha256:<64 hex chars>`)
    /// into 32-byte raw digest `Data`.
    public func parseSHA256(_ canonical: String) async throws -> Data {
        let adapter = self.syncAdapter
        return try await RustBridgeExecutor.run {
            try adapter.parseSHA256(canonical)
        }
    }

    /// Internal test seam for raw UTF-8 byte arrays (e.g., testing invalid UTF-8 bytes).
    internal func parseSHA256UTF8Bytes(_ bytes: [UInt8]) async throws -> Data {
        let adapter = self.syncAdapter
        return try await RustBridgeExecutor.run {
            try adapter.parseSHA256UTF8Bytes(bytes)
        }
    }

    /// Asynchronously formats a 32-byte raw SHA-256 digest `Data` into a canonical
    /// `sha256:<64 hex chars>` string.
    public func formatSHA256(_ digest: Data) async throws -> String {
        let adapter = self.syncAdapter
        return try await RustBridgeExecutor.run {
            try adapter.formatSHA256(digest)
        }
    }

    // MARK: - Validation Primitive Operations

    /// Asynchronously validates whether a token matches the canonical enrollment secret format (`sve1_<64 hex chars>`).
    public func validateEnrollmentToken(_ token: String) async throws -> Bool {
        let adapter = self.syncAdapter
        return try await RustBridgeExecutor.run {
            try adapter.validateEnrollmentToken(token)
        }
    }

    /// Asynchronously validates whether a token matches the canonical device bearer credential format (`svd1_<64 hex chars>`).
    public func validateDeviceBearerToken(_ token: String) async throws -> Bool {
        let adapter = self.syncAdapter
        return try await RustBridgeExecutor.run {
            try adapter.validateDeviceCredential(token)
        }
    }

    /// Asynchronously validates whether a string matches a canonical lowercase hyphenated UUIDv7 LibraryId.
    public func validateLibraryID(_ value: String) async throws -> Bool {
        let adapter = self.syncAdapter
        return try await RustBridgeExecutor.run {
            try adapter.validateLibraryID(value)
        }
    }

    /// Asynchronously validates whether a string matches a canonical lowercase hyphenated UUIDv7 NodeId.
    public func validateNodeID(_ value: String) async throws -> Bool {
        let adapter = self.syncAdapter
        return try await RustBridgeExecutor.run {
            try adapter.validateNodeID(value)
        }
    }

    /// Asynchronously validates whether a string is a valid non-empty LogicalName (<= 1024 UTF-8 bytes).
    public func validateLogicalName(_ value: String) async throws -> Bool {
        let adapter = self.syncAdapter
        return try await RustBridgeExecutor.run {
            try adapter.validateLogicalName(value)
        }
    }
}
