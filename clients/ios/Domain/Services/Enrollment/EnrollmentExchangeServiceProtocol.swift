import Foundation

/// Protocol abstraction for the device enrollment exchange HTTP service.
public protocol EnrollmentExchangeServiceProtocol: Sendable {
    /// Executes a single-shot device enrollment exchange request (`POST /api/v1/device-enrollment/exchange`).
    ///
    /// # Single-Shot & Security Invariants
    /// - Must NOT perform automatic retries on failure or ambiguous network outcomes.
    /// - Response body is bounded to 16 KiB (16,384 bytes).
    /// - Returned device credential (`svd1_`) and opaque IDs are strictly validated.
    ///
    /// - Parameters:
    ///   - endpoint: Validated server base endpoint.
    ///   - token: Validated single-use enrollment token (`sve1_`).
    /// - Returns: Typed `EnrollmentExchangeResult`.
    func exchange(
        endpoint: ServerEndpoint,
        token: EnrollmentToken
    ) async -> EnrollmentExchangeResult
}
