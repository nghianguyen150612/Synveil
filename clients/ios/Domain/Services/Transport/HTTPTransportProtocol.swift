import Foundation

/// Abstract protocol for executing HTTP network requests.
///
/// Implemented by concrete infrastructure (e.g., `URLSessionHTTPTransport` in `Infrastructure/Network/`).
/// Application and Domain layers depend on this contract rather than concrete Apple networking.
public protocol HTTPTransportProtocol: Sendable {
    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse
}
