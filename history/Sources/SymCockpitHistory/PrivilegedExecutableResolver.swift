import Darwin
import Foundation

/// Why a resolved binary was refused for execution as root.
///
/// Every case names the exact failing condition and the expected install
/// location, because the alternative — a bare "refused" — leaves a user with a
/// legitimate install no way to tell a misconfiguration from an attack.
public enum PrivilegedExecutableError: Error, Sendable, Equatable, CustomStringConvertible {
    case notFound(executable: String)
    case notOwnedByRoot(path: String, owner: String)
    case writableByNonRoot(path: String, permissions: String)

    public var description: String {
        switch self {
        case .notFound(let executable):
            return "\(executable) was not found in any standard install location; "
                + "install it with `brew install danieljustus/tap/\(executable)`"
        case .notOwnedByRoot(let path, let owner):
            return "refusing to run \(path) as an administrator: it is owned by \(owner), not root. "
                + "Anything that can write it could run as root behind the administrator prompt. "
                + "Install it in a root-owned location such as /usr/local/bin."
        case .writableByNonRoot(let path, let permissions):
            return "refusing to run \(path) as an administrator: it is writable by group or other (\(permissions)). "
                + "Anything that can write it could run as root behind the administrator prompt. "
                + "Install it in a root-owned location such as /usr/local/bin."
        }
    }
}

/// Ownership and permission bits of one filesystem entry.
public struct PrivilegedPathAttributes: Sendable, Equatable {
    public let ownerUID: uid_t
    public let mode: mode_t

    public init(ownerUID: uid_t, mode: mode_t) {
        self.ownerUID = ownerUID
        self.mode = mode
    }
}

extension BoundedProcessRunner {
    /// Resolves `executable` the same way ``run(executable:arguments:timeoutSeconds:environment:standardInput:)``
    /// does, then refuses it unless the binary *and* every directory above it
    /// are owned by root and not writable by group or other.
    ///
    /// This deliberately does not tighten the shared resolver: the widened
    /// fallback search exists so a GUI app launched by launchd can find sibling
    /// CLIs (`symbrain`, `symvault`), and those run unprivileged. The
    /// constraint belongs only where the resolved path is about to be handed to
    /// `do shell script … with administrator privileges`, where a user-writable
    /// candidate turns a routine password prompt into local root.
    ///
    /// Symlinks are followed before validating, so a root-owned symlink cannot
    /// stand in for a user-writable target.
    ///
    /// Every candidate the search finds is examined, not just the first. On
    /// Apple Silicon the Homebrew prefix is owned by the installing user, so a
    /// `brew install` — the way the "not found" message itself tells people to
    /// install — always fails this check, and `/opt/homebrew/bin` is searched
    /// before `/usr/local/bin`. Stopping at the first hit therefore let a
    /// refused Homebrew copy mask a properly installed root-owned one and made
    /// the refusal unfixable by following its own advice. The refusal reported
    /// when nothing qualifies is the first candidate's, which is the binary the
    /// user would otherwise have run.
    public static func resolvePrivilegedExecutablePath(
        _ executable: String,
        environment: [String: String] = ProcessInfo.processInfo.environment,
        attributes: (String) -> PrivilegedPathAttributes? = Self.attributesOfPath
    ) throws -> String {
        let candidates = resolveExecutableCandidates(executable, environment: environment)
        guard !candidates.isEmpty else {
            throw PrivilegedExecutableError.notFound(executable: executable)
        }
        var firstRefusal: PrivilegedExecutableError?
        for candidate in candidates {
            let canonical = URL(fileURLWithPath: candidate).resolvingSymlinksInPath().path
            do {
                try validatePrivilegedPath(canonical, attributes: attributes)
                return canonical
            } catch let error as PrivilegedExecutableError {
                if firstRefusal == nil { firstRefusal = error }
            }
        }
        throw firstRefusal ?? PrivilegedExecutableError.notFound(executable: executable)
    }

    /// Validates `path` and every ancestor directory up to `/`.
    ///
    /// A trustworthy binary in an untrustworthy directory is not trustworthy:
    /// whoever can write the directory can replace the binary in it.
    static func validatePrivilegedPath(
        _ path: String,
        attributes: (String) -> PrivilegedPathAttributes?
    ) throws {
        var current = URL(fileURLWithPath: path).standardized
        while true {
            let candidate = current.path
            guard let entry = attributes(candidate) else {
                // An entry that cannot be inspected cannot be shown to be safe.
                throw PrivilegedExecutableError.notOwnedByRoot(path: candidate, owner: "an unknown user")
            }
            guard entry.ownerUID == 0 else {
                throw PrivilegedExecutableError.notOwnedByRoot(
                    path: candidate,
                    owner: Self.ownerName(for: entry.ownerUID)
                )
            }
            guard entry.mode & (mode_t(S_IWGRP) | mode_t(S_IWOTH)) == 0 else {
                throw PrivilegedExecutableError.writableByNonRoot(
                    path: candidate,
                    permissions: Self.permissionString(entry.mode)
                )
            }
            if candidate == "/" { return }
            let parent = current.deletingLastPathComponent().standardized
            // deletingLastPathComponent on "/" returns "/" — the guard above
            // already returned, so this only stops a malformed loop.
            if parent.path == candidate { return }
            current = parent
        }
    }

    /// Reads ownership and mode without following the final symlink — the
    /// caller has already canonicalised the path.
    public static func attributesOfPath(_ path: String) -> PrivilegedPathAttributes? {
        var info = stat()
        guard stat(path, &info) == 0 else { return nil }
        return PrivilegedPathAttributes(ownerUID: info.st_uid, mode: info.st_mode)
    }

    static func ownerName(for uid: uid_t) -> String {
        guard let pwd = getpwuid(uid) else { return "uid \(uid)" }
        return String(cString: pwd.pointee.pw_name)
    }

    static func permissionString(_ mode: mode_t) -> String {
        let bits = mode & 0o777
        return String(format: "%04o", bits)
    }
}
