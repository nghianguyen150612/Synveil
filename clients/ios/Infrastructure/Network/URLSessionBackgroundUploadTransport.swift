import Foundation

@MainActor
protocol BackgroundUploadTransportProtocol: AnyObject {
    func inventory() async -> [BackgroundSystemTask]
    func submit(_ request: HTTPTransportRequest, file: URL, intent: BackgroundChunkIntent) throws
        -> Int
    func resume(_ identifier: Int) async
    func cancel(_ identifier: Int) async
}

/// The OS can persist Authorization headers outside Keychain. Cancel is best effort and cannot
/// revoke a request already delivered to the server. No app-level PATCH retry is performed here.
final class URLSessionBackgroundUploadTransport: NSObject, URLSessionDataDelegate,
    URLSessionTaskDelegate, @unchecked Sendable
{
    static let identifier = "com.synveil.ios.upload.background.v1"
    private var session: URLSession!
    private var nonemptyBodies: Set<Int> = []  // serial delegate queue only

    override init() {
        super.init()
        let config = URLSessionConfiguration.background(withIdentifier: Self.identifier)
        config.sessionSendsLaunchEvents = true
        config.isDiscretionary = false
        config.waitsForConnectivity = true
        config.httpCookieStorage = nil
        config.httpShouldSetCookies = false
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        config.timeoutIntervalForRequest = 60
        config.timeoutIntervalForResource = 24 * 60 * 60
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        session = URLSession(configuration: config, delegate: self, delegateQueue: queue)
    }

    func urlSession(
        _ session: URLSession, task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) {
        // Reject all redirects, including cross-origin and HTTPS-to-HTTP redirects.
        completionHandler(nil)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        if !data.isEmpty { nonemptyBodies.insert(dataTask.taskIdentifier) }
        // PATCH evidence never needs a body; never buffer unbounded server data.
        if !data.isEmpty { dataTask.cancel() }
    }

    func urlSession(
        _ session: URLSession, task: URLSessionTask,
        didCompleteWithError error: Error?
    ) {
        let response = task.response as? HTTPURLResponse
        let original = task.originalRequest
        let validRequest =
            original?.httpMethod == "PATCH" && original?.url?.scheme == "https"
            && response?.url == original?.url
        func header(_ name: String, limit: Int) -> String? {
            guard let value = response?.value(forHTTPHeaderField: name), value.utf8.count <= limit
            else { return nil }
            return value
        }
        let evidence = BackgroundUploadEvidence(
            taskIdentifier: task.taskIdentifier,
            status: validRequest ? response?.statusCode : nil,
            offset: header("Upload-Offset", limit: 32),
            requestId: header("X-Request-Id", limit: 128),
            bodyWasEmpty: nonemptyBodies.remove(task.taskIdentifier) == nil,
            failure: error.map { UploadPolicy.classify($0).rawValue })
        let binding = Self.snapshot(task)
        DispatchQueue.main.async {
            BackgroundUploadEventDelivery.shared.receive(binding, evidence: evidence)
        }
    }

    private static func snapshot(_ task: URLSessionTask) -> BackgroundSystemTask {
        let request = task.originalRequest
        return BackgroundSystemTask(
            identifier: task.taskIdentifier, intentId: task.taskDescription,
            requestURL: request?.url, method: request?.httpMethod,
            offset: request?.value(forHTTPHeaderField: "Upload-Offset"),
            length: request?.value(forHTTPHeaderField: "Content-Length"))
    }

    func urlSessionDidFinishEvents(forBackgroundURLSession session: URLSession) {
        DispatchQueue.main.async { BackgroundUploadEventDelivery.shared.finishEvents() }
    }
}

extension URLSessionBackgroundUploadTransport: BackgroundUploadTransportProtocol {
    @MainActor
    func inventory() async -> [BackgroundSystemTask] {
        await session.allTasks.map { Self.snapshot($0) }
    }

    @MainActor
    func submit(_ request: HTTPTransportRequest, file: URL, intent: BackgroundChunkIntent) throws
        -> Int
    {
        guard request.method == .patch, request.body == nil, request.url.scheme == "https",
            request.url.user == nil, request.url.password == nil, request.url.query == nil,
            request.url.fragment == nil, file.isFileURL,
            request.headers["Upload-Offset"] == String(intent.offset),
            request.headers["Content-Length"] == String(intent.length),
            request.headers["Content-Type"] == "application/octet-stream",
            request.headers["Cache-Control"] == "no-store",
            let authorization = request.headers["Authorization"],
            authorization.hasPrefix("Bearer "),
            authorization.count > 7, !authorization.contains("\r"), !authorization.contains("\n"),
            !request.headers.keys.contains(where: {
                ["cookie", "cookie2", "x-csrf-token"].contains($0.lowercased())
            }),
            (1...UploadPolicy.chunkBytes).contains(intent.length)
        else { throw UploadFailure.scopeMismatch }
        let path = request.url.path.split(separator: "/").map(String.init)
        guard path.count >= 4, Array(path.suffix(4).prefix(3)) == ["api", "v1", "upload-sessions"],
            let id = path.last, UUID(uuidString: id)?.uuidString.lowercased() == id
        else { throw UploadFailure.scopeMismatch }
        var native = URLRequest(
            url: request.url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 60)
        native.httpMethod = "PATCH"
        native.allHTTPHeaderFields = request.headers
        let task = session.uploadTask(with: native, fromFile: file)
        task.taskDescription = intent.id  // opaque local intent UUID only
        // Created suspended. The coordinator must persist its binding before resume.
        return task.taskIdentifier
    }

    @MainActor
    func resume(_ identifier: Int) async {
        await session.allTasks.first(where: { $0.taskIdentifier == identifier })?.resume()
    }

    @MainActor
    func cancel(_ identifier: Int) async {
        await session.allTasks.first(where: { $0.taskIdentifier == identifier })?.cancel()
    }
}

/// Holds launch delivery until storage and credentials are initialized. An unsuccessful durable
/// write retains the event and prevents the lifecycle completion acknowledgement.
@MainActor
final class BackgroundUploadEventDelivery {
    static let shared = BackgroundUploadEventDelivery()
    var handler: ((BackgroundSystemTask, BackgroundUploadEvidence) async -> Bool)?
    private var pending: [(BackgroundSystemTask, BackgroundUploadEvidence)] = []
    private var draining = false
    private var processing: Task<Void, Never>?
    private var finished = false
    private var completion: (() -> Void)?

    func installCompletion(_ completion: @escaping () -> Void) {
        self.completion = completion
        acknowledgeIfReady()
    }

    func receive(_ task: BackgroundSystemTask, evidence: BackgroundUploadEvidence) {
        finished = false
        pending.append((task, evidence))
        processPendingEvents()
    }

    func finishEvents() {
        finished = true
        processPendingEvents()
        acknowledgeIfReady()
    }

    func processPendingEvents() {
        guard !draining, let handler, !pending.isEmpty else {
            acknowledgeIfReady()
            return
        }
        draining = true
        processing = Task { @MainActor in
            while let event = pending.first {
                guard await handler(event.0, event.1) else { break }
                pending.removeFirst()
            }
            draining = false
            acknowledgeIfReady()
        }
    }

    func waitForLocalPersistence() async { await processing?.value }

    private func acknowledgeIfReady() {
        guard finished, pending.isEmpty, !draining, let completion else { return }
        self.completion = nil
        finished = false
        completion()
    }
}
