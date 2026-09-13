import Foundation

public enum Version {
    /// Set at build time via `-Xswiftc -D` or via the version file; falls back
    /// to "dev".
    public static let version = "0.4.1"

    public static func info() -> VersionInfo {
        VersionInfo(version: version, schemaVersion: 1)
    }
}
