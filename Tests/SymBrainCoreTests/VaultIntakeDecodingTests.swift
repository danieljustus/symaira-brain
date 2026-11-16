// Decoding of `symvault intake --json` responses with omitted empty lists (#1067).
import Foundation
import Testing
@testable import SymBrainCore

@Suite struct VaultIntakeDecodingTests {
    private func decode(_ json: String) throws -> VaultIntakeResponse {
        try JSONDecoder().decode(VaultIntakeResponse.self, from: Data(json.utf8))
    }

    @Test func resultWithoutDuplicatesDecodes() throws {
        let response = try decode("""
        {"results":[{"file":"/tmp/login.txt","status":"ok",
          "provenance":{"source_path":"/tmp/login.txt","source_name":"login.txt","source_type":"text","size":42,"sha256":"abc"},
          "suggestions":[{"path":"login","field":"username","confidence":0.9}]}]}
        """)
        let result = try #require(response.results.first)
        #expect(result.duplicates.isEmpty)
        #expect(result.suggestions.map(\.field) == ["username"])
        #expect(result.provenance?.sourceName == "login.txt")
    }

    @Test func errorResultWithoutSuggestionsDecodes() throws {
        let response = try decode("""
        {"results":[{"file":"/tmp/broken.bin","status":"error","reason":"unsupported file type"}]}
        """)
        let result = try #require(response.results.first)
        #expect(result.suggestions.isEmpty)
        #expect(result.duplicates.isEmpty)
        #expect(result.reason == "unsupported file type")
        #expect(!result.isOK)
    }

    @Test func presentListsAreKept() throws {
        let response = try decode("""
        {"import_id":"batch-1","results":[{"file":"a","status":"ok","suggestions":[],"duplicates":["login"]}]}
        """)
        #expect(response.importID == "batch-1")
        #expect(response.results.first?.duplicates == ["login"])
    }
}
