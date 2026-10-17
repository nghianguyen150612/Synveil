import Foundation

@MainActor
final class ActivePhotoTransferContextProvider: PhotoTransferContextProviding {
    private let sessionController: SessionController
    private let credentialSink: SecureCredentialSinkProtocol
    private let observationCoordinator: PhotoLibraryObservationCoordinator
    private let store: PhotoObservationStoreProtocol

    init(
        sessionController: SessionController,
        credentialSink: SecureCredentialSinkProtocol,
        observationCoordinator: PhotoLibraryObservationCoordinator,
        store: PhotoObservationStoreProtocol
    ) {
        self.sessionController = sessionController
        self.credentialSink = credentialSink
        self.observationCoordinator = observationCoordinator
        self.store = store
    }

    func currentContext() async -> PhotoTransferSessionContext? {
        guard sessionController.state == .authenticated,
            let endpoint = sessionController.serverEndpoint
        else { return nil }
        let revision = sessionController.lifecycleRevision
        do {
            let session = try await credentialSink.load(expectedServerEndpoint: endpoint)
            guard let profileId = UUID(uuidString: session.record.ownerUserId),
                sessionController.state == .authenticated,
                sessionController.lifecycleRevision == revision,
                sessionController.serverEndpoint == endpoint
            else { return nil }
            guard let authorization = await observationCoordinator.currentAuthorizationSession(),
                authorization.profileId == profileId,
                authorization.ownerId == session.record.ownerUserId
            else { return nil }
            let configuration = try await store.fetchConfiguration(profileId: profileId)
            guard sessionController.state == .authenticated,
                sessionController.lifecycleRevision == revision,
                sessionController.serverEndpoint == endpoint
            else { return nil }
            return PhotoTransferSessionContext(
                authorization: authorization,
                deviceId: session.record.deviceId,
                configuration: configuration)
        } catch {
            return nil
        }
    }
}
