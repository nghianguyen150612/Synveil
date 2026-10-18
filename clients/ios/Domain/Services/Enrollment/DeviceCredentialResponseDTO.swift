import Foundation

/// Wire response DTO for `POST /api/v1/device-enrollment/exchange`.
public struct DeviceCredentialResponseDTO: Decodable, Sendable {
    public let data: DeviceCredentialDataDTO
    public let meta: ResponseMetaDTO?

    public init(data: DeviceCredentialDataDTO, meta: ResponseMetaDTO? = nil) {
        self.data = data
        self.meta = meta
    }
}

/// Data payload of `DeviceCredentialResponseDTO`.
public struct DeviceCredentialDataDTO: Decodable, Sendable {
    public let ownerUserId: String
    public let deviceId: String
    public let credentialId: String
    public let deviceCredential: String
    public let createdAt: String

    enum CodingKeys: String, CodingKey {
        case ownerUserId = "owner_user_id"
        case deviceId = "device_id"
        case credentialId = "credential_id"
        case deviceCredential = "device_credential"
        case createdAt = "created_at"
    }

    public init(
        ownerUserId: String,
        deviceId: String,
        credentialId: String,
        deviceCredential: String,
        createdAt: String
    ) {
        self.ownerUserId = ownerUserId
        self.deviceId = deviceId
        self.credentialId = credentialId
        self.deviceCredential = deviceCredential
        self.createdAt = createdAt
    }
}

/// Wire metadata envelope DTO.
public struct ResponseMetaDTO: Decodable, Sendable {
    public let requestId: String?

    enum CodingKeys: String, CodingKey {
        case requestId = "request_id"
    }

    public init(requestId: String? = nil) {
        self.requestId = requestId
    }
}

/// Generic error response payload returned by API endpoints.
public struct APIErrorResponseDTO: Decodable, Sendable {
    public let error: APIErrorDataDTO?
}

/// Error detail payload in `APIErrorResponseDTO`.
public struct APIErrorDataDTO: Decodable, Sendable {
    public let code: String?
    public let message: String?
}
