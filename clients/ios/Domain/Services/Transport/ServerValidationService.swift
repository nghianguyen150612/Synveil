import Foundation

/// Protocol abstraction for pre-authentication server health validation.
public protocol ServerValidationServiceProtocol: Sendable {
    /// Validates liveness and readiness of the specified server endpoint.
    ///
    /// Executes `/health/live` followed by `/health/ready` iff liveness succeeds.
    func validateServer(endpoint: ServerEndpoint) async -> ConnectionCheckResult
}

/// Domain service executing canonical pre-authentication server health reachability probe.
public final class ServerValidationService: ServerValidationServiceProtocol, Sendable {
    private struct HealthResponse: Codable {
        let status: String
    }

    private struct ErrorEnvelope: Codable {
        let error: ErrorPayload
    }

    private struct ErrorPayload: Codable {
        let code: String
        let message: String
        let requestId: String?
        let retryable: Bool?

        private enum CodingKeys: String, CodingKey {
            case code
            case message
            case requestId = "request_id"
            case retryable
        }
    }

    private static let requestIdRegex = try! NSRegularExpression(
        pattern: "^[A-Za-z0-9._~-]{8,128}$"
    )
    private static let errorCodeRegex = try! NSRegularExpression(
        pattern: "^[a-z][a-z0-9_]{1,63}$"
    )

    private let transport: HTTPTransportProtocol

    /// Initializes `ServerValidationService` with an abstract transport provider.
    ///
    /// - Parameter transport: HTTP transport conforming to `HTTPTransportProtocol`.
    public init(transport: HTTPTransportProtocol) {
        self.transport = transport
    }

    /// Validates liveness (`GET /health/live`) and readiness (`GET /health/ready`).
    public func validateServer(endpoint: ServerEndpoint) async -> ConnectionCheckResult {
        // 1. Check liveness
        let livenessResult = await executeProbe(
            endpoint: endpoint,
            path: "/health/live",
            expectedStatus: "live"
        )
        let livenessRequestId: String?
        switch livenessResult {
        case .success(let reqId):
            livenessRequestId = reqId
        case .failure(let error):
            return .failure(error)
        }

        // 2. Check readiness only if liveness succeeded
        let readinessResult = await executeProbe(
            endpoint: endpoint,
            path: "/health/ready",
            expectedStatus: "ready"
        )
        switch readinessResult {
        case .success(let reqId):
            return .ready(livenessRequestId: livenessRequestId, readinessRequestId: reqId)
        case .failure(let error):
            if case .httpError(let statusCode, let code, let reqId) = error, statusCode == 503 {
                return .aliveButNotReady(requestId: reqId, code: code)
            }
            return .failure(error)
        }
    }

    /// Executes probe against a specific health path.
    private func executeProbe(
        endpoint: ServerEndpoint,
        path: String,
        expectedStatus: String
    ) async -> ProbeResult {
        guard let probeURL = constructProbeURL(baseURL: endpoint.url, path: path) else {
            return .failure(.configurationError)
        }

        let request = HTTPTransportRequest(url: probeURL, method: .get)
        let response: HTTPTransportResponse
        do {
            response = try await transport.send(request)
        } catch let error as SynveilTransportError {
            return .failure(error)
        } catch is CancellationError {
            return .failure(.cancelled)
        } catch {
            return .failure(.offline)
        }

        let headerRequestId = extractSafeHeaderRequestId(response.headers)

        // Reject HTTP redirects
        if response.statusCode >= 300 && response.statusCode <= 399 {
            return .failure(.redirectRejected(statusCode: response.statusCode))
        }

        // Verify JSON Content-Type
        guard isJsonContentType(response.headers) else {
            let ct = response.headers.first(where: {
                $0.key.lowercased() == "content-type"
            })?.value
            return .failure(.unexpectedContentType(contentType: ct))
        }

        if response.statusCode == 200 {
            return parseHealthResponse(
                body: response.body,
                expectedStatus: expectedStatus,
                requestId: headerRequestId
            )
        }

        return parseErrorResponse(
            body: response.body,
            statusCode: response.statusCode,
            headerRequestId: headerRequestId
        )
    }

    /// Safely constructs health URL guaranteeing origin preservation.
    private func constructProbeURL(baseURL: URL, path: String) -> URL? {
        guard var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: true) else {
            return nil
        }
        let basePath = components.path.isEmpty || components.path == "/" ? "" : components.path
        let trimmedBasePath = basePath.hasSuffix("/") ? String(basePath.dropLast()) : basePath
        components.path = trimmedBasePath + path
        return components.url
    }

    /// Parses successful health JSON body.
    private func parseHealthResponse(
        body: Data,
        expectedStatus: String,
        requestId: String?
    ) -> ProbeResult {
        let decoder = JSONDecoder()
        guard let parsed = try? decoder.decode(HealthResponse.self, from: body) else {
            return .failure(.protocolError(.invalidJSON))
        }
        guard parsed.status == expectedStatus else {
            return .failure(.protocolError(.invalidHealthStatus))
        }
        return .success(requestId: requestId)
    }

    /// Parses HTTP error response body.
    private func parseErrorResponse(
        body: Data,
        statusCode: Int,
        headerRequestId: String?
    ) -> ProbeResult {
        let decoder = JSONDecoder()
        guard let envelope = try? decoder.decode(ErrorEnvelope.self, from: body) else {
            return .failure(.protocolError(.invalidErrorEnvelope))
        }
        let payload = envelope.error
        guard
            !payload.code.isEmpty,
            payload.code.count <= 64,
            isValidRegex(payload.code, regex: Self.errorCodeRegex),
            !payload.message.isEmpty,
            payload.message.count <= 512
        else {
            return .failure(.protocolError(.invalidErrorEnvelope))
        }

        let bodyRequestId = payload.requestId.flatMap { sanitizeRequestId($0) }
        let effectiveRequestId = headerRequestId ?? bodyRequestId

        return .failure(
            .httpError(
                statusCode: statusCode,
                code: payload.code,
                requestId: effectiveRequestId
            )
        )
    }

    private func isJsonContentType(_ headers: [String: String]) -> Bool {
        guard
            let contentType = headers.first(where: {
                $0.key.lowercased() == "content-type"
            })?.value
        else {
            return false
        }
        let mime = contentType.components(separatedBy: ";").first?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        return mime == "application/json"
    }

    private func extractSafeHeaderRequestId(_ headers: [String: String]) -> String? {
        guard
            let val = headers.first(where: {
                $0.key.lowercased() == "x-request-id"
            })?.value
        else {
            return nil
        }
        return sanitizeRequestId(val)
    }

    private func sanitizeRequestId(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard isValidRegex(trimmed, regex: Self.requestIdRegex) else {
            return nil
        }
        return trimmed
    }

    private func isValidRegex(_ string: String, regex: NSRegularExpression) -> Bool {
        let range = NSRange(location: 0, length: string.utf16.count)
        return regex.firstMatch(in: string, options: [], range: range) != nil
    }
}
