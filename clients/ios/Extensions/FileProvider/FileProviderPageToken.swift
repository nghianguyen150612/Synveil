import FileProvider
import Foundation

/// Opaque, versioned page token encoding container scope, projection generation, cursor offset,
/// and page size limit.
struct FileProviderPageToken: Codable, Equatable, Sendable {
    static let currentVersion = 1
    static let defaultPageSize = 100

    let version: Int
    let domainFingerprint: String
    let containerIdentifier: String
    let projectionGeneration: UInt64
    let offset: Int
    let limit: Int

    init(
        domain: FileProviderDomainIdentity,
        containerIdentifier: String,
        projectionGeneration: UInt64,
        offset: Int,
        limit: Int = Self.defaultPageSize
    ) {
        self.version = Self.currentVersion
        self.domainFingerprint = domain.fingerprint
        self.containerIdentifier = containerIdentifier
        self.projectionGeneration = projectionGeneration
        self.offset = offset
        self.limit = limit
    }

    func toNSFileProviderPage() throws -> NSFileProviderPage {
        let encoder = JSONEncoder()
        let data = try encoder.encode(self)
        return NSFileProviderPage(data)
    }

    static func fromNSFileProviderPage(
        _ page: NSFileProviderPage,
        expectedDomain: FileProviderDomainIdentity,
        expectedContainerIdentifier: String,
        currentGeneration: UInt64
    ) throws -> FileProviderPageToken {
        guard !page.rawValue.isEmpty else {
            throw FileProviderErrorMapper.unsupported()
        }
        let decoder = JSONDecoder()
        let token: FileProviderPageToken
        do {
            token = try decoder.decode(FileProviderPageToken.self, from: page.rawValue)
        } catch {
            throw FileProviderErrorMapper.unsupported()
        }

        guard token.version == Self.currentVersion,
            token.domainFingerprint == expectedDomain.fingerprint,
            token.containerIdentifier == expectedContainerIdentifier
        else {
            throw FileProviderErrorMapper.unsupported()
        }

        guard token.projectionGeneration == currentGeneration else {
            throw NSError(
                domain: NSFileProviderErrorDomain,
                code: NSFileProviderError.pageExpired.rawValue
            )
        }

        return token
    }
}
