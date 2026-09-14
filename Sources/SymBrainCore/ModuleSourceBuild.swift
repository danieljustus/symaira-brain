// ModuleSourceBuild — validates explicit Brain source checkouts and builds setup argv.

#if os(macOS)
import Foundation

/// Validates an explicitly selected Symaira Brain checkout and constructs the
/// corresponding `symbrain setup --from-source` invocation. This helper uses
/// only Foundation filesystem APIs: the CLI remains authoritative for Git
/// provenance and the actual module build.
public enum ModuleSourceBuild {
    /// A selected location is not structurally a Symaira Brain checkout.
    public enum ValidationError: Error, Sendable, Equatable, LocalizedError {
        case notLocalFile
        case notDirectory
        case missingDirectory(String)
        case missingFile(String)
        case missingGitMetadata

        public var errorDescription: String? {
            switch self {
            case .notLocalFile:
                "Choose a local Symaira Brain source checkout."
            case .notDirectory:
                "The selected location is not a directory."
            case .missingDirectory(let path):
                "The selected checkout is missing the \(path) directory."
            case .missingFile(let path):
                "The selected checkout is missing \(path)."
            case .missingGitMetadata:
                "The selected directory is not a Git checkout or worktree."
            }
        }
    }

    /// Returns the canonical checkout root when *selectedURL* contains the
    /// module sources Brain needs. A regular clone has a `.git` directory;
    /// linked worktrees have a `.git` file beginning with `gitdir:`.
    public static func canonicalSourceRoot(validating selectedURL: URL) throws -> URL {
        guard selectedURL.isFileURL else {
            throw ValidationError.notLocalFile
        }

        let root = selectedURL.standardizedFileURL
            .resolvingSymlinksInPath()
            .standardizedFileURL
        guard isDirectory(root) else {
            throw ValidationError.notDirectory
        }
        guard isDirectory(root.appendingPathComponent("browse", isDirectory: true)) else {
            throw ValidationError.missingDirectory("browse")
        }
        for relativePath in ["go.mod", "operate/Package.swift", "scope/Package.swift", "history/Package.swift"] {
            guard isFile(root.appendingPathComponent(relativePath)) else {
                throw ValidationError.missingFile(relativePath)
            }
        }
        guard hasGitMetadata(at: root) else {
            throw ValidationError.missingGitMetadata
        }
        return root
    }

    /// The exact argument order expected by `symbrain setup`.
    public static func setupArguments(module: String, sourceRoot: URL) -> [String] {
        ["setup", "--from-source", sourceRoot.path, "--modules", module, "--json"]
    }

    /// A shell-safe command for the GUI's copy action. Without an explicit
    /// selection it deliberately remains a non-runnable placeholder rather
    /// than guessing a checkout from the app or environment.
    public static func copyableSetupCommand(module: String, sourceRoot: URL?) -> String {
        let rootArgument = sourceRoot.map { shellQuote($0.path) }
            ?? "'<select-brain-source-checkout>'"
        return "symbrain setup --from-source \(rootArgument) --modules \(module) --json"
    }

    private static func isDirectory(_ url: URL) -> Bool {
        var isDirectory: ObjCBool = false
        return FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory)
            && isDirectory.boolValue
    }

    private static func isFile(_ url: URL) -> Bool {
        var isDirectory: ObjCBool = false
        return FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory)
            && !isDirectory.boolValue
    }

    private static func hasGitMetadata(at root: URL) -> Bool {
        let metadata = root.appendingPathComponent(".git")
        if isDirectory(metadata) {
            return true
        }
        guard isFile(metadata),
              let contents = try? String(contentsOf: metadata, encoding: .utf8)
        else {
            return false
        }
        return contents.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("gitdir:")
    }

    private static func shellQuote(_ value: String) -> String {
        "'\(value.replacingOccurrences(of: "'", with: "'\"'\"'"))'"
    }
}
#endif
