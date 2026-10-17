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
                let code = NSFileProviderError.noSuchItem.rawValue
                return NSError(domain: NSFileProviderErrorDomain, code: code)
            case .lengthMismatch, .sha256Mismatch, .corruptManifest, .publicationFailed:
                let code = NSFileProviderError.cannotSynchronize.rawValue
                return NSError(domain: NSFileProviderErrorDomain, code: code)
            case .missingEntitlement, .containerUnavailable, .fileSystemError,
                .symbolicLink, .unsafePath:
                let code = NSFileProviderError.serverUnreachable.rawValue
                return NSError(domain: NSFileProviderErrorDomain, code: code)
            case .storageQuotaExceeded:
                let code = NSFileProviderError.insufficientQuota.rawValue
                return NSError(domain: NSFileProviderErrorDomain, code: code)
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
