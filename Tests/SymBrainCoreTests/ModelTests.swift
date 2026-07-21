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
