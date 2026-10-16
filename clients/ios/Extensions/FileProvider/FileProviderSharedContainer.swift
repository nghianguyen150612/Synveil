import Darwin
import Foundation

enum FileProviderSharedContainerError: Error, Equatable {
    case missingEntitlement
    case containerUnavailable
    case unsafePath
    case symbolicLink
    case notDirectory
    case snapshotTooLarge
    case storageQuotaExceeded
    case publicationFailed
}

/// Resolves only the explicitly entitled App Group and a fixed versioned File Provider subtree.
/// It never falls back to the app sandbox or the existing app-only SQLite database.
struct FileProviderSharedContainer {
    static let appGroupIdentifier = "group.com.synveil.ios"
    static let maximumSnapshotBytes = 8 * 1024 * 1024
    static let maximumProviderStorageBytes = 64 * 1024 * 1024
    private static let versionedRoot = ["Synveil", "FileProvider", "v1"]

    private let resolveContainerURL: @Sendable (String) -> URL?
    private let fileManager: FileManager
    private let beforeAtomicReplace: @Sendable () throws -> Void

    init(
        fileManager: FileManager = .default,
        resolveContainerURL: @escaping @Sendable (String) -> URL? = { identifier in
            FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: identifier)
        },
        beforeAtomicReplace: @escaping @Sendable () throws -> Void = {}
    ) {
        self.fileManager = fileManager
        self.resolveContainerURL = resolveContainerURL
        self.beforeAtomicReplace = beforeAtomicReplace
    }

    func url(for relativeComponents: [String]) throws -> URL {
        let root = try authorizedRoot()
        var result = root
        for component in Self.versionedRoot + relativeComponents {
            guard Self.isSafePathComponent(component) else {
                throw FileProviderSharedContainerError.unsafePath
            }
            result.appendPathComponent(component, isDirectory: false)
            try rejectSymbolicLink(at: result)
        }
        guard result.standardizedFileURL.path.hasPrefix(root.path + "/") else {
            throw FileProviderSharedContainerError.unsafePath
        }
        return result
    }

    func snapshotURL(
        domain: FileProviderDomainIdentity, libraryID: String
    ) throws -> URL {
        guard !libraryID.isEmpty, libraryID.utf8.count <= 256 else {
            throw FileProviderSharedContainerError.unsafePath
        }
        let libraryScope = FileProviderDomainIdentity.fingerprint(["synveil-library-v1", libraryID])
        return try url(for: ["metadata", domain.rawValue, "\(libraryScope).json"])
    }

    func publishSnapshot(
        _ snapshot: FileProviderMetadataSnapshot,
        expectedDomain: FileProviderDomainIdentity,
        expectedOwnerScopeFingerprint: String,
        capability: FileProviderCapability = .production
    ) throws {
        guard capability.metadataPublicationEnabled else {
            throw FileProviderSharedContainerError.publicationFailed
        }
        let validated = try snapshot.validated(
            expectedDomain: expectedDomain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint)
        let data = try validated.encoded(
            expectedDomain: expectedDomain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint)
        guard data.count <= Self.maximumSnapshotBytes else {
            throw FileProviderSharedContainerError.snapshotTooLarge
        }
        let destination = try snapshotURL(domain: expectedDomain, libraryID: validated.libraryID)
        let root = try authorizedRoot()
        let providerRoot = try directoryURL(root: root, components: Self.versionedRoot)
        let metadataDirectory = try directoryURL(
            root: root,
            components: Self.versionedRoot + ["metadata", expectedDomain.rawValue]
        )
        try enforceStorageQuota(providerRoot: providerRoot, addingBytes: data.count)

        let staging = metadataDirectory.appendingPathComponent(".snapshot-\(UUID().uuidString).tmp")
        do {
            let attributes: [FileAttributeKey: Any] = [
                .protectionKey: FileProtectionType.completeUntilFirstUserAuthentication,
                .posixPermissions: 0o600,
            ]
            guard fileManager.createFile(
                atPath: staging.path, contents: nil, attributes: attributes
            ) else {
                throw FileProviderSharedContainerError.publicationFailed
            }
            let handle = try FileHandle(forWritingTo: staging)
            do {
                try handle.write(contentsOf: data)
                try handle.synchronize()
                try handle.close()
            } catch {
                try? handle.close()
                throw error
            }
            try rejectSymbolicLink(at: destination)
            try beforeAtomicReplace()
            if fileManager.fileExists(atPath: destination.path) {
                _ = try fileManager.replaceItemAt(
                    destination, withItemAt: staging, backupItemName: nil, options: [])
            } else {
                try fileManager.moveItem(at: staging, to: destination)
            }
        } catch let error as FileProviderSharedContainerError {
            try? fileManager.removeItem(at: staging)
            throw error
        } catch {
            try? fileManager.removeItem(at: staging)
            throw FileProviderSharedContainerError.publicationFailed
        }
    }

    private func authorizedRoot() throws -> URL {
        guard let url = resolveContainerURL(Self.appGroupIdentifier) else {
            throw FileProviderSharedContainerError.missingEntitlement
        }
        guard url.isFileURL else { throw FileProviderSharedContainerError.containerUnavailable }
        let root = url.standardizedFileURL
        try rejectSymbolicLink(at: root)
        guard fileManager.fileExists(atPath: root.path) else {
            throw FileProviderSharedContainerError.containerUnavailable
        }
        let attributes = try fileManager.attributesOfItem(atPath: root.path)
        guard attributes[.type] as? FileAttributeType == .typeDirectory else {
            throw FileProviderSharedContainerError.notDirectory
        }
        return root
    }

    private func directoryURL(root: URL, components: [String]) throws -> URL {
        var current = root
        for component in components {
            guard Self.isSafePathComponent(component) else {
                throw FileProviderSharedContainerError.unsafePath
            }
            current.appendPathComponent(component, isDirectory: true)
            try rejectSymbolicLink(at: current)
            if fileManager.fileExists(atPath: current.path) {
                let attributes = try fileManager.attributesOfItem(atPath: current.path)
                guard attributes[.type] as? FileAttributeType == .typeDirectory else {
                    throw FileProviderSharedContainerError.notDirectory
                }
            } else {
                try fileManager.createDirectory(
                    at: current,
                    withIntermediateDirectories: false,
                    attributes: [
                        .protectionKey: FileProtectionType.completeUntilFirstUserAuthentication,
                        .posixPermissions: 0o700,
                    ]
                )
            }
            try fileManager.setAttributes(
                [
                    .protectionKey: FileProtectionType.completeUntilFirstUserAuthentication,
                    .posixPermissions: 0o700,
                ],
                ofItemAtPath: current.path
            )
            var values = URLResourceValues()
            values.isExcludedFromBackup = true
            try current.setResourceValues(values)
        }
        return current
    }

    private func enforceStorageQuota(providerRoot: URL, addingBytes: Int) throws {
        guard fileManager.fileExists(atPath: providerRoot.path) else { return }
        let keys: [URLResourceKey] = [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey]
        guard let enumerator = fileManager.enumerator(
            at: providerRoot,
            includingPropertiesForKeys: keys,
            options: [],
            errorHandler: { _, _ in false }
        )
        else { throw FileProviderSharedContainerError.containerUnavailable }
        var total = 0
        var count = 0
        for case let fileURL as URL in enumerator {
            count += 1
            guard count <= 8_192 else {
                throw FileProviderSharedContainerError.storageQuotaExceeded
            }
            let values = try fileURL.resourceValues(forKeys: Set(keys))
            guard values.isSymbolicLink != true else {
                throw FileProviderSharedContainerError.symbolicLink
            }
            if values.isRegularFile == true {
                total += values.fileSize ?? 0
                guard total <= Self.maximumProviderStorageBytes else {
                    throw FileProviderSharedContainerError.storageQuotaExceeded
                }
            }
        }
        guard addingBytes <= Self.maximumProviderStorageBytes - total else {
            throw FileProviderSharedContainerError.storageQuotaExceeded
        }
    }

    private func rejectSymbolicLink(at url: URL) throws {
        var info = stat()
        if lstat(url.path, &info) == 0 {
            if (info.st_mode & S_IFMT) == S_IFLNK {
                throw FileProviderSharedContainerError.symbolicLink
            }
        } else if errno != ENOENT && errno != ENOTDIR {
            throw FileProviderSharedContainerError.containerUnavailable
        }
    }

    static func isSafePathComponent(_ component: String) -> Bool {
        !component.isEmpty && component != "." && component != ".."
            && component.utf8.count <= 255
            && !component.contains("/") && !component.contains("\\")
            && !component.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains)
    }
}
