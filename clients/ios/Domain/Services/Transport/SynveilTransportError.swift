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
