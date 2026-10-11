import Foundation

struct DownloadOperationId: Hashable, Sendable {
    let rawValue: String
    init() { rawValue = UUID().uuidString.lowercased() }
    init(validating value: String) throws {
        guard UUID(uuidString: value)?.uuidString.lowercased() == value else {
            throw DownloadFailure.invalidMetadata
        }
        rawValue = value
    }
}

struct DownloadExpectedContent: Equatable, Sendable {
    let byteLength: UInt64
    let sha256: String
    init(length: String, sha256: String) throws {
        guard let length = DownloadPolicy.decimal(length),
            LibraryWireValidation.matches(sha256, pattern: "^sha256:[0-9a-f]{64}$")
        else { throw DownloadFailure.invalidMetadata }
        byteLength = length
        self.sha256 = sha256
    }
    var etag: String { "\"\(sha256)\"" }
}

struct DownloadIdentity: Equatable, Sendable {
    let scope: ClientMutationScope
    let nodeId: NodeId
    let versionId: FileVersionId
    let expected: DownloadExpectedContent
    let credentialId: String
    /// Original transient session ownership; preserved as evidence across process restart.
    let sessionId: String
}

struct DownloadRange: Equatable, Sendable {
    let start: UInt64
    let end: UInt64
    init(start: UInt64, end: UInt64, total: UInt64) throws {
        guard start <= end, end < total else { throw DownloadFailure.invalidRange }
        self.start = start
        self.end = end
    }
    var length: UInt64 { end - start + 1 }
    var header: String { "bytes=\(start)-\(end)" }
}

enum DownloadState: String, Sendable, CaseIterable {
    case prepared = "PREPARED", downloading = "DOWNLOADING", partial = "PARTIAL"
    case verifying = "VERIFYING", verified = "VERIFIED", cancelled = "CANCELLED"
    case failedRecoverable = "FAILED_RECOVERABLE", failedPermanent = "FAILED_PERMANENT"
    case integrityFailure = "INTEGRITY_FAILURE", storageUnavailable = "STORAGE_UNAVAILABLE"
    case sessionInvalidated = "SESSION_INVALIDATED", sourceUnavailable = "SOURCE_UNAVAILABLE"

    var canResume: Bool {
        [
            .prepared, .downloading, .partial, .verifying, .cancelled, .failedRecoverable,
            .storageUnavailable,
        ].contains(self)
    }
    var label: String {
        switch self {
        case .prepared: "Preparing"
        case .downloading: "Downloading"
        case .partial: "Paused — Resume Available"
        case .verifying: "Verifying"
        case .verified: "Downloaded and Verified"
        case .cancelled: "Cancelled — Resume Available"
        case .failedRecoverable: "Failed — Resume Available"
        case .storageUnavailable: "Storage unavailable — Recovery Required"
        case .sessionInvalidated: "Session changed"
        case .sourceUnavailable: "Source unavailable"
        case .failedPermanent, .integrityFailure: "Failed — Recovery Required"
        }
    }
}

struct DownloadCheckpoint: Equatable, Sendable {
    let offset: UInt64
    let prefixSHA256: String
}

struct DownloadRecord: Equatable, Sendable {
    let id: DownloadOperationId
    let identity: DownloadIdentity
    let checkpoint: DownloadCheckpoint
    let state: DownloadState
    let attemptCount: Int
    let failure: DownloadFailure?
}

struct DownloadProgress: Equatable, Sendable {
    let operationId: DownloadOperationId
    let durableBytes: UInt64
    let totalBytes: UInt64
    let state: DownloadState
    init(_ record: DownloadRecord) {
        operationId = record.id
        durableBytes = record.checkpoint.offset
        totalBytes = record.identity.expected.byteLength
        state = record.state
    }
}

struct DownloadStorageReservation: Sendable {
    let operationId: DownloadOperationId
    let bytes: UInt64
}

/// No path or URL is authority. Every use must return through the coordinator for durable,
/// filesystem and current-session validation. No export/preview API is introduced here.
struct VerifiedDownloadedFile: Sendable {
    let operationId: DownloadOperationId
    let identity: DownloadIdentity
    let session: LibraryRequestScope
}

enum DownloadRecoveryDecision: Equatable, Sendable {
    case resume(DownloadCheckpoint)
    case verifyPublished
}

enum DownloadFailure: String, Error, Sendable, CaseIterable {
    case unauthenticated, deviceRevoked, scopeMismatch, staleSession, offline, dnsFailure, timeout
    case tlsFailure, redirectRejected, serverUnavailable, sourceUnavailable, invalidMetadata
    case unexpectedStatus, invalidRange, invalidContentLength, etagMismatch, contentLengthMismatch
    case sha256Mismatch, insufficientStorage, stagingQuotaExceeded, stagingMissing,
        filesystemFailure
    case storageFailure, cancelled, integrityFailure, duplicateOperation

    var state: DownloadState {
        switch self {
        case .cancelled: .cancelled
        case .staleSession, .scopeMismatch, .unauthenticated, .deviceRevoked: .sessionInvalidated
        case .sha256Mismatch, .integrityFailure: .integrityFailure
        case .sourceUnavailable: .sourceUnavailable
        case .insufficientStorage, .stagingQuotaExceeded, .filesystemFailure, .storageFailure:
            .storageUnavailable
        case .offline, .dnsFailure, .timeout, .serverUnavailable: .failedRecoverable
        default: .failedPermanent
        }
    }
    var message: String {
        switch self {
        case .offline, .dnsFailure, .timeout, .serverUnavailable:
            "The download stopped. Check your connection, then choose Resume."
        case .insufficientStorage, .stagingQuotaExceeded:
            "There is not enough private storage for this download. Free space and try again."
        case .cancelled: "The download was cancelled. Validated progress can be resumed."
        case .staleSession, .scopeMismatch, .unauthenticated, .deviceRevoked:
            "The authenticated session changed. File access has been revoked."
        case .sourceUnavailable: "The server no longer authorizes this file version."
        case .sha256Mismatch, .integrityFailure:
            "The file failed integrity verification. It is unavailable; contact the server owner."
        case .stagingMissing: "Private cached content is missing. Start a new download."
        case .invalidRange: "The server did not return the requested bytes. Recovery is required."
        case .tlsFailure: "The secure server connection could not be verified."
        default: "The download could not be safely completed. Recovery is required."
        }
    }
}

enum DownloadPolicy {
    static let quota: UInt64 = 512 * 1024 * 1024
    static let freeReserve: UInt64 = 64 * 1024 * 1024
    static let bufferSize = 64 * 1024
    static let emptyDigest =
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    static func decimal(_ value: String) -> UInt64? {
        guard value.utf8.count <= 20,
            LibraryWireValidation.matches(value, pattern: "^(0|[1-9][0-9]*)$")
        else { return nil }
        return UInt64(value)
    }
    static func classify(_ error: Error) -> DownloadFailure {
        if error is CancellationError { return .cancelled }
        if let failure = error as? DownloadFailure { return failure }
        if let failure = error as? LibraryFailure {
            switch failure {
            case .unauthenticated, .authenticationRejected: return .unauthenticated
            case .deviceRevoked: return .deviceRevoked
            case .staleSession, .invalidCredential, .credentialUnavailable: return .staleSession
            case .originMismatch: return .scopeMismatch
            case .offline: return .offline
            case .dnsFailure: return .dnsFailure
            case .timeout: return .timeout
            case .tlsFailure: return .tlsFailure
            case .redirectRejected: return .redirectRejected
            case .serverUnavailable: return .serverUnavailable
            case .cancelled: return .cancelled
            case .httpFailure(let code): return code == 404 ? .sourceUnavailable : .unexpectedStatus
            default: return .invalidMetadata
            }
        }
        if let error = error as? URLError {
            switch error.code {
            case .cancelled: return .cancelled
            case .timedOut: return .timeout
            case .cannotFindHost, .dnsLookupFailed: return .dnsFailure
            case .secureConnectionFailed, .serverCertificateUntrusted,
                .serverCertificateHasBadDate, .serverCertificateHasUnknownRoot,
                .serverCertificateNotYetValid, .clientCertificateRejected,
                .clientCertificateRequired:
                return .tlsFailure
            default: return .offline
            }
        }
        return .storageFailure
    }
}
