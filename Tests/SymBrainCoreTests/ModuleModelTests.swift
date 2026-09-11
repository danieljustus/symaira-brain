import Foundation
import Testing
@testable import SymBrainCore

// The module models are decoded with a plain JSONDecoder on purpose: the CLIs
// return free-form dictionaries whose keys must survive verbatim.

struct MemoryRecordTests {
    @Test func decodesListEntry() throws {
        let json = """
        {
          "id": "734da5e3-a9fa-4d15-9668-3fb3b6833050",
          "content": "Daniel prefers concise commit messages.",
          "scope": "user",
          "metadata": {"source_tool": "hook:claude-code", "confidence": "high"},
          "created_at": "2026-08-10T11:12:48.472717Z",
          "updated_at": "2026-08-10T11:12:48.472717Z",
          "created_by": "hook:claude-code",
          "consolidation_status": "raw",
          "review_status": "approved",
          "importance": 0,
          "decay_factor": 1,
          "tier": "working",
          "expires_at": "2026-08-11T11:12:48.443924Z",
          "access_count": 3
        }
        """
        let record = try JSONDecoder().decode(MemoryRecord.self, from: Data(json.utf8))

        #expect(record.id == "734da5e3-a9fa-4d15-9668-3fb3b6833050")
        #expect(record.scope == "user")
        #expect(record.createdBy == "hook:claude-code")
        #expect(record.reviewStatus == "approved")
        #expect(record.accessCount == 3)
        #expect(record.displayTier == "working")
    }

    @Test func keepsSnakeCaseMetadataKeys() throws {
        let json = """
        {
          "id": "a", "content": "c", "scope": "global",
          "metadata": {"source_tool": "cli", "api_key_hint": "none"},
          "created_at": "2026-08-10T11:12:48Z"
        }
        """
        let record = try JSONDecoder().decode(MemoryRecord.self, from: Data(json.utf8))

        #expect(record.metadata?["source_tool"]?.displayString == "cli")
        #expect(record.metadataPairs.map(\.key) == ["api_key_hint", "source_tool"])
    }

    @Test func reportsEmptyTierAsUntiered() throws {
        let json = """
        {"id": "a", "content": "c", "scope": "agent", "tier": "", "created_at": "2026-08-10T11:12:48Z"}
        """
        let record = try JSONDecoder().decode(MemoryRecord.self, from: Data(json.utf8))

        #expect(record.displayTier == "untiered")
    }

    @Test func decodesSearchHit() throws {
        let json = """
        [{
          "memory": {"id": "a", "content": "c", "scope": "agent", "created_at": "2026-08-10T11:12:48Z"},
          "similarity_score": 1.63848
        }]
        """
        let hits = try JSONDecoder().decode([MemorySearchHit].self, from: Data(json.utf8))

        #expect(hits.count == 1)
        #expect(hits[0].id == "a")
        #expect(hits[0].similarityScore == 1.63848)
    }
}

struct MemoryQueryLogTests {
    @Test func decodesQueryLog() throws {
        let json = """
        {
          "total_queries": 3,
          "tool_breakdown": {"memory_search": 2, "memory_set": 1},
          "actor_breakdown": {"mcp/0.1.0": 3},
          "recent_entries": [{
            "id": "ab4e1763-1753-4524-ae0f-ed56033807b9",
            "actor": "mcp/0.1.0",
            "tool": "memory_search",
            "query_text": "hermes configuration",
            "params": "{}",
            "duration_ms": 244,
            "created_at": "2026-08-08T15:34:40.63654Z"
          }]
        }
        """
        let log = try JSONDecoder().decode(MemoryQueryLog.self, from: Data(json.utf8))

        #expect(log.totalQueries == 3)
        #expect(log.entries.count == 1)
        #expect(log.entries[0].durationMs == 244)
        // Sorted by count descending, then name.
        #expect(log.toolCounts.map(\.name) == ["memory_search", "memory_set"])
        #expect(log.actorCounts.first?.count == 3)
    }

    @Test func toleratesMissingSections() throws {
        let log = try JSONDecoder().decode(
            MemoryQueryLog.self,
            from: Data(#"{"total_queries": 0}"#.utf8)
        )

        #expect(log.entries.isEmpty)
        #expect(log.toolCounts.isEmpty)
    }
}

struct VaultEntryTests {
    @Test func decodesListEntries() throws {
        let json = """
        [{"path": "work/github", "type": "password", "usage_hint": "CI token", "has_value": true, "field_count": 3}]
        """
        let entries = try JSONDecoder().decode([VaultEntrySummary].self, from: Data(json.utf8))

        #expect(entries[0].title == "github")
        #expect(entries[0].group == "work")
        #expect(entries[0].usageHint == "CI token")
        #expect(entries[0].fieldCount == 3)
    }

    @Test func groupsTopLevelEntriesUnderGeneral() throws {
        let entries = try JSONDecoder().decode(
            [VaultEntrySummary].self,
            from: Data(#"[{"path": "router"}]"#.utf8)
        )

        #expect(entries[0].group == "General")
        #expect(entries[0].title == "router")
    }

    @Test func decodesLowercaseDetail() throws {
        let json = """
        {
          "path": "work/github",
          "modified": "2026-08-08T15:34:40Z",
          "fields": {"username": "daniel", "password": "hunter2", "url": "https://github.com"},
          "totp": {"code": "123456", "period": 30, "remaining": 12}
        }
        """
        let detail = try JSONDecoder().decode(VaultEntryDetail.self, from: Data(json.utf8))

        #expect(detail.path == "work/github")
        #expect(detail.primarySecret?.field == "password")
        #expect(detail.primarySecret?.value == "hunter2")
        #expect(detail.totp?.code == "123456")
        // The secret sorts first, the remaining fields alphabetically.
        #expect(detail.sortedFields.map(\.key) == ["password", "url", "username"])
        #expect(detail.sortedFields[0].isSensitive)
        #expect(detail.sortedFields[2].isSensitive == false)
    }

    @Test func decodesUppercaseDetail() throws {
        let json = """
        {"Path": "router", "Modified": "2026-08-08T15:34:40Z", "Fields": {"api_key": "abc"}}
        """
        let detail = try JSONDecoder().decode(VaultEntryDetail.self, from: Data(json.utf8))

        #expect(detail.path == "router")
        #expect(detail.fields["api_key"]?.displayString == "abc")
        #expect(detail.primarySecret?.field == "api_key")
    }

    @Test func classifiesSensitiveFields() {
        #expect(VaultFieldSecurity.isSensitive("password"))
        #expect(VaultFieldSecurity.isSensitive("API_KEY"))
        #expect(VaultFieldSecurity.isSensitive("totp_secret"))
        #expect(VaultFieldSecurity.isSensitive("username") == false)
        #expect(VaultFieldSecurity.isSensitive("url") == false)
    }

    @Test func choosesEverySupportedPrimarySecretField() {
        for field in ["password", "api_key", "token", "private_key", "database_url"] {
            let detail = VaultEntryDetail(path: "work/item", modified: nil, fields: [
                field: .string("value"), "username": .string("daniel")
            ])
            #expect(detail.primarySecret?.field == field)
        }
    }
}

struct MemoryScopeFilterTests {
    @Test func omitsFlagForAllScopes() {
        #expect(MemoryScopeFilter.all.cliValue == nil)
        #expect(MemoryScopeFilter.all.label == "All Scopes")
        #expect(MemoryScopeFilter.project.cliValue == "project")
        #expect(MemoryScopeFilter.project.label == "Project")
    }
}

struct ModuleTimestampTests {
    @Test func parsesFractionalAndPlainTimestamps() {
        #expect(parseModuleTimestamp("2026-08-10T11:12:48.472717Z") != nil)
        #expect(parseModuleTimestamp("2026-06-02T18:34:44+02:00") != nil)
        #expect(parseModuleTimestamp("not a date") == nil)
    }

    @Test func returnsRawStringForUnparsableInput() {
        #expect(formatModuleTimestamp("not a date") == "not a date")
    }
}

struct ReportTextTests {
    @Test func prefersStdoutOverUsageNoiseOnStderr() {
        let report = reportText(
            stdout: "✅ Database: 36 migrations applied",
            stderr: "Error: some health checks failed\nUsage:\n  symmemory doctor [flags]"
        )

        #expect(report == "✅ Database: 36 migrations applied")
    }

    @Test func fallsBackToStderrAndThenPlaceholder() {
        #expect(reportText(stdout: "  ", stderr: "boom") == "boom")
        #expect(reportText(stdout: "", stderr: "") == "No output.")
    }
}

struct AuditEntryIdentityTests {
    @Test func idIsStableAcrossAccesses() throws {
        let json = """
        {"timestamp":"2026-08-10T11:12:48.472717Z","profile":"personal","server":"memory",
         "tool":"memory_search","durationMs":12,"status":"ok"}
        """
        let entry = try JSONDecoder().decode(AuditEntry.self, from: Data(json.utf8))

        #expect(entry.id == entry.id)
        #expect(entry.id.contains("memory_search"))
    }
}


#if os(macOS)
import AppKit

@MainActor
private final class FakeClipboardPasteboard: ClipboardPasteboard {
    var changeCount = 0
    var value: String?
    var concealed = false
    var clearCount = 0

    func clearContents() {
        clearCount += 1
        value = nil
        concealed = false
        changeCount += 1
    }

    func setString(_ string: String, forType: NSPasteboard.PasteboardType) {
        value = string
        changeCount += 1
    }

    func setData(_ data: Data, forType: NSPasteboard.PasteboardType) {
        concealed = true
        changeCount += 1
    }
}

private actor ClipboardManualSleeper {
    private var waiters: [CheckedContinuation<Void, Never>] = []

    func sleep(_ duration: Duration) async throws {
        await withCheckedContinuation { continuation in
            waiters.append(continuation)
        }
    }

    func resumeNext() {
        guard !waiters.isEmpty else { return }
        waiters.removeFirst().resume()
    }
}

@MainActor
struct ClipboardLifetimeTests {
    @Test func clearsOwnedSecretAfterControllableExpiry() async {
        let fake = FakeClipboardPasteboard()
        let clock = ClipboardManualSleeper()
        let controller = ClipboardLifetimeController(
            pasteboard: fake,
            sleep: { duration in try await clock.sleep(duration) }
        )

        controller.write("test-secret", concealed: true)
        await Task.yield()
        #expect(fake.value == "test-secret")
        #expect(fake.concealed)

        await clock.resumeNext()
        await Task.yield()
        #expect(fake.value == nil)
        #expect(fake.clearCount == 2)
    }

    @Test func ownershipTokenKeepsReplacementWithSameValue() async {
        let fake = FakeClipboardPasteboard()
        let clock = ClipboardManualSleeper()
        let controller = ClipboardLifetimeController(
            pasteboard: fake,
            sleep: { duration in try await clock.sleep(duration) }
        )

        controller.write("same", concealed: true)
        fake.setString("same", forType: .string)
        await clock.resumeNext()
        await Task.yield()

        #expect(fake.value == "same")
        #expect(fake.clearCount == 1)
    }

    @Test func ordinaryClipboardCopyIsRetained() async {
        let fake = FakeClipboardPasteboard()
        let clock = ClipboardManualSleeper()
        let controller = ClipboardLifetimeController(
            pasteboard: fake,
            sleep: { duration in try await clock.sleep(duration) }
        )

        controller.write("ordinary", concealed: false)
        await Task.yield()

        #expect(fake.value == "ordinary")
        #expect(fake.concealed == false)
        #expect(fake.clearCount == 1)
    }

    @Test func replacingSecretCancelsOldExpiry() async {
        let fake = FakeClipboardPasteboard()
        let clock = ClipboardManualSleeper()
        let controller = ClipboardLifetimeController(
            pasteboard: fake,
            sleep: { duration in try await clock.sleep(duration) }
        )

        controller.write("first", concealed: true)
        await Task.yield()
        controller.write("second", concealed: true)
        await Task.yield()
        await clock.resumeNext()
        await Task.yield()

        #expect(fake.value == "second")
        #expect(fake.clearCount == 2)
    }
}

struct VaultCredentialMetadataTests {
    @Test func decodesCredentialMetadataWithoutChangingPrimarySecretSelection() throws {
        let data = Data(#"{"path":"work/github","type":"api_key","usage_hint":"CI access","auto_rotate":true,"expires_at":"2030-01-02T03:04:05Z","fields":{"api_key":"fixture-secret","username":"alice","url":"https://example.invalid","notes":"build account"}}"#.utf8)
        let detail = try JSONDecoder().decode(VaultEntryDetail.self, from: data)
        #expect(detail.type == "api_key")
        #expect(detail.usageHint == "CI access")
        #expect(detail.autoRotate == true)
        #expect(detail.expiresAt == "2030-01-02T03:04:05Z")
        #expect(detail.primarySecret?.field == "api_key")
        #expect(detail.primarySecret?.value == "fixture-secret")
    }

    @Test func primarySecretUsesVaultTypeMappingsAndMasksAllSupportedTypes() throws {
        let cases: [(String, String)] = [
            ("password", "password"), ("api_key", "api_key"), ("bearer_token", "token"),
            ("basic_auth", "password"), ("ssh_key", "private_key"), ("certificate", "cert_pem"),
            ("database_url", "connection_string"), ("totp_seed", "seed"), ("custom", "password")
        ]
        for (type, field) in cases {
            let detail = VaultEntryDetail(path: "work/item", modified: nil, fields: [field: .string("secret")], type: type)
            #expect(detail.primarySecret?.field == field)
            #expect(VaultFieldSecurity.isSensitive(field))
        }
        #expect(!VaultFieldSecurity.isSensitive("username"))
        #expect(!VaultFieldSecurity.isSensitive("url"))
        #expect(!VaultFieldSecurity.isSensitive("notes"))
    }

    @Test func basicAuthKeepsUsernameMetadataVisible() {
        let detail = VaultEntryDetail(path: "work/item", modified: nil, fields: [
            "password": .string("secret"), "username": .string("alice")
        ], type: "basic_auth")
        #expect(detail.primarySecret?.field == "password")
        #expect(detail.sortedFields.first?.key == "password")
        #expect(detail.sortedFields.first?.isSensitive == true)
        #expect(detail.sortedFields.last?.key == "username")
        #expect(detail.sortedFields.last?.isSensitive == false)
    }

}
#endif
