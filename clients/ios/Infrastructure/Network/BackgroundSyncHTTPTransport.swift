import Foundation

/// Uses the existing bounded, redirect-rejecting HTTP reader. The unmetered configuration applies
/// to each request; no reachability observer claims that an interface cannot change in flight.
@MainActor
final class BackgroundSyncHTTPTransport: HTTPTransportProtocol {
    private let policy: any BackgroundSyncPolicyStorage
    private let gate: BackgroundSyncRequestGate
    private let unmetered: any HTTPTransportProtocol
    private let cellular: any HTTPTransportProtocol
    init(
        policy: any BackgroundSyncPolicyStorage, gate: BackgroundSyncRequestGate,
        unmetered: (any HTTPTransportProtocol)? = nil,
        cellular: (any HTTPTransportProtocol)? = nil
    ) {
        self.policy = policy
        self.gate = gate
        self.unmetered = unmetered ?? Self.transport(allowsExpensive: false)
        self.cellular = cellular ?? Self.transport(allowsExpensive: true)
    }
    private static func transport(allowsExpensive: Bool) -> URLSessionHTTPTransport {
        URLSessionHTTPTransport(
            requestTimeout: BackgroundSyncLimits.requestTimeout,
            resourceTimeout: BackgroundSyncLimits.resourceTimeout,
            maxResponseBodyBytes: BackgroundSyncLimits.responseBytes,
            allowsCellularAccess: allowsExpensive, allowsExpensiveNetworkAccess: allowsExpensive,
            allowsConstrainedNetworkAccess: false)
    }
    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        try gate.check()
        // Fail closed if a future caller supplies a content or outbound operation to this layer.
        let path = request.url.path
        let permittedRead =
            request.method == .get
            && (path.hasSuffix("/libraries") || path.hasSuffix("/changes")
                || path.contains("/nodes/")
                    && BackgroundSyncIdentity.validIdentifier(request.url.lastPathComponent))
        let permittedAck = request.method == .post && path.hasSuffix("/changes/ack")
        guard permittedRead || permittedAck else { throw SynveilTransportError.configurationError }
        let transport = policy.read().network == .unmetered ? unmetered : cellular
        return try await transport.send(request)
    }
}
