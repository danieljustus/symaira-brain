import Foundation
import Testing
@testable import SymBrainCore

struct SkillLibraryTests {
    @Test func decodesLibraryListing() throws {
        let json = """
        {
          "category_counts": {"repo": 2},
          "issues": [],
          "skills": [{
            "name": "00-sync",
            "description": "Use when a local repo should be made clean and main-only.",
            "path": "/Users/test/.local/share/symskills/library/00-sync",
            "created_at": "2026-08-04T19:47:58Z",
            "modified_at": "2026-08-04T19:47:58Z",
            "last_rendered_at": "2026-08-04T15:17:52Z",
            "installs": [
              {"target": "claude", "path": "/Users/test/.claude/skills/00-sync", "installed_at": "2026-08-04T15:18:07Z"},
              {"target": "codex", "path": "/Users/test/.agents/skills/00-sync", "installed_at": "2026-08-04T15:18:07Z"}
            ],
            "last_used": null
          }]
        }
        """
        let library = try JSONDecoder().decode(SkillLibrary.self, from: Data(json.utf8))
        let skill = try #require(library.skills.first)

        #expect(library.categoryCounts?["repo"] == 2)
        #expect(library.issueMessages.isEmpty)
        #expect(skill.id == "00-sync")
        #expect(skill.targets == ["claude", "codex"])
        #expect(skill.formattedLastUsed == "never")
    }

    @Test func toleratesSkillWithoutInstallsOrRenderTime() throws {
        let json = """
        {"skills": [{"name": "solo", "path": "/tmp/solo", "last_used": "2026-08-04T21:49:44Z"}]}
        """
        let library = try JSONDecoder().decode(SkillLibrary.self, from: Data(json.utf8))
        let skill = try #require(library.skills.first)

        #expect(skill.targets.isEmpty)
        #expect(skill.description == nil)
        #expect(skill.formattedLastUsed != "never")
    }
}

struct SkillStatusTests {
    @Test func decodesStatusReport() throws {
        let json = """
        {
          "installs": [
            {"target": "claude", "name": "00-sync", "path": "/Users/test/.claude/skills/00-sync",
             "status": "stale", "mode": "symlink", "installed_at": "2026-08-04T15:18:07Z",
             "source_hash": "b1480fa9"},
            {"target": "codex", "name": "commit", "path": "/Users/test/.agents/skills/commit",
             "status": "in-sync", "mode": "copy", "installed_at": "2026-08-04T15:18:07Z"}
          ],
          "summary": {"in_sync": 5, "stale": 91, "harness_changed": 0, "conflict": 0,
                      "orphaned": 1, "unmanaged": 2}
        }
        """
        let report = try JSONDecoder().decode(SkillStatusReport.self, from: Data(json.utf8))
        let summary = try #require(report.summary)

        #expect(report.rows.count == 2)
        #expect(report.rows[0].id == "claude/00-sync")
        #expect(summary.needsSync)
        // Zero-valued counts are dropped so the header only shows real state.
        #expect(summary.badges.map(\.label) == ["in sync", "stale", "orphaned", "unmanaged"])
    }

    @Test func reportsNoSyncNeededWhenEverythingIsCurrent() throws {
        let json = #"{"installs": [], "summary": {"in_sync": 7, "stale": 0}}"#
        let report = try JSONDecoder().decode(SkillStatusReport.self, from: Data(json.utf8))

        #expect(report.rows.isEmpty)
        #expect(report.summary?.needsSync == false)
        #expect(report.summary?.badges.map(\.count) == [7])
    }
}

struct SkillTargetsTests {
    @Test func decodesTargetInventory() throws {
        let json = """
        {"targets": [{
          "target": "claude",
          "display_name": "Claude Code",
          "installed": true,
          "evidence": "binary:/opt/homebrew/bin/claude",
          "effective_skill_root": "/Users/test/.claude/skills",
          "skill_root_exists": true,
          "skill_root_readable": true,
          "managed_skills_count": 31,
          "unmanaged_skills_count": 0,
          "install_state": "managed",
          "capabilities": ["render", "install", "symlink"],
          "setup_hint": "Harness is active",
          "verification_status": "verified"
        }]}
        """
        let report = try JSONDecoder().decode(SkillTargetsReport.self, from: Data(json.utf8))
        let target = try #require(report.rows.first)

        #expect(target.title == "Claude Code")
        #expect(target.managedSkillsCount == 31)
        #expect(target.capabilities?.contains("symlink") == true)
    }

    @Test func fallsBackToTargetIDWhenDisplayNameIsMissing() throws {
        let report = try JSONDecoder().decode(
            SkillTargetsReport.self,
            from: Data(#"{"targets": [{"target": "hermes"}]}"#.utf8)
        )

        #expect(report.rows.first?.title == "hermes")
    }
}

struct SkillLogTests {
    @Test func decodesLogRecords() throws {
        let json = """
        [{"ts": "2026-08-06T11:08:29Z", "event": "render", "skill": "cli-render",
          "target": "codex", "path": "/tmp/rendered", "outcome": "ok",
          "tool_version": "0.1.9", "actor": "cli", "scope": "user", "mode": "symlink"}]
        """
        let entries = try JSONDecoder().decode([SkillLogEntry].self, from: Data(json.utf8))
        let entry = try #require(entries.first)

        #expect(entry.event == "render")
        #expect(entry.outcome == "ok")
        #expect(entry.id.contains("cli-render"))
    }
}

struct SkillSyncTests {
    @Test func decodesDryRunPlan() throws {
        let json = """
        {"results": [
          {"target": "claude", "name": "00-sync", "path": "/Users/test/.claude/skills/00-sync",
           "action": "planned", "mode": "symlink"}
        ]}
        """
        let report = try JSONDecoder().decode(SkillSyncReport.self, from: Data(json.utf8))

        #expect(report.rows.count == 1)
        #expect(report.rows[0].action == "planned")
        #expect(report.rows[0].id == "claude/00-sync")
    }

    @Test func treatsMissingResultsAsEmpty() throws {
        let report = try JSONDecoder().decode(SkillSyncReport.self, from: Data("{}".utf8))

        #expect(report.rows.isEmpty)
    }
}

struct SkillsDoctorTests {
    @Test func decodesDoctorReportAndOrdersPaths() throws {
        let json = """
        {
          "config": {
            "library_dir": "/Users/test/.local/share/symskills/library",
            "render_dir": "/Users/test/.local/share/symskills/rendered",
            "cache_dir": "/Users/test/.cache/symskills",
            "profiles_dir": "/Users/test/.config/symskills/profiles",
            "base_dir": "/Users/test/.local/share/symskills/base",
            "Targets": null,
            "vcs": {"enabled": true}
          },
          "config_path": "/Users/test/.config/symskills/config.toml",
          "log_path": "/Users/test/.local/share/symskills/events.jsonl",
          "profiles_dir": "/Users/test/.config/symskills/profiles",
          "project_dir": ".",
          "targets": [{"target": "claude", "user": "/Users/test/.claude/skills"}]
        }
        """
        let report = try JSONDecoder().decode(SkillsDoctorReport.self, from: Data(json.utf8))

        #expect(report.versioningEnabled == true)
        #expect(report.pathRows.map(\.label)
            == ["Config", "Library", "Rendered", "Cache", "Profiles", "Base", "Log", "Project"])
        #expect(report.targets?.first?.user == "/Users/test/.claude/skills")
    }
}
