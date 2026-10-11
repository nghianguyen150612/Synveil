import Foundation
import UniformTypeIdentifiers

enum VerifiedFileExportFailure: Sendable, Equatable {
    case missing, integrity, storage, unsupportedPreview, session, preparation
}

struct VerifiedFileExportPreparation: Sendable {
    let lease: VerifiedFileExportLease?
    let failure: VerifiedFileExportFailure?
}

/// Owns one-use presentation leases. The only URL returned by this boundary is the verified
/// temporary copy, and it is returned only for the matching native presentation purpose.
@MainActor
final class VerifiedFileExportCoordinator {
    private struct Entry {
        let lease: VerifiedFileExportLease
        let capability: VerifiedDownloadedFile
        let displayName: String
        var state: VerifiedFileExportState
    }

    private let downloads: FileDownloadCoordinator
    private var entries: [String: Entry] = [:]
    private var generation: UInt64 = 0

    init(downloads: FileDownloadCoordinator) { self.downloads = downloads }

    func prepare(
        library: LibraryId, node: NodeId, version: FileVersionId,
        purpose: VerifiedFileExportPurpose, displayName: String,
        status: @MainActor (VerifiedFileExportState) -> Void = { _ in }
    ) async -> VerifiedFileExportPreparation {
        let captured = generation
        status(.preparing)
        do {
            let capability = try await downloads.verifiedFile(
                library: library, node: node, version: version)
            guard captured == generation else { throw DownloadFailure.staleSession }
            status(.verifyingCopy)
            let id = UUID().uuidString.lowercased()
            let url = try await downloads.makeExportCopy(
                capability, exportId: id, displayName: displayName)
            if purpose == .preview, !Self.supportsPreview(url: url, displayName: displayName) {
                do {
                    try await downloads.releaseExport(id)
                } catch {
                    status(.failed)
                    return VerifiedFileExportPreparation(lease: nil, failure: .storage)
                }
                status(.failed)
                return VerifiedFileExportPreparation(lease: nil, failure: .unsupportedPreview)
            }
            guard captured == generation else {
                try? await downloads.releaseExport(id)
                throw DownloadFailure.staleSession
            }
            let lease = VerifiedFileExportLease(
                id: id, purpose: purpose, sourceOperationId: capability.operationId,
                versionId: capability.identity.versionId,
                sha256: capability.identity.expected.sha256,
                byteLength: capability.identity.expected.byteLength,
                lifecycleRevision: capability.session.lifecycleRevision)
            entries[id] = Entry(
                lease: lease, capability: capability, displayName: displayName,
                state: .ready)
            status(.ready)
            return VerifiedFileExportPreparation(lease: lease, failure: nil)
        } catch {
            status(.failed)
            return VerifiedFileExportPreparation(lease: nil, failure: Self.failure(error))
        }
    }

    /// A lease can authorize exactly one handoff. SwiftUI receives the URL only at this point.
    func beginPresentation(_ lease: VerifiedFileExportLease) async throws -> URL {
        guard var entry = entries[lease.id], entry.lease == lease, entry.state == .ready
        else { throw DownloadFailure.staleSession }
        // Consume the lease before the first suspension point. Otherwise two callers can both
        // observe READY while the download coordinator revalidates the same temporary copy.
        entry.state = .presented
        entries[lease.id] = entry
        do {
            let validatedURL = try await downloads.validateExportCopy(
                entry.capability, exportId: lease.id, displayName: entry.displayName)
            guard let current = entries[lease.id], current.lease == lease,
                current.state == .presented
            else { throw DownloadFailure.staleSession }
            return validatedURL
        } catch {
            if var current = entries[lease.id], current.lease == lease,
                current.state == .presented
            {
                current.state = .failed
                entries[lease.id] = current
            }
            throw error
        }
    }

    func finish(_ lease: VerifiedFileExportLease) async -> Bool {
        guard let entry = entries[lease.id], entry.lease == lease else { return true }
        entries[lease.id]?.state = .revoking
        do {
            try await downloads.releaseExport(lease.id)
            entries[lease.id] = nil
            return true
        } catch {
            entries[lease.id]?.state = .failed
            return false
        }
    }

    func invalidateSession() {
        generation &+= 1
        let ids = Array(entries.keys)
        entries.removeAll()
        Task { for id in ids { try? await downloads.releaseExport(id) } }
    }

    static func supportsPreview(url: URL, displayName: String) -> Bool {
        let safe = SafeExportFilename.make(displayName)
        let type = UTType(filenameExtension: URL(fileURLWithPath: safe).pathExtension)
        guard let type else { return false }
        guard type.conforms(to: .pdf) || type.conforms(to: .image) || type.conforms(to: .plainText),
            let handle = try? FileHandle(forReadingFrom: url)
        else { return false }
        defer { try? handle.close() }
        guard let header = try? handle.read(upToCount: 512) else { return false }
        if type.conforms(to: .pdf) { return header.starts(with: Data("%PDF-".utf8)) }
        if type.conforms(to: .image) {
            return header.starts(with: Data([0xFF, 0xD8, 0xFF]))
                || header.starts(with: Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]))
        }
        guard
            !header.contains(0),
            let prefix = String(data: header, encoding: .utf8)
        else { return false }
        let trimmed = prefix.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !trimmed.hasPrefix("<!doctype html"), !trimmed.hasPrefix("<html"),
            !trimmed.hasPrefix("<script"), !trimmed.hasPrefix("<?xml")
        else { return false }
        return true
    }

    private static func failure(_ error: Error) -> VerifiedFileExportFailure {
        switch DownloadPolicy.classify(error) {
        case .stagingMissing: .missing
        case .sha256Mismatch, .integrityFailure: .integrity
        case .insufficientStorage, .stagingQuotaExceeded, .filesystemFailure, .storageFailure:
            .storage
        case .staleSession, .scopeMismatch, .unauthenticated, .deviceRevoked: .session
        default: .preparation
        }
    }
}

enum SafeExportFilename {
    static func make(_ logicalName: String) -> String {
        let separators: Set<UInt32> = [0x2215, 0x2044, 0xFF0F, 0x29F8, 0xFF3C, 0x2216]
        var scalars = String.UnicodeScalarView()
        for scalar in logicalName.unicodeScalars {
            if scalar.value < 0x20 || scalar.value == 0x7F || scalar.value == 0
                || separators.contains(scalar.value)
            {
                scalars.append("_".unicodeScalars.first!)
            } else if scalar == "/" || scalar == "\\" {
                scalars.append("_".unicodeScalars.first!)
            } else {
                scalars.append(scalar)
            }
        }
        var candidate = String(scalars).precomposedStringWithCanonicalMapping
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if candidate.hasPrefix("."), candidate.dropFirst().contains(".") == false,
            candidate.count > 1
        {
            candidate = "Synveil Download\(candidate)"
        }
        while candidate.hasPrefix(".") { candidate.removeFirst() }
        if candidate.isEmpty || candidate == "." || candidate == ".." {
            candidate = "Synveil Download"
        }
        if candidate.utf8.count > 180 {
            var ext = (candidate as NSString).pathExtension
            while ext.utf8.count > 32 { ext.removeLast() }
            var stem = (candidate as NSString).deletingPathExtension
            let limit = ext.isEmpty ? 180 : 179 - ext.utf8.count
            while stem.utf8.count > limit { stem.removeLast() }
            candidate = ext.isEmpty ? stem : "\(stem).\(ext)"
        }
        if candidate.hasPrefix(".") || candidate == "~" { candidate = "Synveil Download" }
        return candidate
    }
}
