import Foundation

/// Result of a single health probe endpoint check (`/health/live` or `/health/ready`).
public enum ProbeResult: Sendable, Equatable {
    case success(requestId: String?)
    case failure(SynveilTransportError)
}

/// Overall result of full two-stage server connectivity and readiness validation.
public enum ConnectionCheckResult: Sendable, Equatable {
    case ready(livenessRequestId: String?, readinessRequestId: String?)
    case aliveButNotReady(requestId: String?, code: String?)
    case failure(SynveilTransportError)
}
