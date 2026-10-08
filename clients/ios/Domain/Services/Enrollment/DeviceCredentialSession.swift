import Foundation

/// A validated device credential together with the canonical endpoint that issued it.
///
/// The endpoint is part of session identity: a credential loaded for one Synveil server must not be
/// sent to another server. This value contains the bearer in memory only; its description remains
/// redacted through `DeviceCredential`.
public struct DeviceCredentialSession: Sendable, Equatable {
    public let serverEndpoint: ServerEndpoint
    public let record: DeviceCredentialRecord

    public init(serverEndpoint: ServerEndpoint, record: DeviceCredentialRecord) {
        self.serverEndpoint = serverEndpoint
        self.record = record
    }
}
