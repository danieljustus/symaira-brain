import Foundation
import SymCockpitHistory

public final class HistoryService: HistoryServiceProtocol, @unchecked Sendable {
    private let store: CanonicalHistoryStore

    public init(fileURL: URL? = nil) {
        let resolvedURL: URL
        if let fileURL {
            resolvedURL = fileURL
        } else {
            let stateDirectory = Self.defaultStateDirectory()
            Self.secureDirectory(at: stateDirectory)
            resolvedURL = stateDirectory.appendingPathComponent("history.jsonl")
        }
        self.store = CanonicalHistoryStore(fileURL: resolvedURL)
    }

    private static func defaultStateDirectory() -> URL {
        let environment = ProcessInfo.processInfo.environment
        if let override = environment["SYMOPERATE_STATE_DIR"], !override.isEmpty {
            return URL(fileURLWithPath: override, isDirectory: true)
        }
        if environment["XCTestConfigurationFilePath"] != nil || environment["XCTestBundlePath"] != nil {
            return FileManager.default.temporaryDirectory.appendingPathComponent("symoperate-xctest", isDirectory: true)
        }
        return FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".local/share/symoperate", isDirectory: true)
    }

    static func secureDirectory(at url: URL) {
        let manager = FileManager.default
        try? manager.createDirectory(at: url, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try? manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
    }

    public func record(_ event: HistoryEvent) throws {
        try store.append(try canonicalEvent(from: event))
    }

    public func events() throws -> [HistoryEvent] {
        try store.read().compactMap { decodeOperateEvent(from: $0.payload) }
    }

    private func canonicalEvent(from event: HistoryEvent) throws -> CanonicalHistoryEvent {
        let payload = try payload(from: event)
        guard let timestamp = payload["timestamp"]?.stringValue,
              let action = payload["action"]?.stringValue else {
            throw CocoaError(.coderInvalidValue)
        }
        return CanonicalHistoryEvent(source: "operate", timestamp: timestamp, action: action, payload: payload)
    }

    private func payload(from event: HistoryEvent) throws -> [String: HistoryJSONValue] {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        encoder.outputFormatting = [.sortedKeys]
        guard case .object(let object) = try HistoryJSONValue.fromJSONData(encoder.encode(event)) else {
            throw CocoaError(.coderInvalidValue)
        }
        return object
    }

    private func decodeOperateEvent(from payload: [String: HistoryJSONValue]) -> HistoryEvent? {
        guard let data = try? HistoryJSONValue.object(payload).jsonData() else { return nil }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try? decoder.decode(HistoryEvent.self, from: data)
    }
}
