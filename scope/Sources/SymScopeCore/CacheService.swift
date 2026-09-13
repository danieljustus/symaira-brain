import Foundation

/// Snapshot cache on the XDG cache path (`$XDG_CACHE_HOME/symscope`, or
/// `~/.cache/symscope` when the environment variable is unset).
/// Mirrors the Go original's internal/cache: one JSON file holding the last
/// snapshot, with stats and clear.
public enum CacheService: Sendable {
    /// The resolved cache directory for the current process environment.
    public static var cacheDir: String {
        currentLocation().directory.path
    }

    /// The resolved snapshot cache file for the current process environment.
    public static var cacheFile: String {
        currentLocation().file.path
    }

    struct Location: Sendable, Equatable {
        let directory: URL
        let file: URL
        let legacyFile: URL
    }

    static func location(environment: [String: String], homeDirectory: URL) -> Location {
        let cacheRoot: URL
        if let xdgCacheHome = environment["XDG_CACHE_HOME"], !xdgCacheHome.isEmpty {
            cacheRoot = URL(fileURLWithPath: xdgCacheHome)
        } else {
            cacheRoot = homeDirectory.appendingPathComponent(".cache", isDirectory: true)
        }

        let directory = cacheRoot.appendingPathComponent("symscope", isDirectory: true)
        let legacyFile = homeDirectory
            .appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("Caches", isDirectory: true)
            .appendingPathComponent("symscope", isDirectory: true)
            .appendingPathComponent("snapshot.json")
        return Location(
            directory: directory,
            file: directory.appendingPathComponent("snapshot.json"),
            legacyFile: legacyFile
        )
    }

    private static func currentLocation() -> Location {
        location(
            environment: ProcessInfo.processInfo.environment,
            homeDirectory: FileManager.default.homeDirectoryForCurrentUser
        )
    }

    public struct Stats: Codable, Equatable, Sendable {
        public var path: String
        public var exists: Bool
        public var sizeBytes: Int64
        public var modifiedAt: String?

        public init(path: String, exists: Bool, sizeBytes: Int64, modifiedAt: String?) {
            self.path = path
            self.exists = exists
            self.sizeBytes = sizeBytes
            self.modifiedAt = modifiedAt
        }
    }

    public static func stats() -> Stats {
        stats(at: currentLocation())
    }

    static func stats(at location: Location) -> Stats {
        let fm = FileManager.default
        var modified: String?
        var size: Int64 = 0
        if let attrs = try? fm.attributesOfItem(atPath: location.file.path),
           let fileSize = attrs[.size] as? Int64 {
            size = fileSize
        }
        if let modDate = try? fm.attributesOfItem(atPath: location.file.path)[.modificationDate] as? Date {
            modified = ISO8601DateFormatter().string(from: modDate)
        }
        return Stats(
            path: location.file.path,
            exists: fm.fileExists(atPath: location.file.path),
            sizeBytes: size,
            modifiedAt: modified
        )
    }

    /// Loads the cached snapshot, if present.
    public static func load() -> Snapshot? {
        load(from: currentLocation())
    }

    static func load(from location: Location) -> Snapshot? {
        // The legacy path is intentionally read-only. Once the new cache path
        // contains a readable snapshot, it always wins and is never replaced.
        readSnapshot(at: location.file) ?? readSnapshot(at: location.legacyFile)
    }

    private static func readSnapshot(at url: URL) -> Snapshot? {
        guard let data = try? Data(contentsOf: url) else { return nil }
        return try? JSONDecoder().decode(Snapshot.self, from: data)
    }

    /// Stores a snapshot. Returns nil on success, or the error.
    public static func save(_ snapshot: Snapshot) -> String? {
        save(snapshot, at: currentLocation())
    }

    static func save(_ snapshot: Snapshot, at location: Location) -> String? {
        do {
            let fm = FileManager.default
            try fm.createDirectory(
                at: location.directory,
                withIntermediateDirectories: true,
                attributes: [.posixPermissions: 0o700]
            )
            // Keep the cache private even when the directory already existed.
            try fm.setAttributes([.posixPermissions: 0o700], ofItemAtPath: location.directory.path)
            let data = try JSONEncoder().encode(snapshot)
            try data.write(to: location.file, options: .atomic)
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    public static func clear() -> String? {
        clear(at: currentLocation())
    }

    static func clear(at location: Location) -> String? {
        do {
            if FileManager.default.fileExists(atPath: location.file.path) {
                try FileManager.default.removeItem(at: location.file)
            }
            return nil
        } catch {
            return error.localizedDescription
        }
    }
}
