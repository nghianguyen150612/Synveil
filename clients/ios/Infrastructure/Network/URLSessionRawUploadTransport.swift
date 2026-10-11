import Foundation

/// The upload transport accepts only the five UploadSession routes and bounded bodies.
/// It delegates TLS, redirect rejection, cookie isolation and response limits to the
/// existing foreground ephemeral URLSession transport.
protocol RawUploadTransportProtocol: Sendable {
    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse
}

final class URLSessionRawUploadTransport: RawUploadTransportProtocol, @unchecked Sendable {
    private let transport: any HTTPTransportProtocol

    init(transport: (any HTTPTransportProtocol)? = nil) {
        self.transport =
            transport
            ?? URLSessionHTTPTransport(
                requestTimeout: 30, resourceTimeout: 300,
                maxResponseBodyBytes: UploadPolicy.maximumResponseBytes)
    }

    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        guard request.url.scheme == "https", request.url.user == nil,
            request.url.password == nil, request.url.query == nil, request.url.fragment == nil,
            let components = URLComponents(url: request.url, resolvingAgainstBaseURL: false)
        else { throw UploadFailure.scopeMismatch }
        let segments = components.path.split(separator: "/").map(String.init)
        guard let apiIndex = segments.lastIndex(of: "api") else {
            throw UploadFailure.scopeMismatch
        }
        let route = Array(segments.dropFirst(apiIndex))
        let routeKind: UploadRouteKind
        if route == ["api", "v1", "upload-sessions"], request.method == .post {
            routeKind = .create
        } else if route.count == 4,
            Array(route.prefix(3)) == ["api", "v1", "upload-sessions"],
            UUID(uuidString: route[3])?.uuidString.lowercased() == route[3],
            request.method == .get || request.method == .patch
        {
            routeKind = .session
        } else if route.count == 5,
            Array(route.prefix(3)) == ["api", "v1", "upload-sessions"],
            UUID(uuidString: route[3])?.uuidString.lowercased() == route[3],
            ["complete", "abort"].contains(route[4]), request.method == .post
        {
            routeKind = .terminal
        } else {
            throw UploadFailure.scopeMismatch
        }
        let authorization = request.headers.first(where: {
            $0.key.lowercased() == "authorization"
        })?.value
        guard let authorization, authorization.hasPrefix("Bearer "),
            authorization.count > "Bearer ".count,
            !authorization.unicodeScalars.contains(where: { $0.value == 10 || $0.value == 13 }),
            request.headers.keys.allSatisfy({
                !["cookie", "cookie2", "x-csrf-token"].contains($0.lowercased())
            }),
            request.headers.first(where: { $0.key.lowercased() == "cache-control" })?.value
                .lowercased() == "no-store"
        else { throw UploadFailure.scopeMismatch }
        let bodyLimit: Int
        switch routeKind {
        case .create:
            bodyLimit = UploadPolicy.maximumJSONBytes
            guard
                request.headers.first(where: { $0.key.lowercased() == "content-type" })?.value
                    .components(separatedBy: ";").first?.trimmingCharacters(
                        in: .whitespacesAndNewlines
                    )
                    .lowercased() == "application/json"
            else { throw UploadFailure.invalidMetadata }
        case .session where request.method == .get:
            bodyLimit = 0
        case .session:
            bodyLimit = UploadPolicy.chunkBytes
            guard
                request.headers.first(where: { $0.key.lowercased() == "content-type" })?.value
                    .components(separatedBy: ";").first?.trimmingCharacters(
                        in: .whitespacesAndNewlines
                    )
                    .lowercased() == "application/octet-stream",
                let rawOffset = request.headers.first(where: {
                    $0.key.lowercased() == "upload-offset"
                })?.value,
                DownloadPolicy.decimal(rawOffset) != nil
            else { throw UploadFailure.invalidMetadata }
        case .terminal:
            bodyLimit = 0
        }
        switch request.method {
        case .get:
            guard request.body == nil else { throw UploadFailure.invalidMetadata }
        case .patch:
            guard let body = request.body, !body.isEmpty, body.count <= bodyLimit else {
                throw UploadFailure.chunkTooLarge
            }
        case .post where bodyLimit == 0:
            guard request.body == nil else { throw UploadFailure.invalidMetadata }
        case .post:
            guard let body = request.body, !body.isEmpty, body.count <= bodyLimit else {
                throw UploadFailure.payloadTooLarge
            }
        default:
            throw UploadFailure.invalidMetadata
        }
        do {
            return try await transport.send(request)
        } catch let failure as UploadFailure {
            throw failure
        } catch let failure as SynveilTransportError {
            throw UploadPolicy.classify(failure)
        } catch let error as URLError {
            throw UploadPolicy.classify(error)
        } catch is CancellationError {
            throw UploadFailure.cancelled
        } catch {
            throw UploadFailure.serverUnavailable
        }
    }
}

private enum UploadRouteKind {
    case create, session, terminal
}
