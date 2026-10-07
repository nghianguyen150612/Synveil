import Foundation

/// Maximum allowed response body size for enrollment exchange responses (16 KiB).
private let maxEnrollmentResponseBodyBytes: Int = 16 * 1024

/// Production implementation of `EnrollmentExchangeServiceProtocol`.
public final class EnrollmentExchangeService: EnrollmentExchangeServiceProtocol, Sendable {
    private let transport: HTTPTransportProtocol
    private let rustBridge: RustBridgeProtocol?

    /// Initializes `EnrollmentExchangeService`.
    ///
    /// - Parameters:
    ///   - transport: `HTTPTransportProtocol` network client.
    ///   - rustBridge: Optional `RustBridgeProtocol` instance for credential validation.
    public init(
        transport: HTTPTransportProtocol,
        rustBridge: RustBridgeProtocol? = nil
    ) {
        self.transport = transport
        self.rustBridge = rustBridge
    }

    public func exchange(
        endpoint: ServerEndpoint,
        token: EnrollmentToken
    ) async -> EnrollmentExchangeResult {
        // Construct target URL for POST /api/v1/device-enrollment/exchange
        let baseString = endpoint.url.absoluteString.hasSuffix("/")
            ? endpoint.url.absoluteString
            : endpoint.url.absoluteString + "/"
        guard let exchangeURL = URL(string: "api/v1/device-enrollment/exchange", relativeTo: URL(string: baseString))?.absoluteURL else {
            return .failed(.invalidConfiguration)
        }

        // Encode request body
        let requestDTO = ExchangeDeviceEnrollmentRequest(enrollmentToken: token.rawValue)
        let requestBodyData: Data
        do {
            requestBodyData = try JSONEncoder().encode(requestDTO)
        } catch {
            return .failed(.invalidConfiguration)
        }

        let headers = [
            "Content-Type": "application/json",
            "Accept": "application/json",
        ]

        let transportRequest = HTTPTransportRequest(
            url: exchangeURL,
            method: .post,
            headers: headers,
            body: requestBodyData
        )

        // Single-shot HTTP transport call with 16 KiB response bound
        let response: HTTPTransportResponse
        do {
            response = try await transport.send(
                request: transportRequest,
                maxResponseBodyBytes: maxEnrollmentResponseBodyBytes
            )
        } catch let transportError as SynveilTransportError {
            return mapTransportErrorToExchangeResult(transportError)
        } catch {
            return .failed(.transport(.protocolError(.invalidHeaderValue)))
        }

        let statusCode = response.statusCode

        // Check for HTTP 201 Created
        if statusCode == 201 {
            return await parseSuccessResponse(response: response)
        } else if statusCode == 400 || statusCode == 401 || statusCode == 422 {
            // Invalid/rejected enrollment
            let (code, reqId) = parseErrorDetails(from: response.body)
            let finalReqId = response.requestId ?? reqId
            return .rejected(code: code ?? "invalid_enrollment", requestId: finalReqId)
        } else if statusCode == 503 {
            return .recoveryRequired(.http503)
        } else if statusCode >= 300 && statusCode < 400 {
            return .recoveryRequired(.redirect)
        } else {
            return .recoveryRequired(.responseLoss)
        }
    }

    private func parseSuccessResponse(response: HTTPTransportResponse) async -> EnrollmentExchangeResult {
        // Validate Content-Type
        guard let contentType = response.contentType?.lowercased(),
              contentType.contains("application/json") else {
            return .recoveryRequired(.wrongContentType)
        }

        let decoder = JSONDecoder()
        let dto: DeviceCredentialResponseDTO
        do {
            dto = try decoder.decode(DeviceCredentialResponseDTO.self, from: response.body)
        } catch {
            return .recoveryRequired(.malformedResponse)
        }

        let dataDTO = dto.data
        let rawCredential = dataDTO.deviceCredential

        // Validate returned svd1_ credential using Rust shared core if bridge available, or fallback to regex
        if let bridge = rustBridge {
            do {
                let isValid = try await bridge.validateDeviceBearerToken(rawCredential)
                guard isValid else {
                    return .recoveryRequired(.malformedResponse)
                }
            } catch {
                return .recoveryRequired(.malformedResponse)
            }
        } else {
            guard DeviceCredential.isValid(rawCredential) else {
                return .recoveryRequired(.malformedResponse)
            }
        }

        let credential = DeviceCredential(validatedRawValue: rawCredential)
        let reqId = response.requestId ?? dto.meta?.requestId

        do {
            let record = try DeviceCredentialRecord(
                ownerUserId: dataDTO.ownerUserId,
                deviceId: dataDTO.deviceId,
                credentialId: dataDTO.credentialId,
                credential: credential,
                createdAt: dataDTO.createdAt,
                requestId: reqId
            )
            return .success(record)
        } catch {
            return .recoveryRequired(.malformedResponse)
        }
    }

    private func mapTransportErrorToExchangeResult(_ error: SynveilTransportError) -> EnrollmentExchangeResult {
        switch error {
        case .timeout:
            return .recoveryRequired(.timeout)
        case .offline, .dnsFailure, .tlsError:
            return .recoveryRequired(.disconnect)
        case .redirectRejected:
            return .recoveryRequired(.redirect)
        case .bodyLimitExceeded:
            return .recoveryRequired(.oversizedResponse)
        case .unexpectedContentType:
            return .recoveryRequired(.wrongContentType)
        case .malformedResponse:
            return .recoveryRequired(.malformedResponse)
        case .cancelled:
            return .failed(.cancelled)
        case .configurationError:
            return .failed(.invalidConfiguration)
        case .httpError(let statusCode, _, _):
            if statusCode == 503 {
                return .recoveryRequired(.http503)
            } else if statusCode == 400 || statusCode == 401 || statusCode == 422 {
                return .rejected(code: "invalid_enrollment", requestId: nil)
            } else {
                return .recoveryRequired(.responseLoss)
            }
        case .protocolError:
            return .recoveryRequired(.malformedResponse)
        }
    }

    private func parseErrorDetails(from data: Data) -> (code: String?, requestId: String?) {
        guard !data.isEmpty else { return (nil, nil) }
        do {
            let dto = try JSONDecoder().decode(APIErrorResponseDTO.self, from: data)
            return (dto.error?.code, nil)
        } catch {
            return (nil, nil)
        }
    }
}
