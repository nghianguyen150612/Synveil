import Foundation

struct FileProviderVerifiedItemRecord: Sendable, Equatable {
    let domain: FileProviderDomainIdentity
    let evidence: FileProviderAuthorizationEvidence
    let libraryID: String
    let nodeID: String
    let fileVersionID: String
    let expectedByteLength: UInt64
    let expectedSHA256: String
    let fileURL: URL
}

@MainActor
protocol FileProviderVerifiedContentSource {
    func inspectVerifiedContent(
        libraryID: String,
        nodeID: String,
        fileVersionID: String
    ) async throws -> FileProviderVerifiedItemRecord?
}
