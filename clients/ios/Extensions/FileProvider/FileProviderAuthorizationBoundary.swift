import Foundation

enum FileProviderAuthorizationState: String, Codable, Sendable {
    case authorized
    case loggedOut
    case revoked
    case accountSwitched
}

/// Non-secret evidence a future app-mediated protocol may publish. P053 does not publish or read
/// this marker at runtime, and it never carries a DeviceBearer credential.
struct FileProviderAuthorizationEvidence: Codable, Equatable, Sendable {
    let schemaVersion: Int
    let domainIdentity: String
    let ownerScopeFingerprint: String
    let credentialGenerationFingerprint: String
    let state: FileProviderAuthorizationState
    let authorizationGeneration: UInt64

    init(
        domain: FileProviderDomainIdentity,
        state: FileProviderAuthorizationState,
        authorizationGeneration: UInt64
    ) {
        schemaVersion = 1
        domainIdentity = domain.rawValue
        ownerScopeFingerprint = domain.ownerScopeFingerprint
        credentialGenerationFingerprint = domain.credentialGenerationFingerprint
        self.state = state
        self.authorizationGeneration = authorizationGeneration
    }
}

enum FileProviderAuthorizationDenial: Equatable, Sendable {
    case missingEvidence
    case unsupportedSchema
    case wrongDomain
    case wrongOwnerScope
    case wrongCredentialGeneration
    case loggedOut
    case revoked
    case accountSwitched
    case invalidAuthorizationGeneration
    case protectedDataUnavailable
    case productionCapabilityDisabled
}

enum FileProviderAuthorizationDecision: Equatable, Sendable {
    case permittedForMetadataRead(generation: UInt64)
    case denied(FileProviderAuthorizationDenial)
}

struct FileProviderAuthorizationBoundary: Sendable {
    /// Existing credentials remain in the host app's default access group.
    static let sharedKeychainAccessGroup: String? = nil
    static let extensionMayReadDeviceBearer = false

    let domain: FileProviderDomainIdentity
    let container: FileProviderSharedContainer
    let capability: FileProviderCapability

    init(
        domain: FileProviderDomainIdentity,
        container: FileProviderSharedContainer = FileProviderSharedContainer(),
        capability: FileProviderCapability = .production
    ) {
        self.domain = domain
        self.container = container
        self.capability = capability
    }

    func validateAuthorization() throws -> FileProviderAuthorizationEvidence {
        let authURL = try container.url(for: ["metadata", domain.rawValue, "authorization.json"])
        guard let data = try? Data(contentsOf: authURL),
              let evidence = try? JSONDecoder().decode(FileProviderAuthorizationEvidence.self, from: data)
        else {
            throw FileProviderErrorMapper.unavailable()
        }

        let decision = Self.evaluate(
            evidence: evidence,
            expectedDomain: domain,
            protectedDataAvailable: true,
            capability: capability
        )

        switch decision {
        case .permittedForMetadataRead:
            return evidence
        case .denied:
            throw FileProviderErrorMapper.unavailable()
        }
    }

    static func evaluate(
        evidence: FileProviderAuthorizationEvidence?,
        expectedDomain: FileProviderDomainIdentity,
        protectedDataAvailable: Bool,
        capability: FileProviderCapability = .production
    ) -> FileProviderAuthorizationDecision {
        guard let evidence else { return .denied(.missingEvidence) }
        guard evidence.schemaVersion == 1 else { return .denied(.unsupportedSchema) }
        guard evidence.domainIdentity == expectedDomain.rawValue else {
            return .denied(.wrongDomain)
        }
        guard evidence.ownerScopeFingerprint == expectedDomain.ownerScopeFingerprint else {
            return .denied(.wrongOwnerScope)
        }
        guard
            evidence.credentialGenerationFingerprint
                == expectedDomain.credentialGenerationFingerprint
        else {
            return .denied(.wrongCredentialGeneration)
        }
        guard evidence.authorizationGeneration > 0 else {
            return .denied(.invalidAuthorizationGeneration)
        }
        guard protectedDataAvailable else { return .denied(.protectedDataUnavailable) }
        switch evidence.state {
        case .loggedOut: return .denied(.loggedOut)
        case .revoked: return .denied(.revoked)
        case .accountSwitched: return .denied(.accountSwitched)
        case .authorized: break
        }
        guard capability.metadataPublicationEnabled else {
            return .denied(.productionCapabilityDisabled)
        }
        return .permittedForMetadataRead(generation: evidence.authorizationGeneration)
    }
}
