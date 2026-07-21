// AuditLog — reads JSONL audit entries from ~/.local/share/symbrain/audit/<profile>.jsonl
// or SYMBRAIN_DATA_HOME if set.

#if os(macOS)
import Foundation

/// Reads and parses the symbrain JSONL audit log.
public struct AuditLogReader: Sendable {
    public init() {}

    /// Resolve the audit directory, respecting SYMBRAIN_DATA_HOME.
    private var auditDir: URL {
        let base: String
        if let dataHome = ProcessInfo.processInfo.environment["SYMBRAIN_DATA_HOME"] {
            base = dataHome
        } else {
            base = FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent(".local/share/symbrain").path
        }
        return URL(fileURLWithPath: base).appendingPathComponent("audit", isDirectory: true)
    }

    /// Read the last `limit` entries from the given profile's audit log.
    /// If profile is nil, reads from all `.jsonl` files in the audit directory.
    public func read(profile: String?, limit: Int = 200) -> [AuditEntry] {
        var entries: [AuditEntry] = []

        if let profile {
            let path = auditDir.appendingPathComponent("\(profile).jsonl")
            entries = parseFile(at: path)
        } else {
            let fm = FileManager.default
            guard let files = try? fm.contentsOfDirectory(
                at: auditDir,
                includingPropertiesForKeys: nil
            ) else { return [] }
            for file in files where file.pathExtension == "jsonl" {
                entries.append(contentsOf: parseFile(at: file))
            }
        }

        // Sort newest first and limit
        let sorted = entries.sorted { $0.timestamp > $1.timestamp }
        return Array(sorted.prefix(limit))
    }

    private func parseFile(at url: URL) -> [AuditEntry] {
        guard let data = try? Data(contentsOf: url),
              let text = String(data: data, encoding: .utf8) else { return [] }

        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase

        var results: [AuditEntry] = []
        let lines = text.components(separatedBy: .newlines)
        for line in lines where !line.trimmingCharacters(in: .whitespaces).isEmpty {
            if let entry = try? decoder.decode(AuditEntry.self, from: Data(line.utf8)) {
                results.append(entry)
            }
        }
        return results
    }
}
#endif
