import Foundation

/// Uses the shared Rust UUIDv7 parser, without interchanging domain identity types.
public struct ClientMutationId: Hashable, Sendable {
    public let rawValue: String
    private init(_ value: String) { rawValue = value }

    public static func validated(_ value: String, using bridge: any RustBridgeProtocol) async throws
        -> Self
    {
        guard try await bridge.validateNodeID(value) else {
            throw ClientMutationFailure.invalidPreparation
        }
        return Self(value)
    }

    public static func generate(
        using generator: any ClientMutationIdentityGeneratorProtocol,
        validator: any RustBridgeProtocol
    ) async throws -> Self {
        try await validated(generator.generateClientMutationID(), using: validator)
    }
}

public protocol ClientMutationIdentityGeneratorProtocol: Sendable {
    func generateClientMutationID() async throws -> String
}

/// Server event identity is not a client mutation identity.
public struct ClientMutationJournalEventId: Hashable, Sendable {
    public let rawValue: String
    private init(_ value: String) { rawValue = value }
    static func validated(_ value: String, using bridge: any RustBridgeProtocol) async throws
        -> Self
    {
        guard try await bridge.validateNodeID(value) else {
            throw ClientMutationFailure.protocolFailure
        }
        return Self(value)
    }
}

public struct ClientMutationConflictId: Hashable, Sendable {
    public let rawValue: String
    private init(_ value: String) { rawValue = value }
    static func validated(_ value: String, using bridge: any RustBridgeProtocol) async throws
        -> Self
    {
        guard try await bridge.validateNodeID(value) else {
            throw ClientMutationFailure.protocolFailure
        }
        return Self(value)
    }
}

public struct ClientMutationDeviceId: Hashable, Sendable {
    public let rawValue: String
    private init(_ value: String) { rawValue = value }

    public static func validated(_ value: String, using bridge: any RustBridgeProtocol) async throws
        -> Self
    {
        guard try await bridge.validateNodeID(value) else {
            throw ClientMutationFailure.invalidPreparation
        }
        return Self(value)
    }
}

/// Text is exact, including values beyond floating point and machine-integer precision.
public struct ClientMutationDecimal: Hashable, Sendable {
    public let rawValue: String
    public init(validating value: String) throws {
        guard LibraryWireValidation.matches(value, pattern: "^(0|[1-9][0-9]*)$") else {
            throw ClientMutationFailure.invalidPreparation
        }
        rawValue = value
    }
}

/// Non-secret originating identity; credentials never enter the prepared operation.
public struct ClientMutationScope: Equatable, Sendable {
    public let serverEndpoint: ServerEndpoint
    public let ownerUserId: String
    public let deviceId: ClientMutationDeviceId
    public let libraryId: LibraryId

}

/// Only the future authoritative sync lifecycle may supply this base. No checkpoint GET/ACK here.
public struct ClientMutationBase: Equatable, Sendable {
    public let scope: ClientMutationScope
    public let epoch: ClientMutationDecimal
    public let sequence: ClientMutationDecimal

    init(scope: ClientMutationScope, epoch: ClientMutationDecimal, sequence: ClientMutationDecimal)
        throws
    {
        guard epoch.rawValue != "0" else { throw ClientMutationFailure.invalidPreparation }
        self.scope = scope
        self.epoch = epoch
        self.sequence = sequence
    }
}

public enum ClientMutationKind: String, Codable, Sendable, CaseIterable {
    case createDirectory = "CREATE_DIRECTORY"
    case renameNode = "RENAME_NODE"
    case moveNode = "MOVE_NODE"
    case trashNode = "TRASH_NODE"
    case restoreNode = "RESTORE_NODE"
}

public enum ClientMutationIntent: Equatable, Sendable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    case createDirectory(parentNodeId: NodeId, expectedParentRevision: NodeRevision, name: String)
    case renameNode(nodeId: NodeId, expectedRevision: NodeRevision, newName: String)
    case moveNode(
        nodeId: NodeId, expectedRevision: NodeRevision, newParentNodeId: NodeId,
        expectedNewParentRevision: NodeRevision)
    case trashNode(nodeId: NodeId, expectedRevision: NodeRevision)
    case restoreNode(
        nodeId: NodeId, expectedRevision: NodeRevision, expectedParentNodeId: NodeId,
        expectedParentRevision: NodeRevision)

    public var description: String { "[REDACTED_MUTATION_INTENT]" }
    public var debugDescription: String { description }

    public var kind: ClientMutationKind {
        switch self {
        case .createDirectory: return .createDirectory
        case .renameNode: return .renameNode
        case .moveNode: return .moveNode
        case .trashNode: return .trashNode
        case .restoreNode: return .restoreNode
        }
    }

    var resourceIds: [NodeId] {
        switch self {
        case .createDirectory(let parent, _, _): return [parent]
        case .renameNode(let node, _, _), .trashNode(let node, _): return [node]
        case .moveNode(let node, _, let parent, _), .restoreNode(let node, _, let parent, _):
            return [node, parent]
        }
    }

    func validate(using bridge: any RustBridgeProtocol) async throws {
        for id in resourceIds { _ = try await NodeId.validated(id.rawValue, using: bridge) }
        switch self {
        case .createDirectory(_, _, let name), .renameNode(_, _, let name):
            guard try await bridge.validateLogicalName(name) else {
                throw ClientMutationFailure.invalidPreparation
            }
        case .moveNode(let node, _, let parent, _), .restoreNode(let node, _, let parent, _):
            guard node != parent else { throw ClientMutationFailure.invalidPreparation }
        case .trashNode: break
        }
    }
}

/// Immutable typed payload; encoding exposes only the selected operation's exact keys.
public struct ClientMutationPayload: Encodable, Equatable, Sendable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    let intent: ClientMutationIntent
    public var description: String { "[REDACTED_MUTATION_PAYLOAD]" }
    public var debugDescription: String { description }

    enum CodingKeys: String, CodingKey {
        case parentNodeId = "parent_node_id", expectedParentRevision = "expected_parent_revision",
            name
        case nodeId = "node_id", expectedRevision = "expected_revision", newName = "new_name"
        case newParentNodeId = "new_parent_node_id", expectedNewParentRevision =
            "expected_new_parent_revision"
        case expectedParentNodeId = "expected_parent_node_id"
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch intent {
        case .createDirectory(let parent, let revision, let name):
            try c.encode(parent.rawValue, forKey: .parentNodeId)
            try c.encode(revision.rawValue, forKey: .expectedParentRevision)
            try c.encode(name, forKey: .name)
        case .renameNode(let node, let revision, let name):
            try c.encode(node.rawValue, forKey: .nodeId)
            try c.encode(revision.rawValue, forKey: .expectedRevision)
            try c.encode(name, forKey: .newName)
        case .moveNode(let node, let revision, let parent, let parentRevision):
            try c.encode(node.rawValue, forKey: .nodeId)
            try c.encode(revision.rawValue, forKey: .expectedRevision)
            try c.encode(parent.rawValue, forKey: .newParentNodeId)
            try c.encode(parentRevision.rawValue, forKey: .expectedNewParentRevision)
        case .trashNode(let node, let revision):
            try c.encode(node.rawValue, forKey: .nodeId)
            try c.encode(revision.rawValue, forKey: .expectedRevision)
        case .restoreNode(let node, let revision, let parent, let parentRevision):
            try c.encode(node.rawValue, forKey: .nodeId)
            try c.encode(revision.rawValue, forKey: .expectedRevision)
            try c.encode(parent.rawValue, forKey: .expectedParentNodeId)
            try c.encode(parentRevision.rawValue, forKey: .expectedParentRevision)
        }
    }
}

/// Queue-compatible value. Rehydration must retain the supplied ID, base, intent and encoded bytes.
public struct PreparedClientMutation: Equatable, Sendable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    public let id: ClientMutationId
    public let base: ClientMutationBase
    public let kind: ClientMutationKind
    public let payload: ClientMutationPayload
    public let requestBody: Data
    public var description: String { "[REDACTED_PREPARED_MUTATION]" }
    public var debugDescription: String { description }

    init(
        id: ClientMutationId, base: ClientMutationBase, intent: ClientMutationIntent,
        bridge: any RustBridgeProtocol
    ) async throws {
        try Task.checkCancellation()
        _ = try await ClientMutationId.validated(id.rawValue, using: bridge)
        _ = try await ClientMutationDeviceId.validated(base.scope.deviceId.rawValue, using: bridge)
        _ = try await LibraryId.validated(base.scope.libraryId.rawValue, using: bridge)
        guard base.scope.serverEndpoint.isSecureScheme,
            try await bridge.validateNodeID(base.scope.ownerUserId)
        else {
            throw ClientMutationFailure.invalidPreparation
        }
        try await intent.validate(using: bridge)
        let payload = ClientMutationPayload(intent: intent)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        let body = try encoder.encode(
            ClientMutationRequestDTO(
                mutationId: id.rawValue, baseEpoch: base.epoch.rawValue,
                baseSequence: base.sequence.rawValue, kind: intent.kind, payload: payload))
        try ClientMutationPolicy.validateRequestSize(body)
        try Task.checkCancellation()
        self.id = id
        self.base = base
        kind = intent.kind
        self.payload = payload
        requestBody = body
    }
}

private struct ClientMutationRequestDTO: Encodable {
    let mutationId: String
    let baseEpoch: String
    let baseSequence: String
    let kind: ClientMutationKind
    let payload: ClientMutationPayload
    enum CodingKeys: String, CodingKey {
        case mutationId = "mutation_id", baseEpoch = "base_epoch", baseSequence = "base_sequence",
            kind, payload
    }
}

public enum ClientMutationPolicy {
    public static let maximumRequestBytes = 16_384
    public static let maximumResponseBytes = 64 * 1024
    static func validateRequestSize(_ body: Data) throws {
        guard body.count <= maximumRequestBytes else { throw ClientMutationFailure.payloadTooLarge }
    }
}

public enum ClientMutationNodeState: String, Decodable, Sendable {
    case active = "ACTIVE", trashed = "TRASHED"
}

/// Exactly the mutation projection: no fabricated purge eligibility or restore deadline.
public struct ClientMutationNodeResult: Equatable, Sendable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    public let id: NodeId
    public let libraryId: LibraryId
    public let parentNodeId: NodeId?
    public let kind: NodeKind
    public let state: ClientMutationNodeState
    public let name: String
    public let revision: NodeRevision
    public let currentVersionId: FileVersionId?
    public let trashedAt: Date?
    public let createdAt: Date
    public let updatedAt: Date
    public var description: String { "[REDACTED_MUTATION_NODE]" }
    public var debugDescription: String { description }
}

public struct ClientMutationAppliedResult: Equatable, Sendable {
    public let mutationId: ClientMutationId
    public let kind: ClientMutationKind
    public let replayed: Bool
    public let node: ClientMutationNodeResult
    public let journalEventId: ClientMutationJournalEventId
    public let journalSequence: ClientMutationDecimal
    public let requestId: String
}

public enum ClientMutationConflictReason: String, Decodable, Sendable, CaseIterable {
    case revisionMismatch = "REVISION_MISMATCH", nodeStateChanged = "NODE_STATE_CHANGED"
    case parentChanged = "PARENT_CHANGED", nameOccupied = "NAME_OCCUPIED"
    case destinationChanged = "DESTINATION_CHANGED", resourcePurged = "RESOURCE_PURGED"
}

public struct ClientMutationConflict: Equatable, Sendable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    public let conflictId: ClientMutationConflictId
    public let reason: ClientMutationConflictReason
    public let resourceId: NodeId
    public let expectedRevision: NodeRevision?
    public let currentRevision: NodeRevision?
    public let currentState: NodeState?
    public let currentParentId: NodeId?
    public let currentName: String?
    public let serverEpoch: ClientMutationDecimal
    public let serverSequence: ClientMutationDecimal
    public let replayed: Bool
    public var description: String { "[REDACTED_MUTATION_CONFLICT]" }
    public var debugDescription: String { description }
}

public enum ClientMutationServerRejection: String, Equatable, Sendable {
    case invalidMutation = "invalid_mutation", permissionDenied = "permission_denied", notFound =
        "not_found"
    case dependencyUnavailable = "dependency_unavailable", invalidPersistedData =
        "invalid_persisted_data"
    case internalError = "internal_error"
}

public enum ClientMutationFailure: Error, Equatable, Sendable {
    case preparationRequired, invalidPreparation, scopeMismatch, payloadTooLarge
    case unauthenticated, credentialUnavailable, invalidCredential, originMismatch, staleSession
    case authenticationRejected, deviceRevoked, cancelled, protocolFailure, redirectRejected
    case offline, dnsFailure, timeout, tlsFailure, resourceLimit
    case permanentRejection(ClientMutationServerRejection)
    case transientFailure(ClientMutationServerRejection)
}

public enum ClientMutationSubmissionResult: Equatable, Sendable {
    case applied(ClientMutationAppliedResult)
    /// Error code can be valid while permitted optional details are absent.
    case conflict(ClientMutationConflict?, requestId: String)
    case mutationIdConflict(requestId: String)
    case rebaselineRequired(requestId: String)
    /// No rollback is implied, including cancellation or invalidation after dispatch.
    case outcomeUnknown(ClientMutationFailure)
    case failed(ClientMutationFailure)
}

/// Deliberately module-internal: SwiftUI receives no write interface.
@MainActor
protocol ClientMutationRepositoryProtocol {
    func submit(_ mutation: PreparedClientMutation) async -> ClientMutationSubmissionResult
}
