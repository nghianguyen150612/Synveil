import Foundation

/// Runtime authority switches. P053 ships every File Provider capability closed.
struct FileProviderCapability: Equatable, Sendable {
    let domainRegistrationEnabled: Bool
    let metadataPublicationEnabled: Bool
    let authenticatedNetworkEnabled: Bool
    let mutationEnabled: Bool

    static let production = FileProviderCapability(
        domainRegistrationEnabled: false,
        metadataPublicationEnabled: false,
        authenticatedNetworkEnabled: false,
        mutationEnabled: false
    )

    var permitsAnyProductionOperation: Bool {
        domainRegistrationEnabled || metadataPublicationEnabled || authenticatedNetworkEnabled
            || mutationEnabled
    }
}

enum FileProviderOperation: String, CaseIterable, Sendable {
    case enumerate
    case fetchContents
    case create
    case modify
    case delete
}

enum FileProviderOperationDisposition: Equatable, Sendable {
    case authorizationUnavailable
    case unsupported
}

enum FileProviderOperationPolicy {
    static func disposition(
        for operation: FileProviderOperation,
        capability: FileProviderCapability = .production
    ) -> FileProviderOperationDisposition {
        switch operation {
        case .enumerate, .fetchContents:
            guard capability.authenticatedNetworkEnabled && capability.metadataPublicationEnabled
            else { return .authorizationUnavailable }
            return .unsupported
        case .create, .modify, .delete:
            return .unsupported
        }
    }
}

enum FileProviderDomainManagementError: Error, Equatable {
    case registrationDisabled
    case domainAlreadyRegistered
    case domainNotRegistered
    case managementUnavailable
}

enum FileProviderDomainOperationClassification: Equatable {
    case disabled
    case alreadyRegistered
    case notRegistered
    case unavailable
    case unknown
}

/// Framework-facing operations are injected so registration policy can be tested without creating
/// or deleting user-visible domains. P053 deliberately provides no live manager adapter.
protocol FileProviderDomainManaging: Sendable {
    func registeredDomainIdentifiers() async throws -> Set<String>
    func addDomain(_ identity: FileProviderDomainIdentity) async throws
    func disconnectDomain(identifier: String) async throws
    func removeDomain(identifier: String) async throws
}

struct FileProviderDomainRegistrationCoordinator: Sendable {
    let capability: FileProviderCapability
    let manager: any FileProviderDomainManaging

    init(
        capability: FileProviderCapability = .production,
        manager: any FileProviderDomainManaging
    ) {
        self.capability = capability
        self.manager = manager
    }

    func inspectExistingDomains() async throws -> Set<String> {
        try await manager.registeredDomainIdentifiers()
    }

    func register(_ identity: FileProviderDomainIdentity) async throws {
        guard capability.domainRegistrationEnabled else {
            throw FileProviderDomainManagementError.registrationDisabled
        }
        let existing = try await manager.registeredDomainIdentifiers()
        guard !existing.contains(identity.rawValue) else {
            throw FileProviderDomainManagementError.domainAlreadyRegistered
        }
        try await manager.addDomain(identity)
    }

    func disconnect(identifier: String) async throws {
        guard capability.domainRegistrationEnabled else {
            throw FileProviderDomainManagementError.registrationDisabled
        }
        let existing = try await manager.registeredDomainIdentifiers()
        guard existing.contains(identifier) else {
            throw FileProviderDomainManagementError.domainNotRegistered
        }
        try await manager.disconnectDomain(identifier: identifier)
    }

    func remove(identifier: String) async throws {
        guard capability.domainRegistrationEnabled else {
            throw FileProviderDomainManagementError.registrationDisabled
        }
        let existing = try await manager.registeredDomainIdentifiers()
        guard existing.contains(identifier) else {
            throw FileProviderDomainManagementError.domainNotRegistered
        }
        try await manager.removeDomain(identifier: identifier)
    }

    static func classify(_ error: Error) -> FileProviderDomainOperationClassification {
        guard let error = error as? FileProviderDomainManagementError else { return .unknown }
        switch error {
        case .registrationDisabled: return .disabled
        case .domainAlreadyRegistered: return .alreadyRegistered
        case .domainNotRegistered: return .notRegistered
        case .managementUnavailable: return .unavailable
        }
    }
}
