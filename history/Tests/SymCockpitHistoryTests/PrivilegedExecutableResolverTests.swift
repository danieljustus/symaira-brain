import Darwin
import Foundation
import XCTest
@testable import SymCockpitHistory

/// The shared resolver searches `PATH` and then a fallback list that includes
/// `$HOME/.symaira/bin` and the Homebrew prefixes. Several of those are
/// writable without root on a normal Mac, and the resolved path was handed
/// straight to `do shell script … with administrator privileges`. Anyone who
/// could write one of those directories could therefore have their binary
/// authenticated into root by the user's ordinary password prompt.
final class PrivilegedExecutableResolverTests: XCTestCase {
    private var roots: [URL] = []

    override func tearDown() {
        roots.forEach { try? FileManager.default.removeItem(at: $0) }
        roots.removeAll()
        super.tearDown()
    }

    private func makeRoot() throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("privresolve-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        roots.append(root)
        return root
    }

    private func uniqueExecutableName() -> String {
        "symcockpit-test-\(UUID().uuidString)"
    }

    /// A real user-writable directory: the temporary directory is owned by the
    /// test user, so no fixture setup can make this look root-owned.
    func testRefusesABinaryInAUserWritableDirectory() throws {
        let root = try makeRoot()
        let binDir = root.appendingPathComponent("bin", isDirectory: true)
        try FileManager.default.createDirectory(at: binDir, withIntermediateDirectories: true)
        let executable = uniqueExecutableName()
        let binary = binDir.appendingPathComponent(executable)
        try Data("#!/bin/sh\nexit 0\n".utf8).write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)

        XCTAssertThrowsError(
            try BoundedProcessRunner.resolvePrivilegedExecutablePath(
                executable,
                environment: ["PATH": binDir.path]
            )
        ) { error in
            guard let error = error as? PrivilegedExecutableError else {
                return XCTFail("expected PrivilegedExecutableError, got \(error)")
            }
            guard case .notOwnedByRoot(let path, let owner) = error else {
                return XCTFail("expected notOwnedByRoot, got \(error)")
            }
            XCTAssertTrue(path.hasPrefix("/"), "the failing path is named")
            XCTAssertFalse(owner.isEmpty, "the actual owner is named")
            // The refusal must be actionable: failing condition + where to install.
            XCTAssertTrue(error.description.contains("not root"))
            XCTAssertTrue(error.description.contains("/usr/local/bin"))
        }
    }

    /// The root-owned case cannot be created without sudo, so ownership is
    /// supplied through the injection seam the resolver exposes.
    func testAcceptsARootOwnedNonGroupWritableBinary() throws {
        let attributes: (String) -> PrivilegedPathAttributes? = { _ in
            PrivilegedPathAttributes(ownerUID: 0, mode: mode_t(0o755) | mode_t(S_IFREG))
        }
        XCTAssertNoThrow(
            try BoundedProcessRunner.validatePrivilegedPath("/usr/local/bin/symcockpit", attributes: attributes)
        )
    }

    func testRefusesARootOwnedButGroupWritableBinary() {
        let attributes: (String) -> PrivilegedPathAttributes? = { path in
            let mode: mode_t = path == "/usr/local/bin/symcockpit" ? 0o775 : 0o755
            return PrivilegedPathAttributes(ownerUID: 0, mode: mode)
        }
        XCTAssertThrowsError(
            try BoundedProcessRunner.validatePrivilegedPath("/usr/local/bin/symcockpit", attributes: attributes)
        ) { error in
            guard case .writableByNonRoot(let path, let permissions)? = error as? PrivilegedExecutableError else {
                return XCTFail("expected writableByNonRoot, got \(error)")
            }
            XCTAssertEqual(path, "/usr/local/bin/symcockpit")
            XCTAssertEqual(permissions, "0775")
        }
    }

    /// A root-owned binary inside a directory anyone can write is not safe:
    /// whoever owns the directory can replace the binary in it.
    func testRefusesARootOwnedBinaryUnderAGroupWritableDirectory() {
        let attributes: (String) -> PrivilegedPathAttributes? = { path in
            // Mirrors an Apple-Silicon Homebrew prefix: drwxrwxr-x, admin group.
            let mode: mode_t = path == "/opt/homebrew/bin" ? 0o775 : 0o755
            let uid: uid_t = path == "/opt/homebrew/bin" ? 0 : 0
            return PrivilegedPathAttributes(ownerUID: uid, mode: mode)
        }
        XCTAssertThrowsError(
            try BoundedProcessRunner.validatePrivilegedPath("/opt/homebrew/bin/symcockpit", attributes: attributes)
        ) { error in
            guard case .writableByNonRoot(let path, _)? = error as? PrivilegedExecutableError else {
                return XCTFail("expected writableByNonRoot, got \(error)")
            }
            XCTAssertEqual(path, "/opt/homebrew/bin", "the failing ancestor is named, not the binary")
        }
    }

    func testRefusesAUserOwnedAncestorOfARootOwnedBinary() {
        let attributes: (String) -> PrivilegedPathAttributes? = { path in
            // /opt/homebrew is user-owned on Apple Silicon.
            let uid: uid_t = path == "/opt/homebrew" ? 501 : 0
            return PrivilegedPathAttributes(ownerUID: uid, mode: 0o755)
        }
        XCTAssertThrowsError(
            try BoundedProcessRunner.validatePrivilegedPath("/opt/homebrew/bin/symcockpit", attributes: attributes)
        ) { error in
            guard case .notOwnedByRoot(let path, _)? = error as? PrivilegedExecutableError else {
                return XCTFail("expected notOwnedByRoot, got \(error)")
            }
            XCTAssertEqual(path, "/opt/homebrew")
        }
    }

    func testMissingExecutableIsReportedAsNotFound() {
        XCTAssertThrowsError(
            try BoundedProcessRunner.resolvePrivilegedExecutablePath(
                "symcockpit-does-not-exist",
                environment: ["PATH": "/nonexistent"]
            )
        ) { error in
            guard case .notFound? = error as? PrivilegedExecutableError else {
                return XCTFail("expected notFound, got \(error)")
            }
        }
    }

    /// The constraint must apply only to the privileged path. Unprivileged
    /// resolution for symbrain/symvault keeps the widened fallback search.
    func testUnprivilegedResolutionIsUnchangedForAUserWritableDirectory() throws {
        let root = try makeRoot()
        let binDir = root.appendingPathComponent("bin", isDirectory: true)
        try FileManager.default.createDirectory(at: binDir, withIntermediateDirectories: true)
        let binary = binDir.appendingPathComponent("symbrain")
        try Data("#!/bin/sh\nexit 0\n".utf8).write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)

        XCTAssertEqual(
            BoundedProcessRunner.resolveExecutablePath("symbrain", environment: ["PATH": binDir.path]),
            binary.path,
            "unprivileged resolution must still find a user-installed sibling CLI"
        )
    }

    /// A refused earlier candidate must not mask a qualifying later one. This
    /// is the Homebrew case: `/opt/homebrew/bin` is searched first and is
    /// user-owned on Apple Silicon, so before the search continued past a
    /// refusal, a root-owned `/usr/local/bin` copy was unreachable and fan
    /// control stayed broken no matter how it was installed.
    func testPrefersALaterRootOwnedCandidateOverAnEarlierRefusedOne() throws {
        let root = try makeRoot()
        var directories: [URL] = []
        for name in ["brewbin", "rootbin"] {
            let directory = root.appendingPathComponent(name, isDirectory: true)
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            let binary = directory.appendingPathComponent("symcockpit")
            try Data("#!/bin/sh\nexit 0\n".utf8).write(to: binary)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
            directories.append(directory)
        }
        let rootOwned = directories[1].appendingPathComponent("symcockpit").path

        // Only the second candidate and its ancestors look root-owned.
        let attributes: (String) -> PrivilegedPathAttributes? = { path in
            let owner: uid_t = rootOwned.hasPrefix(path) ? 0 : 501
            return PrivilegedPathAttributes(ownerUID: owner, mode: 0o755)
        }

        XCTAssertEqual(
            try BoundedProcessRunner.resolvePrivilegedExecutablePath(
                "symcockpit",
                environment: ["PATH": directories.map(\.path).joined(separator: ":")],
                attributes: attributes
            ),
            URL(fileURLWithPath: rootOwned).resolvingSymlinksInPath().path
        )
    }

    /// When nothing qualifies, the reported refusal is the first candidate's —
    /// the binary the user would otherwise have authenticated into root.
    func testReportsTheFirstCandidatesRefusalWhenNoneQualify() throws {
        let root = try makeRoot()
        let executable = uniqueExecutableName()
        var directories: [URL] = []
        for name in ["first", "second"] {
            let directory = root.appendingPathComponent(name, isDirectory: true)
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            let binary = directory.appendingPathComponent(executable)
            try Data("#!/bin/sh\nexit 0\n".utf8).write(to: binary)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
            directories.append(directory)
        }
        let first = directories[0].appendingPathComponent(executable).path

        XCTAssertThrowsError(
            try BoundedProcessRunner.resolvePrivilegedExecutablePath(
                executable,
                environment: ["PATH": directories.map(\.path).joined(separator: ":")]
            )
        ) { error in
            guard case .notOwnedByRoot(let path, _)? = error as? PrivilegedExecutableError else {
                return XCTFail("expected notOwnedByRoot, got \(error)")
            }
            XCTAssertTrue(
                first.hasPrefix(path),
                "the refusal must name the first candidate or one of its ancestors, got \(path)"
            )
        }
    }
}
