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
                {"name": "claude", "config_path": "/Users/test/.claude.json", "config_found": true, "config_parsed": true, "installed": false, "profile": null, "profile_exists": false, "profile_missing": false}
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
        #expect(report.harnesses.count == 1)
        #expect(report.harnesses[0].name == "claude")
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
                {"name": "gemini", "path": "/path/gemini.md", "status": "unchanged"}
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
        let directory = FileManager.default.temporaryDirectory
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
