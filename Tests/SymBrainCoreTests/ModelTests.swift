import Foundation
import Testing
@testable import SymBrainCore

struct VersionInfoTests {
    @Test func decodesVersionJSON() throws {
        let json = """
        {"tool":"symbrain","version":"36cce91","schema_version":1,"go_version":"go1.26.5","os":"darwin","arch":"arm64"}
        """
        let data = Data(json.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let info = try decoder.decode(VersionInfo.self, from: data)

        #expect(info.tool == "symbrain")
        #expect(info.version == "36cce91")
        #expect(info.schemaVersion == 1)
        #expect(info.goVersion == "go1.26.5")
        #expect(info.os == "darwin")
        #expect(info.arch == "arm64")
    }
}

struct DoctorReportTests {
    @Test func decodesDoctorJSON() throws {
        let json = """
        {
            "config_dir": {"path": "/Users/test/.config/symbrain", "exists": true},
            "data_dir": {"path": "/Users/test/.local/share/symbrain", "exists": true},
            "cache_dir": {"path": "/Users/test/.cache/symbrain", "exists": false},
            "config": {"path": "/Users/test/.config/symbrain/config.toml", "exists": false, "parsed": true, "error": null},
            "servers": [
                {"name": "vault", "binary": "symvault", "found": true, "path": "/opt/homebrew/bin/symvault", "version": "0.10.1"},
                {"name": "memory", "binary": "symmemory", "found": true, "path": "/opt/homebrew/bin/symmemory", "version": "0.14.0"}
            ],
            "profiles": ["personal"],
            "harnesses": [
                {"name": "claude", "config_path": "/Users/test/.claude.json", "config_found": true, "config_parsed": true, "supports_mcp_install": true, "installed": false, "profile": null, "profile_exists": false, "profile_missing": false},
                {"name": "hermes", "config_path": "", "config_found": false, "config_parsed": false, "supports_mcp_install": false, "installed": false, "profile": null, "profile_exists": false, "profile_missing": false}
            ]
        }
        """
        let data = Data(json.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let report = try decoder.decode(DoctorReport.self, from: data)

        #expect(report.configDir.exists == true)
        #expect(report.dataDir.exists == true)
        #expect(report.cacheDir.exists == false)
        #expect(report.config.parsed == true)
        #expect(report.servers.count == 2)
        #expect(report.servers[0].name == "vault")
        #expect(report.servers[0].found == true)
        #expect(report.servers[0].version == "0.10.1")
        #expect(report.profiles == ["personal"])
        #expect(report.harnesses.count == 2)
        #expect(report.harnesses[0].name == "claude")
        // A capability-only harness is never installable; the Harnesses
        // screen filters on this rather than hardcoding a list of names.
        #expect(report.harnesses[0].supportsMcpInstall == true)
        #expect(report.harnesses[1].name == "hermes")
        #expect(report.harnesses[1].supportsMcpInstall == false)
    }
}

struct ProfileSummaryTests {
    @Test func decodesProfileListJSON() throws {
        let json = """
        [
            {
                "name": "personal",
                "description": "Full-access profile for trusted harnesses",
                "servers": [
                    {"server": "vault", "enabled": true, "mode": "full"},
                    {"server": "memory", "enabled": true, "mode": "read_write"},
                    {"server": "skills", "enabled": true, "mode": null}
                ]
            }
        ]
        """
        let data = Data(json.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let profiles = try decoder.decode([ProfileSummary].self, from: data)

        #expect(profiles.count == 1)
        #expect(profiles[0].name == "personal")
        #expect(profiles[0].servers.count == 3)
        #expect(profiles[0].servers[0].server == "vault")
        #expect(profiles[0].servers[0].mode == "full")
    }
}

struct ProfileDetailTests {
    @Test func decodesProfileShowJSON() throws {
        let json = """
        {
            "name": "personal",
            "description": "Full-access profile for trusted harnesses",
            "audit": {"enabled": true},
            "warnings": [],
            "servers": [
                {
                    "server": "vault",
                    "enabled": true,
                    "mode": "full",
                    "effective_policy": {
                        "server": "vault",
                        "enabled": true,
                        "mode": "full",
                        "exposed": ["find_entries", "generate_password", "get_entry"],
                        "hidden": [],
                        "unknown": []
                    }
                }
            ]
        }
        """
        let data = Data(json.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let detail = try decoder.decode(ProfileDetail.self, from: data)

        #expect(detail.name == "personal")
        #expect(detail.audit?.enabled == true)
        #expect(detail.servers.count == 1)
        #expect(detail.servers[0].effectivePolicy?.exposed.count == 3)
        #expect(detail.servers[0].effectivePolicy?.hidden.isEmpty == true)
    }
}

struct AuditEntryTests {
    @Test func decodesAuditEntryJSON() throws {
        let json = """
        {
            "timestamp": "2026-07-21T10:30:00.123456789Z",
            "profile": "personal",
            "server": "vault",
            "tool": "get_entry",
            "duration_ms": 42,
            "status": "ok"
        }
        """
        let data = Data(json.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let entry = try decoder.decode(AuditEntry.self, from: data)

        #expect(entry.profile == "personal")
        #expect(entry.server == "vault")
        #expect(entry.tool == "get_entry")
        #expect(entry.durationMs == 42)
        #expect(entry.status == "ok")
    }
}

struct SyncSummaryTests {
    @Test func decodesSyncSummaryJSON() throws {
        let json = """
        {
            "targets": [
                {"name": "agents", "path": "/path/.agents.md", "status": "updated"},
                {"name": "claude", "path": "/path/claude.md", "status": "created"}
            ],
            "skills": [
                {"target": "hermes-agent", "status": "updated", "message": "1 skill rendered", "duration_ms": 1234}
            ]
        }
        """
        let data = Data(json.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let summary = try decoder.decode(SyncSummary.self, from: data)

        #expect(summary.targets.count == 2)
        #expect(summary.targets[0].name == "agents")
        #expect(summary.targets[0].path == "/path/.agents.md")
        #expect(summary.targets[0].status == "updated")
        #expect(summary.targets[1].name == "claude")
        #expect(summary.targets[1].path == "/path/claude.md")
        #expect(summary.targets[1].status == "created")

        #expect(summary.skills.count == 1)
        #expect(summary.skills[0].name == "hermes-agent")
        #expect(summary.skills[0].status == "updated")
        #expect(summary.skills[0].message == "1 skill rendered")
        #expect(summary.skills[0].durationMs == 1234)
    }

    @Test func decodesSyncSummaryWithErrorAndSkippedTargets() throws {
        let json = """
        {
            "targets": [
                {"name": "opencode", "path": "/path/opencode.md", "status": "error", "message": "permission denied"},
                {"name": "cursor", "path": "/path/cursor.md", "status": "skipped"},
                {"name": "antigravity", "path": "/path/GEMINI.md", "status": "unchanged"}
            ],
            "skills": []
        }
        """
        let data = Data(json.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let summary = try decoder.decode(SyncSummary.self, from: data)

        #expect(summary.targets.count == 3)
        #expect(summary.targets[0].status == "error")
        #expect(summary.targets[0].message == "permission denied")
        #expect(summary.targets[1].status == "skipped")
        #expect(summary.targets[2].status == "unchanged")
        #expect(summary.skills.isEmpty)
    }

    @Test func decodesSyncSummaryWithCLIMissingDurationAndTargetKey() throws {
        // Real CLI output: skills use "target" key and omit "duration_ms".
        let json = """
        {
            "targets": [
                {"name": "claude", "path": "/path/claude.md", "status": "created"}
            ],
            "skills": [
                {"target": "claude", "status": "ok", "message": "2 skills rendered"}
            ]
        }
        """
        let data = Data(json.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let summary = try decoder.decode(SyncSummary.self, from: data)

        #expect(summary.targets.count == 1)
        #expect(summary.skills.count == 1)
        #expect(summary.skills[0].name == "claude")
        #expect(summary.skills[0].status == "ok")
        #expect(summary.skills[0].message == "2 skills rendered")
        #expect(summary.skills[0].durationMs == nil)
    }
}


struct BinaryResolutionTests {
    @Test func findsBinaryInExtraDirectoryWhenPathIsEmpty() throws {
        let directory = symBrainTestTemporaryDirectory()
            .appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }

        let binary = directory.appendingPathComponent("symbrain")
        try Data("#!/bin/sh\nexit 0\n".utf8).write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: binary.path)

        let client = SymBrainClient(searchPATH: "", extraDirectories: [directory.path])

        #expect(client.resolveBinary() == binary)
    }

    @Test func missingBinaryDiagnosticListsPathAndEveryExtraDirectory() {
        let first = "/tmp/symbrain-test-first"
        let second = "/tmp/symbrain-test-second"
        let client = SymBrainClient(
            searchPATH: "/tmp/symbrain-test-path",
            extraDirectories: [first, second]
        )

        let diagnostic = client.binarySearchDiagnostic

        #expect(diagnostic.contains("/tmp/symbrain-test-path"))
        #expect(diagnostic.contains(first))
        #expect(diagnostic.contains(second))
        #expect(diagnostic.contains("not found"))
        #expect(!diagnostic.contains("available on your PATH"))
    }
}

// MARK: - Real-binary JSON contract (GUI-001)

/// Resolves the opt-in real-binary contract target.
///
/// Two channels, in order:
/// 1. `SYMBRAIN_TEST_BINARY` — for direct invocations and the
///    `SymBrainCoreContract` scheme's test environment.
/// 2. `$HOME/.symbrain-test-binary` — for test runners that do not forward
///    the variable. The marker is honored only for an isolated HOME.
///
/// Without either channel the contract test is disabled — CI runs only the
/// fixture-based tests above.
private func guiContractBinaryPath() -> String? {
    let environment = ProcessInfo.processInfo.environment
    func executable(_ path: String) -> String? {
        FileManager.default.isExecutableFile(atPath: path) ? path : nil
    }
    if let path = environment["SYMBRAIN_TEST_BINARY"], !path.isEmpty,
       let resolved = executable(path) {
        return resolved
    }
    guard guiContractHomeIsIsolated(),
          let home = environment["HOME"], !home.isEmpty else { return nil }
    let marker = URL(fileURLWithPath: home).appendingPathComponent(".symbrain-test-binary")
    guard let data = try? Data(contentsOf: marker),
          let raw = String(data: data, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines),
          !raw.isEmpty,
          let resolved = executable(raw)
    else { return nil }
    return resolved
}

/// Refuses to run against the real user home.
///
/// The contract test performs a `profile add`/`remove` round trip, which
/// writes into `$XDG_CONFIG_HOME/symbrain` (or `$HOME/.config/symbrain` when
/// XDG is unset). Pointing that at the live user config would mutate a real
/// installation, so the test requires an isolated `HOME` and forbids an
/// XDG config root inside the real one. Invocations must therefore set an
/// isolated HOME alongside the binary marker.
private func guiContractHomeIsIsolated() -> Bool {
    let environment = ProcessInfo.processInfo.environment
    guard let home = environment["HOME"], !home.isEmpty,
          let passwd = getpwuid(getuid()),
          let pwDir = passwd.pointee.pw_dir
    else { return false }
    let realHome = URL(fileURLWithPath: String(cString: pwDir))
        .standardizedFileURL.resolvingSymlinksInPath().path
    let testHome = URL(fileURLWithPath: home)
        .standardizedFileURL.resolvingSymlinksInPath().path
    guard !realHome.isEmpty, testHome != realHome,
          !testHome.hasPrefix(realHome + "/")
    else { return false }
    if let xdgConfig = environment["XDG_CONFIG_HOME"], !xdgConfig.isEmpty {
        let configPath = URL(fileURLWithPath: xdgConfig)
            .standardizedFileURL.resolvingSymlinksInPath().path
        if configPath == realHome || configPath.hasPrefix(realHome + "/") {
            return false
        }
    }
    return true
}

struct ConfiguredBinaryContractTests {
    /// Proves the exact JSON contracts the Swift client decodes against a
    /// real `symbrain` binary: `version`, `doctor`, the `profile`
    /// add/list/show/remove round trip and `sync --dry-run`. Every decode
    /// uses `CLIRunner.runDecoding` (snake_case, exit-code checked) through
    /// `SymBrainClient`, so a Go↔Rust schema difference surfaces here as a
    /// thrown `invalidJSON`/`executionFailed` error or a failed assertion.
    @Test(.enabled(if: guiContractBinaryPath() != nil && guiContractHomeIsIsolated()))
    func decodesEveryClientJSONSurfaceFromConfiguredBinary() async throws {
        let path = try #require(guiContractBinaryPath())
        let binary = URL(fileURLWithPath: path)
        let client = SymBrainClient(userOverride: binary)
        #expect(client.resolveBinary() == binary)

        // version --json — the GUI<->core schema handshake (versionkit).
        let info = try await client.version()
        #expect(info.tool == "symbrain")
        #expect(!info.version.isEmpty)
        #expect(info.schemaVersion == 1)

        // doctor --json — full report decode against an isolated config.
        let report = try await client.doctor()
        #expect(!report.configDir.path.isEmpty)
        #expect(!report.dataDir.path.isEmpty)
        #expect(!report.cacheDir.path.isEmpty)
        #expect(!report.config.path.isEmpty)
        #expect(report.profiles.isEmpty)
        #expect(report.harnesses.contains { $0.name == "claude" })
        #expect(report.servers.contains { $0.binary == "symvault" })

        // profile add/list/show/remove round trip (writes stay in the
        // isolated XDG/HOME the invocation set up).
        let name = "gui-contract"
        _ = try await client.profileAdd(name: name, from: "personal")
        let listed = try await client.profileList()
        #expect(listed.contains { $0.name == name })
        #expect(listed.first?.servers.contains { $0.server == "vault" } == true)

        let detail = try await client.profileShow(name: name)
        #expect(detail.name == name)
        #expect(!detail.description.isEmpty)
        #expect(!detail.servers.isEmpty)

        _ = try await client.profileRemove(name: name)
        let afterRemove = try await client.profileList()
        #expect(afterRemove.isEmpty)

        // sync --json --dry-run — report decode; dry-run never writes.
        let sync = try await client.sync(dryRun: true)
        #expect(!sync.targets.isEmpty)
        #expect(sync.targets.allSatisfy { !$0.name.isEmpty && !$0.path.isEmpty || $0.status == "skipped" })
    }
}
