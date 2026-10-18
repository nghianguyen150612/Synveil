import Foundation
import Network

public struct PhotoBackupNetworkState: Sendable, Equatable {
    public let isSatisfied: Bool
    public let isWiFi: Bool
    public let isCellular: Bool
    public let isConstrained: Bool
    public let isExpensive: Bool

    public init(
        isSatisfied: Bool,
        isWiFi: Bool,
        isCellular: Bool,
        isConstrained: Bool,
        isExpensive: Bool
    ) {
        self.isSatisfied = isSatisfied
        self.isWiFi = isWiFi
        self.isCellular = isCellular
        self.isConstrained = isConstrained
        self.isExpensive = isExpensive
    }

    public static let unavailable = PhotoBackupNetworkState(
        isSatisfied: false, isWiFi: false, isCellular: false,
        isConstrained: false, isExpensive: false)
}

public protocol PhotoBackupNetworkChecking: Sendable {
    func currentState() -> PhotoBackupNetworkState
}

/// Read-only path observation for foreground policy checks. It never schedules or owns uploads.
public final class PhotoBackupNetworkPathMonitor: PhotoBackupNetworkChecking, @unchecked Sendable {
    private let monitor: NWPathMonitor
    private let queue = DispatchQueue(label: "com.synveil.ios.photo-network", qos: .utility)
    private let lock = NSLock()
    private var latestState = PhotoBackupNetworkState.unavailable

    public init() {
        let monitor = NWPathMonitor()
        self.monitor = monitor
        monitor.pathUpdateHandler = { [weak self] path in
            guard let self else { return }
            let state = PhotoBackupNetworkState(
                isSatisfied: path.status == .satisfied,
                isWiFi: path.usesInterfaceType(.wifi),
                isCellular: path.usesInterfaceType(.cellular),
                isConstrained: path.isConstrained,
                isExpensive: path.isExpensive)
            self.lock.lock()
            self.latestState = state
            self.lock.unlock()
        }
        monitor.start(queue: queue)
    }

    deinit { monitor.cancel() }

    public func currentState() -> PhotoBackupNetworkState {
        lock.lock()
        defer { lock.unlock() }
        return latestState
    }
}
