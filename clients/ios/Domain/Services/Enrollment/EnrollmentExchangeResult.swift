import Foundation

/// Specific reasons for an ambiguous enrollment outcome where recovery is required.
public enum EnrollmentRecoveryReason: Sendable, Equatable {
    case timeout
    case disconnect
    case responseLoss
    case http503
    case redirect
    case malformedResponse
    case oversizedResponse
    case wrongContentType
}

/// Reasons for early client-side or configuration failures before/during exchange.
public enum EnrollmentFailureReason: Sendable, Equatable {
    case invalidConfiguration
    case cancelled
    case transport(SynveilTransportError)
    case secureStorageUnavailable
}

/// Typed result returned by device enrollment exchange logic.
public enum EnrollmentExchangeResult: Sendable, Equatable {
    case success(DeviceCredentialRecord)
    case rejected(code: String?, requestId: String?)
    case recoveryRequired(EnrollmentRecoveryReason)
    case failed(EnrollmentFailureReason)
}
