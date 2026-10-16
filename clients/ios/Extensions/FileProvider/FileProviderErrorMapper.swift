import FileProvider
import Foundation

enum FileProviderErrorMapper {
    static func error(for disposition: FileProviderOperationDisposition) -> NSError {
        let code: NSFileProviderError.Code
        switch disposition {
        case .authorizationUnavailable:
            code = .notAuthenticated
        case .unsupported:
            code = .cannotSynchronize
        }
        return NSError(domain: NSFileProviderErrorDomain, code: code.rawValue)
    }

    static func map(_ error: Error) -> NSError {
        if let contentError = error as? FileProviderContentStoreError {
            switch contentError {
            case .manifestNotFound, .contentObjectNotFound:
                return NSError(domain: NSFileProviderErrorDomain, code: NSFileProviderError.noSuchItem.rawValue)
            case .lengthMismatch, .sha256Mismatch, .corruptManifest, .publicationFailed:
                return NSError(domain: NSFileProviderErrorDomain, code: NSFileProviderError.cannotSynchronize.rawValue)
            case .missingEntitlement, .containerUnavailable, .fileSystemError, .symbolicLink, .unsafePath:
                return NSError(domain: NSFileProviderErrorDomain, code: NSFileProviderError.serverUnreachable.rawValue)
            case .storageQuotaExceeded:
                return NSError(domain: NSFileProviderErrorDomain, code: NSFileProviderError.insufficientQuota.rawValue)
            case .cancelled:
                return NSError(domain: NSCocoaErrorDomain, code: NSUserCancelledError)
            }
        }
        guard let failure = error as? FileProviderSynchronizationError else {
            if error is CancellationError {
                return NSError(domain: NSCocoaErrorDomain, code: NSUserCancelledError)
            }
            if error as? FileProviderSharedContainerError == .missingEntitlement {
                return NSError(
                    domain: NSFileProviderErrorDomain,
                    code: NSFileProviderError.serverUnreachable.rawValue)
            }
            return unsupported()
        }
        let code: NSFileProviderError.Code
        switch failure {
        case .unauthorized: code = .notAuthenticated
        case .noSuchItem: code = .noSuchItem
        case .pageExpired: code = .pageExpired
        case .anchorExpired: code = .syncAnchorExpired
        case .cancelled: return NSError(domain: NSCocoaErrorDomain, code: NSUserCancelledError)
        case .unavailable, .busy: code = .serverUnreachable
        case .corrupt, .incomplete, .incompatible, .rollback: code = .cannotSynchronize
        }
        return NSError(domain: NSFileProviderErrorDomain, code: code.rawValue)
    }

    static func unavailable() -> NSError {
        error(for: .authorizationUnavailable)
    }

    static func unsupported() -> NSError {
        error(for: .unsupported)
    }
}
