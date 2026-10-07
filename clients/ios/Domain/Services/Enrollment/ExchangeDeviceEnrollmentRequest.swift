import Foundation

/// JSON request body for `POST /api/v1/device-enrollment/exchange`.
public struct ExchangeDeviceEnrollmentRequest: Encodable, Sendable {
    public let enrollmentToken: String

    enum CodingKeys: String, CodingKey {
        case enrollmentToken = "enrollment_token"
    }

    public init(enrollmentToken: String) {
        self.enrollmentToken = enrollmentToken
    }
}
