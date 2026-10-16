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

    static func unavailable() -> NSError {
        error(for: .authorizationUnavailable)
    }

    static func unsupported() -> NSError {
        error(for: .unsupported)
    }
}
