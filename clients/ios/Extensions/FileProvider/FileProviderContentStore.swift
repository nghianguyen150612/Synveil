import CryptoKit
import Darwin
import Foundation

enum FileProviderContentStoreError: Error, Equatable {
    case missingEntitlement
    case containerUnavailable
    case unsafePath
    case symbolicLink
    case publicationFailed
    case storageQuotaExceeded
    case lengthMismatch
    case sha256Mismatch
    case manifestNotFound
    case contentObjectNotFound
    case corruptManifest
    case fileSystemError
    case cancelled
}

struct FileProviderContentStore: Sendable {
    let container: FileProviderSharedContainer
    private let fileManager: FileManager

    init(
        container: FileProviderSharedContainer = FileProviderSharedContainer(),
        fileManager: FileManager = .default
    ) {
        self.container = container
        self.fileManager = fileManager
    }

    /// Computes streaming SHA-256 and byte length of a file in 64 KiB chunks.
    static func verifyFileIntegrity(
        at url: URL,
        expectedLength: UInt64,
        expectedSHA256: String,
        checkCancellation: @Sendable () throws -> Void = {}
    ) throws {
        try checkCancellation()
        let handle: FileHandle
        do {
            handle = try FileHandle(forReadingFrom: url)
        } catch {
            throw FileProviderContentStoreError.fileSystemError
        }
        defer { try? handle.close() }

        var hasher = SHA256()
        var totalBytes: UInt64 = 0
        let chunkSize = 64 * 1024

        while true {
            try checkCancellation()
            let chunk: Data
            do {
                guard let data = try handle.read(upToCount: chunkSize), !data.isEmpty else {
                    break
                }
                chunk = data
            } catch {
                throw FileProviderContentStoreError.fileSystemError
            }
            totalBytes += UInt64(chunk.count)
            guard totalBytes <= expectedLength else {
                throw FileProviderContentStoreError.lengthMismatch
            }
            hasher.update(data: chunk)
        }

        guard totalBytes == expectedLength else {
            throw FileProviderContentStoreError.lengthMismatch
        }

        let digest = hasher.finalize()
        let hexSHA256 = "sha256:" + digest.map { String(format: "%02x", $0) }.joined()
        guard hexSHA256 == expectedSHA256 else {
            throw FileProviderContentStoreError.sha256Mismatch
        }
    }

    func manifestURL(domain: FileProviderDomainIdentity, fileVersionID: String) throws -> URL {
        guard FileProviderSharedContainer.isSafePathComponent(fileVersionID) else {
            throw FileProviderContentStoreError.unsafePath
        }
        let relative = ["content", domain.rawValue, "manifests", "\(fileVersionID).json"]
        return try container.url(for: relative)
    }

    func objectURL(domain: FileProviderDomainIdentity, storageKey: String) throws -> URL {
        guard FileProviderSharedContainer.isSafePathComponent(storageKey) else {
            throw FileProviderContentStoreError.unsafePath
        }
        return try container.url(for: ["content", domain.rawValue, "objects", storageKey])
    }

    func publish(
        sourceURL: URL,
        manifest: FileProviderContentManifest,
        expectedDomain: FileProviderDomainIdentity,
        expectedOwnerScopeFingerprint: String,
        capability: FileProviderCapability = .production,
        checkCancellation: @Sendable () throws -> Void = {}
    ) throws -> URL {
        guard capability.metadataPublicationEnabled else {
            throw FileProviderContentStoreError.publicationFailed
        }

        let validatedManifest = try manifest.validate(
            expectedDomain: expectedDomain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint
        )

        // Reject an impossible declared allocation before streaming or converting UInt64 to Int.
        // Keep the quota check below as a second, live check against existing shared storage.
        guard validatedManifest.expectedByteLength
            <= UInt64(FileProviderSharedContainer.maximumProviderStorageBytes)
        else {
            throw FileProviderContentStoreError.storageQuotaExceeded
        }

        let manifestData = try validatedManifest.encoded(
            expectedDomain: expectedDomain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint
        )
        let requiredBytes = Int(validatedManifest.expectedByteLength) + manifestData.count
        do {
            try container.checkGenerationQuota(addingBytes: requiredBytes)
        } catch FileProviderSharedContainerError.storageQuotaExceeded {
            throw FileProviderContentStoreError.storageQuotaExceeded
        }

        // Verify source file integrity; a quota-pass never substitutes for exact byte validation.
        try Self.verifyFileIntegrity(
            at: sourceURL,
            expectedLength: validatedManifest.expectedByteLength,
            expectedSHA256: validatedManifest.expectedSHA256,
            checkCancellation: checkCancellation
        )

        let targetObject = try objectURL(
            domain: expectedDomain,
            storageKey: validatedManifest.storageKey
        )
        let targetManifest = try manifestURL(
            domain: expectedDomain,
            fileVersionID: validatedManifest.fileVersionID
        )

        // Ensure directories exist
        let stagingDir = try createDirectoryIfNeeded(
            relativeComponents: ["content", expectedDomain.rawValue, "staging"]
        )
        _ = try createDirectoryIfNeeded(
            relativeComponents: ["content", expectedDomain.rawValue, "objects"]
        )
        _ = try createDirectoryIfNeeded(
            relativeComponents: ["content", expectedDomain.rawValue, "manifests"]
        )

        let tempObjectID = UUID().uuidString.lowercased()
        let tempObjectURL = stagingDir.appendingPathComponent(".obj-\(tempObjectID).tmp")
        let tempManifestURL = stagingDir.appendingPathComponent(".man-\(tempObjectID).tmp")

        defer {
            try? fileManager.removeItem(at: tempObjectURL)
            try? fileManager.removeItem(at: tempManifestURL)
        }

        try checkCancellation()

        // Copy content file to staging
        do {
            if fileManager.fileExists(atPath: tempObjectURL.path) {
                try fileManager.removeItem(at: tempObjectURL)
            }
            try fileManager.copyItem(at: sourceURL, to: tempObjectURL)
            try fileManager.setAttributes(
                [
                    .protectionKey: FileProtectionType.completeUntilFirstUserAuthentication,
                    .posixPermissions: 0o600,
                ],
                ofItemAtPath: tempObjectURL.path
            )
        } catch {
            throw FileProviderContentStoreError.publicationFailed
        }

        // Re-verify staged object integrity
        try Self.verifyFileIntegrity(
            at: tempObjectURL,
            expectedLength: validatedManifest.expectedByteLength,
            expectedSHA256: validatedManifest.expectedSHA256,
            checkCancellation: checkCancellation
        )

        // Write staged manifest
        let manifestCreated = fileManager.createFile(
            atPath: tempManifestURL.path,
            contents: manifestData,
            attributes: [
                .protectionKey: FileProtectionType.completeUntilFirstUserAuthentication,
                .posixPermissions: 0o600,
            ]
        )
        guard manifestCreated else {
            throw FileProviderContentStoreError.publicationFailed
        }

        try checkCancellation()

        // Atomic placement
        do {
            if fileManager.fileExists(atPath: targetObject.path) {
                _ = try fileManager.replaceItemAt(
                    targetObject,
                    withItemAt: tempObjectURL,
                    backupItemName: nil,
                    options: []
                )
            } else {
                try fileManager.moveItem(at: tempObjectURL, to: targetObject)
            }

            if fileManager.fileExists(atPath: targetManifest.path) {
                _ = try fileManager.replaceItemAt(
                    targetManifest,
                    withItemAt: tempManifestURL,
                    backupItemName: nil,
                    options: []
                )
            } else {
                try fileManager.moveItem(at: tempManifestURL, to: targetManifest)
            }
        } catch {
            throw FileProviderContentStoreError.publicationFailed
        }

        return targetObject
    }

    func readManifest(
        domain: FileProviderDomainIdentity,
        fileVersionID: String,
        expectedOwnerScopeFingerprint: String
    ) throws -> FileProviderContentManifest {
        let url = try manifestURL(domain: domain, fileVersionID: fileVersionID)
        guard fileManager.fileExists(atPath: url.path) else {
            throw FileProviderContentStoreError.manifestNotFound
        }
        let data: Data
        do {
            data = try Data(contentsOf: url, options: [.mappedIfSafe])
        } catch {
            throw FileProviderContentStoreError.corruptManifest
        }
        return try FileProviderContentManifest.decode(
            data,
            expectedDomain: domain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint
        )
    }

    func verifyAndAccessContent(
        domain: FileProviderDomainIdentity,
        fileVersionID: String,
        expectedOwnerScopeFingerprint: String,
        expectedAuthorizationGeneration: UInt64,
        checkCancellation: @Sendable () throws -> Void = {}
    ) throws -> (manifest: FileProviderContentManifest, contentURL: URL) {
        try checkCancellation()
        let manifest = try readManifest(
            domain: domain,
            fileVersionID: fileVersionID,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint
        )

        guard manifest.authorizationGeneration == expectedAuthorizationGeneration else {
            throw FileProviderContentStoreError.corruptManifest
        }

        let contentURL = try objectURL(domain: domain, storageKey: manifest.storageKey)
        guard fileManager.fileExists(atPath: contentURL.path) else {
            throw FileProviderContentStoreError.contentObjectNotFound
        }

        try Self.verifyFileIntegrity(
            at: contentURL,
            expectedLength: manifest.expectedByteLength,
            expectedSHA256: manifest.expectedSHA256,
            checkCancellation: checkCancellation
        )

        return (manifest, contentURL)
    }

    func removeDomainContent(domain: FileProviderDomainIdentity) throws {
        let root = try container.url(for: ["content", domain.rawValue])
        if fileManager.fileExists(atPath: root.path) {
            try? fileManager.removeItem(at: root)
        }
    }

    private func createDirectoryIfNeeded(relativeComponents: [String]) throws -> URL {
        let url = try container.url(for: relativeComponents)
        if !fileManager.fileExists(atPath: url.path) {
            try fileManager.createDirectory(
                at: url,
                withIntermediateDirectories: true,
                attributes: [
                    .protectionKey: FileProtectionType.completeUntilFirstUserAuthentication,
                    .posixPermissions: 0o700,
                ]
            )
        }
        return url
    }
}
