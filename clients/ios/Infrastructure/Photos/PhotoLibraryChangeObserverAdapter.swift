import Foundation
import Photos

public final class PhotoLibraryChangeObserverAdapter: NSObject, PHPhotoLibraryChangeObserver,
    @unchecked Sendable
{
    private let lock = NSLock()
    private var onChangeHandler: (@Sendable () -> Void)?
    private var isRegistered = false
    private var revisionValue: UInt64 = 0

    public override init() {
        super.init()
    }

    public func register(onChange: @escaping @Sendable () -> Void) {
        lock.lock()
        defer { lock.unlock() }
        self.onChangeHandler = onChange
        if !isRegistered {
            PHPhotoLibrary.shared().register(self)
            isRegistered = true
        }
    }

    public func unregister() {
        lock.lock()
        defer { lock.unlock() }
        if isRegistered {
            PHPhotoLibrary.shared().unregisterChangeObserver(self)
            isRegistered = false
        }
        onChangeHandler = nil
    }

    public func photoLibraryDidChange(_ changeInstance: PHChange) {
        lock.lock()
        revisionValue &+= 1
        let handler = onChangeHandler
        lock.unlock()

        handler?()
    }

    public var revision: UInt64 {
        lock.lock()
        defer { lock.unlock() }
        return revisionValue
    }

    deinit {
        if isRegistered {
            PHPhotoLibrary.shared().unregisterChangeObserver(self)
        }
    }
}
