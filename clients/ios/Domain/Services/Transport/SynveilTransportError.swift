import Foundation

/// Specific protocol error classification for transport payload parsing.
public enum ProtocolErrorKind: String, Sendable, Equatable {
    case invalidJSON
    case invalidHealthStatus
    case invalidErrorEnvelope
}

/// Typed Swift error model for network transport and server probing operations.
public enum SynveilTransportError: Error, Sendable, Equatable {
    case offline
    case dnsFailure
    case timeout
    case tlsError
    case redirectRejected(statusCode: Int)
    case httpError(statusCode: Int, code: String?, requestId: String?)
    case bodyLimitExceeded
    case unexpectedContentType(contentType: String?)
    case malformedResponse
    case protocolError(ProtocolErrorKind)
    case configurationError
    case cancelled
}

extension SynveilTransportError {
    /// Only recognized connectivity failures authorize offline fallback. Unknown failures remain
    /// unverifiable; their localized descriptions never determine recovery or cache access.
    static func classify(_ error: Error) -> Self {
        if let failure = error as? Self { return failure }
        if error is CancellationError { return .cancelled }
        guard let error = error as? URLError else { return .malformedResponse }
        switch error.code {
        case .notConnectedToInternet, .cannotConnectToHost, .networkConnectionLost,
            .internationalRoamingOff, .dataNotAllowed:
            return .offline
        case .cannotFindHost, .dnsLookupFailed: return .dnsFailure
        case .timedOut: return .timeout
        case .serverCertificateUntrusted, .serverCertificateHasBadDate,
            .serverCertificateHasUnknownRoot, .serverCertificateNotYetValid,
            .secureConnectionFailed, .clientCertificateRejected, .clientCertificateRequired:
            return .tlsError
        case .cancelled: return .cancelled
        default: return .malformedResponse
        }
    }
}
