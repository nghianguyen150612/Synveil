import Foundation
import Photos

public final class PhotoLibraryPermissionAdapter: PhotoLibraryPermissionChecking,
    @unchecked Sendable
{
    public init() {}

    public func currentPermissionState() async -> PhotoPermissionState {
        let status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
        return mapStatus(status)
    }

    public func requestPermission() async -> PhotoPermissionState {
        let status = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
        return mapStatus(status)
    }

    private func mapStatus(_ status: PHAuthorizationStatus) -> PhotoPermissionState {
        switch status {
        case .notDetermined:
            return .notDetermined
        case .authorized:
            return .authorizedFull
        case .limited:
            return .authorizedLimited
        case .denied:
            return .denied
        case .restricted:
            return .restricted
        @unknown default:
            return .unknownFutureStatus
        }
    }
}
