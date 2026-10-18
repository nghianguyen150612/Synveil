import Foundation

struct SessionAuthorizationIdentity: Equatable, Sendable {
    let endpoint: String, owner: String, device: String, credential: String
    init(_ session: DeviceCredentialSession) {
        endpoint = session.serverEndpoint.urlString
        owner = session.record.ownerUserId
        device = session.record.deviceId
        credential = session.record.credentialId
    }
}

struct SessionAuthorizationCheckLease: Sendable {
    let identity: SessionAuthorizationIdentity
    let id: String
    let requiresVerification: Bool
}

enum SessionAuthorizationCheckOutcome: Equatable, Sendable {
    case authorized, transientFailure, rejected, verificationRequired
    case authorizedCompleteLibraryCatalog(Set<String>)
}

protocol SessionAuthorizationEvidenceProtocol: Sendable {
    func begin(_ session: DeviceCredentialSession) async throws -> SessionAuthorizationCheckLease
    func finish(_ lease: SessionAuthorizationCheckLease, outcome: SessionAuthorizationCheckOutcome)
        async throws
}

/// A verification GET cannot precede its durable uncertainty marker. Missing/untrusted storage
/// stops restoration verification; no network result can be lost behind a previous cache grant.
struct SQLiteSessionAuthorizationEvidence: SessionAuthorizationEvidenceProtocol {
    let database: MutationQueueSQLiteStore?
    func begin(_ session: DeviceCredentialSession) async throws -> SessionAuthorizationCheckLease {
        guard let database else { throw MutationQueueFailure.unavailable }
        return try await database.beginSessionAuthorizationCheck(
            SessionAuthorizationIdentity(session))
    }
    func finish(_ lease: SessionAuthorizationCheckLease, outcome: SessionAuthorizationCheckOutcome)
        async throws
    {
        guard let database else { throw MutationQueueFailure.unavailable }
        try await database.finishSessionAuthorizationCheck(lease, outcome: outcome)
    }
}
