import Foundation

/// Pure Swift boundary protocol exposing Rust core services to the Application layer.
///
/// # Architectural Invariants
/// - Is pure Swift; imports no raw FFI module (`SynveilRustFFI`).
/// - Exposes no raw pointers or `SynveilFfiBuffer` instances.
/// - Conforms to `Sendable`.
/// - All methods are `async throws` returning native Swift values (`Data`, `String`, `Bool`).
public protocol RustBridgeProtocol: Sendable {
    /// Asynchronously parses a canonical SHA-256 string (`sha256:<64 hex chars>`)
    /// into 32-byte raw digest `Data`.
    func parseSHA256(_ canonical: String) async throws -> Data

    /// Asynchronously formats 32-byte raw SHA-256 digest `Data` into a
    /// canonical `sha256:<64 hex chars>` string.
    func formatSHA256(_ digest: Data) async throws -> String

    /// Asynchronously validates whether a token matches the canonical enrollment secret
    /// format (`sve1_<64 hex chars>`).
    func validateEnrollmentToken(_ token: String) async throws -> Bool

    /// Asynchronously validates whether a token matches the canonical device bearer
    /// credential format (`svd1_<64 hex chars>`).
    func validateDeviceBearerToken(_ token: String) async throws -> Bool

    /// Asynchronously validates whether a string matches a canonical lowercase
    /// hyphenated UUIDv7 LibraryId.
    func validateLibraryID(_ value: String) async throws -> Bool

    /// Asynchronously validates whether a string matches a canonical lowercase
    /// hyphenated UUIDv7 NodeId.
    func validateNodeID(_ value: String) async throws -> Bool

    /// Asynchronously validates whether a string is a valid non-empty LogicalName
    /// (<= 1024 UTF-8 bytes).
    func validateLogicalName(_ value: String) async throws -> Bool
}
