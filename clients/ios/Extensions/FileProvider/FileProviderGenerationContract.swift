import CryptoKit
import Foundation

/// v2 is a domain commit envelope, not a reinterpretation of partial v1 snapshots.
/// Each Library unit remains a complete, independently validated v1 hierarchy. The catalog is
/// complete for this authorization scope. Oversize units are rejected; no truncation is allowed.
struct FileProviderGenerationManifest: Codable {
    static let format = 2
    static let retainedGenerations = 3
    let schemaVersion: Int
    let domain: FileProviderDomainIdentity
    let authorization: FileProviderAuthorizationEvidence
    let records: [FileProviderGenerationRecord]
}

struct FileProviderGenerationRecord: Codable {
    let generation: UInt64
    let libraries: [FileProviderLibraryUnit]
    let pendingSignals: [String]
}

struct FileProviderLibraryUnit: Codable {
    let libraryID: String
    let filename: String
    let digest: String
}

enum FileProviderSynchronizationError: Error, Equatable {
    case unauthorized
    case unavailable
    case incompatible
    case corrupt
    case incomplete
    case rollback
    case pageExpired
    case anchorExpired
    case noSuchItem
    case cancelled
    case busy
}

/// Checksummed, bounded opaque page/anchor payload. A checksum detects corruption, not authority;
/// every decoded field is also checked against the current app-issued authorization and manifest.
struct FileProviderCursor: Codable {
    let version: Int
    let purpose: String
    let domain: String
    let owner: String
    let authorization: UInt64
    let container: String
    let generation: UInt64
    let target: UInt64
    let offset: Int

    // 162 bytes: version, purpose, three 32-byte scope hashes, four big-endian UInt64s,
    // and a 32-byte checksum. Apple's NSFileProviderPage limit is 500 bytes.
    func encoded() throws -> Data {
        var data = Data([
            UInt8(version),
            purpose == "page" ? 1 : purpose == "name" ? 3 : purpose == "date" ? 4 : 2,
        ])
        for hex in [domain, owner, container] {
            guard hex.count == 64 else { throw FileProviderSynchronizationError.corrupt }
            var index = hex.startIndex
            for _ in 0..<32 {
                let end = hex.index(index, offsetBy: 2)
                guard let byte = UInt8(hex[index..<end], radix: 16) else {
                    throw FileProviderSynchronizationError.corrupt
                }
                data.append(byte)
                index = end
            }
        }
        guard offset >= 0 else { throw FileProviderSynchronizationError.corrupt }
        for value in [authorization, generation, target, UInt64(offset)] {
            var big = value.bigEndian
            withUnsafeBytes(of: &big) { data.append(contentsOf: $0) }
        }
        data.append(contentsOf: Self.checksum(data))
        return data
    }

    static func scope(_ value: String) -> String {
        FileProviderDomainIdentity.fingerprint(["synveil-fp-cursor-scope-v1", value])
    }

    static func decode(_ data: Data) throws -> Self {
        guard data.count == 162 else { throw FileProviderSynchronizationError.corrupt }
        let bytes = Array(data)
        guard bytes[0] == 1, [1, 2, 3, 4].contains(bytes[1]),
            Array(bytes.suffix(32)) == checksum(Data(bytes.prefix(130)))
        else { throw FileProviderSynchronizationError.corrupt }
        func hex(_ offset: Int) -> String {
            bytes[offset..<(offset + 32)].map { String(format: "%02x", $0) }.joined()
        }
        func number(_ offset: Int) -> UInt64 {
            bytes[offset..<(offset + 8)].reduce(0) { ($0 << 8) | UInt64($1) }
        }
        guard number(122) <= UInt64(Int.max) else { throw FileProviderSynchronizationError.corrupt }
        return Self(
            version: 1,
            purpose: bytes[1] == 1
                ? "page" : bytes[1] == 3 ? "name" : bytes[1] == 4 ? "date" : "anchor",
            domain: hex(2),
            owner: hex(34), authorization: number(98), container: hex(66),
            generation: number(106), target: number(114), offset: Int(number(122)))
    }

    private static func checksum(_ data: Data) -> [UInt8] {
        Array(SHA256.hash(data: data))
    }
}

enum FileProviderGenerationCodec {
    static func encode<T: Encodable>(_ value: T) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        encoder.dateEncodingStrategy = .iso8601
        return try encoder.encode(value)
    }

    static func digest(_ data: Data) -> String {
        FileProviderDomainIdentity.fingerprint([
            "synveil-fp-generation-v2", data.base64EncodedString(),
        ])
    }
}
